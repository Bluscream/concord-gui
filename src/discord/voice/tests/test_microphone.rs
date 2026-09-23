//! Microphone capture and outbound pacing.
//!
//! These sixteen were dropped when voice/tests.rs was split into this
//! directory - the file was deleted with them still in it, and nothing
//! failed, because a test that no longer exists cannot report anything.

use super::super::dave::VoiceDaveOutboundPayload;
use super::super::microphone::capture::{
    record_voice_input_chunk, record_voice_input_pcm_stats, voice_input_buffer_size,
    voice_input_channel_rank, voice_input_f32_to_stereo_i16, voice_input_i16_to_stereo_i16,
    voice_input_sample_format_rank, voice_input_u8_to_stereo_i16,
    voice_microphone_min_callback_frames,
};
use super::super::microphone::transmit::{
    advance_voice_media_clock, drain_voice_microphone_pcm_queue,
};
use super::super::playback::voice_output_buffer_size;
use super::super::runtime::stop_voice_connection_task;
use super::*;
use test_rtp::{assert_fake_packet, fake_outbound_state, test_voice_gateway_session};

#[test]
fn fake_outbound_stop_paces_finite_silence_then_speaking_off() {
    let mut state = fake_outbound_state(AEAD_AES256_GCM_RTPSIZE, 20);
    state.set_capture_gate(true, false);

    assert_eq!(
        state
            .send_opus_frame(b"opus-frame")
            .expect("frame should send"),
        VoiceOutboundSendOutcome::Sent
    );

    for index in 0..DISCORD_TRAILING_SILENCE_FRAMES {
        state.advance_media_clock_frames(1);
        assert_eq!(
            state
                .send_trailing_silence_frame_with_dave_payload(
                    VoiceDaveOutboundPayload::Plain(DISCORD_OPUS_SILENCE_FRAME.to_vec()),
                    index + 1 == DISCORD_TRAILING_SILENCE_FRAMES,
                )
                .expect("one trailing silence frame should send"),
            VoiceOutboundSendOutcome::Sent
        );
        let expected_event_count =
            index + 3 + usize::from(index + 1 == DISCORD_TRAILING_SILENCE_FRAMES);
        assert_eq!(state.events().len(), expected_event_count);
        assert_fake_packet(
            AEAD_AES256_GCM_RTPSIZE,
            &state.events()[index + 2],
            8 + index as u16,
            1920 + index as u32 * DISCORD_OPUS_TIMESTAMP_INCREMENT,
            &DISCORD_OPUS_SILENCE_FRAME,
            (21 + index as u32).to_be_bytes(),
            false,
        );
    }
    assert_eq!(
        state.events()[DISCORD_TRAILING_SILENCE_FRAMES + 2],
        VoiceOutboundSendEvent::Speaking {
            speaking: false,
            ssrc: 42,
        }
    );
    assert_eq!(state.rtp.sequence, 13);
    assert_eq!(state.rtp.timestamp, 5760);
    assert_eq!(state.nonce_suffix, 26);
}

#[test]
fn fake_outbound_stop_sends_speaking_off_when_capture_gate_closes() {
    let mut state = fake_outbound_state(AEAD_AES256_GCM_RTPSIZE, 20);
    state.set_capture_gate(true, false);
    assert_eq!(
        state
            .send_opus_frame(b"opus-frame")
            .expect("frame should send"),
        VoiceOutboundSendOutcome::Sent
    );
    let event_count = state.events().len();
    let rtp = state.rtp;
    let nonce_suffix = state.nonce_suffix;

    state.set_capture_gate(true, true);
    assert_eq!(
        state
            .stop_speaking()
            .expect("muted stop should send speaking off"),
        VoiceOutboundSendOutcome::Sent
    );
    assert_eq!(state.events().len(), event_count + 1);
    assert_eq!(
        state.events()[event_count],
        VoiceOutboundSendEvent::Speaking {
            speaking: false,
            ssrc: 42,
        }
    );
    assert_eq!(state.rtp, rtp);
    assert_eq!(state.nonce_suffix, nonce_suffix);

    state.speaking = true;
    state.set_capture_gate(false, false);
    assert_eq!(
        state
            .stop_speaking()
            .expect("disallowed stop should send speaking off"),
        VoiceOutboundSendOutcome::Sent
    );
    assert_eq!(state.events().len(), event_count + 2);
    assert_eq!(
        state.events()[event_count + 1],
        VoiceOutboundSendEvent::Speaking {
            speaking: false,
            ssrc: 42,
        }
    );
    assert_eq!(state.rtp, rtp);
    assert_eq!(state.nonce_suffix, nonce_suffix);
}

