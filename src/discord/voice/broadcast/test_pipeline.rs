use std::{
    collections::HashMap,
    sync::{Arc, Mutex as StdMutex},
    time::{Duration, Instant},
};

use serde_json::{Value, json};
use tokio::{
    net::UdpSocket,
    sync::{Mutex, mpsc},
    time::timeout,
};

use super::super::media::build_rtcp_sender_report;
use super::super::runtime::MAX_VOICE_RECONNECT_ATTEMPTS;
use super::super::{
    DISCORD_OPUS_TIMESTAMP_INCREMENT, DISCORD_STREAM_VIDEO_PAYLOAD_TYPE,
    DISCORD_STREAM_VIDEO_RTX_PAYLOAD_TYPE, DISCORD_VOICE_PAYLOAD_TYPE, DiscoveredVoiceAddress,
    VOICE_OP_SPEAKING, VoiceConnectionEnd, VoiceDaveState, VoiceRuntimeEvent, capture,
    opus::VoiceOpusEncode,
    rtp::{VoiceRtpDecryptor, build_voice_rtp_packet_with_marker, parse_rtp_header},
    system_audio::{self, SYSTEM_AUDIO_FRAME_QUEUE},
};
use super::*;

#[cfg(test)]
mod cases {
    use super::tests::shared::*;
    use super::*;
    use crate::discord::{
        ids::Id, voice::AEAD_XCHACHA20_POLY1305_RTPSIZE, voice::DISCORD_OPUS_20MS_STEREO_SAMPLES,
    };

    #[test]
    pub fn broadcast_runtime_stops_after_bounded_reconnect_attempts() {
        let (mut state, mut active) = connected_broadcast_runtime();

        for attempt in 1..=MAX_VOICE_RECONNECT_ATTEMPTS {
            state.apply(&VoiceRuntimeEvent::BroadcastStreamConnectionEstablished {
                connection_id: active.connection_id,
                stream_key: active.request.stream_key.clone(),
            });
            let update = state.apply(&broadcast_connection_ended(
                &active,
                VoiceConnectionEnd::Reconnect,
            ));
            assert!(
                update.close_stream_key.is_none(),
                "retry {attempt} should keep the broadcast request active"
            );
            active = update
                .connect
                .expect("retry within the limit should reconnect broadcast");
        }

        state.apply(&VoiceRuntimeEvent::BroadcastStreamConnectionEstablished {
            connection_id: active.connection_id,
            stream_key: active.request.stream_key.clone(),
        });
        let stopped = state.apply(&broadcast_connection_ended(
            &active,
            VoiceConnectionEnd::Reconnect,
        ));
        assert!(stopped.connect.is_none());
        assert_eq!(
            stopped.close_stream_key.as_deref(),
            Some(active.request.stream_key.as_str())
        );
        assert!(stopped.send_delete);
    }

    #[test]
    pub fn broadcast_runtime_resets_reconnect_budget_only_after_stable_connection() {
        let (mut state, initial) = connected_broadcast_runtime();
        let retry = state
            .apply(&broadcast_connection_ended(
                &initial,
                VoiceConnectionEnd::Reconnect,
            ))
            .connect
            .expect("first failure should reconnect");
        assert_eq!(state.reconnect_attempts, 1);

        state.apply(&VoiceRuntimeEvent::BroadcastStreamConnectionStable {
            connection_id: initial.connection_id,
            stream_key: initial.request.stream_key,
        });
        assert_eq!(
            state.reconnect_attempts, 1,
            "a stale connection must not reset the active retry budget"
        );

        state.apply(&VoiceRuntimeEvent::BroadcastStreamConnectionEstablished {
            connection_id: retry.connection_id,
            stream_key: retry.request.stream_key.clone(),
        });
        assert_eq!(
            state.reconnect_attempts, 1,
            "initial media setup must not reset the retry budget"
        );

        state.apply(&VoiceRuntimeEvent::BroadcastStreamConnectionStable {
            connection_id: retry.connection_id,
            stream_key: retry.request.stream_key,
        });
        assert_eq!(state.reconnect_attempts, 0);
    }

