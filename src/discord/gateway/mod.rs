use std::{
    collections::{BTreeSet, HashMap, HashSet, VecDeque},
    sync::{Arc, RwLock},
    time::Duration,
};

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, UserMarker},
};
use flate2::{Decompress, FlushDecompress, Status};
use futures::SinkExt;
use rand::Rng;
use reqwest::Url;
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio::time::Instant;
use tokio_tungstenite::tungstenite::{Message as WsMessage, protocol::CloseFrame};

use super::{
    ActivityInfo, PresenceStatus, VoiceScope, client::AppEventPublisher, events::AppEvent,
    fingerprint::ClientFingerprint, state::DiscordState,
};
use crate::logging;

mod connection;
mod frame_handler;
mod parser;
mod payloads;
mod scheduler;
mod sender;

#[cfg(test)]
mod tests;

// The tests address these as `super::*`, which is how they read before the
// split moved each one into a submodule beside this file.
pub use connection::run_gateway;
#[cfg(test)]
use frame_handler::parse_gateway_frame;
#[cfg(test)]
use frame_handler::{close_code_outcome, dispatch_command, gateway_request, ready_installation_id};
#[cfg(test)]
use payloads::{
    build_identify_payload, build_resume_payload, create_stream_payload, delete_stream_payload,
    direct_message_subscribe_payload, guild_channel_subscribe_payload, presence_update_payload,
    request_guild_members_by_ids_payload, search_guild_members_payload, voice_state_update_payload,
    watch_stream_payload,
};
#[cfg(test)]
use sender::gateway_guild_member_rate_limit;

