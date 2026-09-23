#[cfg(feature = "voice-playback")]
use super::super::audio_buffer::voice_output_prebuffer_frames;
use super::super::opus::VoicePlaybackDecodeState;
use super::*;
#[cfg(feature = "voice-playback")]
use crate::support::audio_output::{f32_sample_to_i16, f32_sample_to_u8, f32_sample_to_u16};
use tokio::time::timeout;

#[test]
fn voice_decode_state_applies_participant_settings_before_final_output_limit() {
    let mut state = VoicePlaybackDecodeState::default();
    let poll_samples =
        VOICE_PLAYBACK_POLL_SAMPLES_PER_CHANNEL * usize::from(DISCORD_VOICE_CHANNELS);
    state.replace_participant_playback_settings(HashMap::from([
        (
            Id::new(10),
            VoiceParticipantPlaybackSettings {
                volume: VoiceParticipantVolumePercent::new(200),
                muted: false,
                video_hidden: false,
            },
        ),
        (
            Id::new(11),
            VoiceParticipantPlaybackSettings {
                muted: true,
                video_hidden: false,
                ..VoiceParticipantPlaybackSettings::default()
            },
        ),
    ]));
    state.push_decoded_samples_for_user(1, Some(Id::new(10)), vec![0.75; poll_samples]);
    state.push_decoded_samples_for_user(2, Some(Id::new(11)), vec![1.0; poll_samples]);

    let mixed = state
        .next_pending_mix()
        .expect("audible participant should produce a mix");

    assert!(mixed.iter().all(|sample| (*sample - 1.5).abs() < 0.0001));

    #[cfg(feature = "voice-playback")]
    {
        let reduced_after_participant_boost = apply_voice_playback_gain_and_limit(mixed[0], 0.5);
        assert_voice_sample_near(reduced_after_participant_boost, 0.75);

        let boosted_quieter_peak = apply_voice_playback_gain_and_limit(0.75, 2.0);
        let boosted_louder_peak = apply_voice_playback_gain_and_limit(1.0, 2.0);
        assert!(boosted_quieter_peak < boosted_louder_peak);
        assert!(boosted_louder_peak <= VOICE_SOFT_LIMIT_CEILING);
    }
}

#[test]
fn voice_post_process_reduces_alternating_high_frequency_noise() {
    let mut post_process = VoicePlaybackPostProcess::default();
    let mut samples: Vec<f32> = vec![1.0, 1.0, -1.0, -1.0, 1.0, 1.0, -1.0, -1.0];

    post_process.process(&mut samples);

    assert!(samples[2].abs() < 1.0);
    assert!(samples[4].abs() < 1.0);
    assert!(samples[6].abs() < 1.0);
}

#[cfg(feature = "voice-playback")]
#[test]
fn extra_output_channels_use_converted_silence() {
    let mut u8_output = [0u8; 4];
    write_voice_output_frame(&mut u8_output, 0.5, -0.5, 1.0, f32_sample_to_u8);
    assert_eq!(
        u8_output,
        [
            f32_sample_to_u8(0.5),
            f32_sample_to_u8(-0.5),
            f32_sample_to_u8(0.0),
            f32_sample_to_u8(0.0)
        ]
    );

    let mut u16_output = [0u16; 4];
    write_voice_output_frame(&mut u16_output, 0.5, -0.5, 1.0, f32_sample_to_u16);
    assert_eq!(
        u16_output,
        [
            f32_sample_to_u16(0.5),
            f32_sample_to_u16(-0.5),
            f32_sample_to_u16(0.0),
            f32_sample_to_u16(0.0),
        ]
    );

    let mut i16_output = [1i16; 4];
    write_voice_output_frame(&mut i16_output, 0.5, -0.5, 1.0, f32_sample_to_i16);
    assert_eq!(
        i16_output,
        [f32_sample_to_i16(0.5), f32_sample_to_i16(-0.5), 0, 0]
    );
}

pub(super) fn assert_voice_sample_near(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() < 0.0001,
        "expected {actual} to be close to {expected}"
    );
}