    #[test]
    pub fn broadcast_reconnect_backoff_keeps_the_first_retry_immediate() {
        assert_eq!(broadcast_reconnect_delay(0), Duration::ZERO);
        assert_eq!(broadcast_reconnect_delay(1), Duration::ZERO);

        let second_retry = broadcast_reconnect_delay(2);
        assert!(second_retry >= Duration::from_millis(250));
        assert!(second_retry <= Duration::from_millis(312));

        let third_retry = broadcast_reconnect_delay(3);
        assert!(third_retry >= Duration::from_millis(500));
        assert!(third_retry <= Duration::from_millis(625));
    }

    #[test]
    pub fn broadcast_identify_declares_screen_stream() {
        let payload: Value = serde_json::from_str(&stream_broadcast_identify_payload(&session()))
            .expect("broadcast identify is valid json");
        assert_eq!(payload["op"], 0);
        assert_eq!(payload["d"]["video"], true);
        assert_eq!(payload["d"]["streams"][0]["type"], "screen");
        assert_eq!(payload["d"]["streams"][0]["rid"], STREAM_RID);
    }

    #[test]
    pub fn broadcast_ready_selects_requested_rid_and_derives_missing_rtx_ssrc() {
        let ready = json!({
            "d": {
                "streams": [
                    {"type": "video", "rid": "50", "ssrc": 50, "rtx_ssrc": 51},
                    {"type": "video", "rid": STREAM_RID, "ssrc": 100}
                ]
            }
        });

        assert_eq!(
            parse_broadcast_video_ssrcs(&ready),
            Ok(BroadcastVideoSsrcs {
                video_ssrc: 100,
                rtx_ssrc: 101,
            })
        );
    }

    #[test]
    pub fn broadcast_gateway_payloads_declare_outbound_h264_and_active_video() {
        let selected: Value = serde_json::from_str(&stream_broadcast_select_protocol_payload(
            &DiscoveredVoiceAddress {
                address: "127.0.0.1".to_owned(),
                port: 5000,
            },
            AEAD_XCHACHA20_POLY1305_RTPSIZE,
        ))
        .expect("broadcast select protocol payload is valid json");
        assert_eq!(selected["d"]["codecs"][0]["name"], "opus");
        assert_eq!(selected["d"]["codecs"][0]["encode"], true);
        assert_eq!(selected["d"]["codecs"][0]["decode"], false);
        assert_eq!(selected["d"]["codecs"][1]["name"], "H264");
        assert_eq!(
            selected["d"]["codecs"][1]["payload_type"],
            DISCORD_STREAM_VIDEO_PAYLOAD_TYPE
        );
        assert_eq!(
            selected["d"]["codecs"][1]["rtx_payload_type"],
            DISCORD_STREAM_VIDEO_RTX_PAYLOAD_TYPE
        );
        assert_eq!(selected["d"]["codecs"][1]["encode"], true);
        assert_eq!(selected["d"]["codecs"][1]["decode"], false);

        let video = BroadcastVideoSsrcs {
            video_ssrc: 100,
            rtx_ssrc: 101,
        };
        let announced: Value = serde_json::from_str(&stream_broadcast_video_payload(99, video))
            .expect("broadcast video payload is valid json");
        assert_eq!(announced["op"], 12);
        assert_eq!(announced["d"]["audio_ssrc"], 99);
        assert_eq!(announced["d"]["video_ssrc"], 100);
        assert_eq!(announced["d"]["rtx_ssrc"], 101);
        assert_eq!(announced["d"]["streams"][0]["type"], "video");
        assert_eq!(announced["d"]["streams"][0]["rid"], STREAM_RID);
        assert_eq!(announced["d"]["streams"][0]["ssrc"], 100);
        assert_eq!(announced["d"]["streams"][0]["rtx_ssrc"], 101);
        assert_eq!(announced["d"]["streams"][0]["active"], true);
        assert_eq!(
            announced["d"]["streams"][0]["max_bitrate"],
            capture::STREAM_TRANSPORT_BITRATE
        );
        assert_eq!(
            announced["d"]["streams"][0]["max_framerate"],
            capture::STREAM_CAPTURE_FPS
        );
        assert_eq!(
            announced["d"]["streams"][0]["max_resolution"]["width"],
            capture::STREAM_CAPTURE_WIDTH
        );
        assert_eq!(
            announced["d"]["streams"][0]["max_resolution"]["height"],
            capture::STREAM_CAPTURE_HEIGHT
        );
    }