pub(in crate::discord) use parser::parse_activity;
use parser::parse_user_account_dispatch;
pub(crate) use parser::{
    parse_channel_info, parse_member_info, parse_message_info, parse_thread_member_info,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GatewayCommand {
    SearchGuildMembers {
        guild_id: Id<GuildMarker>,
        query: String,
        limit: u16,
        presences: bool,
        nonce: String,
    },
    RequestGuildMembersByIds {
        guild_id: Id<GuildMarker>,
        user_ids: Vec<Id<UserMarker>>,
        presences: bool,
    },
    SubscribeDirectMessage {
        channel_id: Id<ChannelMarker>,
    },
    SubscribeGuildChannel {
        guild_id: Id<GuildMarker>,
        channel_id: Id<ChannelMarker>,
    },
    UpdateMemberListSubscription {
        guild_id: Id<GuildMarker>,
        channel_id: Id<ChannelMarker>,
        thread_id: Option<Id<ChannelMarker>>,
        ranges: Vec<(u32, u32)>,
    },
    UpdateVoiceState {
        /// `None` for DM and group-DM calls, which Discord joins with a null
        /// `guild_id` and the DM `channel_id` as the voice target.
        guild_id: Option<Id<GuildMarker>>,
        channel_id: Option<Id<ChannelMarker>>,
        self_mute: bool,
        self_deaf: bool,
    },
    WatchStream {
        stream_key: String,
    },
    CreateStream {
        scope: VoiceScope,
        channel_id: Id<ChannelMarker>,
    },
    DeleteStream {
        stream_key: String,
    },
    UpdatePresence {
        status: PresenceStatus,
        activities: Vec<ActivityInfo>,
    },
    Shutdown {
        voice_leave: Option<GatewayVoiceStateUpdate>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GatewayVoiceStateUpdate {
    pub guild_id: Option<Id<GuildMarker>>,
    pub channel_id: Option<Id<ChannelMarker>>,
    pub self_mute: bool,
    pub self_deaf: bool,
}

#[derive(Clone)]
pub(crate) struct GatewayRuntime {
    pub(crate) fingerprint: Arc<ClientFingerprint>,
    pub(crate) state: Arc<RwLock<DiscordState>>,
    pub(crate) gateway_session_id: Arc<RwLock<Option<String>>>,
    pub(crate) event_publisher: AppEventPublisher,
}

/// Discord user-account gateway endpoint. We pin to `v=9` because the v9
/// dispatch shapes line up with everything `parse_user_account_event` already
/// understands. Discord's browser client uses the stateful `zlib-stream`
/// transport mode, which keeps large READY payloads bounded on the wire.
const GATEWAY_QUERY: &str = "v=9&encoding=json&compress=zlib-stream";

/// Where to connect, from what Discord last told us.
///
/// Discord's own guidance is that clients cache this and refetch only when the
/// cached URL fails to connect, so this reads the cache rather than the
/// network. The query is this client's protocol version rather than anything
/// Discord served, which is why it is added here and not stored.
fn gateway_url() -> String {
    crate::discord::remote_config::load().gateway_websocket_url(GATEWAY_QUERY)
}

/// Bitmask Discord checks before delivering user-account-only payloads such as
/// `READY_SUPPLEMENTAL.merged_presences.friends` and per-friend
/// `PRESENCE_UPDATE` dispatches. Without these bits set Discord assumes the
/// session is a bot and silently drops friend presence streaming.
///
/// Bits enabled (sum 253):
///   0  LAZY_USER_NOTIFICATIONS
///   2  VERSIONED_READ_STATES
///   3  VERSIONED_USER_GUILD_SETTINGS
///   4  DEDUPE_USER_OBJECTS
///   5  PRIORITIZED_READY_PAYLOAD
///   6  MULTIPLE_GUILD_EXPERIMENT_POPULATIONS
///   7  NON_CHANNEL_READ_STATES
const USER_ACCOUNT_CAPABILITIES: u64 = 253;

// Some user-account READY payloads exceed tungstenite's default 16 MiB frame
// cap. Keep both compressed input and decompressed output bounded while still
// allowing large accounts to finish their initial sync.
const GATEWAY_WEBSOCKET_LIMIT: usize = 64 << 20;
const ZLIB_STREAM_SUFFIX: [u8; 4] = [0x00, 0x00, 0xff, 0xff];
const ZLIB_OUTPUT_CHUNK_SIZE: usize = 8 << 10;

const RECONNECT_BASE_DELAY: Duration = Duration::from_millis(500);
const RECONNECT_MAX_DELAY: Duration = Duration::from_secs(30);
// Discord applies this budget to every JSON event on one Gateway connection.
// WebSocket control frames such as Pong and Close are not Gateway events.
const GATEWAY_SEND_LIMIT: usize = 120;
const GATEWAY_SEND_WINDOW: Duration = Duration::from_secs(60);
const GATEWAY_SHUTDOWN_LEAVE_TIMEOUT: Duration = Duration::from_millis(1_500);
/// How long a guild waits before another member request may go out for it.
const GUILD_MEMBER_REQUEST_RESPONSE_TTL: Duration = Duration::from_secs(2 * 60);
const MAX_PENDING_GUILD_MEMBER_REQUESTS: usize = 512;
const MAX_SENT_GUILD_MEMBER_REQUESTS: usize = 512;
const MAX_GATEWAY_RETRY_DELAY: Duration = Duration::from_secs(30 * 60);

type GatewayStream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// Discord uses one zlib stream for the lifetime of a Gateway connection.
/// Individual JSON payloads end with a sync-flush marker and may span several
/// WebSocket binary messages, so neither the input buffer nor the inflater can
/// be recreated for each frame.
struct GatewayZlibDecoder {
    inflater: Decompress,
    pending: Vec<u8>,
}

impl Default for GatewayZlibDecoder {
    fn default() -> Self {
        Self {
            inflater: Decompress::new(true),
            pending: Vec::new(),
        }
    }
}

impl GatewayZlibDecoder {
    fn decode(&mut self, chunk: &[u8]) -> Result<Option<String>, String> {
        let pending_len = self
            .pending
            .len()
            .checked_add(chunk.len())
            .ok_or_else(|| "compressed gateway payload size overflow".to_owned())?;
        if pending_len > GATEWAY_WEBSOCKET_LIMIT {
            return Err(format!(
                "compressed gateway payload exceeds {GATEWAY_WEBSOCKET_LIMIT} bytes"
            ));
        }
        self.pending.extend_from_slice(chunk);
        if !self.pending.ends_with(&ZLIB_STREAM_SUFFIX) {
            return Ok(None);
        }

        let compressed = std::mem::take(&mut self.pending);
        let mut input_offset = 0;
        let mut output = Vec::new();
        loop {
            let mut buffer = [0; ZLIB_OUTPUT_CHUNK_SIZE];
            let input_before = self.inflater.total_in();
            let output_before = self.inflater.total_out();
            let status = self
                .inflater
                .decompress(
                    &compressed[input_offset..],
                    &mut buffer,
                    FlushDecompress::Sync,
                )
                .map_err(|error| format!("gateway zlib decode failed: {error}"))?;
            let consumed = usize::try_from(self.inflater.total_in() - input_before)
                .map_err(|_| "gateway zlib input count exceeds platform size".to_owned())?;
            let produced = usize::try_from(self.inflater.total_out() - output_before)
                .map_err(|_| "gateway zlib output count exceeds platform size".to_owned())?;
            input_offset += consumed;
            output.extend_from_slice(&buffer[..produced]);

            if output.len() > GATEWAY_WEBSOCKET_LIMIT {
                return Err(format!(
                    "decompressed gateway payload exceeds {GATEWAY_WEBSOCKET_LIMIT} bytes"
                ));
            }
            if matches!(status, Status::StreamEnd) {
                if input_offset != compressed.len() {
                    return Err(
                        "gateway zlib stream ended before the input was consumed".to_owned()
                    );
                }
                self.inflater = Decompress::new(true);
                break;
            }
            if input_offset == compressed.len() && produced < buffer.len() {
                break;
            }
            if consumed == 0 && produced == 0 {
                if input_offset == compressed.len() {
                    break;
                }
                return Err("gateway zlib decoder made no progress".to_owned());
            }
        }

        String::from_utf8(output)
            .map(Some)
            .map_err(|error| format!("gateway payload is not valid UTF-8: {error}"))
    }
}

/// Shared, lockable WebSocket sink. Both the heartbeat task and the main
/// dispatch loop need to send over the same connection, so the sink lives
/// behind a `Mutex<Arc<…>>` instead of being moved into either side.
type WriterHandle = Arc<Mutex<futures::stream::SplitSink<GatewayStream, WsMessage>>>;

#[derive(Clone)]
struct GatewaySender {
    // Heartbeats, session setup, and voice state use the urgent queue so a
    // backlog of UI commands cannot delay time-sensitive traffic.
    urgent_tx: mpsc::UnboundedSender<GatewaySendRequest>,
    normal_tx: mpsc::UnboundedSender<GatewaySendRequest>,
}

struct GatewaySendRequest {
    payload: String,
    completion: Option<oneshot::Sender<Result<(), String>>>,
}

#[derive(Default)]
struct GatewaySendWindow {
    sent_at: VecDeque<Instant>,
}

#[derive(Default)]
struct SubscriptionDeduper {
    direct_messages: HashSet<Id<ChannelMarker>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum GuildMemberRequestKind {
    Search {
        query: String,
        limit: u16,
        presences: bool,
    },
    ByIds {
        user_ids: Vec<Id<UserMarker>>,
        presences: bool,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct GuildMemberRequest {
    guild_id: Id<GuildMarker>,
    nonce: String,
    kind: GuildMemberRequestKind,
}

#[derive(Clone, Debug)]
struct PendingGuildMemberRequest {
    request: GuildMemberRequest,
    send_at: Instant,
}

struct InFlightGuildMemberRequest {
    completion: oneshot::Receiver<Result<(), String>>,
}

struct ScheduledGuildMemberRequest {
    request: GuildMemberRequest,
    accepted: bool,
    retry_at: Option<Instant>,
}

struct SentGuildMemberRequest {
    request: GuildMemberRequest,
    sent_at: Instant,
}

struct GuildMemberRequestScheduler {
    pending: VecDeque<PendingGuildMemberRequest>,
    in_flight: Option<ScheduledGuildMemberRequest>,
    awaiting_response: VecDeque<SentGuildMemberRequest>,
    /// Earliest a guild may be asked again, kept outside `pending` so it
    /// survives a request being dropped and re-queued across a resume.
    guild_rate_limit_until: HashMap<Id<GuildMarker>, Instant>,
    next_nonce: u64,
}

#[derive(Default)]
struct GatewaySessionResources {
    guild_member_requests: GuildMemberRequestScheduler,
    last_presence: Option<GatewayPresence>,
}

struct GatewayPresence {
    status: PresenceStatus,
    activities: Vec<ActivityInfo>,
}

#[derive(Clone, Copy)]
struct GatewayPublishContext<'a> {
    state: &'a Arc<RwLock<DiscordState>>,
    gateway_session_id: &'a Arc<RwLock<Option<String>>>,
    event_publisher: &'a AppEventPublisher,
}

#[derive(Clone, Copy)]
struct FrameContext<'a> {
    sequence_cell: &'a Arc<Mutex<Option<u64>>>,
    heartbeat_ack: &'a Arc<Mutex<HeartbeatAckState>>,
    sender: &'a GatewaySender,
    fingerprint: &'a ClientFingerprint,
    publish: GatewayPublishContext<'a>,
}

#[derive(Default)]
struct HeartbeatAckState {
    awaiting_ack: bool,
}

impl HeartbeatAckState {
    fn mark_heartbeat_sent(&mut self) -> bool {
        if self.awaiting_ack {
            return false;
        }
        self.awaiting_ack = true;
        true
    }

    fn mark_ack_received(&mut self) {
        self.awaiting_ack = false;
    }
}

/// What to do after one connection lifecycle ends.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ConnectionOutcome {
    /// The websocket dropped or Discord asked us to reconnect. Try to RESUME
    /// using the saved session_id + sequence number.
    Resume,
    /// Authentication failed or Discord told us the session is dead. Throw
    /// the saved session away and start over with a fresh IDENTIFY.
    Reidentify,
    /// The downstream consumers went away, so stop the loop entirely.
    Stop,
    /// Discord rejected this gateway session in a way that retrying the same
    /// token or shard configuration cannot fix. Keep the UI alive so it can
    /// show the published gateway error.
    Fatal,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum GatewayHandshake {
    Identify,
    Resume { session_id: String, sequence: u64 },
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct GatewayConnectionPlan {
    url: String,
    handshake: GatewayHandshake,
    recovery_warning: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum MalformedFrameRecovery {
    #[default]
    None,
    ResumeAttempted {
        after_sequence: Option<u64>,
    },
}

/// Mutable Gateway bookkeeping that survives reconnects. The session cursor
/// supports op-6 RESUME, while the recovery marker prevents a malformed replay
/// from reconnecting forever at the same confirmed sequence.
#[derive(Default)]
struct SessionState {
    session_id: Option<String>,
    resume_url: Option<String>,
    last_sequence: Option<u64>,
    has_received_ready: bool,
    /// Whether the current connection reached READY or RESUMED. Read and
    /// cleared by the reconnect loop to reset the backoff after a healthy
    /// session.
    established: bool,
    malformed_frame_recovery: MalformedFrameRecovery,
}

fn normalized_resume_url(resume_url: &str) -> Result<String, String> {
    let mut url =
        Url::parse(resume_url).map_err(|error| format!("invalid Gateway resume URL: {error}"))?;
    if url.scheme() != "wss" {
        return Err("invalid Gateway resume URL: scheme must be wss".to_owned());
    }
    if url.host_str().is_none() {
        return Err("invalid Gateway resume URL: host is missing".to_owned());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("invalid Gateway resume URL: credentials are not allowed".to_owned());
    }
    if url.fragment().is_some() {
        return Err("invalid Gateway resume URL: fragments are not allowed".to_owned());
    }
    let retained_query = url
        .query_pairs()
        .filter(|(name, _)| !matches!(name.as_ref(), "v" | "encoding" | "compress"))
        .map(|(name, value)| (name.into_owned(), value.into_owned()))
        .collect::<Vec<_>>();

    url.set_query(None);
    {
        let mut query = url.query_pairs_mut();
        query.extend_pairs(retained_query);
        query.append_pair("v", "9");
        query.append_pair("encoding", "json");
        query.append_pair("compress", "zlib-stream");
    }
    Ok(url.to_string())
}

#[derive(Debug, Eq, PartialEq)]
struct MalformedGatewayFrame {
    message: String,
    outcome: FrameOutcome,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FrameOutcome {
    Continue,
    Resume,
    Reidentify,
}

#[derive(Debug, Eq, PartialEq)]
struct GuildMemberRateLimit {
    pub(super) guild_id: Id<GuildMarker>,
    pub(super) nonce: Option<String>,
    pub(super) retry_after: Duration,
}