#[cfg(feature = "voice-playback")]
#[test]
fn voice_audio_buffer_resamples_non_48khz_output_clock() {
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    tx.try_send(vec![0.0, 0.0, 1.0, 1.0, 2.0, 2.0, 3.0, 3.0])
        .expect("decoded samples should queue");
    let stats = Arc::new(VoiceAudioOutputStats::default());
    stats
        .queued_frames
        .store(VOICE_AUDIO_OUTPUT_PREBUFFER_FRAMES, Ordering::Relaxed);
    let mut buffer = VoiceAudioBuffer::new(rx, 24_000, stats);
    buffer.begin_output(0);

    assert_eq!(buffer.next_stereo_frame(), Some([0.0, 0.0]));
    assert_eq!(buffer.next_stereo_frame(), Some([2.0, 2.0]));
    let faded = buffer
        .next_stereo_frame()
        .expect("resampled underrun should fade from the last frame");
    assert!(faded[0] < 2.0 && faded[0] > 0.0);
    assert_eq!(faded[0], faded[1]);
}

#[cfg(feature = "voice-playback")]
#[test]
fn voice_audio_buffer_fades_short_underruns() {
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    tx.try_send(vec![1.0, -1.0])
        .expect("decoded samples should queue");
    let stats = Arc::new(VoiceAudioOutputStats::default());
    stats.record_pcm_enqueue(1);
    stats
        .queued_frames
        .store(VOICE_AUDIO_OUTPUT_PREBUFFER_FRAMES, Ordering::Relaxed);
    let mut buffer = VoiceAudioBuffer::new(rx, DISCORD_VOICE_SAMPLE_RATE, Arc::clone(&stats));
    buffer.begin_output(0);

    assert_eq!(buffer.next_stereo_frame(), Some([1.0, -1.0]));
    let faded = buffer
        .next_stereo_frame()
        .expect("underrun should produce a short fade tail");

    assert!(faded[0] < 1.0 && faded[0] > 0.0);
    assert!(faded[1] > -1.0 && faded[1] < 0.0);
    assert_eq!(stats.output_underruns.load(Ordering::Relaxed), 1);
    assert_eq!(stats.recent_pcm_underruns.load(Ordering::Relaxed), 1);
}

#[cfg(feature = "voice-playback")]
#[test]
fn voice_output_prebuffer_includes_one_device_callback() {
    let cases = [
        (4_096, 48_000, 6_976),
        (96_000, 48_000, 98_880),
        (4_096, 192_000, 3_904),
        (4_096, 24_000, 11_072),
    ];

    for (callback_frames, output_sample_rate, expected) in cases {
        assert_eq!(
            voice_output_prebuffer_frames(callback_frames, output_sample_rate),
            expected
        );
    }

    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    tx.try_send(vec![1.0, -1.0])
        .expect("decoded samples should queue");
    let stats = Arc::new(VoiceAudioOutputStats::default());
    let mut buffer = VoiceAudioBuffer::new(rx, DISCORD_VOICE_SAMPLE_RATE, Arc::clone(&stats));

    stats.queued_frames.store(6_975, Ordering::Relaxed);
    buffer.begin_output(4_096);
    assert_eq!(buffer.next_stereo_frame(), None);

    stats.queued_frames.store(6_976, Ordering::Relaxed);
    buffer.begin_output(4_096);
    assert_eq!(buffer.next_stereo_frame(), Some([1.0, -1.0]));
}

#[test]
fn remote_speaking_activity_ignores_silence_and_unplayable_media() {
    assert!(!voice_media_payload_counts_as_remote_activity(
        &VoiceMediaPayload::Plain(DISCORD_OPUS_SILENCE_FRAME.to_vec()),
    ));
    assert!(!voice_media_payload_counts_as_remote_activity(
        &VoiceMediaPayload::DaveDecrypted {
            user_id: 42,
            opus: DISCORD_OPUS_SILENCE_FRAME.to_vec(),
        },
    ));
    assert!(!voice_media_payload_counts_as_remote_activity(
        &VoiceMediaPayload::DaveUnexpectedPlain { payload_len: 4 },
    ));
    assert!(!voice_media_payload_counts_as_remote_activity(
        &VoiceMediaPayload::DaveMissingUser { payload_len: 4 },
    ));
    assert!(!voice_media_payload_counts_as_remote_activity(
        &VoiceMediaPayload::DaveNotReady {
            user_id: 42,
            payload_len: 4,
        },
    ));
    assert!(!voice_media_payload_counts_as_remote_activity(
        &VoiceMediaPayload::DaveDecryptFailed {
            user_id: 42,
            message: "failed".to_owned(),
        },
    ));
    assert!(voice_media_payload_counts_as_remote_activity(
        &VoiceMediaPayload::Plain(b"opus".to_vec()),
    ));
    assert!(voice_media_payload_counts_as_remote_activity(
        &VoiceMediaPayload::DaveDecrypted {
            user_id: 42,
            opus: b"opus".to_vec(),
        },
    ));
}

