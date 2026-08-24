use std::net::Ipv4Addr;

use super::media::build_rtcp_sender_report;

#[cfg(test)]
mod cases {
    use super::super::*;
    use super::*;
    use tokio::time::timeout;

    #[test]
    pub fn stream_generic_nack_groups_missing_sequences_into_pid_and_bitmask() {
        let nack = build_rtcp_nack(7, 42, &[1_000, 1_001, 1_003, 1_020]);

        assert_eq!(&nack[..4], &[0x81, 205, 0, 4]);
        assert_eq!(&nack[4..8], &7u32.to_be_bytes());
        assert_eq!(&nack[8..12], &42u32.to_be_bytes());
        assert_eq!(&nack[12..16], &[0x03, 0xe8, 0, 5]);
        assert_eq!(&nack[16..20], &[0x03, 0xfc, 0, 0]);
    }

    #[test]
    pub fn stream_pli_throttle_limits_the_active_video_source_to_one_request_per_second() {
        let now = Instant::now();
        let mut throttle = StreamPliThrottle::default();

        assert!(throttle.permit(42, now));
        assert!(!throttle.permit(
            42,
            now + STREAM_KEYFRAME_REQUEST_INTERVAL - Duration::from_millis(1)
        ));
        assert!(throttle.permit(42, now + STREAM_KEYFRAME_REQUEST_INTERVAL));
        assert!(throttle.permit(
            43,
            now + STREAM_KEYFRAME_REQUEST_INTERVAL + Duration::from_millis(1)
        ));
    }

    #[test]
    pub fn stream_transport_sequence_parses_native_one_byte_extensions() {
        let extensions = [0x30, 0xaa, 0, 0x51, 0x12, 0x34, 0];

        assert_eq!(
            parse_stream_transport_sequence(Some(RTP_ONE_BYTE_EXTENSION_PROFILE), &extensions),
            Some(0x1234)
        );
        assert_eq!(
            parse_stream_transport_sequence(
                Some(RTP_ONE_BYTE_EXTENSION_PROFILE),
                &[0x53, 0x12, 0x34, 0x80, 0x10],
            ),
            Some(0x1234)
        );
        assert_eq!(
            parse_stream_transport_sequence(Some(0x1000), &extensions),
            None
        );
        assert_eq!(
            parse_stream_transport_sequence(Some(RTP_ONE_BYTE_EXTENSION_PROFILE), &[0x50, 0x12],),
            None
        );
    }

    #[test]
    pub fn stream_transport_feedback_reports_loss_arrival_deltas_and_wrap() {
        assert_eq!(
            extend_transport_sequence(0, Some(u32::from(u16::MAX))),
            1 << 16
        );
        assert_eq!(
            extend_transport_sequence(u16::MAX, Some(1 << 16)),
            u32::from(u16::MAX)
        );

        let mut feedback = StreamTransportFeedback::default();
        feedback.observe(u16::MAX - 1, Duration::from_millis(64));
        feedback.observe(u16::MAX, Duration::from_micros(64_250));
        feedback.observe(1, Duration::from_millis(65));

        let packet = feedback
            .take_feedback(7, 42)
            .expect("received transport packets should produce feedback");
        assert_eq!(&packet[..4], &[0x8f, RTCP_TRANSPORT_LAYER_FEEDBACK, 0, 6]);
        assert_eq!(&packet[4..8], &7u32.to_be_bytes());
        assert_eq!(&packet[8..12], &42u32.to_be_bytes());
        assert_eq!(&packet[12..14], &(u16::MAX - 1).to_be_bytes());
        assert_eq!(&packet[14..16], &4u16.to_be_bytes());
        assert_eq!(&packet[16..19], &[0, 0, 1]);
        assert_eq!(packet[19], 0);
        assert_eq!(&packet[20..22], &0xd440u16.to_be_bytes());
        assert_eq!(&packet[22..25], &[0, 1, 3]);
        assert_eq!(&packet[25..], &[0, 0, 0]);
        assert!(feedback.take_feedback(7, 42).is_none());

        let compound = build_stream_rtcp_compound(7, None, Some(&packet));
        let key = [0x42; 32];
        let encryptor = VoiceRtpEncryptor::new(AEAD_AES256_GCM_RTPSIZE, &key)
            .expect("transport feedback encryptor should initialize");
        let encrypted = encryptor
            .encrypt_rtcp_feedback(&compound, 11u32.to_be_bytes())
            .expect("transport feedback should encrypt as compound RTCP");
        let decryptor = VoiceRtpDecryptor::new(AEAD_AES256_GCM_RTPSIZE, &key)
            .expect("transport feedback decryptor should initialize");
        assert_eq!(
            decryptor
                .decrypt_rtcp_feedback(&encrypted)
                .expect("transport feedback should decrypt"),
            compound
        );

        let mut reordered = StreamTransportFeedback::default();
        reordered.observe(11, Duration::from_millis(90));
        reordered.observe(10, Duration::from_millis(100));
        let packet = reordered
            .take_feedback(7, 42)
            .expect("reordered transport packets should produce feedback");
        assert_eq!(&packet[12..16], &[0, 10, 0, 2]);
        assert_eq!(&packet[20..22], &0xd800u16.to_be_bytes());
        assert_eq!(packet[22], 144);
        assert_eq!(&packet[23..25], &(-40i16).to_be_bytes());
    }

