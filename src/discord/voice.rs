#[cfg(test)]
use std::num::NonZeroU16;
use std::{
    collections::HashMap,
    fmt,
    sync::Arc,
    time::{Duration, Instant},
};

#[cfg(feature = "voice-playback")]
mod audio_buffer;
mod audio_runtime;
mod broadcast;
#[cfg(feature = "stream-broadcast")]
mod capture;
#[cfg(not(feature = "stream-broadcast"))]
#[path = "voice/capture_disabled.rs"]
mod capture;
mod capture_cancellation;
mod constants;
mod dave;
mod devices;
mod gateway;
mod info;
mod levels;
mod media;
#[cfg(any(test, feature = "voice-playback"))]
mod microphone;
#[cfg(feature = "voice-playback")]
mod noise;
mod opus;
mod outbound;
mod playback;
mod preview;
mod rtp;
mod runtime;
mod state;
mod stream;
mod system_audio;
mod tasks;
mod types;

pub use capture::list_stream_capture_targets;
pub use devices::{VoiceAudioSourceOptions, VoiceAudioSources, list_voice_audio_sources};
#[cfg(all(feature = "voice-playback", not(test)))]
use gateway::voice_speaking_payload;
#[cfg(test)]
use gateway::*;
#[cfg(not(test))]
use gateway::{run_voice_gateway_session, send_voice_binary, send_voice_text};
pub use info::{
    StreamCaptureTarget, StreamCaptureTargetKind, StreamCreateInfo, StreamDeleteInfo,
    StreamServerInfo, StreamUpdateInfo, VoiceConnectionStatus, VoiceScope, VoiceServerInfo,
    VoiceSoundKind, VoiceStateInfo,
};
#[cfg(all(feature = "voice-playback", target_os = "linux", not(test)))]
use microphone::log_captured_alsa_errors;
#[cfg(all(feature = "voice-playback", not(test)))]
use microphone::run_voice_udp_transmit;
#[cfg(test)]
use microphone::*;
pub(crate) use preview::StreamPreviewUploader;
#[cfg(test)]
use runtime::{VoiceRuntimeAction, VoiceRuntimeState};
pub(crate) use runtime::{forward_app_event, run_voice_runtime};
pub(in crate::discord) use state::StreamState;
pub(in crate::discord) use state::VoiceState;
pub use state::{CurrentVoiceConnectionState, VoiceAudioSettings, VoiceParticipantState};

#[cfg(feature = "voice-playback")]
use self::opus::VoiceDecodedAudioOutput;
use self::opus::VoiceOpusDecode;
#[cfg(any(test, feature = "voice-playback"))]
use self::opus::VoiceOpusEncode;
#[cfg(test)]
use self::opus::mix_voice_decoded_samples;
use self::outbound::VoiceOutboundSendBlockReason;
#[cfg(any(test, feature = "voice-playback"))]
use self::outbound::{VoiceOutboundSendEvent, VoiceOutboundSendOutcome, VoiceOutboundSendState};
#[cfg(test)]
use ::opus::{Channels, Decoder as OpusDecoder, SampleRate as OpusSampleRate};
#[cfg(all(test, feature = "voice-playback"))]
use audio_buffer::{VoiceAudioBuffer, VoiceAudioOutputStats};
use audio_runtime::VoiceAudioRuntime;
use dave::{VoiceDaveState, VoiceMediaPayload, voice_speaking_microphone_active};
#[cfg(test)]
use dave::{VoiceSpeakingState, looks_like_dave_media_frame};
#[cfg(feature = "voice-playback")]
use playback::VoiceAudioOutput;
#[cfg(test)]
use playback::VoicePlaybackPlayoutBuffer;
use playback::{VoicePlaybackFrame, VoicePlaybackGate};
#[cfg(test)]
use playback::{VoicePlaybackPostProcess, VoicePlayoutFrame};
#[cfg(all(test, feature = "voice-playback"))]
use playback::{apply_voice_playback_gain_and_limit, write_voice_output_frame};
#[cfg(any(test, feature = "voice-playback"))]
use rtp::VoiceOutboundRtpState;
use rtp::{
    RtpHeader, VoiceRtpDecryptor, VoiceRtpEncryptor, looks_like_rtcp_packet, parse_rtp_header,
    rtcp_sender_ssrc,
};

#[cfg(test)]
use aes_gcm::{
    Aes256Gcm, Nonce as AesGcmNonce,
    aead::{Aead, KeyInit, Payload},
};
#[cfg(test)]
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
#[cfg(feature = "voice-playback")]
use cpal::traits::{DeviceTrait, StreamTrait};
use futures::{SinkExt, StreamExt};
use serde_json::{Value, json};
#[cfg(feature = "voice-playback")]
use std::sync::Mutex as StdMutex;
#[cfg(feature = "voice-playback")]
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use tokio::{
    net::UdpSocket,
    sync::{Mutex, mpsc, watch},
    task::JoinHandle,
    time::{sleep, timeout},
};
use tokio_tungstenite::{connect_async, tungstenite::Message as WsMessage};

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, UserMarker},
};
use crate::logging;
pub use levels::{
    MicrophoneSensitivityDb, VoiceParticipantPlaybackSettings, VoiceParticipantVolumePercent,
    VoiceVolumePercent,
};

use super::{client::AppEventPublisher, events::AppEvent, gateway::GatewayCommand};

pub(crate) use constants::*;
pub(crate) use tasks::*;
pub(crate) use types::*;

type VoiceGatewayStream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;
type VoiceWriter = Arc<Mutex<futures::stream::SplitSink<VoiceGatewayStream, WsMessage>>>;
type VoiceReader = futures::stream::SplitStream<VoiceGatewayStream>;

#[cfg(test)]
mod tests;