#[test]
fn voice_gateway_opcode_rejects_values_that_do_not_fit_u8() {
    assert_eq!(gateway::voice_gateway_opcode(&json!({ "op": 2 })), Some(2));
    assert_eq!(gateway::voice_gateway_opcode(&json!({ "op": 258 })), None);
    assert_eq!(gateway::voice_gateway_opcode(&json!({ "op": "2" })), None);
}

#[test]
fn remote_speaking_activity_queue_is_bounded_and_recovers_capacity() {
    let (tx, mut rx) = mpsc::channel(1);
    let first = Id::new(10);
    let second = Id::new(20);

    gateway::queue_remote_speaking_activity(&tx, first);
    gateway::queue_remote_speaking_activity(&tx, second);

    assert_eq!(rx.try_recv(), Ok(first));
    assert!(matches!(
        rx.try_recv(),
        Err(mpsc::error::TryRecvError::Empty)
    ));

    gateway::queue_remote_speaking_activity(&tx, second);
    assert_eq!(rx.try_recv(), Ok(second));
}

#[test]
fn microphone_sensitivity_filters_quiet_pcm_frames() {
    let quiet = vec![100i16; DISCORD_OPUS_20MS_STEREO_SAMPLES];
    let normal = vec![1500i16; DISCORD_OPUS_20MS_STEREO_SAMPLES];
    let loud = vec![4000i16; DISCORD_OPUS_20MS_STEREO_SAMPLES];

    assert!(voice_pcm_frame_reaches_sensitivity(
        &quiet,
        MicrophoneSensitivityDb::new(-60),
    ));
    assert!(!voice_pcm_frame_reaches_sensitivity(
        &quiet,
        MicrophoneSensitivityDb::new(-30),
    ));
    assert!(voice_pcm_frame_reaches_sensitivity(
        &normal,
        MicrophoneSensitivityDb::default(),
    ));
    assert!(voice_pcm_frame_reaches_sensitivity(
        &loud,
        MicrophoneSensitivityDb::new(-20),
    ));
}

#[cfg(feature = "voice-playback")]
#[test]
fn microphone_gate_hangover_keeps_short_quiet_gaps_open() {
    let quiet = vec![100i16; DISCORD_OPUS_20MS_STEREO_SAMPLES];
    let normal = vec![1500i16; DISCORD_OPUS_20MS_STEREO_SAMPLES];
    let mut gate = VoiceMicrophoneGateState::default();

    assert!(gate.allows_frame(&normal, MicrophoneSensitivityDb::default()));
    for _ in 0..VOICE_MIC_GATE_HANGOVER_FRAMES {
        assert!(gate.allows_frame(&quiet, MicrophoneSensitivityDb::default()));
    }
    assert!(!gate.allows_frame(&quiet, MicrophoneSensitivityDb::default()));

    gate.allows_frame(&normal, MicrophoneSensitivityDb::default());
    gate.reset();
    assert!(!gate.allows_frame(&quiet, MicrophoneSensitivityDb::default()));
}

#[test]
fn voice_volume_scales_i16_pcm_frame() {
    let mut frame = vec![1000, -1000, i16::MAX, i16::MIN];

    let limited =
        apply_voice_microphone_gain_and_limit(&mut frame, VoiceVolumePercent::new(50).gain());

    assert_eq!(frame, vec![500, -500, 16384, -16384]);
    assert_eq!(limited, 0);

    let mut boosted = vec![1000, -1000, 20_000, -20_000];
    let limited =
        apply_voice_microphone_gain_and_limit(&mut boosted, VoiceVolumePercent::new(200).gain());

    assert_eq!(boosted[0..2], [2000, -2000]);
    assert!(boosted[2] < i16::MAX);
    assert!(boosted[3] > i16::MIN);
    assert_eq!(boosted[2], -boosted[3]);
    assert_eq!(limited, 2);
}

