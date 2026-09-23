use super::payloads::*;
use super::sender::*;
use super::*;
use crate::discord::fingerprint::discord_gateway_headers;
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::{
    client::IntoClientRequest, handshake::client::Request, protocol::WebSocketConfig,
};

pub(super) async fn handle_json_frame(
    text: &str,
    session: &mut SessionState,
    resources: &mut GatewaySessionResources,
    frame_context: FrameContext<'_>,
) -> FrameOutcome {
    let value = match parse_gateway_frame(text, session) {
        Ok(value) => value,
        Err(failure) => {
            log_and_publish_gateway_error(frame_context.publish, failure.message).await;
            return failure.outcome;
        }
    };
    handle_frame(value, session, frame_context, resources).await
}

pub(super) fn parse_gateway_frame(
    text: &str,
    session: &mut SessionState,
) -> Result<Value, MalformedGatewayFrame> {
    serde_json::from_str(text).map_err(|error| MalformedGatewayFrame {
        message: format!("gateway JSON parse failed: {error}"),
        // A single Resume can repair transient transport corruption. If the
        // same confirmed sequence fails again, abandon the replay buffer and
        // re-identify instead of reconnecting forever.
        outcome: session.malformed_frame_outcome(),
    })
}

pub(super) fn gateway_websocket_config() -> WebSocketConfig {
    WebSocketConfig::default()
        .max_message_size(Some(GATEWAY_WEBSOCKET_LIMIT))
        .max_frame_size(Some(GATEWAY_WEBSOCKET_LIMIT))
}

pub(super) fn gateway_request(
    url: &str,
    fingerprint: &ClientFingerprint,
) -> Result<Request, String> {
    let mut request = url
        .into_client_request()
        .map_err(|error| format!("websocket request failed: {error}"))?;
    request
        .headers_mut()
        .extend(discord_gateway_headers(fingerprint));
    Ok(request)
}

/// Blank out an account token in a frame about to be sent.
///
/// Textual rather than by parsing: this runs on a payload that is already
/// serialised, and a trace that had to re-parse every frame to hide one field
/// would cost more than it is worth.
pub(super) fn redact_token(payload: &str) -> String {
    let Some(start) = payload.find("\"token\":\"") else {
        return payload.to_owned();
    };
    let value_at = start + "\"token\":\"".len();
    let Some(end) = payload[value_at..].find('"') else {
        // A malformed frame is not worth guessing at, and printing it whole
        // could print the token.
        return "[unparsable frame, withheld]".to_owned();
    };
    format!(
        "{}[redacted]{}",
        &payload[..value_at],
        &payload[value_at + end..]
    )
}