#[test]
fn fake_outbound_trailing_silence_uses_dave_policy() {
    let mut dave = VoiceDaveState::new(&test_voice_gateway_session());
    dave.reinit(1).expect("DAVE session should initialize");
    let mut state = fake_outbound_state(AEAD_AES256_GCM_RTPSIZE, 20);
    state.set_capture_gate(true, false);
    state.speaking = true;
    let rtp = state.rtp;

    assert_eq!(
        state
            .send_trailing_silence_frame_with_dave(&mut dave, false)
            .expect("DAVE not-ready silence should still send speaking off"),
        VoiceOutboundSendOutcome::Sent
    );
    assert_eq!(
        state.events(),
        &[VoiceOutboundSendEvent::Speaking {
            speaking: false,
            ssrc: 42,
        }]
    );
    assert_eq!(state.rtp, rtp);
    assert_eq!(state.nonce_suffix, 20);
    assert!(!state.speaking);
}

#[cfg(feature = "voice-playback")]
#[test]
fn microphone_capture_stats_track_callback_size_and_clipping() {
    let stats = VoiceMicrophoneCaptureStats::default();
    let first_captured_at = stats.started_at + Duration::from_millis(10);
    let second_captured_at = first_captured_at + Duration::from_millis(10);

    record_voice_input_chunk(
        960,
        2,
        first_captured_at,
        first_captured_at + Duration::from_millis(10),
        &stats,
    );
    record_voice_input_chunk(
        480,
        2,
        second_captured_at,
        second_captured_at + Duration::from_millis(5),
        &stats,
    );
    record_voice_input_pcm_stats(&[0, i16::MAX, i16::MIN + 1, 120], &stats);

    assert_eq!(stats.chunks.load(Ordering::Relaxed), 2);
    assert_eq!(stats.frames.load(Ordering::Relaxed), 720);
    assert_eq!(voice_microphone_min_callback_frames(&stats), 240);
    assert_eq!(stats.max_callback_frames.load(Ordering::Relaxed), 480);
    assert_eq!(stats.peak_sample.load(Ordering::Relaxed), 32767);
    assert_eq!(stats.clipped_samples.load(Ordering::Relaxed), 2);
}

#[test]
fn microphone_capture_time_advances_rtp_clock_across_missing_frames() {
    let mut state = fake_outbound_state(AEAD_AES256_GCM_RTPSIZE, 10);
    let started_at = Instant::now();
    let mut previous_frame_at = None;

    advance_voice_media_clock(&mut state, &mut previous_frame_at, started_at);
    assert_eq!(state.rtp.timestamp, 1920);

    advance_voice_media_clock(
        &mut state,
        &mut previous_frame_at,
        started_at + Duration::from_millis(20),
    );
    assert_eq!(state.rtp.timestamp, 2880);

    advance_voice_media_clock(
        &mut state,
        &mut previous_frame_at,
        started_at + Duration::from_millis(120),
    );
    assert_eq!(state.rtp.timestamp, 7680);
    assert_eq!(state.rtp.sequence, 7);
    assert_eq!(state.nonce_suffix, 10);
}

