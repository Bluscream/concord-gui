use std::time::Duration;

use tokio::{sync::mpsc, time::timeout};

use super::super::media::GatewayChildTasks;
use super::super::{
    StreamBroadcastRequest, StreamCreateInfo, StreamServerInfo, VoiceConnectionEnd,
    VoiceRuntimeEvent, VoiceScope, VoiceSessionDescription,
};
use super::*;

#[cfg(test)]
pub mod shared {
    use super::*;

    use crate::discord::{
        ids::{
            Id,
            marker::{ChannelMarker, GuildMarker, UserMarker},
        },
        voice::{
            AEAD_XCHACHA20_POLY1305_RTPSIZE, StreamCaptureTarget, StreamCaptureTargetKind,
            VoiceStateInfo,
        },
    };

    pub fn request() -> StreamBroadcastRequest {
        StreamBroadcastRequest {
            stream_key: "guild:10:20:30".to_owned(),
            scope: VoiceScope::Guild(Id::new(10)),
            channel_id: Id::new(20),
            target: StreamCaptureTarget {
                kind: StreamCaptureTargetKind::Display,
                id: 1,
                title: "Screen: Display".to_owned(),
            },
        }
    }

    #[test]
    pub fn newer_capture_request_invalidates_the_previous_generation() {
        let registry = StreamBroadcastCaptureRegistry::default();
        let stream_key = "guild:10:20:30";

        registry.activate(stream_key.to_owned(), 1);
        assert!(registry.is_active(stream_key, 1));

        registry.activate(stream_key.to_owned(), 2);
        assert!(!registry.is_active(stream_key, 1));
        assert!(registry.is_active(stream_key, 2));
    }

    pub fn session() -> StreamBroadcastGatewaySession {
        StreamBroadcastGatewaySession {
            connection_id: 1,
            request: request(),
            current_user_id: Id::new(30),
            session_id: "session".to_owned(),
            rtc_server_id: "11".to_owned(),
            rtc_channel_id: Id::new(20),
            endpoint: "streams.example".to_owned(),
            token: "token".to_owned(),
            reconnect_delay: Duration::ZERO,
        }
    }

    #[test]
    pub fn stream_broadcast_gateway_session_debug_redacts_token() {
        let mut session = session();
        session.token = "broadcast-secret-token".to_owned();
        let debug = format!("{session:?}");

        assert!(!debug.contains("broadcast-secret-token"));
        assert!(debug.contains("<redacted>"));
    }

    #[test]
    pub fn broadcast_failure_classifies_transport_as_reconnect_and_local_media_as_stop() {
        let transport =
            BroadcastConnectionFailure::from("broadcast websocket connection failed".to_owned());
        let local_media =
            BroadcastConnectionFailure::stop("start stream capture failed".to_owned());

        assert_eq!(transport.outcome, VoiceConnectionEnd::Reconnect);
        assert_eq!(local_media.outcome, VoiceConnectionEnd::Stop);
    }

    #[test]
    pub fn closed_capture_frame_channel_preserves_pending_error() {
        let (errors_tx, mut errors_rx) = mpsc::unbounded_channel();
        errors_tx
            .send("display recorder creation failed".to_owned())
            .expect("capture error receiver should remain open");
        drop(errors_tx);

        let failure = capture_completion_after_frame_channel_closed(&mut errors_rx)
            .expect_err("pending capture error should stop the broadcast");

        assert_eq!(
            failure,
            BroadcastConnectionFailure::stop("display recorder creation failed")
        );
    }

    pub fn connected_broadcast_runtime()
    -> (StreamBroadcastRuntimeState, StreamBroadcastGatewaySession) {
        let mut state = StreamBroadcastRuntimeState::default();
        state.apply(&VoiceRuntimeEvent::CurrentUserReady(Some(Id::new(30))));
        state.apply(&VoiceRuntimeEvent::VoiceState(VoiceStateInfo {
            guild_id: Some(Id::new(10)),
            channel_id: Some(Id::new(20)),
            user_id: Id::new(30),
            session_id: Some("session".to_owned()),
            member: None,
            deaf: false,
            mute: false,
            self_deaf: false,
            self_mute: false,
            self_stream: false,
            self_video: false,
        }));
        state.apply(&VoiceRuntimeEvent::BroadcastStreamRequested(request()));
        state.apply(&VoiceRuntimeEvent::StreamCreate(StreamCreateInfo {
            stream_key: request().stream_key,
            rtc_server_id: "11".to_owned(),
            rtc_channel_id: Id::new(20),
            viewer_ids: Vec::new(),
            paused: false,
        }));
        let update = state.apply(&VoiceRuntimeEvent::StreamServer(StreamServerInfo {
            stream_key: request().stream_key,
            endpoint: Some("streams.example".to_owned()),
            token: "token".to_owned(),
        }));
        let session = update.connect.expect("broadcast session should be ready");
        (state, session)
    }

