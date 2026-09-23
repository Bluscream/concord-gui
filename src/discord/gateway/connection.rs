use super::frame_handler::*;
use super::payloads::*;
use super::sender::*;
use super::*;
use futures::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::time::{Instant, sleep, timeout};
use tokio_tungstenite::{connect_async_tls_with_config, tungstenite::Message as WsMessage};

use crate::support::tls;

impl SessionState {
    pub(super) fn clear(&mut self) {
        self.session_id = None;
        self.resume_url = None;
        self.last_sequence = None;
        self.malformed_frame_recovery = MalformedFrameRecovery::None;
    }

    pub(super) fn can_resume(&self) -> bool {
        self.session_id.is_some() && self.resume_url.is_some() && self.last_sequence.is_some()
    }

    pub(super) fn next_connection(&mut self) -> GatewayConnectionPlan {
        if !self.can_resume() {
            let had_partial_resume_state = self.session_id.is_some()
                || self.resume_url.is_some()
                || self.last_sequence.is_some();
            self.clear();
            return GatewayConnectionPlan {
                url: gateway_url(),
                handshake: GatewayHandshake::Identify,
                recovery_warning: had_partial_resume_state.then(|| {
                    "Gateway resume state is incomplete; starting a new session".to_owned()
                }),
            };
        }

        let resume_url = self
            .resume_url
            .as_deref()
            .expect("resume eligibility requires a resume URL");
        match normalized_resume_url(resume_url) {
            Ok(url) => GatewayConnectionPlan {
                url,
                handshake: GatewayHandshake::Resume {
                    session_id: self
                        .session_id
                        .clone()
                        .expect("resume eligibility requires a session id"),
                    sequence: self
                        .last_sequence
                        .expect("resume eligibility requires a sequence"),
                },
                recovery_warning: None,
            },
            Err(error) => {
                self.clear();
                GatewayConnectionPlan {
                    url: gateway_url(),
                    handshake: GatewayHandshake::Identify,
                    recovery_warning: Some(error),
                }
            }
        }
    }

    pub(super) fn malformed_frame_outcome(&mut self) -> FrameOutcome {
        if !self.can_resume() {
            return FrameOutcome::Reidentify;
        }
        let after_sequence = self.last_sequence;
        if self.malformed_frame_recovery
            == (MalformedFrameRecovery::ResumeAttempted { after_sequence })
        {
            return FrameOutcome::Reidentify;
        }
        self.malformed_frame_recovery = MalformedFrameRecovery::ResumeAttempted { after_sequence };
        FrameOutcome::Resume
    }

    pub(super) fn record_sequence(&mut self, sequence: u64) {
        self.last_sequence = Some(sequence);
        self.malformed_frame_recovery = MalformedFrameRecovery::None;
    }

    pub(super) fn abandon_failed_resume(&mut self, handshake: &GatewayHandshake) -> bool {
        if !matches!(handshake, GatewayHandshake::Resume { .. }) {
            return false;
        }
        self.clear();
        true
    }
}

pub async fn run_gateway(
    token: String,
    mut commands: mpsc::UnboundedReceiver<GatewayCommand>,
    runtime: GatewayRuntime,
) {
    let mut session = SessionState::default();
    let mut resources = GatewaySessionResources::default();
    let mut backoff = RECONNECT_BASE_DELAY;
    let mut publish_gateway_closed = true;

    loop {
        let publish = GatewayPublishContext {
            state: &runtime.state,
            gateway_session_id: &runtime.gateway_session_id,
            event_publisher: &runtime.event_publisher,
        };
        let outcome = match connect_and_run(
            &token,
            &mut commands,
            &mut session,
            &mut resources,
            &runtime.fingerprint,
            publish,
        )
        .await
        {
            Ok(outcome) => outcome,
            Err(error) => {
                logging::error("gateway", format!("connection error: {error}"));
                publish_gateway_event(
                    publish,
                    AppEvent::GatewayError {
                        message: format!("connection error: {error}"),
                    },
                )
                .await;
                ConnectionOutcome::Resume
            }
        };

        match outcome {
            ConnectionOutcome::Stop => break,
            ConnectionOutcome::Resume => {}
            ConnectionOutcome::Reidentify => {
                session.clear();
                clear_published_gateway_session(publish);
            }
            ConnectionOutcome::Fatal => {
                publish_gateway_closed = false;
                break;
            }
        }

        // Exponential backoff with full jitter so a flapping network doesn't
        // hammer Discord. A connection that reached READY/RESUMED was healthy,
        // so its disconnect restarts the backoff from the base delay.
        if std::mem::take(&mut session.established) {
            backoff = RECONNECT_BASE_DELAY;
        }
        let jitter = rand::thread_rng().gen_range(0..=backoff.as_millis() as u64);
        let delay = Duration::from_millis(jitter);
        logging::debug(
            "gateway",
            format!("reconnecting in {}ms", delay.as_millis()),
        );
        sleep(delay).await;
        backoff = (backoff * 2).min(RECONNECT_MAX_DELAY);
    }

    if publish_gateway_closed {
        let publish = GatewayPublishContext {
            state: &runtime.state,
            gateway_session_id: &runtime.gateway_session_id,
            event_publisher: &runtime.event_publisher,
        };
        publish_gateway_event(publish, AppEvent::GatewayClosed).await;
    }
}