    #[test]
    pub fn broadcast_speaking_registers_audio_ssrc_as_soundshare() {
        let payload: Value = serde_json::from_str(&stream_broadcast_speaking_payload(1234))
            .expect("broadcast speaking payload is valid json");
        assert_eq!(payload["op"], VOICE_OP_SPEAKING);
        assert_eq!(payload["d"]["speaking"], SOUNDSHARE_SPEAKING_FLAG);
        assert_eq!(payload["d"]["ssrc"], 1234);
    }

    #[test]
    pub fn broadcast_audio_clock_preserves_capture_frame_gaps() {
        assert_eq!(broadcast_audio_elapsed_frames(None, 0), 1);
        assert_eq!(broadcast_audio_elapsed_frames(Some(0), 3), 3);
        assert_eq!(broadcast_audio_elapsed_frames(Some(3), 3), 1);
    }

    #[test]
    pub fn broadcast_packet_encryptor_allocates_unique_nonces_concurrently() {
        let encryptor = Arc::new(
            BroadcastPacketEncryptor::with_nonce(AEAD_XCHACHA20_POLY1305_RTPSIZE, &[9; 32], 1)
                .expect("broadcast packet encryptor should build"),
        );
        let mut workers = Vec::new();
        for _ in 0..4 {
            let encryptor = Arc::clone(&encryptor);
            workers.push(std::thread::spawn(move || {
                (0..256)
                    .map(|_| {
                        u32::from_be_bytes(
                            encryptor
                                .take_nonce("test RTP")
                                .expect("test nonce should be available"),
                        )
                    })
                    .collect::<Vec<_>>()
            }));
        }

        let mut nonces = workers
            .into_iter()
            .flat_map(|worker| worker.join().expect("nonce worker should finish"))
            .collect::<Vec<_>>();
        nonces.sort_unstable();
        nonces.dedup();

        assert_eq!(nonces.len(), 1_024);
        assert_eq!(nonces.first(), Some(&1));
        assert_eq!(nonces.last(), Some(&1_024));

        let exhausted = BroadcastPacketEncryptor::with_nonce(
            AEAD_XCHACHA20_POLY1305_RTPSIZE,
            &[9; 32],
            u32::MAX - 1,
        )
        .expect("broadcast packet encryptor should build near nonce exhaustion");
        assert_eq!(
            exhausted
                .take_nonce("test RTP")
                .expect("last safe nonce should be available"),
            (u32::MAX - 1).to_be_bytes()
        );
        assert_eq!(
            exhausted
                .take_nonce("test RTP")
                .expect_err("nonce allocation must stop before wrapping"),
            "broadcast test RTP nonce exhausted"
        );
    }