    #[test]
    pub fn stream_video_starts_at_idr_with_cached_parameter_sets() {
        let parameter_sets = vec![0, 0, 0, 1, 0x67, 0x11, 0, 0, 0, 1, 0x68, 0x22];
        let predicted = vec![0, 0, 0, 1, 0x41, 0x33];
        let idr = vec![0, 0, 0, 1, 0x65, 0x44];
        let mut gate = H264StartupGate::default();

        assert_eq!(gate.accept(parameter_sets), None);
        assert_eq!(gate.accept(predicted.clone()), None);

        let startup = gate
            .accept(idr)
            .expect("IDR should start local video playback");
        assert_eq!(h264_nal_types(&startup), vec![7, 8, 5]);
        assert!(gate.is_started());
        assert_eq!(gate.accept(predicted.clone()), Some(predicted));
    }

    #[test]
    pub fn stream_video_waits_for_parameter_sets_before_accepting_idr() {
        let idr = vec![0, 0, 0, 1, 0x65, 0x44];
        let parameter_sets = vec![0, 0, 0, 1, 0x67, 0x11, 0, 0, 0, 1, 0x68, 0x22];
        let mut gate = H264StartupGate::default();

        assert_eq!(gate.accept(idr.clone()), None);
        assert!(!gate.is_started());
        assert_eq!(gate.accept(parameter_sets), None);

        let startup = gate
            .accept(idr)
            .expect("IDR should start after parameter sets arrive");
        assert_eq!(h264_nal_types(&startup), vec![7, 8, 5]);
        assert!(gate.is_started());
    }

    #[test]
    pub fn stream_video_replays_the_initial_gop_after_player_readiness() {
        let startup_frame = vec![
            0, 0, 0, 1, 0x67, 0x11, 0, 0, 0, 1, 0x68, 0x22, 0, 0, 0, 1, 0x65, 0x33,
        ];
        let predicted = vec![0, 0, 0, 1, 0x41, 0x44];
        let mut gate = H264StartupGate::default();
        let mut buffer = H264StartupBuffer::default();

        assert_eq!(
            accept_or_buffer_h264(false, &mut gate, &mut buffer, startup_frame.clone(), 90_000,)
                .map(|frame| frame.encoded),
            None
        );
        assert!(gate.is_started());
        assert_eq!(buffer.len(), 1);
        assert_eq!(
            accept_or_buffer_h264(false, &mut gate, &mut buffer, predicted.clone(), 93_000,)
                .map(|frame| frame.encoded),
            None
        );
        assert_eq!(buffer.len(), 2);
        assert_eq!(
            buffer
                .frames
                .pop_front()
                .map(|frame| (frame.encoded, frame.source_timestamp)),
            Some((startup_frame, 90_000))
        );
        assert_eq!(
            buffer
                .frames
                .pop_front()
                .map(|frame| (frame.encoded, frame.source_timestamp)),
            Some((predicted.clone(), 93_000))
        );
        assert_eq!(
            accept_or_buffer_h264(true, &mut gate, &mut buffer, predicted.clone(), 96_000)
                .map(|frame| (frame.encoded, frame.source_timestamp)),
            Some((predicted, 96_000))
        );

        let mut clock = LocalRtpClock::default();
        clock.anchor(93_000, 10_000);
        assert_eq!(
            clock.rebase(96_000, Duration::from_secs(10), VIDEO_RTP_CLOCK_RATE, None,),
            13_000
        );
        assert!(stream_player_input_is_ready(
            "[cplayer] Opening done: /tmp/concord-video.sdp"
        ));
        assert!(!stream_player_input_is_ready(
            "[cplayer] Starting playback..."
        ));
    }

