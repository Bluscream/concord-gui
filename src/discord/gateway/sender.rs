use super::frame_handler::redact_token;
use super::*;
use futures::SinkExt;
use serde_json::Value;
use std::collections::VecDeque;
use tokio::time::{Instant, sleep};
use tokio_tungstenite::tungstenite::Message as WsMessage;

impl GatewaySender {
    pub(super) async fn send_urgent(&self, payload: String) -> Result<(), String> {
        let (completion_tx, completion_rx) = oneshot::channel();
        self.urgent_tx
            .send(GatewaySendRequest {
                payload,
                completion: Some(completion_tx),
            })
            .map_err(|_| "gateway writer task stopped".to_owned())?;
        completion_rx
            .await
            .map_err(|_| "gateway writer task stopped before send completed".to_owned())?
    }

    pub(super) fn enqueue_urgent_text(&self, payload: String) -> Result<(), String> {
        self.urgent_tx
            .send(GatewaySendRequest {
                payload,
                completion: None,
            })
            .map_err(|_| "gateway writer task stopped".to_owned())
    }

    pub(super) fn enqueue_text(&self, payload: String) -> Result<(), String> {
        self.normal_tx
            .send(GatewaySendRequest {
                payload,
                completion: None,
            })
            .map_err(|_| "gateway writer task stopped".to_owned())
    }

    pub(super) fn enqueue_normal(
        &self,
        payload: String,
    ) -> Result<oneshot::Receiver<Result<(), String>>, String> {
        let (completion_tx, completion_rx) = oneshot::channel();
        self.normal_tx
            .send(GatewaySendRequest {
                payload,
                completion: Some(completion_tx),
            })
            .map_err(|_| "gateway writer task stopped".to_owned())?;
        Ok(completion_rx)
    }
}

impl GatewaySendWindow {
    pub(super) fn delay_at(&mut self, now: Instant) -> Option<Duration> {
        while self
            .sent_at
            .front()
            .is_some_and(|sent_at| now.duration_since(*sent_at) >= GATEWAY_SEND_WINDOW)
        {
            self.sent_at.pop_front();
        }
        if self.sent_at.len() < GATEWAY_SEND_LIMIT {
            return None;
        }
        self.sent_at
            .front()
            .map(|sent_at| (*sent_at + GATEWAY_SEND_WINDOW).duration_since(now))
    }

    pub(super) fn record(&mut self, now: Instant) {
        self.sent_at.push_back(now);
    }
}

pub(super) fn spawn_gateway_sender(
    writer: WriterHandle,
) -> (
    GatewaySender,
    mpsc::UnboundedReceiver<String>,
    tokio::task::JoinHandle<()>,
) {
    let (urgent_tx, urgent_rx) = mpsc::unbounded_channel();
    let (normal_tx, normal_rx) = mpsc::unbounded_channel();
    let (error_tx, error_rx) = mpsc::unbounded_channel();
    let task = tokio::spawn(run_gateway_sender(writer, urgent_rx, normal_rx, error_tx));
    (
        GatewaySender {
            urgent_tx,
            normal_tx,
        },
        error_rx,
        task,
    )
}

pub(super) fn gateway_guild_member_rate_limit(value: &Value) -> Option<GuildMemberRateLimit> {
    let data = value.get("d")?;
    if data.get("opcode").and_then(Value::as_u64) != Some(8) {
        return None;
    }
    let guild_id = data
        .get("meta")?
        .get("guild_id")?
        .as_str()?
        .parse::<u64>()
        .ok()
        .and_then(Id::new_checked)?;
    let retry_after = data.get("retry_after")?.as_f64()?;
    if !retry_after.is_finite() || retry_after < 0.0 {
        return None;
    }
    Some(GuildMemberRateLimit {
        guild_id,
        nonce: data
            .get("meta")
            .and_then(|meta| meta.get("nonce"))
            .and_then(Value::as_str)
            .map(str::to_owned),
        retry_after: Duration::from_secs_f64(
            retry_after.min(MAX_GATEWAY_RETRY_DELAY.as_secs_f64()),
        ),
    })
}

pub(super) fn gateway_guild_member_chunk_nonce(value: &Value) -> Option<&str> {
    value
        .get("d")
        .and_then(|data| data.get("nonce"))
        .and_then(Value::as_str)
}

fn drain_gateway_requests(
    receiver: &mut mpsc::UnboundedReceiver<GatewaySendRequest>,
    queue: &mut VecDeque<GatewaySendRequest>,
    open: &mut bool,
) {
    while *open {
        match receiver.try_recv() {
            Ok(request) => queue.push_back(request),
            Err(mpsc::error::TryRecvError::Empty) => return,
            Err(mpsc::error::TryRecvError::Disconnected) => {
                *open = false;
                return;
            }
        }
    }
}

pub(super) async fn run_gateway_sender(
    writer: WriterHandle,
    mut urgent_rx: mpsc::UnboundedReceiver<GatewaySendRequest>,
    mut normal_rx: mpsc::UnboundedReceiver<GatewaySendRequest>,
    error_tx: mpsc::UnboundedSender<String>,
) {
    let mut urgent = VecDeque::new();
    let mut normal = VecDeque::new();
    let mut urgent_open = true;
    let mut normal_open = true;
    let mut window = GatewaySendWindow::default();

    loop {
        drain_gateway_requests(&mut urgent_rx, &mut urgent, &mut urgent_open);
        drain_gateway_requests(&mut normal_rx, &mut normal, &mut normal_open);

        if urgent.is_empty() && normal.is_empty() {
            if !urgent_open && !normal_open {
                return;
            }
            tokio::select! {
                biased;
                request = urgent_rx.recv(), if urgent_open => {
                    match request {
                        Some(request) => urgent.push_back(request),
                        None => urgent_open = false,
                    }
                }
                request = normal_rx.recv(), if normal_open => {
                    match request {
                        Some(request) => normal.push_back(request),
                        None => normal_open = false,
                    }
                }
            }
            continue;
        }

        if let Some(delay) = window.delay_at(Instant::now()) {
            tokio::select! {
                biased;
                request = urgent_rx.recv(), if urgent_open => {
                    match request {
                        Some(request) => urgent.push_back(request),
                        None => urgent_open = false,
                    }
                }
                _ = sleep(delay) => {}
            }
            continue;
        }

        let request = urgent
            .pop_front()
            .or_else(|| normal.pop_front())
            .expect("gateway send queue is not empty");
        window.record(Instant::now());
        // Outbound frames too, so a trace shows both halves of a conversation.
        // Redacted rather than skipped: IDENTIFY carries the account token,
        // and a log that quietly omitted the frame would leave a gap exactly
        // where somebody debugging a login needs to look.
        if logging::trace_enabled() {
            logging::trace("gateway", format!("-> {}", redact_token(&request.payload)));
        }
        let result = {
            let mut writer = writer.lock().await;
            writer
                .send(WsMessage::Text(request.payload.into()))
                .await
                .map_err(|error| format!("websocket send failed: {error}"))
        };
        if let Some(completion) = request.completion {
            let _ = completion.send(result.clone());
        }
        if let Err(error) = result {
            let _ = error_tx.send(error);
            return;
        }
    }
}
