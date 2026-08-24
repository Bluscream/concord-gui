use std::{
    collections::{HashMap, VecDeque},
    sync::{
        Arc, Mutex as StdMutex,
        atomic::{AtomicU32, Ordering},
    },
    time::{Duration, Instant},
};

use futures::StreamExt;
use rand::random;
use serde_json::{Value, json};
use tokio::{
    net::UdpSocket,
    sync::{Mutex, mpsc, oneshot},
    task::JoinHandle,
    time::{Instant as TokioInstant, sleep, sleep_until, timeout},
};
use tokio_tungstenite::{connect_async, tungstenite::Message as WsMessage};
use uuid::Uuid;

use super::media::{
    GatewayChildTasks, build_rtcp_sender_report, current_unix_time, packetize_h264_payloads,
};
use super::runtime::MAX_VOICE_RECONNECT_ATTEMPTS;
use super::{
    DISCORD_OPUS_TIMESTAMP_INCREMENT, DISCORD_STREAM_VIDEO_PAYLOAD_TYPE,
    DISCORD_STREAM_VIDEO_RTX_PAYLOAD_TYPE, DISCORD_VOICE_PAYLOAD_TYPE, DiscoveredVoiceAddress,
    RTP_AEAD_NONCE_SUFFIX_BYTES, RTP_AEAD_TAG_BYTES, RTP_HEADER_EXTENSION_BYTES,
    RTP_HEADER_MIN_LEN, RTP_VERSION, StreamBroadcastRequest, StreamCreateInfo, StreamServerInfo,
    VOICE_OP_READY, VOICE_OP_SESSION_DESCRIPTION, VOICE_OP_SPEAKING,
    VOICE_WEBSOCKET_CONNECT_TIMEOUT, VoiceConnectionEnd, VoiceDaveState, VoiceRuntimeEvent,
    VoiceScope, VoiceSessionDescription, VoiceStatusPublisher, capture,
    dave::VoiceDaveOutboundPayload,
    gateway,
    opus::VoiceOpusEncode,
    preview::{StreamPreviewUploadTask, StreamPreviewUploader},
    rtp::{
        VoiceRtpDecryptor, VoiceRtpEncryptor, build_voice_rtp_packet_with_marker,
        looks_like_rtcp_packet, parse_rtp_header,
    },
    system_audio::{self, SYSTEM_AUDIO_FRAME_QUEUE},
};

pub mod types;
pub use types::*;
pub mod pipeline;
pub use pipeline::*;
pub mod session;
pub use session::*;
pub mod rtcp;
pub use rtcp::*;

#[cfg(test)]
pub mod tests;
#[cfg(test)]
mod test_pipeline;
