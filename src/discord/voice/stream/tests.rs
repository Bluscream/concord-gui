use super::media::GatewayChildTasks;
use super::*;

#[cfg(test)]
pub mod shared {
    use super::*;

    use tokio::sync::oneshot;

    pub struct TaskDropSignal(Option<oneshot::Sender<()>>);

    impl Drop for TaskDropSignal {
        fn drop(&mut self) {
            if let Some(dropped) = self.0.take() {
                let _ = dropped.send(());
            }
        }
    }

    pub fn cancellable_test_task() -> (JoinHandle<()>, oneshot::Receiver<()>, oneshot::Receiver<()>)
    {
        let (started_tx, started_rx) = oneshot::channel();
        let (dropped_tx, dropped_rx) = oneshot::channel();
        let task = tokio::spawn(async move {
            let _drop_signal = TaskDropSignal(Some(dropped_tx));
            let _ = started_tx.send(());
            std::future::pending::<()>().await;
        });
        (task, started_rx, dropped_rx)
    }

    #[tokio::test]
    pub async fn dropping_stream_child_tasks_aborts_every_child() {
        let (heartbeat, heartbeat_started, heartbeat_dropped) = cancellable_test_task();
        let (keepalive, keepalive_started, keepalive_dropped) = cancellable_test_task();
        let (media, media_started, media_dropped) = cancellable_test_task();
        let mut child_tasks = GatewayChildTasks::default();
        child_tasks.replace_heartbeat(heartbeat).await;
        child_tasks.replace_udp_ping(keepalive).await;
        child_tasks.replace_media(media).await;

        heartbeat_started.await.expect("heartbeat test task starts");
        keepalive_started.await.expect("keepalive test task starts");
        media_started.await.expect("media test task starts");
        drop(child_tasks);

        for dropped in [heartbeat_dropped, keepalive_dropped, media_dropped] {
            timeout(Duration::from_secs(1), dropped)
                .await
                .expect("stream child task is aborted promptly")
                .expect("stream child task drop signal is sent");
        }
    }

    #[tokio::test]
    pub async fn dropping_stream_player_log_tasks_aborts_both_readers() {
        let (stdout, stdout_started, stdout_dropped) = cancellable_test_task();
        let (stderr, stderr_started, stderr_dropped) = cancellable_test_task();
        let log_tasks = StreamPlayerLogTasks::new([stdout, stderr]);

        stdout_started.await.expect("stdout test task starts");
        stderr_started.await.expect("stderr test task starts");
        drop(log_tasks);

        for dropped in [stdout_dropped, stderr_dropped] {
            timeout(Duration::from_secs(1), dropped)
                .await
                .expect("stream player log task is aborted promptly")
                .expect("stream player log task drop signal is sent");
        }
    }

    #[test]
    pub fn stream_player_readiness_accepts_only_the_current_media_generation() {
        assert!(stream_player_ready_is_current(Some(8), 8));
        assert!(!stream_player_ready_is_current(Some(7), 8));
        assert!(!stream_player_ready_is_current(None, 8));
    }

    pub fn stream_request() -> StreamWatchRequest {
        StreamWatchRequest {
            stream_key: "guild:10:20:99".to_owned(),
            scope: VoiceScope::Guild(Id::new(10)),
            channel_id: Id::new(20),
            owner_id: Id::new(99),
            display_name: "Streamer".to_owned(),
        }
    }

    pub fn current_voice_state() -> VoiceStateInfo {
        VoiceStateInfo {
            guild_id: Some(Id::new(10)),
            channel_id: Some(Id::new(20)),
            user_id: Id::new(5),
            session_id: Some("parent-session".to_owned()),
            member: None,
            deaf: false,
            mute: false,
            self_deaf: false,
            self_mute: false,
            self_stream: false,
            self_video: false,
        }
    }