    #[tokio::test]
    pub async fn local_video_forwarder_replays_then_continues_the_source_clock() {
        let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("test sender should bind");
        let receiver = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("test receiver should bind");
        let target = match receiver
            .local_addr()
            .expect("test receiver should have an address")
        {
            std::net::SocketAddr::V4(target) => target,
            std::net::SocketAddr::V6(_) => panic!("test receiver should use IPv4"),
        };
        let media_started_at = Instant::now();
        let destination = LocalStreamVideoDestination {
            socket: &socket,
            target,
            ssrc: 42,
            media_started_at,
        };
        let mut startup = H264StartupBuffer::default();
        assert!(startup.push(BufferedH264Frame {
            encoded: vec![0, 0, 0, 1, 0x65, 0x11],
            source_timestamp: 90_000,
        }));
        assert!(startup.push(BufferedH264Frame {
            encoded: vec![0, 0, 0, 1, 0x41, 0x22],
            source_timestamp: 93_000,
        }));
        let mut forwarder = LocalStreamVideoForwarder::default();

        forwarder.replay_startup(&mut startup, &destination).await;
        forwarder
            .forward_live(
                &destination,
                &BufferedH264Frame {
                    encoded: vec![0, 0, 0, 1, 0x41, 0x33],
                    source_timestamp: 96_000,
                },
                None,
            )
            .await;
        let mut timestamps = Vec::new();
        let mut packet = [0u8; 1500];
        for _ in 0..3 {
            let (received, _) = timeout(Duration::from_secs(1), receiver.recv_from(&mut packet))
                .await
                .expect("local video packet should arrive")
                .expect("local video packet should be readable");
            timestamps.push(
                parse_rtp_header(&packet[..received])
                    .expect("local video packet should contain a valid RTP header")
                    .timestamp,
            );
        }

        assert!(startup.is_empty());
        assert_eq!(forwarder.frames, 3);
        assert_eq!(timestamps[2], timestamps[1].wrapping_add(3_000));
    }

    #[test]
    pub fn stream_keyframe_request_uses_encrypted_compound_rtcp() {
        let sender_ssrc: u32 = 0x0102_0304;
        let media_ssrc = 0x0506_0708;
        let pli = build_rtcp_pli(sender_ssrc, media_ssrc);
        let feedback = build_stream_rtcp_compound(sender_ssrc, None, Some(&pli));
        assert_eq!(&feedback[..4], &[0x80, 201, 0, 1]);
        assert_eq!(&feedback[4..8], &sender_ssrc.to_be_bytes());

        for mode in [AEAD_AES256_GCM_RTPSIZE, AEAD_XCHACHA20_POLY1305_RTPSIZE] {
            let key = [0x42; 32];
            let encryptor =
                VoiceRtpEncryptor::new(mode, &key).expect("feedback encryptor should initialize");
            let encrypted = encryptor
                .encrypt_rtcp_feedback(&feedback, 9u32.to_be_bytes())
                .expect("RTCP feedback should encrypt");
            assert_eq!(&encrypted[..8], &feedback[..8]);
            assert_eq!(
                encrypted.len(),
                feedback.len() + RTP_AEAD_TAG_BYTES + RTP_AEAD_NONCE_SUFFIX_BYTES
            );

            let decryptor =
                VoiceRtpDecryptor::new(mode, &key).expect("feedback decryptor should initialize");
            let decrypted = decryptor
                .decrypt_rtcp_feedback(&encrypted)
                .expect("RTCP feedback body should decrypt");
            assert_eq!(decrypted, feedback);
        }
    }

