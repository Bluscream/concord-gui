use super::super::dave::VoiceDaveOutboundPayload;
use super::super::opus::VoicePlaybackDecodeState;
use super::*;
use test_codec::assert_voice_sample_near;
use test_rtp::test_voice_gateway_session;
use tokio::sync::mpsc;

#[test]
fn voice_session_description_reuses_only_the_same_transport_key_and_mode() {
    let current = VoiceSessionDescription {
        audio_codec: "opus".to_owned(),
        mode: "aead_xchacha20_poly1305_rtpsize".to_owned(),
        secret_key: vec![1, 2, 3],
        dave_protocol_version: Some(1),
        video_codec: None,
        media_session_id: "media-session".to_owned(),
        keyframe_interval: None,
    };

    for (next, expected) in [
        (
            VoiceSessionDescription {
                dave_protocol_version: Some(2),
                ..current.clone()
            },
            true,
        ),
        (
            VoiceSessionDescription {
                secret_key: vec![4, 5, 6],
                ..current.clone()
            },
            false,
        ),
        (
            VoiceSessionDescription {
                audio_codec: "opus".to_owned(),
                mode: "aead_aes256_gcm_rtpsize".to_owned(),
                ..current.clone()
            },
            false,
        ),
    ] {
        assert_eq!(current.uses_same_transport(&next), expected, "{next:?}");
    }
}

#[test]
fn voice_dave_state_tracks_speaking_ssrc_mapping() {
    let session = VoiceGatewaySession {
        connection_id: 0,
        scope: VoiceScope::Guild(Id::new(1)),
        channel_id: Id::new(10),
        user_id: Id::new(20),
        session_id: "voice-session".to_owned(),
        endpoint: "voice.example.com".to_owned(),
        token: "voice-token".to_owned(),
    };
    let mut state = VoiceDaveState::new(&session);

    state.record_speaking_state(VoiceSpeakingState {
        user_id: Some(30),
        ssrc: Some(1234),
        speaking: Some(1),
    });

    assert_eq!(state.ssrc_user_ids.get(&1234), Some(&30));
    assert_eq!(state.user_id_for_ssrc(1234), Some(Id::new(30)));
    assert_eq!(state.user_id_for_ssrc(9999), None);
    assert!(state.known_user_ids.contains(&30));
}

#[test]
fn voice_dave_active_drops_non_dave_payloads() {
    let session = test_voice_gateway_session();
    let mut state = VoiceDaveState::new(&session);
    state.reinit(1).expect("DAVE session should initialize");

    assert_eq!(
        state.unwrap_media_payload_for_ssrc(1234, b"plain-opus"),
        VoiceMediaPayload::DaveUnexpectedPlain { payload_len: 10 }
    );
}

#[test]
fn voice_speaking_uses_microphone_bit_only() {
    assert!(!voice_speaking_microphone_active(0));
    assert!(voice_speaking_microphone_active(1));
    assert!(!voice_speaking_microphone_active(2));
    assert!(voice_speaking_microphone_active(5));
}

#[test]
fn voice_speaking_tracker_keeps_local_and_remote_activity_separate() {
    let remote_user = Id::new(30);
    let local_user = Id::new(20);
    let mut tracker = VoiceSpeakingTracker::new(local_user);
    let now = Instant::now();

    assert_eq!(tracker.record_remote(local_user, true, now), None);
    assert!(tracker.remote_deadlines.is_empty());
    assert_eq!(tracker.record_remote(remote_user, true, now), Some(true));
    assert_eq!(
        tracker.record_remote(remote_user, true, now + VOICE_REMOTE_SPEAKING_TTL / 2),
        None
    );
    assert!(
        tracker
            .expire_remote(now + VOICE_REMOTE_SPEAKING_TTL)
            .is_empty()
    );
    assert_eq!(
        tracker.expire_remote(now + VOICE_REMOTE_SPEAKING_TTL + VOICE_REMOTE_SPEAKING_TTL / 2),
        vec![remote_user]
    );
    assert_eq!(tracker.record_remote(remote_user, false, now), None);
    assert_eq!(tracker.record_remote(remote_user, true, now), Some(true));
    assert_eq!(tracker.record_remote(remote_user, false, now), Some(false));

    assert_eq!(tracker.record_local(true), Some(true));
    assert_eq!(tracker.record_local(true), None);
    assert_eq!(tracker.clear_all(), vec![local_user]);
}