#[test]
fn voice_microphone_protection_soft_limits_extreme_samples() {
    let mut frame = vec![1000, -1000, i16::MAX, i16::MIN];

    let limited = apply_voice_microphone_gain_and_limit(&mut frame, 1.0);

    assert_eq!(frame[0], 1000);
    assert_eq!(frame[1], -1000);
    assert!(frame[2] < i16::MAX);
    assert!(frame[3] > i16::MIN);
    assert!((i32::from(frame[2]) + i32::from(frame[3])).abs() <= 1);
    assert_eq!(limited, 2);
}

#[cfg(feature = "voice-playback")]
#[test]
fn voice_microphone_conditioning_combines_gain_before_soft_limiting() {
    let mut frame = vec![1000, 12_000, 20_000, -12_000, -20_000];
    let mut microphone_gate = VoiceMicrophoneGateState::default();
    let mut transmit_stats = VoiceUdpTransmitStats::default();

    condition_voice_microphone_frame(
        &mut frame,
        VoiceCaptureGate {
            capture_enabled: true,
            transmit_enabled: true,
            use_voice_activity: true,
            noise_suppression: false,
            microphone_sensitivity: MicrophoneSensitivityDb::default(),
            microphone_volume: VoiceVolumePercent::new(200),
        },
        &mut microphone_gate,
        &mut transmit_stats,
    );

    assert_eq!(frame[0], 3000);
    assert!(frame[1] < frame[2]);
    assert!(frame[2] < i16::MAX);
    assert_eq!(frame[1], -frame[3]);
    assert_eq!(frame[2], -frame[4]);
    assert_eq!(transmit_stats.limited_samples, 4);
}

#[test]
fn voice_microphone_overload_detects_dense_clipping_not_single_peaks() {
    let mut normal_loud = vec![8_000i16; DISCORD_OPUS_20MS_STEREO_SAMPLES];
    normal_loud[0] = i16::MAX;
    normal_loud[1] = i16::MIN + 1;
    assert!(!voice_microphone_frame_is_overloaded(&normal_loud));

    let mut below_threshold = vec![0i16; DISCORD_OPUS_20MS_STEREO_SAMPLES];
    for sample in below_threshold
        .iter_mut()
        .take(VOICE_MIC_OVERLOAD_MIN_CLIPPED_SAMPLES - 1)
    {
        *sample = i16::MAX;
    }
    assert!(!voice_microphone_frame_is_overloaded(&below_threshold));

    let mut overloaded = vec![0i16; DISCORD_OPUS_20MS_STEREO_SAMPLES];
    for sample in overloaded
        .iter_mut()
        .take(VOICE_MIC_OVERLOAD_MIN_CLIPPED_SAMPLES)
    {
        *sample = i16::MAX;
    }
    assert!(voice_microphone_frame_is_overloaded(&overloaded));
}

#[cfg(feature = "voice-playback")]
#[test]
fn microphone_gate_blanks_handling_noise_envelope() {
    let mut gate = VoiceMicrophoneGateState::default();
    let normal = vec![1500i16; DISCORD_OPUS_20MS_STEREO_SAMPLES];
    let mut handling_noise = vec![0i16; DISCORD_OPUS_20MS_STEREO_SAMPLES];
    handling_noise[0] = i16::MAX;
    handling_noise[1] = i16::MIN + 1;
    for sample in handling_noise
        .iter_mut()
        .skip(2)
        .take(VOICE_MIC_OVERLOAD_MIN_CLIPPED_SAMPLES - 2)
    {
        *sample = i16::MAX;
    }

    let overload_decision = gate
        .overload_decision(&handling_noise)
        .expect("handling-noise frame should be blanked");
    assert_eq!(
        overload_decision.kind,
        VoiceMicrophoneOverloadKind::HandlingNoise
    );
    assert_eq!(overload_decision.gain, VOICE_MIC_HANDLING_NOISE_GAIN);

    for _ in 0..VOICE_MIC_HANDLING_NOISE_SUPPRESSION_FRAMES {
        let recovery_decision = gate
            .overload_decision(&normal)
            .expect("handling-noise envelope should be blanked");
        assert_eq!(
            recovery_decision.kind,
            VoiceMicrophoneOverloadKind::Recovery
        );
        assert_eq!(recovery_decision.gain, VOICE_MIC_HANDLING_NOISE_GAIN);
    }
    assert!(gate.overload_decision(&normal).is_none());

    gate.overload_decision(&handling_noise);
    gate.reset();
    assert!(gate.overload_decision(&normal).is_none());
}