    #[tokio::test]
    pub async fn broadcast_audio_sender_preserves_queued_frame_order() {
        let receiver = UdpSocket::bind("127.0.0.1:0")
            .await
            .expect("test audio receiver should bind");
        let sender = Arc::new(
            UdpSocket::bind("127.0.0.1:0")
                .await
                .expect("test audio sender should bind"),
        );
        sender
            .connect(
                receiver
                    .local_addr()
                    .expect("test audio receiver should have an address"),
            )
            .await
            .expect("test audio sender should connect");

        let (frames_tx, frames_rx) = mpsc::channel(SYSTEM_AUDIO_FRAME_QUEUE);
        for index in 0..3 {
            frames_tx
                .send(system_audio::SystemAudioFrame {
                    samples: vec![index as i16; DISCORD_OPUS_20MS_STEREO_SAMPLES],
                    captured_at: Instant::now(),
                    frame_index: index as u64,
                })
                .await
                .expect("test audio frame should queue");
        }
        drop(frames_tx);

        let description = voice_description();
        let packet_encryptor =
            Arc::new(BroadcastPacketEncryptor::new(&description).expect("encryptor should build"));
        let stats = Arc::new(StdMutex::new(BroadcastSendStats::new()));
        let transport =
            BroadcastAudioTransport::new(42, Arc::clone(&packet_encryptor), Arc::clone(&stats));
        let initial_sequence = transport.sequence;
        let initial_timestamp = transport.timestamp;
        let dave_state = Arc::new(Mutex::new(VoiceDaveState::new_for_identity(
            Id::new(30),
            10,
        )));
        let encoder =
            VoiceOpusEncode::new_system_audio().expect("system audio Opus encoder should build");
        let sender_task = tokio::spawn(run_stream_broadcast_audio(
            Arc::clone(&sender),
            dave_state,
            Arc::new(system_audio::SystemAudioCaptureStats::default()),
            frames_rx,
            encoder,
            transport,
        ));
        let decryptor = VoiceRtpDecryptor::new(&description.mode, &description.secret_key)
            .expect("test audio decryptor should build");

        let mut packet = vec![0u8; 2_048];
        for index in 0..3u32 {
            let length = timeout(Duration::from_secs(1), receiver.recv(&mut packet))
                .await
                .expect("ordered audio packet should arrive")
                .expect("ordered audio packet should receive");
            let header =
                parse_rtp_header(&packet[..length]).expect("audio RTP header should parse");
            let decrypted = decryptor
                .decrypt_packet(&packet[..length], &header)
                .expect("audio RTP packet should decrypt");

            assert_eq!(header.payload_type, DISCORD_VOICE_PAYLOAD_TYPE);
            assert_eq!(header.sequence, initial_sequence.wrapping_add(index as u16));
            assert_eq!(
                header.timestamp,
                initial_timestamp
                    .wrapping_add(DISCORD_OPUS_TIMESTAMP_INCREMENT.wrapping_mul(index + 1))
            );
            assert_eq!(header.ssrc, 42);
            assert_eq!(header.marker, index == 0);
            assert!(!decrypted.media_payload.is_empty());
        }

        timeout(Duration::from_secs(1), sender_task)
            .await
            .expect("audio sender task should finish")
            .expect("audio sender task should join")
            .expect("audio sender should preserve queued frame order");
    }

    #[test]
    pub fn sender_report_maps_video_clock_and_counters() {
        let packet =
            build_rtcp_sender_report(42, Duration::from_secs(1_700_000_000), 90_000, 12, 34_567);
        assert_eq!(packet.len(), 28);
        assert_eq!(&packet[..4], &[0x80, 200, 0, 6]);
        assert_eq!(
            u32::from_be_bytes(packet[4..8].try_into().expect("SSRC")),
            42
        );
        assert_eq!(
            u32::from_be_bytes(packet[16..20].try_into().expect("RTP timestamp")),
            90_000
        );
        assert_eq!(
            u32::from_be_bytes(packet[20..24].try_into().expect("packet count")),
            12
        );
        assert_eq!(
            u32::from_be_bytes(packet[24..28].try_into().expect("octet count")),
            34_567
        );
    }