#[cfg(feature = "voice-playback")]
#[test]
fn local_speaking_follows_microphone_activity_and_emits_only_edges() {
    let quiet = vec![100i16; DISCORD_OPUS_20MS_STEREO_SAMPLES];
    let normal = vec![1500i16; DISCORD_OPUS_20MS_STEREO_SAMPLES];
    let voice_activity_gate = VoiceCaptureGate {
        capture_enabled: true,
        transmit_enabled: true,
        use_voice_activity: true,
        noise_suppression: false,
        microphone_sensitivity: MicrophoneSensitivityDb::default(),
        microphone_volume: VoiceVolumePercent::default(),
    };
    assert!(voice_microphone_frame_is_active(
        voice_activity_gate,
        &mut VoiceMicrophoneGateState::default(),
        &normal,
    ));
    assert!(!voice_microphone_frame_is_active(
        voice_activity_gate,
        &mut VoiceMicrophoneGateState::default(),
        &quiet,
    ));
    assert!(voice_microphone_frame_is_active(
        VoiceCaptureGate {
            use_voice_activity: false,
            ..voice_activity_gate
        },
        &mut VoiceMicrophoneGateState::default(),
        &quiet,
    ));
    assert!(!voice_microphone_frame_is_active(
        VoiceCaptureGate {
            transmit_enabled: false,
            ..voice_activity_gate
        },
        &mut VoiceMicrophoneGateState::default(),
        &normal,
    ));

    let (speaking_tx, mut speaking_rx) = mpsc::unbounded_channel();
    let mut local_speaking = false;

    publish_local_speaking_edge(&speaking_tx, &mut local_speaking, false);
    publish_local_speaking_edge(&speaking_tx, &mut local_speaking, true);
    publish_local_speaking_edge(&speaking_tx, &mut local_speaking, true);
    publish_local_speaking_edge(&speaking_tx, &mut local_speaking, false);
    publish_local_speaking_edge(&speaking_tx, &mut local_speaking, false);

    assert_eq!(speaking_rx.try_recv(), Ok(true));
    assert_eq!(speaking_rx.try_recv(), Ok(false));
    assert_eq!(
        speaking_rx.try_recv(),
        Err(mpsc::error::TryRecvError::Empty)
    );
}

#[test]
fn voice_dave_outbound_opus_fails_closed_unless_ready() {
    let mut state = VoiceDaveState::new(&test_voice_gateway_session());

    assert_eq!(
        state.prepare_outbound_opus(b"opus-frame"),
        VoiceDaveOutboundPayload::Plain(b"opus-frame".to_vec())
    );

    state.protocol_version = NonZeroU16::new(1);
    assert_eq!(
        state.prepare_outbound_opus(b"opus-frame"),
        VoiceDaveOutboundPayload::Blocked(VoiceOutboundSendBlockReason::DaveOutboundMissingSession)
    );

    state.reinit(1).expect("DAVE session should initialize");
    assert_eq!(
        state.prepare_outbound_opus(b"opus-frame"),
        VoiceDaveOutboundPayload::Blocked(VoiceOutboundSendBlockReason::DaveOutboundNotReady)
    );

    state.reinit(0).expect("DAVE should disable cleanly");
    assert_eq!(
        state.prepare_outbound_opus(b"opus-frame"),
        VoiceDaveOutboundPayload::Plain(b"opus-frame".to_vec())
    );
}

#[test]
fn dave_media_detection_requires_magic_marker() {
    assert!(!looks_like_dave_media_frame(b"opus-frame"));

    let mut payload = vec![0u8; DAVE_MIN_SUPPLEMENTAL_BYTES];
    let marker_start = payload.len() - DAVE_MAGIC_MARKER.len();
    payload[marker_start..].copy_from_slice(&DAVE_MAGIC_MARKER);

    assert!(looks_like_dave_media_frame(&payload));
}

#[test]
fn voice_playback_frame_uses_only_playable_media_payloads() {
    let header = RtpHeader {
        has_padding: false,
        marker: false,
        payload_type: DISCORD_VOICE_PAYLOAD_TYPE,
        sequence: 7,
        timestamp: 8,
        ssrc: 9,
        authenticated_header_len: 12,
        encrypted_extension_body_len: 0,
        payload_offset: 12,
    };
    let mapped_user_id = Id::new(41);

    assert_eq!(
        voice_playback_frame(
            &VoiceMediaPayload::Plain(b"opus".to_vec()),
            &header,
            Some(mapped_user_id),
        ),
        Some(VoicePlaybackFrame {
            ssrc: 9,
            user_id: Some(mapped_user_id),
            sequence: 7,
            timestamp: 8,
            opus: b"opus".to_vec(),
        })
    );
    assert_eq!(
        voice_playback_frame(
            &VoiceMediaPayload::Plain(b"unmapped-opus".to_vec()),
            &header,
            None,
        ),
        Some(VoicePlaybackFrame {
            ssrc: 9,
            user_id: None,
            sequence: 7,
            timestamp: 8,
            opus: b"unmapped-opus".to_vec(),
        })
    );
    assert_eq!(
        voice_playback_frame(
            &VoiceMediaPayload::DaveDecrypted {
                user_id: 42,
                opus: b"dave-opus".to_vec(),
            },
            &header,
            Some(mapped_user_id),
        ),
        Some(VoicePlaybackFrame {
            ssrc: 9,
            user_id: Some(Id::new(42)),
            sequence: 7,
            timestamp: 8,
            opus: b"dave-opus".to_vec(),
        })
    );
    assert_eq!(
        voice_playback_frame(
            &VoiceMediaPayload::DaveUnexpectedPlain { payload_len: 4 },
            &header,
            Some(mapped_user_id),
        ),
        None
    );
    assert_eq!(
        voice_playback_frame(
            &VoiceMediaPayload::DaveMissingUser { payload_len: 4 },
            &header,
            Some(mapped_user_id),
        ),
        None
    );
}

