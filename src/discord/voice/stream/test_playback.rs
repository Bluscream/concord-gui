use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    io::Write,
    net::{Ipv4Addr, SocketAddrV4},
    path::Path,
    process::Stdio,
    sync::atomic::{AtomicBool, Ordering},
};

use rand::random;
use tempfile::NamedTempFile;
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, BufReader},
    process::Command,
};
use uuid::Uuid;

use crate::support::media_player::MediaPlayerIpcEndpoint;

use super::media::{
    GatewayChildTasks, annex_b_nals, build_rtcp_sender_report, current_unix_time,
    packetize_h264_payloads,
};
use super::runtime::MAX_VOICE_RECONNECT_ATTEMPTS;
use serde_json::json;
use super::tests::tests::*;
use std::time::{Instant, Duration};


#[cfg(test)]
mod test_playback {
    use super::*;
    use super::super::*;
    use serde_json::json;

    #[test]
    pub fn stream_video_source_prefers_highest_active_quality() {
        let value = json!({
            "op": 12,
            "d": {
                "user_id": "99",
                "audio_ssrc": 10,
                "video_ssrc": 20,
                "streams": [
                    {"ssrc": 20, "rtx_ssrc": 21, "quality": 50, "active": true},
                    {
                        "ssrc": 30,
                        "rtx_ssrc": 31,
                        "quality": 100,
                        "active": true,
                        "max_resolution": {"type": "fixed", "width": 2560, "height": 1440}
                    }
                ]
            }
        });
        assert_eq!(
            parse_stream_video_source(&value, Id::new(99)),
            Some(StreamVideoSource {
                audio_ssrc: 10,
                video_ssrc: 30,
                rtx_ssrc: Some(31),
                pixel_count: Some(2560 * 1440),
            })
        );
    }

    #[test]
    pub fn stream_runtime_waits_for_parent_voice_and_both_stream_events() {
        let mut state = StreamRuntimeState::default();
        assert!(
            state
                .apply(&VoiceRuntimeEvent::WatchStreamRequested(stream_request()))
                .connect
                .is_none()
        );
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
        let session = update.connect.expect("stream session should now be ready");
        assert_eq!(session.session_id, "parent-session");
        assert_eq!(session.rtc_server_id, "400");
        assert_eq!(session.rtc_channel_id, Id::new(401));
        assert_eq!(session.request.owner_id, Id::new(99));
    }

    #[test]
    pub fn stream_runtime_pending_cancel_ends_playback_once() {
        let request = stream_request();
        let mut state = StreamRuntimeState::default();
        state.apply(&VoiceRuntimeEvent::WatchStreamRequested(request.clone()));

        let cancelled = state.apply(&VoiceRuntimeEvent::WatchStreamCancelled {
            stream_key: request.stream_key.clone(),
        });
        let ended = cancelled
            .playback_ended
            .expect("pending stream cancellation ends preparing playback");
        assert_eq!(ended.request, request);
        assert!(!ended.reconnecting);

        let repeated = state.apply(&VoiceRuntimeEvent::WatchStreamCancelled {
            stream_key: ended.request.stream_key,
        });
        assert!(repeated.playback_ended.is_none());
    }

    #[test]
    pub fn stream_runtime_rotates_active_stream_servers() {
        let (mut state, initial) = connected_stream_runtime();

        let rotated = state.apply(&VoiceRuntimeEvent::StreamServer(StreamServerInfo {
            stream_key: initial.request.stream_key.clone(),
            endpoint: Some("replacement.example.com".to_owned()),
            token: "replacement-token".to_owned(),
        }));
        assert_eq!(
            rotated.close_stream_key.as_deref(),
            Some(initial.request.stream_key.as_str())
        );
        assert!(!rotated.send_delete);
        assert!(
            rotated
                .playback_ended
                .as_ref()
                .is_some_and(|ended| ended.reconnecting)
        );
        let replacement = rotated
            .connect
            .expect("new stream server starts a replacement connection");
        assert_eq!(replacement.endpoint, "replacement.example.com");
        assert_eq!(replacement.token, "replacement-token");

        let unavailable = state.apply(&VoiceRuntimeEvent::StreamServer(StreamServerInfo {
            stream_key: initial.request.stream_key.clone(),
            endpoint: None,
            token: "pending-token".to_owned(),
        }));
        assert_eq!(
            unavailable.close_stream_key.as_deref(),
            Some(initial.request.stream_key.as_str())
        );
        assert!(!unavailable.send_delete);
        assert!(unavailable.connect.is_none());

        let reallocated = state.apply(&VoiceRuntimeEvent::StreamServer(StreamServerInfo {
            stream_key: initial.request.stream_key.clone(),
            endpoint: Some("reallocated.example.com".to_owned()),
            token: "reallocated-token".to_owned(),
        }));
        let active = reallocated
            .connect
            .expect("reallocated stream server reconnects");
        assert_ne!(active.connection_id, replacement.connection_id);

        let stale_end = state.apply(&stream_connection_ended(
            &replacement,
            VoiceConnectionEnd::Stop,
        ));
        assert!(stale_end.close_stream_key.is_none());
        assert!(stale_end.connect.is_none());
        assert_eq!(
            state
                .active
                .as_ref()
                .expect("reallocated connection remains active")
                .connection_id,
            active.connection_id
        );
    }