pub(super) async fn handle_frame(
    value: Value,
    session: &mut SessionState,
    context: FrameContext<'_>,
    resources: &mut GatewaySessionResources,
) -> FrameOutcome {
    let op = value.get("op").and_then(Value::as_u64).unwrap_or_default();
    // Every frame the gateway sends, before this client has decided what it
    // means. The payload is written whole under trace: a dispatch this build
    // does not model yet appears here as itself, which is the only way to see
    // one at all. It carries no credential - the token goes up in IDENTIFY,
    // never down.
    if logging::trace_enabled() {
        let dispatch = value.get("t").and_then(Value::as_str).unwrap_or("-");
        logging::trace("gateway", format!("<- op={op} t={dispatch} {value}"));
    }
    match op {
        // Dispatch
        0 => {
            if let Some(seq) = value.get("s").and_then(Value::as_u64) {
                session.record_sequence(seq);
                *context.sequence_cell.lock().await = Some(seq);
            }
            let dispatch_type = value.get("t").and_then(Value::as_str).unwrap_or("");
            let mut publish_reidentified = false;
            if dispatch_type == "RATE_LIMITED"
                && let Some(rate_limit) = gateway_guild_member_rate_limit(&value)
            {
                logging::debug(
                    "gateway",
                    format!(
                        "guild member requests rate limited: guild={} retry_after_ms={}",
                        rate_limit.guild_id.get(),
                        rate_limit.retry_after.as_millis()
                    ),
                );
                resources.guild_member_requests.apply_rate_limit(
                    rate_limit.guild_id,
                    rate_limit.nonce.as_deref(),
                    rate_limit.retry_after,
                    Instant::now(),
                );
            } else if dispatch_type == "GUILD_MEMBERS_CHUNK"
                && let Some(nonce) = gateway_guild_member_chunk_nonce(&value)
            {
                resources.guild_member_requests.acknowledge(nonce);
            }
            // Capture the session_id and resume_url from READY so a later
            // disconnect can RESUME instead of redoing the heavy initial sync.
            if dispatch_type == "READY"
                && let Some(d) = value.get("d")
            {
                let was_reidentify = session.has_received_ready;
                if let Some(installation_id) = ready_installation_id(d) {
                    match context.fingerprint.update_installation_id(installation_id) {
                        Ok(true) => {
                            logging::debug("fingerprint", "updated installation id from READY");
                        }
                        Ok(false) => {}
                        Err(error) => logging::debug(
                            "fingerprint",
                            format!("could not persist READY installation id: {error}"),
                        ),
                    }
                }
                session.session_id = d
                    .get("session_id")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                session.resume_url = d
                    .get("resume_gateway_url")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                *context
                    .publish
                    .gateway_session_id
                    .write()
                    .expect("gateway session id lock is not poisoned") = session.session_id.clone();
                if was_reidentify {
                    publish_reidentified = true;
                }
                session.has_received_ready = true;
                session.established = true;
            } else if dispatch_type == "RESUMED" {
                session.established = true;
                publish_gateway_event(context.publish, AppEvent::GatewayResumed).await;
            }
            if let Some(parsed) = parse_user_account_dispatch(value) {
                publish_gateway_event(
                    context.publish,
                    AppEvent::GatewayDispatchReceived {
                        dispatch: parsed.dispatch,
                    },
                )
                .await;
                for app_event in parsed.events {
                    publish_gateway_event(context.publish, app_event).await;
                }
            }
            if publish_reidentified {
                publish_gateway_event(context.publish, AppEvent::GatewayReidentified).await;
            }
            FrameOutcome::Continue
        }
        // Answer Discord heartbeat requests immediately. The background task
        // only paces our own heartbeat sends.
        1 => {
            let seq = *context.sequence_cell.lock().await;
            let payload = json!({"op": 1, "d": seq}).to_string();
            context.heartbeat_ack.lock().await.mark_heartbeat_sent();
            if let Err(error) = send_text(context.sender, payload).await {
                let message = format!("heartbeat response send failed: {error}");
                log_and_publish_gateway_error(context.publish, message).await;
            }
            FrameOutcome::Continue
        }
        // Discord wants us to drop and resume. Saved session_id and seq make
        // the resume cheap.
        7 => {
            logging::debug("gateway", "RECONNECT requested");
            FrameOutcome::Resume
        }
        // `d` tells us whether an invalid session is resumable. Anything else
        // means we have to throw it away.
        9 => {
            let resumable = value.get("d").and_then(Value::as_bool).unwrap_or(false);
            logging::debug("gateway", format!("INVALID_SESSION resumable={resumable}"));
            if resumable {
                FrameOutcome::Resume
            } else {
                FrameOutcome::Reidentify
            }
        }
        11 => {
            context.heartbeat_ack.lock().await.mark_ack_received();
            FrameOutcome::Continue
        }
        other => {
            logging::debug("gateway", format!("unhandled gateway op={other}"));
            FrameOutcome::Continue
        }
    }
}

pub(super) async fn publish_gateway_event(context: GatewayPublishContext<'_>, event: AppEvent) {
    context.event_publisher.publish(event).await;
}

pub(super) fn clear_published_gateway_session(context: GatewayPublishContext<'_>) {
    *context
        .gateway_session_id
        .write()
        .expect("gateway session id lock is not poisoned") = None;
}

/// A Resume is only useful when its connection and initial handshake both
/// succeed. Transport failures after the socket opens must not select the same
/// failed Resume plan forever, so the next loop starts a fresh Identify.
pub(super) fn gateway_setup_failure(
    session: &mut SessionState,
    handshake: &GatewayHandshake,
    context: GatewayPublishContext<'_>,
    error: impl Into<String>,
) -> String {
    if session.abandon_failed_resume(handshake) {
        clear_published_gateway_session(context);
    }
    error.into()
}

pub(super) fn ready_installation_id(ready: &Value) -> Option<&str> {
    ready
        .get("apex_experiments")
        .and_then(|experiments| experiments.get("installation"))
        .and_then(Value::as_str)
}

pub(super) async fn log_and_publish_gateway_error(
    context: GatewayPublishContext<'_>,
    message: String,
) {
    logging::error("gateway", &message);
    publish_gateway_event(context, AppEvent::GatewayError { message }).await;
}

pub(super) fn close_outcome(frame: Option<&CloseFrame>) -> ConnectionOutcome {
    let Some(frame) = frame else {
        return ConnectionOutcome::Resume;
    };
    close_code_outcome(u16::from(frame.code))
}

pub(super) fn close_code_outcome(code: u16) -> ConnectionOutcome {
    // Authentication and gateway configuration failures are not transient.
    // Retrying the same IDENTIFY would hide the real problem behind Loading...
    // and can loop forever for codes such as 4004.
    match code {
        4004 | 4010..=4014 => ConnectionOutcome::Fatal,
        4007 | 4009 => ConnectionOutcome::Reidentify,
        4000..=4003 | 4005 | 4008 => ConnectionOutcome::Resume,
        _ => ConnectionOutcome::Reidentify,
    }
}

pub(super) fn websocket_close_message(context: &str, frame: Option<&CloseFrame>) -> String {
    if let Some(frame) = frame {
        format!(
            "{context}: code={} reason={:?}",
            u16::from(frame.code),
            frame.reason.as_str()
        )
    } else {
        context.to_owned()
    }
}