#[cfg(feature = "voice-playback")]
#[test]
fn microphone_freshness_policy_bounds_queue_depth_and_frame_age() {
    let now = Instant::now();

    // A full stored queue catches up to the newest three live frames instead
    // of preserving hundreds of milliseconds of old speech.
    let (tx, mut rx) = tokio::sync::mpsc::channel(VOICE_MIC_PCM_FRAME_QUEUE);
    let initial = VoiceMicrophoneFrame {
        samples: vec![0],
        captured_at: now - Duration::from_millis(320),
    };
    for index in 1u8..=15 {
        tx.try_send(VoiceMicrophoneFrame {
            samples: vec![i16::from(index)],
            captured_at: now - Duration::from_millis(u64::from(15 - index) * 20),
        })
        .expect("backlog frame should queue");
    }

    let (selected, dropped) = select_fresh_voice_microphone_frame(initial, &mut rx, now);
    let selected = selected.expect("a fresh frame should remain");

    assert_eq!(selected.samples, vec![13]);
    assert_eq!(dropped, 13);
    assert_eq!(rx.len().saturating_add(1), VOICE_MIC_MAX_LIVE_FRAMES);
    assert!(now.saturating_duration_since(selected.captured_at) <= VOICE_MIC_MAX_FRAME_AGE);

    // A frame exactly on the age boundary remains live when it is the only
    // available audio.
    let (_tx, mut rx) = tokio::sync::mpsc::channel(1);
    let boundary = VoiceMicrophoneFrame {
        samples: vec![50],
        captured_at: now - VOICE_MIC_MAX_FRAME_AGE,
    };
    let (selected, dropped) = select_fresh_voice_microphone_frame(boundary, &mut rx, now);

    assert_eq!(
        selected.expect("boundary frame should remain live").samples,
        vec![50]
    );
    assert_eq!(dropped, 0);

    // Once the age budget is exceeded, transmitting silence or waiting for a
    // new live frame is better than sending stale speech.
    let (_tx, mut rx) = tokio::sync::mpsc::channel(1);
    let stale = VoiceMicrophoneFrame {
        samples: vec![99],
        captured_at: now - VOICE_MIC_MAX_FRAME_AGE - Duration::from_millis(1),
    };
    let (selected, dropped) = select_fresh_voice_microphone_frame(stale, &mut rx, now);

    assert!(selected.is_none());
    assert_eq!(dropped, 1);
}

#[cfg(feature = "voice-playback")]
#[test]
fn microphone_input_conversion_produces_20ms_stereo_frames() {
    let mono = vec![0.5f32; DISCORD_OPUS_FRAME_SAMPLES_PER_CHANNEL];
    let stereo = voice_input_f32_to_stereo_i16(&mono, 1);
    assert_eq!(stereo.len(), DISCORD_OPUS_20MS_STEREO_SAMPLES);
    assert_eq!(stereo[0], stereo[1]);
    assert!(stereo[0] > 0);

    let interleaved = voice_input_i16_to_stereo_i16(&[1, 2, 3, 4, 5, 6], 3);
    assert_eq!(interleaved, vec![1, 2, 4, 5]);

    let unsigned = voice_input_u8_to_stereo_i16(&[0, 255], 2);
    assert_eq!(unsigned, vec![i16::MIN, 32512]);
}

#[cfg(feature = "voice-playback")]
#[test]
fn microphone_pcm_drain_clears_backlog_before_reenable() {
    let (tx, mut rx) = tokio::sync::mpsc::channel(VOICE_MIC_PCM_FRAME_QUEUE);
    let now = Instant::now();

    tx.try_send(VoiceMicrophoneFrame {
        samples: vec![10],
        captured_at: now,
    })
    .expect("first frame should queue");
    tx.try_send(VoiceMicrophoneFrame {
        samples: vec![20],
        captured_at: now,
    })
    .expect("second frame should queue");

    drain_voice_microphone_pcm_queue(&mut rx);

    assert!(matches!(
        rx.try_recv(),
        Err(tokio::sync::mpsc::error::TryRecvError::Empty)
    ));
}

#[cfg(feature = "voice-playback")]
#[test]
fn microphone_pcm_frames_count_full_queue_drops() {
    let (tx, mut rx) = tokio::sync::mpsc::channel(1);
    let stats = Arc::new(VoiceMicrophoneCaptureStats::default());
    let mut frames =
        VoiceMicrophonePcmFrames::new(tx, Arc::clone(&stats), DISCORD_VOICE_SAMPLE_RATE);
    let samples = vec![1i16; DISCORD_OPUS_20MS_STEREO_SAMPLES * 2];

    frames.push_stereo_samples(&samples, Instant::now());

    assert_eq!(
        rx.try_recv()
            .expect("first frame should queue")
            .samples
            .len(),
        DISCORD_OPUS_20MS_STEREO_SAMPLES
    );
    assert_eq!(stats.queued_frames.load(Ordering::Relaxed), 1);
    assert_eq!(stats.dropped_frames.load(Ordering::Relaxed), 1);
}