    #[test]
    pub fn broadcast_rtp_history_indexes_across_wrap_and_evicts_old_packets() {
        let mut history = BroadcastRtpHistory::new();
        let start = u16::MAX - 1;
        for offset in 0..=3 {
            let sequence = start.wrapping_add(offset);
            history
                .remember(
                    build_voice_rtp_packet_with_marker(
                        sequence,
                        90_000,
                        42,
                        false,
                        &[offset as u8],
                    )
                    .expect("test RTP packet should build"),
                )
                .expect("test RTP packet should enter history");
        }

        for offset in 0..=3 {
            let sequence = start.wrapping_add(offset);
            let packet = history
                .get(sequence)
                .expect("wrapped sequence should remain addressable");
            assert_eq!(
                parse_rtp_header(packet)
                    .expect("history packet should remain valid")
                    .sequence,
                sequence
            );
        }

        for offset in 4..STREAM_RTP_HISTORY_CAPACITY + 4 {
            let sequence = start.wrapping_add(offset as u16);
            history
                .remember(
                    build_voice_rtp_packet_with_marker(
                        sequence,
                        90_000,
                        42,
                        false,
                        &[offset as u8],
                    )
                    .expect("test RTP packet should build"),
                )
                .expect("test RTP packet should enter history");
        }

        assert!(history.get(start).is_none());
        assert!(
            history
                .get(start.wrapping_add((STREAM_RTP_HISTORY_CAPACITY + 3) as u16))
                .is_some()
        );
    }

    #[test]
    pub fn h264_packetizer_marks_only_final_packet_and_adds_extensions() {
        let frame = [0, 0, 0, 1, 0x65]
            .into_iter()
            .chain(std::iter::repeat_n(0xaa, 3_000))
            .collect::<Vec<_>>();
        let mut sequence = 7;
        let mut transport_sequence = 99;
        let packets = packetize_discord_h264_frame(
            &frame,
            90_000,
            42,
            &mut sequence,
            &mut transport_sequence,
        );

        assert!(packets.len() > 1);
        for (index, packet) in packets.iter().enumerate() {
            let header = parse_rtp_header(packet).expect("broadcast RTP packet is valid");
            assert_eq!(header.payload_type, DISCORD_STREAM_VIDEO_PAYLOAD_TYPE);
            assert_eq!(header.timestamp, 90_000);
            assert_eq!(header.ssrc, 42);
            assert_eq!(header.marker, index + 1 == packets.len());
            assert!(header.encrypted_extension_body_len > 0);
        }
    }

    #[test]
    pub fn rtp_pacer_limits_bursts_and_saved_bandwidth() {
        let now = Instant::now();
        let mut small_frame_pacer = BroadcastRtpPacer::new(now);
        assert_eq!(small_frame_pacer.pacing_interval(1, 1_100, now), None);

        let small_frame_interval = small_frame_pacer
            .pacing_interval(3, 3_300, now)
            .expect("multiple packets should be paced");
        assert!(small_frame_interval <= STREAM_RTP_MAX_PACKET_SPACING);

        let packet_count = 250;
        let estimated_wire_bytes = 280_000;
        let gap_count = packet_count as u32 - 1;
        let frame_budget = duration_for_bitrate(
            estimated_wire_bytes,
            u64::from(capture::STREAM_TRANSPORT_BITRATE),
        );
        let burst_budget = duration_for_bitrate(estimated_wire_bytes, STREAM_RTP_MAX_BURST_BITRATE);
        let mut pacer = BroadcastRtpPacer::new(now);

        let credited_interval = pacer
            .pacing_interval(packet_count, estimated_wire_bytes, now)
            .expect("large frame should be paced");
        let credited_pacing = credited_interval * gap_count;
        assert!(credited_pacing >= frame_budget - STREAM_RTP_BURST_CREDIT);
        assert!(credited_pacing >= burst_budget);
        assert!(credited_pacing < frame_budget);

        let depleted_at = now + credited_pacing;
        let depleted_interval = pacer
            .pacing_interval(packet_count, estimated_wire_bytes, depleted_at)
            .expect("large frame without saved credit should be paced");
        let depleted_pacing = depleted_interval * gap_count;
        let depleted_budget = frame_budget
            .saturating_sub(credited_pacing)
            .saturating_add(frame_budget)
            .saturating_sub(STREAM_RTP_BURST_CREDIT);
        assert!(depleted_pacing >= depleted_budget.max(burst_budget));
        assert!(depleted_pacing > credited_pacing);

        let refilled_at = depleted_at + depleted_pacing + Duration::from_secs(10);
        let refilled_interval = pacer
            .pacing_interval(packet_count, estimated_wire_bytes, refilled_at)
            .expect("idle transport should refill only bounded credit");
        assert_eq!(refilled_interval, credited_interval);
    }