    #[test]
    pub fn stream_compound_rtcp_reports_source_and_round_trips() {
        let sender_ssrc: u32 = 7;
        let media_ssrc: u32 = 42;
        let mut control = StreamRtcpControl::default();
        control.set_source(media_ssrc);
        for (sequence, timestamp, arrival) in [
            (u16::MAX - 1, 0, Duration::ZERO),
            (u16::MAX, 900, Duration::from_millis(10)),
            (0, 1_800, Duration::from_millis(20)),
        ] {
            control.observe_rtp(sequence, timestamp, arrival);
        }
        control.observe_sender_report(
            StreamRtcpSenderReport {
                sender_ssrc: media_ssrc,
                ntp_timestamp: 0x0102_0304_0506_0708,
                rtp_timestamp: 1_800,
                packet_count: 3,
                octet_count: 3_000,
            },
            Duration::from_secs(1),
        );
        let block = control
            .report_block(Duration::from_millis(1_500))
            .expect("received video should produce a report block");
        assert_eq!(block.source_ssrc, media_ssrc);
        assert_eq!(block.fraction_lost, 0);
        assert_eq!(block.cumulative_lost, 0);
        assert_eq!(block.extended_highest_sequence, 1 << 16);
        assert_eq!(block.interarrival_jitter, 0);
        assert_eq!(block.last_sender_report, 0x0304_0506);
        assert_eq!(block.delay_since_last_sender_report, 1 << 15);

        let pli = build_rtcp_pli(sender_ssrc, media_ssrc);
        let feedback = build_stream_rtcp_compound(sender_ssrc, Some(block), Some(&pli));
        assert_eq!(&feedback[..4], &[0x81, 201, 0, 7]);
        assert_eq!(&feedback[4..8], &sender_ssrc.to_be_bytes());
        assert_eq!(&feedback[8..12], &media_ssrc.to_be_bytes());
        assert_eq!(&feedback[13..16], &[0, 0, 0]);
        assert_eq!(&feedback[16..20], &(1u32 << 16).to_be_bytes());
        assert_eq!(&feedback[20..24], &0u32.to_be_bytes());
        assert_eq!(&feedback[24..28], &0x0304_0506u32.to_be_bytes());
        assert_eq!(&feedback[28..32], &(1u32 << 15).to_be_bytes());
        assert_eq!(&feedback[32..36], &[0x81, RTCP_SOURCE_DESCRIPTION, 0, 4]);
        assert_eq!(&feedback[36..40], &sender_ssrc.to_be_bytes());
        assert_eq!(&feedback[40..42], &[RTCP_SDES_CNAME, 9]);
        assert_eq!(&feedback[42..51], b"concord-7");
        assert_eq!(feedback[51], 0);
        assert_eq!(&feedback[52..], &pli);

        let mut packet_types = Vec::new();
        let mut offset = 0;
        while offset < feedback.len() {
            packet_types.push(feedback[offset + 1]);
            let length_words_minus_one =
                u16::from_be_bytes([feedback[offset + 2], feedback[offset + 3]]);
            offset += (usize::from(length_words_minus_one) + 1) * 4;
        }
        assert_eq!(offset, feedback.len());
        assert_eq!(
            packet_types,
            vec![
                RTCP_RECEIVER_REPORT,
                RTCP_SOURCE_DESCRIPTION,
                RTCP_PAYLOAD_SPECIFIC_FEEDBACK,
            ]
        );

        for mode in [AEAD_AES256_GCM_RTPSIZE, AEAD_XCHACHA20_POLY1305_RTPSIZE] {
            let key = [0x42; 32];
            let encryptor =
                VoiceRtpEncryptor::new(mode, &key).expect("feedback encryptor should initialize");
            let encrypted = encryptor
                .encrypt_rtcp_feedback(&feedback, 10u32.to_be_bytes())
                .expect("compound RTCP feedback should encrypt");
            let decryptor =
                VoiceRtpDecryptor::new(mode, &key).expect("feedback decryptor should initialize");
            let decrypted = decryptor
                .decrypt_rtcp_feedback(&encrypted)
                .expect("compound RTCP feedback should decrypt");

            assert_eq!(decrypted, feedback);
        }
    }

    #[test]
    pub fn stream_receiver_report_measures_interval_loss_and_jitter() {
        let mut control = StreamRtcpControl::default();
        control.set_source(42);
        control.observe_rtp(10, 0, Duration::ZERO);
        control.observe_rtp(12, 1_800, Duration::from_millis(30));

        let first = control
            .report_block(Duration::from_secs(1))
            .expect("received video should produce a report block");
        assert_eq!(first.fraction_lost, 85);
        assert_eq!(first.cumulative_lost, 1);
        assert_eq!(first.interarrival_jitter, 56);

        control.observe_rtp(11, 900, Duration::from_millis(35));
        let repaired = control
            .report_block(Duration::from_secs(2))
            .expect("late video should update the next report interval");
        assert_eq!(repaired.fraction_lost, 0);
        assert_eq!(repaired.cumulative_lost, 0);
    }

    #[test]
    pub fn local_sender_report_maps_rtp_to_a_shared_ntp_clock() {
        let report = build_rtcp_sender_report(
            0x0102_0304,
            Duration::new(1, 500_000_000),
            90_000,
            30,
            45_000,
        );

        assert_eq!(&report[..4], &[0x80, 200, 0, 6]);
        assert_eq!(&report[4..8], &0x0102_0304u32.to_be_bytes());
        assert_eq!(&report[8..12], &2_208_988_801u32.to_be_bytes());
        assert_eq!(&report[12..16], &0x8000_0000u32.to_be_bytes());
        assert_eq!(&report[16..20], &90_000u32.to_be_bytes());
        assert_eq!(&report[20..24], &30u32.to_be_bytes());
        assert_eq!(&report[24..28], &45_000u32.to_be_bytes());
    }