    pub fn connected_stream_runtime() -> (StreamRuntimeState, StreamGatewaySession) {
        let mut state = StreamRuntimeState::default();
        state.apply(&VoiceRuntimeEvent::WatchStreamRequested(stream_request()));
        state.apply(&VoiceRuntimeEvent::CurrentUserReady(Some(Id::new(5))));
        state.apply(&VoiceRuntimeEvent::VoiceState(current_voice_state()));
        state.apply(&VoiceRuntimeEvent::StreamCreate(StreamCreateInfo {
            stream_key: "guild:10:20:99".to_owned(),
            rtc_server_id: "400".to_owned(),
            rtc_channel_id: Id::new(401),
            viewer_ids: Vec::new(),
            paused: false,
        }));
        let update = state.apply(&VoiceRuntimeEvent::StreamServer(StreamServerInfo {
            stream_key: "guild:10:20:99".to_owned(),
            endpoint: Some("stream.example.com".to_owned()),
            token: "stream-token".to_owned(),
        }));
        let session = update.connect.expect("stream session should be ready");
        (state, session)
    }

    #[test]
    pub fn stream_gateway_session_debug_redacts_token() {
        let (_, session) = connected_stream_runtime();
        let debug = format!("{session:?}");

        assert!(!debug.contains("stream-token"));
        assert!(debug.contains("<redacted>"));
    }

    pub fn stream_connection_ended(
        session: &StreamGatewaySession,
        outcome: VoiceConnectionEnd,
    ) -> VoiceRuntimeEvent {
        VoiceRuntimeEvent::StreamConnectionEnded {
            connection_id: session.connection_id,
            stream_key: session.request.stream_key.clone(),
            outcome,
        }
    }

    #[test]
    pub fn h264_fu_a_round_trips_through_local_packetizer() {
        let frame = [0, 0, 0, 1, 0x65]
            .into_iter()
            .chain(std::iter::repeat_n(0xaa, 3000))
            .collect::<Vec<_>>();
        let mut sequence = 7;
        let packets = packetize_h264_frame(&frame, 90_000, 42, &mut sequence);
        assert!(packets.len() > 1);

        let mut depacketizer = H264Depacketizer::default();
        let mut decoded = None;
        for packet in packets {
            let header = parse_rtp_header(&packet).expect("local RTP packet is valid");
            if let H264DepacketizerOutput::Frame(frame) =
                depacketizer.push(&header, &packet[header.payload_offset..])
            {
                decoded = Some(frame);
            }
        }
        assert_eq!(decoded, Some(frame));
    }

    #[test]
    pub fn h264_assembly_enforces_packet_and_memory_budgets() {
        let mut oversized = H264Depacketizer::default();
        let mut oversized_header =
            stream_video_header(DISCORD_STREAM_VIDEO_PAYLOAD_TYPE, 1, 90_000, 42);
        oversized_header.marker = false;
        let mut oversized_payload = vec![0xaa; STREAM_H264_ACCESS_UNIT_MAX_BYTES + 1];
        oversized_payload[0] = 0x7c;
        oversized_payload[1] = 0x85;
        assert!(matches!(
            oversized.push(&oversized_header, &oversized_payload),
            H264DepacketizerOutput::BudgetExceeded
        ));

        let mut fragmented = H264Depacketizer::default();
        let mut fragment_header =
            stream_video_header(DISCORD_STREAM_VIDEO_PAYLOAD_TYPE, 1, 93_000, 42);
        fragment_header.marker = false;
        assert!(matches!(
            fragmented.push(&fragment_header, &[0x7c, 0x85, 0xaa]),
            H264DepacketizerOutput::Pending
        ));
        for sequence in 2..=STREAM_H264_ACCESS_UNIT_MAX_PACKETS {
            fragment_header.sequence = u16::try_from(sequence).expect("packet budget fits u16");
            assert!(matches!(
                fragmented.push(&fragment_header, &[0x7c, 0x05, 0xaa]),
                H264DepacketizerOutput::Pending
            ));
        }
        fragment_header.sequence =
            u16::try_from(STREAM_H264_ACCESS_UNIT_MAX_PACKETS + 1).expect("packet budget fits u16");
        assert!(matches!(
            fragmented.push(&fragment_header, &[0x7c, 0x45, 0xaa]),
            H264DepacketizerOutput::BudgetExceeded
        ));

        let mut startup = H264StartupBuffer::default();
        for timestamp in 0..32 {
            assert!(startup.push(BufferedH264Frame {
                encoded: vec![0; 1024 * 1024],
                source_timestamp: timestamp,
            }));
        }
        assert!(!startup.push(BufferedH264Frame {
            encoded: vec![0],
            source_timestamp: 32,
        }));
        assert!(startup.is_empty());
        assert_eq!(startup.bytes, 0);
    }