    pub fn broadcast_connection_ended(
        session: &StreamBroadcastGatewaySession,
        outcome: VoiceConnectionEnd,
    ) -> VoiceRuntimeEvent {
        VoiceRuntimeEvent::BroadcastStreamConnectionEnded {
            connection_id: session.connection_id,
            stream_key: session.request.stream_key.clone(),
            outcome,
        }
    }

    pub fn voice_description() -> VoiceSessionDescription {
        VoiceSessionDescription {
            mode: AEAD_XCHACHA20_POLY1305_RTPSIZE.to_owned(),
            secret_key: vec![9; 32],
            dave_protocol_version: None,
            video_codec: Some("H264".to_owned()),
        }
    }

    pub struct DropNotice(Option<tokio::sync::oneshot::Sender<()>>);

    impl Drop for DropNotice {
        fn drop(&mut self) {
            if let Some(sender) = self.0.take() {
                let _ = sender.send(());
            }
        }
    }

    #[tokio::test]
    pub async fn broadcast_child_tasks_abort_when_their_owner_is_dropped() {
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (dropped_tx, dropped_rx) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            let _notice = DropNotice(Some(dropped_tx));
            let _ = started_tx.send(());
            std::future::pending::<()>().await;
        });
        started_rx
            .await
            .expect("test broadcast child task should start");
        let mut tasks = GatewayChildTasks::default();
        tasks.replace_media(task).await;

        drop(tasks);

        timeout(Duration::from_secs(1), dropped_rx)
            .await
            .expect("broadcast child task should stop")
            .expect("broadcast child task should report cleanup");
    }

    #[tokio::test]
    pub async fn broadcast_child_tasks_stop_media_before_replacement() {
        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
        let (cleaned_tx, cleaned_rx) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            let _ = stop_rx.await;
            let _ = cleaned_tx.send(());
        });
        let mut tasks = GatewayChildTasks::default();
        tasks.install_media_gracefully(task, stop_tx);

        tasks.shutdown_media().await;

        cleaned_rx
            .await
            .expect("old media should finish cleanup before replacement starts");
        assert!(!tasks.has_media());
    }

    #[test]
    pub fn broadcast_media_replacement_ignores_previous_cleanup_result() {
        assert!(
            broadcast_media_result_for_generation(2, 1, Ok(())).is_none(),
            "a replaced media task must not stop its replacement"
        );
        assert_eq!(
            broadcast_media_result_for_generation(2, 2, Ok(())),
            Some(Ok(()))
        );
    }

    #[tokio::test]
    pub async fn broadcast_audio_task_aborts_when_its_owner_is_dropped() {
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (dropped_tx, dropped_rx) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            let _notice = DropNotice(Some(dropped_tx));
            let _ = started_tx.send(());
            std::future::pending::<()>().await;
        });
        started_rx
            .await
            .expect("test broadcast audio task should start");
        let audio_task = BroadcastAudioTask {
            task: Some(task),
            capture: None,
        };

        drop(audio_task);

        timeout(Duration::from_secs(1), dropped_rx)
            .await
            .expect("broadcast audio task should stop")
            .expect("broadcast audio task should report cleanup");
    }

    #[test]
    pub fn broadcast_runtime_connects_only_after_voice_create_and_server_state() {
        let mut state = StreamBroadcastRuntimeState::default();
        let guild_id = Id::<GuildMarker>::new(10);
        let channel_id = Id::<ChannelMarker>::new(20);
        let user_id = Id::<UserMarker>::new(30);

        state.apply(&VoiceRuntimeEvent::CurrentUserReady(Some(user_id)));
        state.apply(&VoiceRuntimeEvent::VoiceState(VoiceStateInfo {
            guild_id: Some(guild_id),
            channel_id: Some(channel_id),
            user_id,
            session_id: Some("session".to_owned()),
            member: None,
            deaf: false,
            mute: false,
            self_deaf: false,
            self_mute: false,
            self_stream: false,
            self_video: false,
        }));
        assert!(
            state
                .apply(&VoiceRuntimeEvent::BroadcastStreamRequested(request()))
                .connect
                .is_none()
        );
        assert!(
            state
                .apply(&VoiceRuntimeEvent::StreamCreate(StreamCreateInfo {
                    stream_key: request().stream_key,
                    rtc_server_id: "11".to_owned(),
                    rtc_channel_id: channel_id,
                    viewer_ids: Vec::new(),
                    paused: false,
                }))
                .connect
                .is_none()
        );

        let update = state.apply(&VoiceRuntimeEvent::StreamServer(StreamServerInfo {
            stream_key: request().stream_key,
            endpoint: Some("streams.example".to_owned()),
            token: "token".to_owned(),
        }));

        let session = update.connect.expect("all broadcast state is ready");
        assert_eq!(session.current_user_id, user_id);
        assert_eq!(session.request.channel_id, channel_id);
        assert_eq!(session.endpoint, "streams.example");
    }

    #[test]
    pub fn broadcast_runtime_pending_cancel_ends_preparing_once() {
        let request = request();
        let mut state = StreamBroadcastRuntimeState::default();
        state.apply(&VoiceRuntimeEvent::BroadcastStreamRequested(
            request.clone(),
        ));

        let cancelled = state.apply(&VoiceRuntimeEvent::BroadcastStreamCancelled {
            stream_key: request.stream_key.clone(),
        });
        assert_eq!(cancelled.broadcast_ended, Some(request.clone()));

        let repeated = state.apply(&VoiceRuntimeEvent::BroadcastStreamCancelled {
            stream_key: request.stream_key,
        });
        assert!(repeated.broadcast_ended.is_none());
    }

    #[test]
    pub fn capture_failure_reports_error_and_ends_the_preparing_broadcast() {
        let request = request();
        let mut state = StreamBroadcastRuntimeState::default();
        state.apply(&VoiceRuntimeEvent::BroadcastStreamRequested(
            request.clone(),
        ));

        let failed = state.apply(&VoiceRuntimeEvent::BroadcastStreamCaptureFailed {
            request_id: 1,
            stream_key: request.stream_key.clone(),
            error: "PipeWire format negotiation failed".to_owned(),
        });

        assert_eq!(
            failed.error.as_deref(),
            Some("Could not broadcast stream: PipeWire format negotiation failed")
        );
        assert_eq!(failed.broadcast_ended, Some(request));
        assert!(state.requested.is_none());
    }

    #[test]
    pub fn broadcast_request_repairs_an_orphaned_active_session() {
        let active = session();
        let mut state = StreamBroadcastRuntimeState {
            active: Some(active.clone()),
            ..StreamBroadcastRuntimeState::default()
        };

        let update = state.apply(&VoiceRuntimeEvent::BroadcastStreamRequested(request()));

        assert_eq!(
            update.close_stream_key.as_deref(),
            Some(active.request.stream_key.as_str())
        );
        assert!(update.send_delete);
        assert!(state.active.is_none());
        assert_eq!(state.requested, Some(request()));
    }

    #[test]
    pub fn broadcast_runtime_rotates_active_stream_servers() {
        let (mut state, initial) = connected_broadcast_runtime();

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
        assert!(rotated.retain_capture);
        assert!(rotated.broadcast_ended.is_none());
        let replacement = rotated
            .connect
            .expect("new stream server starts a replacement broadcast");
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
        assert!(unavailable.retain_capture);
        assert!(unavailable.connect.is_none());

        let reallocated = state.apply(&VoiceRuntimeEvent::StreamServer(StreamServerInfo {
            stream_key: initial.request.stream_key.clone(),
            endpoint: Some("reallocated.example.com".to_owned()),
            token: "reallocated-token".to_owned(),
        }));
        let active = reallocated
            .connect
            .expect("reallocated stream server reconnects broadcast");
        assert_ne!(active.connection_id, replacement.connection_id);

        let stale_end = state.apply(&VoiceRuntimeEvent::BroadcastStreamConnectionEnded {
            connection_id: replacement.connection_id,
            stream_key: replacement.request.stream_key,
            outcome: VoiceConnectionEnd::Stop,
        });
        assert!(stale_end.close_stream_key.is_none());
        assert!(stale_end.connect.is_none());
        assert_eq!(
            state
                .active
                .as_ref()
                .expect("reallocated broadcast remains active")
                .connection_id,
            active.connection_id
        );
    }
}