#[cfg(feature = "voice-playback")]
#[test]
fn microphone_pcm_frames_resample_44100_to_48000() {
    let (tx, mut rx) = tokio::sync::mpsc::channel(4);
    let stats = Arc::new(VoiceMicrophoneCaptureStats::default());
    let mut frames = VoiceMicrophonePcmFrames::new(tx, Arc::clone(&stats), 44_100);
    let input_frames = 883;
    let mut samples = Vec::with_capacity(input_frames * DISCORD_VOICE_CHANNELS_USIZE);
    for index in 0..input_frames {
        samples.push(index as i16);
        samples.push(-(index as i16));
    }

    frames.push_stereo_samples(&samples, Instant::now());
    let frame = rx
        .try_recv()
        .expect("resampled 20 ms frame should be queued");

    assert_eq!(frame.samples.len(), DISCORD_OPUS_20MS_STEREO_SAMPLES);
    assert_eq!(frame.samples[0], 0);
    assert_eq!(frame.samples[1], 0);
    assert!(frame.samples[frame.samples.len() - 2] > 870);
    assert!(frame.samples[frame.samples.len() - 1] < -870);
    assert!(rx.try_recv().is_err());
    assert_eq!(stats.queued_frames.load(Ordering::Relaxed), 1);
    assert_eq!(stats.dropped_frames.load(Ordering::Relaxed), 0);
}

#[test]
fn voice_audio_source_watch_retains_only_the_latest_selection() {
    let (audio_sources_tx, audio_sources_rx) = watch::channel(VoiceAudioSourceSelection {
        generation: 0,
        sources: VoiceAudioSources::default(),
    });
    audio_sources_tx.send_replace(VoiceAudioSourceSelection {
        generation: 1,
        sources: VoiceAudioSources {
            input: Some("mic-1".to_owned()),
            output: None,
        },
    });
    audio_sources_tx.send_replace(VoiceAudioSourceSelection {
        generation: 2,
        sources: VoiceAudioSources {
            input: Some("mic-2".to_owned()),
            output: Some("speaker-2".to_owned()),
        },
    });

    assert_eq!(
        audio_sources_rx.borrow().clone(),
        VoiceAudioSourceSelection {
            generation: 2,
            sources: VoiceAudioSources {
                input: Some("mic-2".to_owned()),
                output: Some("speaker-2".to_owned()),
            },
        }
    );
}

#[cfg(feature = "voice-playback")]
#[test]
fn voice_input_buffer_size_follows_the_requested_milliseconds() {
    // v2.5.19 (#356) stopped asking for a small fixed callback, and v2.5.20
    // (#363) replaced the adaptive recovery with an explicit setting: a user
    // who names a buffer length gets exactly that many frames.
    assert_eq!(
        voice_input_buffer_size(MicrophoneBufferMs::new(20), DISCORD_VOICE_SAMPLE_RATE),
        cpal::BufferSize::Fixed(960)
    );
    assert_eq!(
        voice_input_buffer_size(MicrophoneBufferMs::new(10), DISCORD_VOICE_SAMPLE_RATE),
        cpal::BufferSize::Fixed(480)
    );
}

#[cfg(feature = "voice-playback")]
#[test]
fn voice_input_config_prefers_mono_then_sample_format() {
    assert!(voice_input_channel_rank(1) < voice_input_channel_rank(2));
    assert!(
        voice_input_sample_format_rank(cpal::SampleFormat::F32)
            < voice_input_sample_format_rank(cpal::SampleFormat::I16)
    );
    assert!(
        voice_input_sample_format_rank(cpal::SampleFormat::I16)
            < voice_input_sample_format_rank(cpal::SampleFormat::U16)
    );
}

#[cfg(feature = "voice-playback")]
#[test]
fn voice_output_buffer_size_requests_bounded_low_latency_buffer() {
    let cases = [
        (
            true,
            cpal::SupportedBufferSize::Range {
                min: 128,
                max: 8_192,
            },
            cpal::BufferSize::Fixed(VOICE_PULSE_OUTPUT_BUFFER_FRAMES),
        ),
        (
            true,
            cpal::SupportedBufferSize::Range {
                min: 4_096,
                max: 8_192,
            },
            cpal::BufferSize::Fixed(4_096),
        ),
        (
            true,
            cpal::SupportedBufferSize::Range { min: 128, max: 960 },
            cpal::BufferSize::Fixed(960),
        ),
        (
            true,
            cpal::SupportedBufferSize::Unknown,
            cpal::BufferSize::Default,
        ),
        (
            false,
            cpal::SupportedBufferSize::Range {
                min: 128,
                max: 8_192,
            },
            cpal::BufferSize::Default,
        ),
    ];

    for (use_low_latency_pulse_audio, supported, expected) in cases {
        assert_eq!(
            voice_output_buffer_size(use_low_latency_pulse_audio, &supported),
            expected
        );
    }
}