#[cfg(feature = "voice-playback")]
#[test]
fn microphone_gate_ramps_after_non_handling_transient() {
    let mut gate = VoiceMicrophoneGateState::default();
    let normal = vec![1500i16; DISCORD_OPUS_20MS_STEREO_SAMPLES];
    let mut transient = vec![0i16; DISCORD_OPUS_20MS_STEREO_SAMPLES];
    for sample in transient
        .iter_mut()
        .take(VOICE_MIC_OVERLOAD_SEVERE_CLIPPED_SAMPLES)
    {
        *sample = i16::MAX;
    }

    let overload_decision = gate
        .overload_decision(&transient)
        .expect("transient frame should be attenuated");
    assert_eq!(
        overload_decision.kind,
        VoiceMicrophoneOverloadKind::Transient
    );
    assert_eq!(overload_decision.gain, VOICE_MIC_OVERLOAD_TRANSIENT_GAIN);

    let mut previous_gain = overload_decision.gain;
    for frame_index in 0..VOICE_MIC_OVERLOAD_RECOVERY_FRAMES {
        let recovery_decision = gate
            .overload_decision(&normal)
            .expect("transient recovery should be ramped");
        assert_eq!(
            recovery_decision.kind,
            VoiceMicrophoneOverloadKind::Recovery
        );
        if frame_index == 0 {
            assert!(
                (recovery_decision.gain - VOICE_MIC_OVERLOAD_RECOVERY_START_GAIN).abs()
                    < f32::EPSILON
            );
        }
        assert!(recovery_decision.gain > previous_gain);
        assert!(recovery_decision.gain <= 1.0);
        previous_gain = recovery_decision.gain;
    }
    assert!(gate.overload_decision(&normal).is_none());
}

#[test]
fn voice_microphone_overload_gain_keeps_shouted_frame_audible() {
    let mut shouted = vec![0i16; DISCORD_OPUS_20MS_STEREO_SAMPLES];
    for sample in shouted
        .iter_mut()
        .take(VOICE_MIC_OVERLOAD_MIN_CLIPPED_SAMPLES)
    {
        *sample = i16::MAX;
    }

    let gain = voice_microphone_overload_gain(&shouted)
        .expect("clipped shouted frame should be gain-reduced");
    apply_voice_microphone_gain_and_limit(&mut shouted, gain);

    assert_eq!(gain, VOICE_MIC_OVERLOAD_ATTENUATION_GAIN);
    assert!(shouted.iter().any(|sample| *sample > 0));
    assert!(
        shouted
            .iter()
            .all(|sample| i32::from(*sample).abs() < i32::from(i16::MAX))
    );
}

#[test]
fn voice_microphone_blanks_clipped_frames_except_handling_noise() {
    let mut sparse_clip = vec![2000i16; DISCORD_OPUS_20MS_STEREO_SAMPLES];
    for sample in sparse_clip
        .iter_mut()
        .take(VOICE_MIC_OVERLOAD_MIN_CLIPPED_SAMPLES - 2)
    {
        *sample = i16::MAX;
    }

    let mut attenuated = vec![0i16; DISCORD_OPUS_20MS_STEREO_SAMPLES];
    for sample in attenuated
        .iter_mut()
        .take(VOICE_MIC_OVERLOAD_MIN_CLIPPED_SAMPLES)
    {
        *sample = i16::MAX;
    }

    let mut handling_noise = vec![0i16; DISCORD_OPUS_20MS_STEREO_SAMPLES];
    handling_noise[0] = i16::MAX;
    handling_noise[1] = i16::MIN + 1;

    let cases = [
        ("sparse unclassified clip", sparse_clip, None, true),
        (
            "attenuated clip",
            attenuated,
            Some(VoiceMicrophoneOverloadKind::Attenuated),
            true,
        ),
        (
            "handling noise",
            handling_noise,
            Some(VoiceMicrophoneOverloadKind::HandlingNoise),
            false,
        ),
        (
            "clean frame",
            vec![1500i16; DISCORD_OPUS_20MS_STEREO_SAMPLES],
            None,
            false,
        ),
    ];

    for (name, frame, expected_kind, needs_blank) in cases {
        let raw_decision = voice_microphone_overload_decision(&frame);
        assert_eq!(
            raw_decision.map(|decision| decision.kind),
            expected_kind,
            "{name}"
        );
        assert_eq!(
            voice_microphone_clipped_frame_needs_blank(&frame, raw_decision),
            needs_blank,
            "{name}"
        );
    }
}