    #[tokio::test]
    pub async fn broadcast_rtcp_feedback_requires_transport_authentication() {
        let description = voice_description();
        let packet_encryptor =
            Arc::new(BroadcastPacketEncryptor::new(&description).expect("encryptor should build"));
        let stats = Arc::new(StdMutex::new(BroadcastSendStats::new()));
        let mut transport = BroadcastVideoTransport::new(
            &description,
            BroadcastVideoSsrcs {
                video_ssrc: 42,
                rtx_ssrc: 43,
            },
            Arc::clone(&packet_encryptor),
            stats,
        )
        .expect("video transport should build");
        let socket = UdpSocket::bind("127.0.0.1:0")
            .await
            .expect("test RTCP socket should bind");
        let mut pli = vec![0x81, 206, 0, 2];
        pli.extend_from_slice(&7u32.to_be_bytes());
        pli.extend_from_slice(&42u32.to_be_bytes());

        assert!(
            !transport
                .handle_udp_packet(&socket, &pli)
                .await
                .expect("plaintext RTCP should be ignored"),
            "plaintext RTCP must not request a keyframe"
        );

        let encrypted = packet_encryptor
            .encrypt_rtcp_packet(&pli, "test RTCP")
            .expect("test RTCP should encrypt");
        assert!(
            transport
                .handle_udp_packet(&socket, &encrypted)
                .await
                .expect("authenticated RTCP should be accepted"),
            "authenticated PLI should request a keyframe"
        );
    }

    #[test]
    pub fn rtcp_feedback_requests_retransmission_and_keyframe_recovery() {
        let sender_ssrc = 7u32;
        let video_ssrc = 42u32;
        let mut feedback = Vec::new();

        feedback.extend_from_slice(&[0x81, 201, 0, 7]);
        feedback.extend_from_slice(&sender_ssrc.to_be_bytes());
        feedback.extend_from_slice(&video_ssrc.to_be_bytes());
        feedback.extend_from_slice(&[64, 0, 0, 2]);
        feedback.extend_from_slice(&[0; 16]);

        feedback.extend_from_slice(&[0x81, 205, 0, 3]);
        feedback.extend_from_slice(&sender_ssrc.to_be_bytes());
        feedback.extend_from_slice(&video_ssrc.to_be_bytes());
        feedback.extend_from_slice(&1_000u16.to_be_bytes());
        feedback.extend_from_slice(&0b0000_0000_0000_0101u16.to_be_bytes());

        feedback.extend_from_slice(&[0x81, 206, 0, 2]);
        feedback.extend_from_slice(&sender_ssrc.to_be_bytes());
        feedback.extend_from_slice(&video_ssrc.to_be_bytes());

        let parsed =
            parse_broadcast_rtcp_feedback(&feedback, video_ssrc).expect("feedback should parse");

        assert_eq!(parsed.nack_sequences, vec![1_000, 1_001, 1_003]);
        assert!(parsed.request_keyframe);
        assert_eq!(
            parsed.receiver_reports,
            vec![BroadcastReceiverReport {
                reporter_ssrc: sender_ssrc,
                fraction_lost: 64,
                cumulative_lost: 2,
            }]
        );

        let mut previous_loss = HashMap::new();
        assert!(receiver_report_has_new_loss(
            &mut previous_loss,
            sender_ssrc,
            2
        ));
        assert!(!receiver_report_has_new_loss(
            &mut previous_loss,
            sender_ssrc,
            2
        ));
        assert!(receiver_report_has_new_loss(
            &mut previous_loss,
            sender_ssrc,
            3
        ));
        assert!(receiver_report_has_new_loss(
            &mut previous_loss,
            sender_ssrc + 1,
            1
        ));
    }