#[cfg(feature = "voice-playback")]
#[test]
fn voice_speaking_payload_matches_expected_shape() {
    let on: Value = serde_json::from_str(&voice_speaking_payload(1234, true))
        .expect("speaking-on payload should be JSON");
    assert_eq!(on["op"].as_u64(), Some(u64::from(VOICE_OP_SPEAKING)));
    assert_eq!(on["d"]["speaking"].as_u64(), Some(1));
    assert_eq!(on["d"]["delay"].as_u64(), Some(0));
    assert_eq!(on["d"]["ssrc"].as_u64(), Some(1234));

    let off: Value = serde_json::from_str(&voice_speaking_payload(1234, false))
        .expect("speaking-off payload should be JSON");
    assert_eq!(off["d"]["speaking"].as_u64(), Some(0));
}

#[cfg(feature = "voice-playback")]
#[test]
fn voice_udp_transmit_reports_only_failures_to_the_gateway() {
    let (failure_tx, mut failure_rx) = mpsc::unbounded_channel();

    publish_voice_udp_transmit_failure(Ok(()), &failure_tx);
    assert_eq!(failure_rx.try_recv(), Err(mpsc::error::TryRecvError::Empty));

    publish_voice_udp_transmit_failure(Err("closed voice websocket".to_owned()), &failure_tx);
    assert_eq!(
        failure_rx
            .try_recv()
            .expect("fatal transmit errors should reach the gateway"),
        "closed voice websocket"
    );
}

#[cfg(feature = "voice-playback")]
#[tokio::test]
async fn voice_child_tasks_waits_for_udp_transmit_shutdown() {
    let (done_tx, done_rx) = tokio::sync::oneshot::channel();
    let mut child_tasks = VoiceChildTasks::default();
    child_tasks.udp_transmit = Some(tokio::spawn(async move {
        sleep(Duration::from_millis(10)).await;
        let _ = done_tx.send(());
    }));

    child_tasks.shutdown_all().await;

    done_rx
        .await
        .expect("shutdown should await UDP transmit completion");
}

#[tokio::test]
async fn voice_runtime_requests_speaking_cleanup_after_connection_task_failure() {
    let mut connection_task = Some(tokio::spawn(async {
        panic!("simulated voice connection task failure");
    }));
    let expected_session = test_voice_gateway_session();
    let mut connection_session = Some(expected_session.clone());
    let mut audio_sources_tx = None;
    let mut capture_gate_tx = None;
    let mut playback_gate_tx = None;

    let stopped_session = stop_voice_connection_task(
        &mut connection_task,
        &mut connection_session,
        &mut audio_sources_tx,
        &mut capture_gate_tx,
        &mut playback_gate_tx,
        "test failed voice connection stop",
    )
    .await;

    assert_eq!(stopped_session, Some(expected_session));
    assert!(connection_task.is_none());
    assert!(connection_session.is_none());
}

#[tokio::test]
async fn voice_runtime_stops_connection_task_by_closing_gate_channels() {
    let (audio_sources_tx, mut audio_sources_rx) = watch::channel(VoiceAudioSourceSelection {
        generation: 0,
        sources: VoiceAudioSources::default(),
    });
    let (capture_gate_tx, mut capture_gate_rx) = mpsc::unbounded_channel();
    let (playback_gate_tx, mut playback_gate_rx) = mpsc::unbounded_channel();
    let (done_tx, done_rx) = tokio::sync::oneshot::channel();
    let mut connection_task = Some(tokio::spawn(async move {
        assert!(audio_sources_rx.changed().await.is_err());
        assert!(capture_gate_rx.recv().await.is_none());
        assert!(playback_gate_rx.recv().await.is_none());
        let _ = done_tx.send(());
    }));
    let mut connection_session = None;
    let mut audio_sources_tx = Some(audio_sources_tx);
    let mut capture_gate_tx = Some(capture_gate_tx);
    let mut playback_gate_tx = Some(playback_gate_tx);

    let stopped_session = stop_voice_connection_task(
        &mut connection_task,
        &mut connection_session,
        &mut audio_sources_tx,
        &mut capture_gate_tx,
        &mut playback_gate_tx,
        "test voice connection stop",
    )
    .await;

    assert!(stopped_session.is_none());
    done_rx
        .await
        .expect("connection task should finish after gate channels close");
    assert!(connection_task.is_none());
    assert!(audio_sources_tx.is_none());
    assert!(capture_gate_tx.is_none());
    assert!(playback_gate_tx.is_none());
}