#[test]
fn voice_microphone_handling_noise_uses_adjacent_delta_without_dense_clipping() {
    let mut handling_noise = vec![0i16; DISCORD_OPUS_20MS_STEREO_SAMPLES];
    handling_noise[0] = 22_000;
    handling_noise[1] = -20_001;

    let decision = voice_microphone_overload_decision(&handling_noise)
        .expect("large adjacent delta should classify handling noise");

    assert_eq!(decision.kind, VoiceMicrophoneOverloadKind::HandlingNoise);
    assert_eq!(decision.gain, VOICE_MIC_HANDLING_NOISE_GAIN);
    assert_eq!(voice_microphone_clipped_sample_count(&handling_noise), 0);
}

#[test]
fn voice_microphone_overload_promotes_sparse_clipped_transients_to_handling_noise() {
    for (name, second_sample, min_delta, max_delta) in [
        (
            "impulse",
            -3_233,
            VOICE_MIC_OVERLOAD_IMPULSE_DELTA,
            VOICE_MIC_HANDLING_NOISE_DELTA,
        ),
        (
            "step",
            i16::MAX,
            VOICE_MIC_OVERLOAD_CLIPPED_STEP_DELTA,
            VOICE_MIC_OVERLOAD_IMPULSE_DELTA,
        ),
    ] {
        let mut frame = vec![0i16; DISCORD_OPUS_20MS_STEREO_SAMPLES];
        frame[0] = i16::MAX;
        frame[1] = second_sample;

        let decision = voice_microphone_overload_decision(&frame)
            .unwrap_or_else(|| panic!("clipped {name} should be gain-reduced"));

        assert_eq!(
            decision.kind,
            VoiceMicrophoneOverloadKind::HandlingNoise,
            "{name}"
        );
        assert_eq!(decision.gain, VOICE_MIC_HANDLING_NOISE_GAIN, "{name}");
        let max_adjacent_delta = voice_microphone_max_adjacent_delta(&frame);
        assert!(max_adjacent_delta >= min_delta, "{name}");
        assert!(max_adjacent_delta < max_delta, "{name}");
        assert!(
            voice_microphone_clipped_sample_count(&frame) < VOICE_MIC_OVERLOAD_MIN_CLIPPED_SAMPLES,
            "{name}"
        );
    }
}

#[test]
fn voice_microphone_same_polarity_clip_threshold_selects_attenuation_or_blank() {
    for (name, clipped_samples, expected_kind, expected_gain) in [
        (
            "sub-extreme",
            VOICE_MIC_OVERLOAD_EXTREME_CLIPPED_SAMPLES - 1,
            VoiceMicrophoneOverloadKind::Transient,
            VOICE_MIC_OVERLOAD_TRANSIENT_GAIN,
        ),
        (
            "extreme",
            VOICE_MIC_OVERLOAD_EXTREME_CLIPPED_SAMPLES,
            VoiceMicrophoneOverloadKind::HandlingNoise,
            VOICE_MIC_HANDLING_NOISE_GAIN,
        ),
    ] {
        let mut clipped = vec![0i16; DISCORD_OPUS_20MS_STEREO_SAMPLES];
        clipped[..clipped_samples].fill(i16::MAX);

        let decision = voice_microphone_overload_decision(&clipped)
            .unwrap_or_else(|| panic!("{name} clipped frame should be classified"));
        assert_eq!(decision.kind, expected_kind, "{name}");
        assert_eq!(decision.gain, expected_gain, "{name}");
        assert_eq!(
            voice_microphone_overload_gain(&clipped),
            Some(expected_gain),
            "{name}"
        );
        assert_eq!(
            voice_microphone_clipped_sample_count(&clipped),
            clipped_samples,
            "{name}"
        );
    }
}