pub(super) fn dispatch_command(
    sender: &GatewaySender,
    command: GatewayCommand,
    subscription_deduper: &mut SubscriptionDeduper,
    resources: &mut GatewaySessionResources,
) -> Result<(), String> {
    if !subscription_deduper.should_send(&command) {
        logging::debug("gateway", "skipping duplicate channel subscription");
        return Ok(());
    }

    if let GatewayCommand::UpdatePresence { status, activities } = &command {
        resources.last_presence = Some(GatewayPresence {
            status: *status,
            activities: activities.clone(),
        });
    }
    let urgent = matches!(
        command,
        GatewayCommand::UpdateVoiceState { .. }
            | GatewayCommand::WatchStream { .. }
            | GatewayCommand::CreateStream { .. }
            | GatewayCommand::DeleteStream { .. }
    );
    let payload = match command {
        GatewayCommand::SearchGuildMembers {
            guild_id,
            query,
            limit,
            presences,
            nonce,
        } => {
            logging::debug(
                "gateway",
                format!(
                    "requesting guild members: guild={} query_len={} limit={} presences={}",
                    guild_id.get(),
                    query.len(),
                    limit,
                    presences
                ),
            );
            if !resources.guild_member_requests.enqueue_search(
                guild_id,
                query,
                limit,
                presences,
                nonce,
                Instant::now(),
            ) {
                logging::debug(
                    "gateway",
                    "dropping guild member search because the session queue is full",
                );
            }
            return Ok(());
        }
        GatewayCommand::RequestGuildMembersByIds {
            guild_id,
            user_ids,
            presences,
        } => {
            logging::debug(
                "gateway",
                format!(
                    "requesting guild members by id: guild={} users={} presences={}",
                    guild_id.get(),
                    user_ids.len(),
                    presences
                ),
            );
            resources.guild_member_requests.enqueue_by_ids(
                guild_id,
                user_ids,
                presences,
                Instant::now(),
            );
            return Ok(());
        }
        GatewayCommand::SubscribeDirectMessage { channel_id } => {
            logging::debug(
                "gateway",
                format!("subscribing to DM: channel={}", channel_id.get()),
            );
            direct_message_subscribe_payload(channel_id)
        }
        GatewayCommand::SubscribeGuildChannel {
            guild_id,
            channel_id,
        } => {
            logging::debug(
                "gateway",
                format!(
                    "subscribing to guild channel: guild={} channel={}",
                    guild_id.get(),
                    channel_id.get()
                ),
            );
            guild_channel_subscribe_payload(guild_id, channel_id, &[(0, 99)], None)
        }
        GatewayCommand::UpdateMemberListSubscription {
            guild_id,
            channel_id,
            thread_id,
            ranges,
        } => {
            logging::debug(
                "gateway",
                format!(
                    "updating member list ranges: guild={} channel={} ranges={:?}",
                    guild_id.get(),
                    channel_id.get(),
                    ranges
                ),
            );
            let thread_member_lists = thread_id.into_iter().collect::<Vec<_>>();
            guild_channel_subscribe_payload(
                guild_id,
                channel_id,
                &ranges,
                Some(&thread_member_lists),
            )
        }
        GatewayCommand::UpdateVoiceState {
            guild_id,
            channel_id,
            self_mute,
            self_deaf,
        } => {
            logging::debug(
                "gateway",
                format!(
                    "updating voice state: guild={} channel={} self_mute={} self_deaf={}",
                    guild_id.map(|id| id.get()).unwrap_or_default(),
                    channel_id.map(|id| id.get()).unwrap_or_default(),
                    self_mute,
                    self_deaf,
                ),
            );
            voice_state_update_payload(guild_id, channel_id, self_mute, self_deaf)
        }
        GatewayCommand::WatchStream { stream_key } => {
            logging::debug("gateway", format!("watching stream: {stream_key}"));
            watch_stream_payload(&stream_key)
        }
        GatewayCommand::CreateStream { scope, channel_id } => {
            logging::debug(
                "gateway",
                format!("creating stream: scope={scope:?} channel={channel_id}"),
            );
            create_stream_payload(scope, channel_id)
        }
        GatewayCommand::DeleteStream { stream_key } => {
            logging::debug("gateway", format!("deleting stream: {stream_key}"));
            delete_stream_payload(&stream_key)
        }
        GatewayCommand::UpdatePresence { status, activities } => {
            logging::debug(
                "gateway",
                format!(
                    "updating presence status: {} activities={}",
                    status.label(),
                    activities.len()
                ),
            );
            presence_update_payload(status, &activities)
        }
        GatewayCommand::Shutdown { .. } => return Ok(()),
    };
    if urgent {
        sender.enqueue_urgent_text(payload)
    } else {
        sender.enqueue_text(payload)
    }
}

pub(super) async fn close_websocket(writer: &WriterHandle) -> Result<(), String> {
    let mut writer = writer.lock().await;
    writer
        .close()
        .await
        .map_err(|error| format!("websocket close failed: {error}"))
}

pub(super) async fn send_text(sender: &GatewaySender, payload: String) -> Result<(), String> {
    sender.send_urgent(payload).await
}