    #[test]
    pub fn stream_runtime_stops_after_consecutive_pre_playback_failures() {
        let (mut state, mut active) = connected_stream_runtime();

        for attempt in 1..=MAX_VOICE_RECONNECT_ATTEMPTS {
            let update = state.apply(&stream_connection_ended(
                &active,
                VoiceConnectionEnd::Reconnect,
            ));
            assert!(
                update.close_stream_key.is_none(),
                "retry {attempt} should keep the watch request active"
            );
            active = update
                .connect
                .expect("retry within the limit should reconnect");
        }

        let stopped = state.apply(&stream_connection_ended(
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
    pub fn stream_runtime_stops_immediately_after_terminal_failure() {
        let (mut state, active) = connected_stream_runtime();

        let stopped = state.apply(&stream_connection_ended(&active, VoiceConnectionEnd::Stop));

        assert!(stopped.connect.is_none());
        assert_eq!(
            stopped.close_stream_key.as_deref(),
            Some(active.request.stream_key.as_str())
        );
        assert!(stopped.send_delete);
    }

    #[test]
    pub fn stream_runtime_resets_retries_only_after_stable_playback() {
        let (mut state, initial) = connected_stream_runtime();
        let first_retry = state.apply(&stream_connection_ended(
            &initial,
            VoiceConnectionEnd::Reconnect,
        ));
        let mut active = first_retry
            .connect
            .expect("the first transport failure should reconnect");

        state.apply(&VoiceRuntimeEvent::StreamConnectionEstablished {
            connection_id: active.connection_id,
            stream_key: active.request.stream_key.clone(),
        });

        for _ in 0..MAX_VOICE_RECONNECT_ATTEMPTS {
            active = state
                .apply(&stream_connection_ended(
                    &active,
                    VoiceConnectionEnd::Reconnect,
                ))
                .connect
                .expect("stable playback should restore the full retry budget");
        }
    }

    #[test]
    pub fn stream_failures_distinguish_player_and_transport_errors() {
        for error in [
            std::io::Error::new(std::io::ErrorKind::NotFound, "missing"),
            std::io::Error::new(std::io::ErrorKind::PermissionDenied, "not executable"),
        ] {
            let failure = stream_player_spawn_failure(error);
            assert_eq!(failure.outcome, VoiceConnectionEnd::Stop);
        }
        assert_eq!(
            StreamConnectionFailure::from("stream UDP receive failed".to_owned()).outcome,
            VoiceConnectionEnd::Reconnect
        );
    }

    #[test]
    pub fn stream_gateway_payloads_request_stream_audio_and_h264() {
        let session = StreamGatewaySession {
            connection_id: 1,
            request: stream_request(),
            current_user_id: Id::new(5),
            session_id: "parent-session".to_owned(),
            rtc_server_id: "400".to_owned(),
            rtc_channel_id: Id::new(401),
            endpoint: "stream.example.com".to_owned(),
            token: "stream-token".to_owned(),
            reconnect_delay: Duration::ZERO,
        };
        let identify: Value =
            serde_json::from_str(&stream_identify_payload(&session)).expect("valid identify json");
        assert_eq!(identify["d"]["server_id"], "400");
        assert_eq!(identify["d"]["channel_id"], "401");
        assert_eq!(identify["d"]["video"], true);
        assert!(identify["d"].get("streams").is_none());

        let selected: Value = serde_json::from_str(&stream_select_protocol_payload(
            &DiscoveredVoiceAddress {
                address: "127.0.0.1".to_owned(),
                port: 5000,
            },
            AEAD_XCHACHA20_POLY1305_RTPSIZE,
        ))
        .expect("valid select protocol json");
        assert_eq!(selected["d"]["codecs"][0]["name"], "opus");
        assert_eq!(selected["d"]["codecs"][0]["payload_type"], 120);
        assert_eq!(selected["d"]["codecs"][0]["encode"], false);
        assert_eq!(selected["d"]["codecs"][0]["decode"], true);
        assert_eq!(selected["d"]["codecs"][1]["name"], "H264");
        assert_eq!(selected["d"]["codecs"][1]["payload_type"], 103);
        assert_eq!(selected["d"]["codecs"][1]["decode"], true);

        let wants: Value = serde_json::from_str(&stream_media_sink_wants_payload(
            800,
            900,
            Some(2560 * 1440),
        ))
        .expect("valid media sink wants json");
        assert_eq!(
            wants,
            json!({
                "op": 15,
                "d": {
                    "800": 100,
                    "900": 100,
                    "any": 0,
                    "pixelCounts": {"900": 2560 * 1440}
                }
            })
        );
    }

    #[test]
    pub fn stream_player_controls_the_complete_broadcast() {
        let player = stream_player_command(
            Path::new("/tmp/concord-stream.sdp"),
            Path::new("/tmp/concord-stream-input.conf"),
            "neo",
            "/tmp/concord-stream-mpv.sock",
        )
        .as_std()
        .get_args()
        .map(|argument| argument.to_string_lossy().into_owned())
        .collect::<Vec<_>>();

        assert_eq!(
            player,
            vec![
                "--no-config",
                "--terminal=yes",
                "--load-scripts=no",
                "--osc=no",
                "--msg-level=all=warn,cplayer=v,lavf=v,vd=v,ad=v",
                "--aid=no",
                "--input-ipc-server=/tmp/concord-stream-mpv.sock",
                "--input-conf=/tmp/concord-stream-input.conf",
                "--hwdec=auto-safe",
                "--vd-lavc-threads=0",
                "--stream-buffer-size=1MiB",
                "--audio-buffer=0.05",
                "--cache=yes",
                "--cache-pause=no",
                "--cache-pause-initial=no",
                "--cache-secs=0.15",
                "--demuxer-readahead-secs=0.15",
                "--demuxer-max-bytes=16MiB",
                "--demuxer-max-back-bytes=0",
                "--demuxer=lavf",
                "--demuxer-lavf-format=sdp",
                "--demuxer-lavf-probe-info=nostreams",
                "--demuxer-lavf-analyzeduration=0.1",
                "--demuxer-lavf-probesize=32",
                "--demuxer-lavf-buffersize=262144",
                "--demuxer-lavf-o=protocol_whitelist=[file,udp,rtp],buffer_size=4194304,max_delay=50000,reorder_queue_size=512",
                "--force-window=immediate",
                "--auto-window-resize=no",
                "--geometry=1280x720",
                "--title=Concord - neo's stream",
                "--video-latency-hacks=no",
                "--video-sync=audio",
                "--framedrop=vo",
                "--video-timing-offset=0",
                "--",
                "/tmp/concord-stream.sdp",
            ]
        );
        assert_eq!(
            STREAM_PLAYER_INPUT_CONFIG,
            "SPACE ignore\np ignore\nPAUSE ignore\nPLAYPAUSE ignore\nPAUSEONLY ignore\nXF86_PAUSE ignore\n. ignore\n, ignore\n"
        );
    }

    #[test]
    pub fn stream_player_audio_waits_for_real_media_and_player_readiness() {
        let mut audio = StreamPlayerAudioState::default();

        assert!(!audio.take_enable_request(false));
        assert!(!audio.take_enable_request(true));

        audio.observe_real_packet();
        assert!(!audio.take_enable_request(false));
        assert!(audio.take_enable_request(true));
        assert!(
            !audio.take_enable_request(true),
            "audio should be enabled only once"
        );
    }

    pub fn recovered_stream_audio_packet(sequence: u16, timestamp: u32) -> RecoveredStreamAudioPacket {
        RecoveredStreamAudioPacket {
            marker: false,
            sequence,
            timestamp,
            opus: vec![sequence as u8],
        }
    }

    #[test]
    pub fn stream_audio_recovery_orders_delayed_packets_and_drops_duplicates() {
        let now = Instant::now();
        let mut recovery = StreamAudioRecovery::default();

        assert!(
            recovery
                .push(recovered_stream_audio_packet(10, 10_000), now)
                .ready
                .is_empty()
        );
        assert!(
            recovery
                .push(
                    recovered_stream_audio_packet(12, 11_920),
                    now + Duration::from_millis(20),
                )
                .ready
                .is_empty()
        );
        assert!(
            recovery
                .push(
                    recovered_stream_audio_packet(11, 10_960),
                    now + Duration::from_millis(40),
                )
                .ready
                .is_empty()
        );

        let update = recovery.poll(now + STREAM_AUDIO_REORDER_DELAY);
        assert_eq!(
            update
                .ready
                .iter()
                .map(|packet| packet.sequence)
                .collect::<Vec<_>>(),
            vec![10, 11, 12]
        );
        assert_eq!(update.skipped_sequences, 0);
        assert_eq!(update.dropped_stale_packets, 0);

        let duplicate = recovery.push(
            recovered_stream_audio_packet(12, 11_920),
            now + STREAM_AUDIO_REORDER_DELAY,
        );
        assert!(duplicate.ready.is_empty());
        assert_eq!(duplicate.dropped_stale_packets, 1);
    }

    #[test]
    pub fn stream_audio_recovery_exposes_expired_gaps_across_sequence_wrap() {
        let now = Instant::now();
        let mut recovery = StreamAudioRecovery::default();

        assert!(
            recovery
                .push(recovered_stream_audio_packet(u16::MAX - 1, 10_000), now,)
                .ready
                .is_empty()
        );
        assert_eq!(
            recovery
                .poll(now + STREAM_AUDIO_REORDER_DELAY)
                .ready
                .into_iter()
                .map(|packet| packet.sequence)
                .collect::<Vec<_>>(),
            vec![u16::MAX - 1]
        );
        assert!(
            recovery
                .push(
                    recovered_stream_audio_packet(0, 11_920),
                    now + STREAM_AUDIO_REORDER_DELAY + Duration::from_millis(20),
                )
                .ready
                .is_empty()
        );

        let update =
            recovery.poll(now + STREAM_AUDIO_REORDER_DELAY * 2 + Duration::from_millis(20));
        assert_eq!(update.skipped_sequences, 1);
        assert_eq!(
            update
                .ready
                .iter()
                .map(|packet| packet.sequence)
                .collect::<Vec<_>>(),
            vec![0]
        );

        let late = recovery.push(
            recovered_stream_audio_packet(u16::MAX, 10_960),
            now + STREAM_AUDIO_REORDER_DELAY * 2 + Duration::from_millis(20),
        );
        assert_eq!(late.dropped_stale_packets, 1);
        assert!(late.ready.is_empty());
    }

    #[test]
    pub fn local_rtp_clocks_share_live_time_without_source_clock_offsets() {
        let mut audio = LocalStreamAudioClock::default();
        let mut video = LocalRtpClock::default();

        assert_eq!(
            audio
                .rebase(3_000_000, Duration::from_millis(100), None)
                .local_timestamp,
            4_800
        );
        assert_eq!(
            video.rebase(
                90_000_000,
                Duration::from_millis(125),
                VIDEO_RTP_CLOCK_RATE,
                None,
            ),
            11_250
        );
        assert_eq!(
            audio
                .rebase(3_000_960, Duration::from_millis(120), None)
                .local_timestamp,
            5_760
        );
        assert_eq!(
            video.rebase(
                90_003_000,
                Duration::from_millis(158),
                VIDEO_RTP_CLOCK_RATE,
                None,
            ),
            14_250
        );
    }

    #[test]
    pub fn stream_audio_clock_reanchors_a_backward_source_timestamp() {
        let mut clock = LocalStreamAudioClock::default();

        let first = clock.rebase(3_181_287_385, Duration::ZERO, None);
        assert_eq!(first.local_timestamp, 0);
        assert!(first.discontinuity.is_none());

        let before_reset = clock.rebase(3_181_475_545, Duration::from_millis(3_920), None);
        assert_eq!(before_reset.local_timestamp, 188_160);
        assert!(before_reset.discontinuity.is_none());

        let reset = clock.rebase(3_181_114_584, Duration::from_millis(3_940), None);
        assert_eq!(reset.local_timestamp, 189_120);
        assert_eq!(
            reset.discontinuity,
            Some(StreamAudioClockDiscontinuity {
                source_delta_ticks: -360_961,
                elapsed_delta_ticks: 960,
            })
        );

        let after_reset = clock.rebase(3_181_115_544, Duration::from_millis(3_960), None);
        assert_eq!(after_reset.local_timestamp, 190_080);
        assert!(after_reset.discontinuity.is_none());
        assert_eq!(
            clock.timestamp_at(Duration::from_millis(3_980)),
            Some(191_040)
        );
    }

    #[test]
    pub fn stream_audio_clock_recognizes_recent_replayed_timestamps() {
        let mut clock = LocalStreamAudioClock::default();
        let _ = clock.rebase(10_000, Duration::ZERO, None);

        assert!(clock.is_recent_replay(10_000));
        assert!(clock.is_recent_replay(6_160));
        assert!(!clock.is_recent_replay(4_239));
        assert!(!clock.is_recent_replay(10_960));
    }

    #[test]
    pub fn stream_audio_clock_preserves_a_source_timestamp_wrap() {
        let mut clock = LocalStreamAudioClock::default();

        let before_wrap = clock.rebase(u32::MAX - 479, Duration::from_millis(100), None);
        let after_wrap = clock.rebase(480, Duration::from_millis(120), None);

        assert_eq!(before_wrap.local_timestamp, 4_800);
        assert_eq!(after_wrap.local_timestamp, 5_760);
        assert!(after_wrap.discontinuity.is_none());
    }

}