fn test_playback_frame(ssrc: u32, user_id: Option<u64>, sequence: u16) -> VoicePlaybackFrame {
    test_playback_frame_with_timestamp(
        ssrc,
        user_id,
        sequence,
        u32::from(sequence) * DISCORD_OPUS_TIMESTAMP_INCREMENT,
    )
}

fn test_playback_frame_with_timestamp(
    ssrc: u32,
    user_id: Option<u64>,
    sequence: u16,
    timestamp: u32,
) -> VoicePlaybackFrame {
    VoicePlaybackFrame {
        ssrc,
        user_id: user_id.map(Id::new),
        sequence,
        timestamp,
        opus: vec![sequence as u8],
    }
}

#[test]
fn voice_playout_buffer_reorders_nearby_packets() {
    let now = Instant::now();
    let mut buffer = VoicePlaybackPlayoutBuffer::default();

    assert!(buffer.push(test_playback_frame(9, Some(42), 12), now));
    assert!(buffer.push(test_playback_frame(9, Some(42), 10), now));
    assert_eq!(buffer.next_frame(now), None);
    assert!(buffer.push(test_playback_frame(9, Some(42), 11), now));

    assert_eq!(
        buffer.next_frame(now + VOICE_PLAYBACK_FRAME_DURATION),
        Some(VoicePlayoutFrame::Audio(test_playback_frame(
            9,
            Some(42),
            10
        )))
    );
    assert_eq!(
        buffer.next_frame(now + VOICE_PLAYBACK_FRAME_DURATION * 2),
        Some(VoicePlayoutFrame::Audio(test_playback_frame(
            9,
            Some(42),
            11
        )))
    );
    assert_eq!(
        buffer.next_frame(now + VOICE_PLAYBACK_FRAME_DURATION * 3),
        Some(VoicePlayoutFrame::Audio(test_playback_frame(
            9,
            Some(42),
            12
        )))
    );
}

#[test]
fn voice_playout_buffer_schedules_packets_by_rtp_timestamp_delta() {
    struct Case {
        name: &'static str,
        timestamp_step: u32,
        step_duration: Duration,
        early_duration: Duration,
    }

    for case in [
        Case {
            name: "20ms Discord packet",
            timestamp_step: DISCORD_OPUS_TIMESTAMP_INCREMENT,
            step_duration: VOICE_PLAYBACK_FRAME_DURATION,
            early_duration: Duration::from_millis(10),
        },
        Case {
            name: "10ms Abaddon packet",
            timestamp_step: 480,
            step_duration: Duration::from_millis(10),
            early_duration: Duration::from_millis(5),
        },
    ] {
        let now = Instant::now();
        let playout_start = now + VOICE_PLAYBACK_JITTER_BUFFER_DELAY;
        let mut buffer = VoicePlaybackPlayoutBuffer::default();
        let timestamps = [
            case.timestamp_step * 10,
            case.timestamp_step * 11,
            case.timestamp_step * 12,
        ];

        assert!(buffer.push(
            test_playback_frame_with_timestamp(9, Some(42), 10, timestamps[0]),
            now
        ));
        assert!(buffer.push(
            test_playback_frame_with_timestamp(9, Some(42), 11, timestamps[1]),
            now
        ));
        assert!(buffer.push(
            test_playback_frame_with_timestamp(9, Some(42), 12, timestamps[2]),
            now
        ));

        assert_eq!(
            buffer.next_frame(playout_start),
            Some(VoicePlayoutFrame::Audio(
                test_playback_frame_with_timestamp(9, Some(42), 10, timestamps[0])
            )),
            "{} should emit the first frame at playout start",
            case.name
        );
        assert_eq!(
            buffer.next_frame(playout_start + case.early_duration),
            None,
            "{} should wait for the RTP timestamp delta",
            case.name
        );
        assert_eq!(
            buffer.next_frame(playout_start + case.step_duration),
            Some(VoicePlayoutFrame::Audio(
                test_playback_frame_with_timestamp(9, Some(42), 11, timestamps[1])
            )),
            "{} should emit the second frame after its timestamp delta",
            case.name
        );
        assert_eq!(
            buffer.next_frame(playout_start + case.step_duration * 2),
            Some(VoicePlayoutFrame::Audio(
                test_playback_frame_with_timestamp(9, Some(42), 12, timestamps[2])
            )),
            "{} should keep the same timestamp cadence",
            case.name
        );
    }
}