#[test]
fn voice_identify_payload_matches_expected_shape() {
    let session = VoiceGatewaySession {
        connection_id: 0,
        scope: VoiceScope::Guild(Id::new(1)),
        channel_id: Id::new(10),
        user_id: Id::new(20),
        session_id: "voice-session".to_owned(),
        endpoint: "voice.example.com".to_owned(),
        token: "voice-token".to_owned(),
    };
    let payload: Value = serde_json::from_str(&voice_identify_payload(&session))
        .expect("voice identify payload is valid JSON");

    assert_eq!(payload["op"].as_u64(), Some(0));
    assert_eq!(payload["d"]["server_id"].as_str(), Some("1"));
    assert_eq!(payload["d"]["user_id"].as_str(), Some("20"));
    assert_eq!(payload["d"]["channel_id"].as_str(), Some("10"));
    assert_eq!(payload["d"]["session_id"].as_str(), Some("voice-session"));
    assert_eq!(payload["d"]["token"].as_str(), Some("voice-token"));
    assert_eq!(
        payload["d"]["max_dave_protocol_version"].as_u64(),
        Some(u64::from(davey::DAVE_PROTOCOL_VERSION))
    );

    let heartbeat: Value = serde_json::from_str(&voice_heartbeat_payload(42))
        .expect("voice heartbeat payload is valid JSON");
    assert_eq!(heartbeat["op"].as_u64(), Some(3));
    assert!(heartbeat["d"]["t"].as_i64().is_some());
    assert_eq!(heartbeat["d"]["seq_ack"].as_i64(), Some(42));

    let resume: Value = serde_json::from_str(&voice_resume_payload(&session, 43))
        .expect("voice resume payload is valid JSON");
    assert_eq!(resume["op"].as_u64(), Some(7));
    assert_eq!(resume["d"]["server_id"].as_str(), Some("1"));
    assert_eq!(resume["d"]["channel_id"].as_str(), Some("10"));
    assert_eq!(resume["d"]["session_id"].as_str(), Some("voice-session"));
    assert_eq!(resume["d"]["token"].as_str(), Some("voice-token"));
    assert_eq!(resume["d"]["seq_ack"].as_i64(), Some(43));

    let mut heartbeat_ack = VoiceHeartbeatAckState::default();
    assert!(heartbeat_ack.mark_sent());
    assert!(!heartbeat_ack.mark_sent());
    heartbeat_ack.mark_acknowledged();
    assert!(heartbeat_ack.mark_sent());
}

#[test]
fn voice_gateway_url_normalizes_endpoint() {
    assert_eq!(
        voice_gateway_url("voice.example.com:2048/").as_deref(),
        Ok("wss://voice.example.com:2048/?v=9")
    );
    assert_eq!(
        voice_gateway_url("wss://voice.example.com").as_deref(),
        Ok("wss://voice.example.com/?v=9")
    );
    assert_eq!(
        voice_gateway_url("https://voice.example.com").as_deref(),
        Ok("wss://voice.example.com/?v=9")
    );
    assert_eq!(
        voice_gateway_url("   /").expect_err("empty endpoint should be rejected"),
        "voice endpoint is empty"
    );
}

#[test]
fn voice_ready_payload_parses_udp_transport_fields() {
    let payload = json!({
        "op": 2,
        "d": {
            "ssrc": 0x01020304u32,
            "ip": "203.0.113.10",
            "port": 50000u64,
            "modes": [
                "aead_xchacha20_poly1305_rtpsize",
                "aead_aes256_gcm_rtpsize"
            ],
        },
    });

    let ready = parse_voice_ready_payload(&payload).expect("ready payload should parse");

    assert_eq!(ready.ssrc, 0x01020304);
    assert_eq!(ready.ip, "203.0.113.10");
    assert_eq!(ready.port, 50000);
    assert_eq!(
        choose_encryption_mode(&ready.modes).as_deref(),
        Ok(AEAD_AES256_GCM_RTPSIZE)
    );
}

