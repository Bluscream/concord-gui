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
use super::*;


pub mod types;
pub use types::*;
pub mod signals;
pub use signals::*;
pub mod recovery;
pub use recovery::*;
pub mod rtcp;
pub use rtcp::*;
pub mod clock;
pub use clock::*;
pub mod forwarders;
pub use forwarders::*;

#[cfg(test)]
pub mod tests;
#[cfg(test)]
mod test_recovery;
#[cfg(test)]
mod test_playback;