    #[test]
    pub fn stream_parses_sender_reports_from_compound_rtcp() {
        let sender_ssrc: u32 = 0x0102_0304;
        let mut compound = build_rtcp_receiver_report(7, None);
        compound.extend_from_slice(&build_rtcp_sender_report(
            sender_ssrc,
            Duration::new(1, 500_000_000),
            90_000,
            30,
            45_000,
        ));

        let reports = parse_stream_rtcp_sender_reports(&compound)
            .expect("compound RTCP should contain a valid sender report");

        assert_eq!(
            reports,
            vec![StreamRtcpSenderReport {
                sender_ssrc,
                ntp_timestamp: (u64::from(2_208_988_801u32) << 32) | 0x8000_0000,
                rtp_timestamp: 90_000,
                packet_count: 30,
                octet_count: 45_000,
            }]
        );
    }

    #[test]
    pub fn stream_rejects_a_truncated_rtcp_sender_report() {
        let mut report = build_rtcp_sender_report(42, Duration::from_secs(1), 90_000, 30, 45_000);
        report[2..4].copy_from_slice(&100u16.to_be_bytes());

        assert_eq!(
            parse_stream_rtcp_sender_reports(&report),
            Err("RTCP packet length exceeds the compound packet".to_owned())
        );
    }

    #[test]
    pub fn stream_presentation_clock_aligns_audio_and_video_sender_time() {
        let ntp_timestamp = (u64::from(2_208_988_801u32) << 32) | 0x8000_0000;
        let mut clock = StreamPresentationClock::default();
        clock.observe_sender_report(
            StreamRtcpSenderReport {
                sender_ssrc: 7,
                ntp_timestamp,
                rtp_timestamp: 3_000_000,
                packet_count: 0,
                octet_count: 0,
            },
            Duration::from_millis(100),
        );
        clock.observe_sender_report(
            StreamRtcpSenderReport {
                sender_ssrc: 42,
                ntp_timestamp,
                rtp_timestamp: 90_000_000,
                packet_count: 0,
                octet_count: 0,
            },
            Duration::from_millis(105),
        );

        let synchronized_audio = clock.map_timestamp(7, 3_004_800, OPUS_RTP_CLOCK_RATE);
        let synchronized_video = clock.map_timestamp(42, 90_009_000, VIDEO_RTP_CLOCK_RATE);
        assert_eq!(synchronized_audio, Some(9_600));
        assert_eq!(synchronized_video, Some(18_000));

        let mut local_audio = LocalStreamAudioClock::default();
        let mut local_video = LocalRtpClock::default();
        assert_eq!(
            local_audio
                .rebase(3_004_800, Duration::from_millis(205), synchronized_audio,)
                .local_timestamp,
            9_600
        );
        assert_eq!(
            local_video.rebase(
                90_009_000,
                Duration::from_millis(205),
                VIDEO_RTP_CLOCK_RATE,
                synchronized_video,
            ),
            18_000
        );
    }

    #[test]
    pub fn stream_presentation_clock_preserves_rtp_timestamp_wrap() {
        let mut clock = StreamPresentationClock::default();
        clock.observe_sender_report(
            StreamRtcpSenderReport {
                sender_ssrc: 42,
                ntp_timestamp: u64::from(2_208_988_801u32) << 32,
                rtp_timestamp: u32::MAX - 8_999,
                packet_count: 0,
                octet_count: 0,
            },
            Duration::from_millis(100),
        );

        assert_eq!(
            clock.map_timestamp(42, 0, VIDEO_RTP_CLOCK_RATE),
            Some(18_000)
        );
    }

    #[test]
    pub fn stream_sdp_keeps_audio_and_video_on_one_input() {
        let sdp = stream_sdp(50_000, 50_001, 50_002, 50_003);

        assert!(sdp.contains("m=audio 50000 RTP/AVP 111\r\n"));
        assert!(sdp.contains("a=rtcp:50001 IN IP4 127.0.0.1\r\n"));
        assert!(sdp.contains("m=video 50002 RTP/AVP 96\r\n"));
        assert!(sdp.contains("a=rtcp:50003 IN IP4 127.0.0.1\r\n"));
    }
}