#[test]
fn udp_discovery_and_select_protocol_match_expected_shapes() {
    let packet = udp_discovery_request(0x01020304);

    assert_eq!(packet.len(), UDP_DISCOVERY_PACKET_LEN);
    assert_eq!(
        &packet[..8],
        &[0x00, 0x01, 0x00, 0x46, 0x01, 0x02, 0x03, 0x04]
    );
    assert!(packet[8..].iter().all(|byte| *byte == 0));

    let mut response = [0u8; UDP_DISCOVERY_PACKET_LEN];
    response[0..2].copy_from_slice(&2u16.to_be_bytes());
    response[2..4].copy_from_slice(&70u16.to_be_bytes());
    response[4..8].copy_from_slice(&0x01020304u32.to_be_bytes());
    response[8..21].copy_from_slice(b"203.0.113.10\0");
    response[72..74].copy_from_slice(&50000u16.to_be_bytes());

    let discovered = parse_udp_discovery_response(&response, 0x01020304)
        .expect("discovery response should parse");

    assert_eq!(
        discovered,
        DiscoveredVoiceAddress {
            address: "203.0.113.10".to_owned(),
            port: 50000,
        }
    );
    let payload: Value = serde_json::from_str(&voice_select_protocol_payload(
        &discovered,
        AEAD_XCHACHA20_POLY1305_RTPSIZE,
    ))
    .expect("select protocol payload should be valid JSON");

    assert_eq!(payload["op"].as_u64(), Some(1));
    assert_eq!(payload["d"]["protocol"].as_str(), Some("udp"));
    assert_eq!(
        payload["d"]["data"]["address"].as_str(),
        Some("203.0.113.10")
    );
    assert_eq!(payload["d"]["data"]["port"].as_u64(), Some(50000));
    assert_eq!(
        payload["d"]["data"]["mode"].as_str(),
        Some(AEAD_XCHACHA20_POLY1305_RTPSIZE)
    );
}

#[test]
fn udp_ping_uses_documented_magic_and_echoed_sequence() {
    let request = udp_ping_request(0x0102_0304);
    let response = [0x13, 0x37, 0xf0, 0x0d, 0x01, 0x02, 0x03, 0x04];

    assert_eq!(request, [0x13, 0x37, 0xca, 0xfe, 0x01, 0x02, 0x03, 0x04]);
    assert_eq!(parse_udp_ping_response(&response), Some(0x0102_0304));
}

#[tokio::test]
async fn voice_udp_ping_sends_initial_sequence() {
    let receiver = UdpSocket::bind("127.0.0.1:0")
        .await
        .expect("receiver should bind");
    let sender = Arc::new(
        UdpSocket::bind("127.0.0.1:0")
            .await
            .expect("sender should bind"),
    );
    sender
        .connect(
            receiver
                .local_addr()
                .expect("receiver should have an address"),
        )
        .await
        .expect("sender should connect");
    let udp_ping = tokio::spawn(run_voice_udp_ping(sender));
    let mut packet = [0u8; UDP_PING_PACKET_LEN];

    let received = timeout(Duration::from_secs(1), receiver.recv(&mut packet))
        .await
        .expect("UDP ping should arrive")
        .expect("receiver should read the UDP ping");

    udp_ping.abort();
    assert_eq!(received, UDP_PING_PACKET_LEN);
    assert_eq!(packet, udp_ping_request(0));
}

#[test]
fn voice_session_description_parses_mode_and_redacts_secret() {
    let payload = json!({
        "op": 4,
        "d": {
            "audio_codec": "opus",
            "mode": AEAD_XCHACHA20_POLY1305_RTPSIZE,
            "secret_key": (0u8..32).collect::<Vec<_>>(),
            "dave_protocol_version": 1,
            "video_codec": "H264",
            "media_session_id": "media-session-1",
            "keyframe_interval": 1_000,
        },
    });

    let description =
        parse_voice_session_description(&payload).expect("session description should parse");
    let debug = format!("{description:?}");

    assert_eq!(description.audio_codec, "opus");
    assert_eq!(description.mode, AEAD_XCHACHA20_POLY1305_RTPSIZE);
    assert_eq!(description.secret_key.len(), 32);
    assert_eq!(description.dave_protocol_version, Some(1));
    assert_eq!(description.video_codec.as_deref(), Some("H264"));
    assert_eq!(description.media_session_id, "media-session-1");
    assert_eq!(description.keyframe_interval, Some(1_000));
    assert!(debug.contains("<redacted>"));
    assert!(!debug.contains("31"));
}