    pub fn stream_video_header(
        payload_type: u8,
        sequence: u16,
        timestamp: u32,
        ssrc: u32,
    ) -> RtpHeader {
        RtpHeader {
            has_padding: false,
            marker: true,
            payload_type,
            sequence,
            timestamp,
            ssrc,
            authenticated_header_len: RTP_HEADER_MIN_LEN,
            encrypted_extension_body_len: 0,
            payload_offset: RTP_HEADER_MIN_LEN,
        }
    }

    #[test]
    pub fn stream_video_reorders_primary_and_rtx_packets_before_depacketization() {
        let source = StreamVideoSource {
            audio_ssrc: 7,
            video_ssrc: 42,
            rtx_ssrc: Some(43),
            pixel_count: None,
        };
        let now = Instant::now();
        let mut recovery = StreamVideoRecovery::default();

        let first_payload = b"first".to_vec();
        let first_payload_ptr = first_payload.as_ptr();
        let first = recover_stream_video_packet(
            stream_video_header(DISCORD_STREAM_VIDEO_PAYLOAD_TYPE, 10, 90_000, 42),
            first_payload,
            source,
        )
        .expect("primary video packet should be accepted");
        assert_eq!(first.payload.as_ptr(), first_payload_ptr);
        assert_eq!(
            recovery
                .push(first, now)
                .ready
                .into_iter()
                .map(|packet| packet.header.sequence)
                .collect::<Vec<_>>(),
            vec![10]
        );

        let third = recover_stream_video_packet(
            stream_video_header(DISCORD_STREAM_VIDEO_PAYLOAD_TYPE, 12, 90_000, 42),
            b"third".to_vec(),
            source,
        )
        .expect("later primary video packet should be accepted");
        assert!(recovery.push(third, now).ready.is_empty());
        assert_eq!(recovery.take_nack_if_due(now), Some(vec![11]));

        let repaired = recover_stream_video_packet(
            stream_video_header(DISCORD_STREAM_VIDEO_RTX_PAYLOAD_TYPE, 800, 90_000, 43),
            vec![0, 11, b's', b'e', b'c', b'o', b'n', b'd'],
            source,
        )
        .expect("RTX video packet should recover its original packet");
        assert_eq!(repaired.header.sequence, 11);
        assert_eq!(repaired.header.ssrc, source.video_ssrc);
        assert_eq!(repaired.payload, b"second");
        assert_eq!(
            recovery
                .push(repaired, now)
                .ready
                .into_iter()
                .map(|packet| packet.header.sequence)
                .collect::<Vec<_>>(),
            vec![11, 12]
        );
        assert_eq!(recovery.take_nack_if_due(now), None);
    }

    #[test]
    pub fn stream_video_recovery_keeps_waiting_beyond_the_old_packet_window() {
        let now = Instant::now();
        let mut recovery = StreamVideoRecovery::default();
        let packet = |sequence, payload| RecoveredStreamVideoPacket {
            header: stream_video_header(DISCORD_STREAM_VIDEO_PAYLOAD_TYPE, sequence, 90_000, 42),
            payload,
        };

        assert_eq!(recovery.push(packet(100, vec![1]), now).ready.len(), 1);
        assert!(recovery.push(packet(102, vec![2, 3]), now).ready.is_empty());
        let update = recovery.push(packet(230, vec![4]), now + Duration::from_millis(25));

        assert!(update.reset.is_none());
        assert!(update.ready.is_empty());
        assert_eq!(recovery.pending.len(), 2);
        assert_eq!(recovery.pending_bytes, 3);
    }