pub(super) async fn connect_and_run(
    token: &str,
    commands: &mut mpsc::UnboundedReceiver<GatewayCommand>,
    session: &mut SessionState,
    resources: &mut GatewaySessionResources,
    fingerprint: &ClientFingerprint,
    publish: GatewayPublishContext<'_>,
) -> Result<ConnectionOutcome, String> {
    let connection = session.next_connection();
    if let Some(error) = connection.recovery_warning.as_ref() {
        log_and_publish_gateway_error(publish, error.clone()).await;
        clear_published_gateway_session(publish);
    }
    logging::debug("gateway", format!("connecting to {}", connection.url));

    let request = gateway_request(&connection.url, fingerprint)
        .map_err(|error| gateway_setup_failure(session, &connection.handshake, publish, error))?;
    let connector = tls::websocket_connector()
        .map_err(|error| gateway_setup_failure(session, &connection.handshake, publish, error))?;
    let (ws, _response) = connect_async_tls_with_config(
        request,
        Some(gateway_websocket_config()),
        false,
        Some(connector),
    )
    .await
    .map_err(|error| {
        gateway_setup_failure(
            session,
            &connection.handshake,
            publish,
            format!("websocket connect failed: {error}"),
        )
    })?;
    let (writer, mut reader) = ws.split();
    let writer = Arc::new(Mutex::new(writer));
    let (sender, mut gateway_send_error_rx, gateway_writer_task) =
        spawn_gateway_sender(Arc::clone(&writer));
    let mut subscription_deduper = SubscriptionDeduper::default();
    let mut zlib_decoder = GatewayZlibDecoder::default();

    // Discord must speak first with op-10 HELLO carrying heartbeat_interval.
    // If the first frame is anything else, fail fast and try a clean
    // re-identify.
    let hello_frame = loop {
        match reader.next().await {
            Some(Ok(WsMessage::Text(text))) => break text.to_string(),
            Some(Ok(WsMessage::Binary(chunk))) => {
                match zlib_decoder.decode(&chunk).map_err(|error| {
                    gateway_setup_failure(session, &connection.handshake, publish, error)
                })? {
                    Some(text) => break text,
                    None => continue,
                }
            }
            Some(Ok(WsMessage::Close(frame))) => {
                let message =
                    websocket_close_message("websocket closed before HELLO", frame.as_ref());
                log_and_publish_gateway_error(publish, message).await;
                return Ok(ConnectionOutcome::Reidentify);
            }
            Some(Ok(_)) => {
                return Err(gateway_setup_failure(
                    session,
                    &connection.handshake,
                    publish,
                    "unexpected control frame before HELLO",
                ));
            }
            Some(Err(error)) => {
                return Err(gateway_setup_failure(
                    session,
                    &connection.handshake,
                    publish,
                    format!("read HELLO failed: {error}"),
                ));
            }
            None => {
                return Err(gateway_setup_failure(
                    session,
                    &connection.handshake,
                    publish,
                    "connection closed before HELLO",
                ));
            }
        }
    };
    let hello: Value = serde_json::from_str(&hello_frame).map_err(|error| {
        gateway_setup_failure(
            session,
            &connection.handshake,
            publish,
            format!("HELLO parse: {error}"),
        )
    })?;
    if hello.get("op").and_then(Value::as_u64) != Some(10) {
        return Err(gateway_setup_failure(
            session,
            &connection.handshake,
            publish,
            format!(
                "first frame was not HELLO: {}",
                hello.get("op").and_then(Value::as_u64).unwrap_or_default()
            ),
        ));
    }
    let heartbeat_interval_ms = hello
        .get("d")
        .and_then(|d| d.get("heartbeat_interval"))
        .and_then(Value::as_u64)
        .unwrap_or(41250);
    let heartbeat_interval = Duration::from_millis(heartbeat_interval_ms);

    // Either resume with the saved session or send a fresh IDENTIFY. RESUME
    // tells Discord to replay missed dispatches. This is good for transient drops.
    // IDENTIFY rebuilds the world from scratch.
    match &connection.handshake {
        GatewayHandshake::Resume {
            session_id,
            sequence,
        } => {
            let payload = build_resume_payload(token, session_id, *sequence);
            send_text(&sender, payload).await.map_err(|error| {
                gateway_setup_failure(session, &connection.handshake, publish, error)
            })?;
            logging::debug("gateway", "RESUME sent");
        }
        GatewayHandshake::Identify => {
            resources
                .guild_member_requests
                .start_new_session(Instant::now());
            let client_state = {
                let state = publish
                    .state
                    .read()
                    .expect("discord state lock is not poisoned");
                if session.has_received_ready && resources.last_presence.is_none() {
                    resources.last_presence = current_gateway_presence(&state);
                }
                state.client_cache_state()
            };
            let reidentify_presence = session
                .has_received_ready
                .then_some(resources.last_presence.as_ref())
                .flatten();
            let payload =
                build_identify_payload(token, fingerprint, reidentify_presence, client_state);
            send_text(&sender, payload).await.map_err(|error| {
                gateway_setup_failure(session, &connection.handshake, publish, error)
            })?;
            logging::debug("gateway", "IDENTIFY sent");
        }
    }

    // Background heartbeat task driven by Discord's interval. We jitter the
    // first beat per the API recommendation. The task reads the latest seq
    // from a shared atomic via the sequence cell.
    let sender_for_heartbeat = sender.clone();
    let sequence_cell: Arc<Mutex<Option<u64>>> = Arc::new(Mutex::new(session.last_sequence));
    let sequence_for_heartbeat = Arc::clone(&sequence_cell);
    let heartbeat_ack: Arc<Mutex<HeartbeatAckState>> = Arc::default();
    let heartbeat_ack_for_task = Arc::clone(&heartbeat_ack);
    let (heartbeat_timeout_tx, mut heartbeat_timeout_rx) = mpsc::unbounded_channel();
    let initial_jitter = Duration::from_millis(
        rand::thread_rng().gen_range(0..=heartbeat_interval.as_millis() as u64),
    );
    let heartbeat_task = tokio::spawn(async move {
        sleep(initial_jitter).await;
        loop {
            {
                let mut state = heartbeat_ack_for_task.lock().await;
                if !state.mark_heartbeat_sent() {
                    logging::error("gateway", "heartbeat ACK timeout; reconnecting");
                    let _ = heartbeat_timeout_tx.send(());
                    break;
                }
            }
            let seq = *sequence_for_heartbeat.lock().await;
            let payload = json!({"op": 1, "d": seq}).to_string();
            if let Err(error) = send_text(&sender_for_heartbeat, payload).await {
                logging::error("gateway", format!("heartbeat send failed: {error}"));
                let _ = heartbeat_timeout_tx.send(());
                break;
            }
            sleep(heartbeat_interval).await;
        }
    });

    // Main loop: race incoming frames against outgoing work. Keep branch
    // polling fair because a busy command or dispatch stream must not starve a
    // due member request or its writer completion.
    let mut member_request_send: Option<InFlightGuildMemberRequest> = None;
    let outcome = loop {
        let member_request_delay = member_request_send
            .is_none()
            .then(|| resources.guild_member_requests.next_delay(Instant::now()))
            .flatten();
        tokio::select! {
            maybe_command = commands.recv() => {
                match maybe_command {
                    Some(command) => {
                        if let GatewayCommand::Shutdown { voice_leave } = command {
                            if let Some(voice_leave) = voice_leave {
                                let leave_result = timeout(
                                    GATEWAY_SHUTDOWN_LEAVE_TIMEOUT,
                                    sender.send_urgent(voice_state_update_payload(
                                        voice_leave.guild_id,
                                        voice_leave.channel_id,
                                        voice_leave.self_mute,
                                        voice_leave.self_deaf,
                                    )),
                                )
                                .await
                                .map_err(|_| {
                                    "voice leave timed out before gateway shutdown".to_owned()
                                })
                                .and_then(|result| result);
                                if let Err(error) = leave_result {
                                    log_and_publish_gateway_error(
                                        publish,
                                        format!(
                                            "voice leave before gateway shutdown failed: {error}"
                                        ),
                                    )
                                    .await;
                                }
                            }
                            if let Err(error) = close_websocket(&writer).await {
                                log_and_publish_gateway_error(
                                    publish,
                                    format!("gateway shutdown failed: {error}"),
                                )
                                .await;
                            }
                            break ConnectionOutcome::Stop;
                        } else if let Err(error) =
                            dispatch_command(
                                &sender,
                                command,
                                &mut subscription_deduper,
                                resources,
                            )
                        {
                            let message = format!("command send failed: {error}");
                            log_and_publish_gateway_error(publish, message).await;
                            break ConnectionOutcome::Resume;
                        }
                    }
                    None => break ConnectionOutcome::Stop,
                }
            }
            frame = reader.next() => {
                match frame {
                    Some(Ok(WsMessage::Text(text))) => {
                        let frame_context = FrameContext {
                            sequence_cell: &sequence_cell,
                            heartbeat_ack: &heartbeat_ack,
                            sender: &sender,
                            fingerprint,
                            publish,
                        };
                        match handle_json_frame(
                            &text,
                            session,
                            resources,
                            frame_context,
                        ).await {
                            FrameOutcome::Continue => {}
                            FrameOutcome::Resume => break ConnectionOutcome::Resume,
                            FrameOutcome::Reidentify => break ConnectionOutcome::Reidentify,
                        }
                    }
                    Some(Ok(WsMessage::Binary(chunk))) => {
                        let text = match zlib_decoder.decode(&chunk) {
                            Ok(Some(text)) => text,
                            Ok(None) => continue,
                            Err(error) => {
                                log_and_publish_gateway_error(publish, error).await;
                                break ConnectionOutcome::Resume;
                            }
                        };
                        let frame_context = FrameContext {
                            sequence_cell: &sequence_cell,
                            heartbeat_ack: &heartbeat_ack,
                            sender: &sender,
                            fingerprint,
                            publish,
                        };
                        match handle_json_frame(
                            &text,
                            session,
                            resources,
                            frame_context,
                        ).await {
                            FrameOutcome::Continue => {}
                            FrameOutcome::Resume => break ConnectionOutcome::Resume,
                            FrameOutcome::Reidentify => break ConnectionOutcome::Reidentify,
                        }
                    }
                    Some(Ok(WsMessage::Ping(payload))) => {
                        let mut writer = writer.lock().await;
                        if let Err(error) = writer.send(WsMessage::Pong(payload)).await {
                            let message = format!("websocket pong send failed: {error}");
                            log_and_publish_gateway_error(publish, message).await;
                            break ConnectionOutcome::Resume;
                        }
                    }
                    Some(Ok(WsMessage::Pong(_))) | Some(Ok(WsMessage::Frame(_))) => {}
                    Some(Ok(WsMessage::Close(frame))) => {
                        let outcome = close_outcome(frame.as_ref());
                        let message = websocket_close_message("websocket closed", frame.as_ref());
                        log_and_publish_gateway_error(publish, message).await;
                        break outcome;
                    }
                    Some(Err(error)) => {
                        let message = format!("websocket read error: {error}");
                        log_and_publish_gateway_error(publish, message).await;
                        break ConnectionOutcome::Resume;
                    }
                    None => {
                        let message = "websocket closed without frame".to_owned();
                        log_and_publish_gateway_error(publish, message).await;
                        break ConnectionOutcome::Resume;
                    }
                }
            }
            _ = heartbeat_timeout_rx.recv() => {
                break ConnectionOutcome::Resume;
            }
            Some(error) = gateway_send_error_rx.recv() => {
                log_and_publish_gateway_error(publish, error).await;
                break ConnectionOutcome::Resume;
            }
            send_result = async {
                member_request_send
                    .as_mut()
                    .expect("guard ensures a guild member request is in flight")
                    .wait()
                    .await
            }, if member_request_send.is_some() => {
                member_request_send
                    .take()
                    .expect("completed guild member request exists");
                if let Err(error) = send_result {
                    let message = format!("guild member request send failed: {error}");
                    log_and_publish_gateway_error(publish, message).await;
                    break ConnectionOutcome::Resume;
                }
                resources
                    .guild_member_requests
                    .complete_send(Instant::now());
            }
            _ = sleep(member_request_delay.unwrap_or_default()), if member_request_delay.is_some() => {
                let Some(payload) = resources
                    .guild_member_requests
                    .start_due(Instant::now())
                else {
                    continue;
                };
                let completion = match sender.enqueue_normal(payload) {
                    Ok(completion) => completion,
                    Err(error) => {
                        let message = format!("guild member request send failed: {error}");
                        log_and_publish_gateway_error(publish, message).await;
                        break ConnectionOutcome::Resume;
                    }
                };
                member_request_send = Some(InFlightGuildMemberRequest { completion });
            }
        }
    };

    if matches!(
        outcome,
        ConnectionOutcome::Resume | ConnectionOutcome::Reidentify
    ) {
        resources
            .guild_member_requests
            .prepare_reconnect(Instant::now());
    } else {
        resources
            .guild_member_requests
            .cancel_in_flight(Instant::now());
    }
    heartbeat_task.abort();
    gateway_writer_task.abort();
    Ok(outcome)
}