#[test]
fn voice_playout_buffer_emits_packet_loss_for_missing_sequence() {
    let cases = [
        (
            "20ms",
            DISCORD_OPUS_TIMESTAMP_INCREMENT,
            VOICE_PLAYBACK_FRAME_DURATION,
        ),
        ("10ms", 480, Duration::from_millis(10)),
    ];

    for (name, timestamp_step, step_duration) in cases {
        let now = Instant::now();
        let playout_start = now + VOICE_PLAYBACK_JITTER_BUFFER_DELAY;
        let mut buffer = VoicePlaybackPlayoutBuffer::default();
        let first = test_playback_frame_with_timestamp(9, Some(42), 10, timestamp_step * 10);
        let third = test_playback_frame_with_timestamp(9, Some(42), 12, timestamp_step * 12);

        assert!(buffer.push(first.clone(), now));
        assert!(buffer.push(third.clone(), now));
        assert!(buffer.push(
            test_playback_frame_with_timestamp(9, Some(42), 13, timestamp_step * 13),
            now
        ));

        assert_eq!(
            buffer.next_frame(playout_start),
            Some(VoicePlayoutFrame::Audio(first)),
            "{name}"
        );
        assert_eq!(
            buffer.next_frame(playout_start + step_duration),
            Some(VoicePlayoutFrame::PacketLoss {
                ssrc: 9,
                user_id: Some(Id::new(42)),
                sequence: 11,
                timestamp_step,
            }),
            "{name}"
        );
        assert_eq!(
            buffer.next_frame(playout_start + step_duration * 2),
            Some(VoicePlayoutFrame::Audio(third)),
            "{name}"
        );
    }
}

#[test]
fn voice_playout_buffer_drops_stale_packets_after_playout_advances() {
    let now = Instant::now();
    let mut buffer = VoicePlaybackPlayoutBuffer::default();

    assert!(buffer.push(test_playback_frame(9, Some(42), 7), now));
    assert!(buffer.push(test_playback_frame(9, Some(42), 8), now));
    assert!(buffer.push(test_playback_frame(9, Some(42), 9), now));
    assert_eq!(
        buffer.next_frame(now + VOICE_PLAYBACK_FRAME_DURATION),
        Some(VoicePlayoutFrame::Audio(test_playback_frame(
            9,
            Some(42),
            7
        )))
    );
    assert_eq!(
        buffer.next_frame(now + VOICE_PLAYBACK_FRAME_DURATION * 2),
        Some(VoicePlayoutFrame::Audio(test_playback_frame(
            9,
            Some(42),
            8
        )))
    );

    assert!(!buffer.push(test_playback_frame(9, Some(42), 7), now));
}

#[test]
fn voice_decoded_samples_mix_same_tick_frames() {
    let mixed =
        mix_voice_decoded_samples(&[vec![0.5, 0.25, -0.5, -0.25], vec![0.5, -0.25, 0.5, -0.75]])
            .expect("same-tick decoded frames should mix");
    let gain = 1.0 / 2.0f32.sqrt();

    assert_voice_sample_near(mixed[0], 1.0 * gain);
    assert_voice_sample_near(mixed[1], 0.0);
    assert_voice_sample_near(mixed[2], 0.0);
    assert_voice_sample_near(mixed[3], -gain);
}

#[test]
fn voice_decode_state_outputs_one_poll_quantum_per_mix() {
    let mut state = VoicePlaybackDecodeState::default();
    let poll_samples =
        VOICE_PLAYBACK_POLL_SAMPLES_PER_CHANNEL * usize::from(DISCORD_VOICE_CHANNELS);

    state.push_decoded_samples(1, vec![1.0; poll_samples * 2]);
    state.push_decoded_samples(2, vec![0.5; poll_samples]);

    let first = state
        .next_pending_mix()
        .expect("first poll should mix pending samples");
    let second = state
        .next_pending_mix()
        .expect("second poll should drain 20ms frame remainder");

    assert_eq!(first.len(), poll_samples);
    assert_eq!(second.len(), poll_samples);
    assert!(state.next_pending_mix().is_none());
}