    pub fn one_byte_rtp_extension(packet: &[u8], wanted_id: u8) -> Option<&[u8]> {
        let header = parse_rtp_header(packet).ok()?;
        let mut extensions = packet.get(header.authenticated_header_len..header.payload_offset)?;
        while let Some((&descriptor, remaining)) = extensions.split_first() {
            if descriptor == 0 {
                extensions = remaining;
                continue;
            }
            let extension_id = descriptor >> 4;
            if extension_id == 15 {
                return None;
            }
            let value_len = usize::from(descriptor & 0x0f) + 1;
            let value = remaining.get(..value_len)?;
            if extension_id == wanted_id {
                return Some(value);
            }
            extensions = remaining.get(value_len..)?;
        }
        None
    }

    #[test]
    pub fn rtx_packet_preserves_original_packet_identity_and_payload() {
        let original = build_discord_video_rtp_packet(10, 90_000, 42, true, 20, b"h264");
        let original_header =
            parse_rtp_header(&original).expect("original RTP packet should parse");
        let rtx = build_discord_video_rtx_packet(&original, 100, 30, 40)
            .expect("RTX packet should build");
        let rtx_header = parse_rtp_header(&rtx).expect("RTX packet should parse");

        assert_eq!(
            rtx_header.payload_type,
            DISCORD_STREAM_VIDEO_RTX_PAYLOAD_TYPE
        );
        assert_eq!(rtx_header.sequence, 30);
        assert_eq!(rtx_header.timestamp, original_header.timestamp);
        assert_eq!(rtx_header.ssrc, 100);
        assert_eq!(rtx_header.marker, original_header.marker);
        assert_eq!(
            &rtx[rtx_header.payload_offset..rtx_header.payload_offset + 2],
            &original_header.sequence.to_be_bytes()
        );
        assert_eq!(
            &rtx[rtx_header.payload_offset + 2..],
            &original[original_header.payload_offset..]
        );
        assert_eq!(
            one_byte_rtp_extension(&original, RTP_EXTENSION_RID),
            Some(STREAM_RID.as_bytes())
        );
        assert_eq!(
            one_byte_rtp_extension(&original, RTP_EXTENSION_REPAIRED_RID),
            None
        );
        assert_eq!(one_byte_rtp_extension(&rtx, RTP_EXTENSION_RID), None);
        assert_eq!(
            one_byte_rtp_extension(&rtx, RTP_EXTENSION_REPAIRED_RID),
            Some(STREAM_RID.as_bytes())
        );
    }
}

#[cfg(test)]
mod stream_kind_tests {
    use super::*;
    use crate::discord::voice::{StreamCaptureTarget, StreamCaptureTargetKind};

    pub fn target(kind: StreamCaptureTargetKind) -> StreamCaptureTarget {
        StreamCaptureTarget {
            kind,
            id: 0,
            title: String::new(),
        }
    }

    #[test]
    pub fn a_camera_is_announced_as_video_and_a_screen_as_screen() {
        // Different types on the wire. Discord's own clients decide layout and
        // quality from this, so a camera sent as "screen" announces the wrong
        // thing about a feed that otherwise works.
        assert_eq!(
            stream_kind(&target(StreamCaptureTargetKind::Camera)),
            "video"
        );

        for kind in [
            StreamCaptureTargetKind::Display,
            StreamCaptureTargetKind::Window,
            StreamCaptureTargetKind::Portal,
        ] {
            assert_eq!(stream_kind(&target(kind)), "screen", "{kind:?}");
        }
    }
}