    #[test]
    pub fn stream_video_recovery_expires_an_unrepaired_gap() {
        let now = Instant::now();
        let mut recovery = StreamVideoRecovery::default();
        let first = RecoveredStreamVideoPacket {
            header: stream_video_header(DISCORD_STREAM_VIDEO_PAYLOAD_TYPE, 20, 90_000, 42),
            payload: vec![1],
        };
        let third = RecoveredStreamVideoPacket {
            header: stream_video_header(DISCORD_STREAM_VIDEO_PAYLOAD_TYPE, 22, 90_000, 42),
            payload: vec![3],
        };

        assert_eq!(recovery.push(first, now).ready.len(), 1);
        assert!(recovery.push(third, now).ready.is_empty());
        assert_eq!(
            recovery.take_expired_gap(now + STREAM_VIDEO_GAP_TIMEOUT / 2),
            None
        );
        assert_eq!(
            recovery.take_expired_gap(now + STREAM_VIDEO_GAP_TIMEOUT),
            Some(StreamVideoRecoveryReset {
                distance: 1,
                pending_packets: 1,
                pending_bytes: 1,
                gap_age: Some(STREAM_VIDEO_GAP_TIMEOUT),
            })
        );
        assert!(recovery.pending.is_empty());
        assert_eq!(recovery.pending_bytes, 0);
    }

    #[test]
    pub fn stream_video_recovery_resets_only_when_a_pending_budget_is_exceeded() {
        let now = Instant::now();
        let packet = |sequence, payload| RecoveredStreamVideoPacket {
            header: stream_video_header(DISCORD_STREAM_VIDEO_PAYLOAD_TYPE, sequence, 90_000, 42),
            payload,
        };

        let mut packet_limited = StreamVideoRecovery::default();
        assert_eq!(packet_limited.push(packet(0, vec![0]), now).ready.len(), 1);
        for sequence in 2..=u16::try_from(STREAM_VIDEO_MAX_PENDING_PACKETS + 1)
            .expect("video packet budget fits u16")
        {
            assert!(
                packet_limited
                    .push(packet(sequence, vec![0]), now)
                    .reset
                    .is_none()
            );
        }
        let packet_reset = packet_limited.push(
            packet(
                u16::try_from(STREAM_VIDEO_MAX_PENDING_PACKETS + 2)
                    .expect("video packet budget fits u16"),
                vec![0],
            ),
            now,
        );
        assert_eq!(
            packet_reset.reset,
            Some(StreamVideoRecoveryReset {
                distance: u16::try_from(STREAM_VIDEO_MAX_PENDING_PACKETS + 1)
                    .expect("video packet budget fits u16"),
                pending_packets: STREAM_VIDEO_MAX_PENDING_PACKETS + 1,
                pending_bytes: STREAM_VIDEO_MAX_PENDING_PACKETS + 1,
                gap_age: Some(Duration::ZERO),
            })
        );

        let mut byte_limited = StreamVideoRecovery::default();
        assert_eq!(byte_limited.push(packet(0, vec![0]), now).ready.len(), 1);
        assert!(
            byte_limited
                .push(packet(2, vec![0; STREAM_VIDEO_MAX_PENDING_BYTES]), now)
                .reset
                .is_none()
        );
        let byte_reset = byte_limited.push(packet(3, vec![0]), now);
        assert_eq!(
            byte_reset.reset,
            Some(StreamVideoRecoveryReset {
                distance: 2,
                pending_packets: 2,
                pending_bytes: STREAM_VIDEO_MAX_PENDING_BYTES + 1,
                gap_age: Some(Duration::ZERO),
            })
        );
    }
}
