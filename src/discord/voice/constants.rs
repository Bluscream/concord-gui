use super::*;

pub(super) const VOICE_GATEWAY_VERSION: u8 = 9;
pub(super) const VOICE_WEBSOCKET_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
#[cfg(not(feature = "stream-broadcast"))]
pub(super) const STREAM_BROADCAST_FEATURE_DISABLED: &str =
    "stream broadcasting requires the stream-broadcast feature";

pub(crate) fn ensure_stream_broadcast_available() -> Result<(), String> {
    #[cfg(feature = "stream-broadcast")]
    {
        Ok(())
    }
    #[cfg(not(feature = "stream-broadcast"))]
    {
        Err(STREAM_BROADCAST_FEATURE_DISABLED.to_owned())
    }
}

pub(super) const VOICE_RESUME_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(15);
pub(super) const VOICE_CONNECTION_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(3);
pub(super) const VOICE_CONNECTION_STABLE_INTERVAL: Duration = Duration::from_secs(10);
pub(super) const UDP_DISCOVERY_PACKET_LEN: usize = 74;
pub(super) const UDP_DISCOVERY_TIMEOUT: Duration = Duration::from_secs(5);
pub(super) const UDP_KEEPALIVE_PACKET_LEN: usize = 8;
pub(super) const UDP_KEEPALIVE_INTERVAL: Duration = Duration::from_secs(5);
pub(super) const RTP_HEADER_MIN_LEN: usize = 12;
pub(super) const RTP_VERSION: u8 = 2;
pub(super) const DISCORD_VOICE_PAYLOAD_TYPE: u8 = 0x78;
pub(super) const DISCORD_STREAM_VIDEO_PAYLOAD_TYPE: u8 = 103;
pub(super) const DISCORD_STREAM_VIDEO_RTX_PAYLOAD_TYPE: u8 = 104;
pub(super) const LOCAL_STREAM_AUDIO_PAYLOAD_TYPE: u8 = 111;
pub(super) const LOCAL_STREAM_VIDEO_PAYLOAD_TYPE: u8 = 96;
pub(super) const RTP_HEADER_EXTENSION_BYTES: usize = 4;
pub(super) const RTP_EXTENSION_WORD_BYTES: usize = 4;
pub(super) const RTP_AEAD_TAG_BYTES: usize = 16;
pub(super) const RTP_AEAD_NONCE_SUFFIX_BYTES: usize = 4;
pub(super) const RTCP_MIN_PACKET_BYTES: usize = 4;
pub(super) const RTCP_SENDER_SSRC_OFFSET: usize = 4;
pub(super) const RTCP_SENDER_SSRC_BYTES: usize = 4;
pub(super) const DAVE_MIN_SUPPLEMENTAL_BYTES: usize = 11;
pub(super) const DAVE_MAGIC_MARKER: [u8; 2] = [0xfa, 0xfa];
pub(super) const DISCORD_VOICE_SAMPLE_RATE: u32 = 48_000;
pub(super) const DISCORD_VOICE_CHANNELS: u16 = 2;
#[cfg(feature = "voice-playback")]
pub(super) const DISCORD_VOICE_CHANNELS_USIZE: usize = DISCORD_VOICE_CHANNELS as usize;

#[allow(dead_code)]
pub(super) const DISCORD_OPUS_FRAME_SAMPLES_PER_CHANNEL: usize = 960;
#[allow(dead_code)]
pub(super) const DISCORD_OPUS_20MS_STEREO_SAMPLES: usize =
    DISCORD_OPUS_FRAME_SAMPLES_PER_CHANNEL * DISCORD_VOICE_CHANNELS as usize;
#[cfg(any(test, feature = "voice-playback"))]
pub(super) const DISCORD_OPUS_FRAME_DURATION: Duration = Duration::from_millis(20);
#[allow(dead_code)]
pub(super) const DISCORD_OPUS_TIMESTAMP_INCREMENT: u32 =
    DISCORD_OPUS_FRAME_SAMPLES_PER_CHANNEL as u32;
#[allow(dead_code)]
pub(super) const DISCORD_OPUS_SILENCE_FRAME: [u8; 3] = [0xf8, 0xff, 0xfe];
#[allow(dead_code)]
pub(super) const DISCORD_TRAILING_SILENCE_FRAMES: usize = 5;
#[allow(dead_code)]
pub(super) const OPUS_MAX_ENCODED_FRAME_BYTES: usize = 4000;

#[cfg(feature = "voice-playback")]
pub(super) const VOICE_MIC_PCM_FRAME_QUEUE: usize = 16;
#[cfg(feature = "voice-playback")]
pub(super) const VOICE_MIC_MAX_LIVE_FRAMES: usize = 3;
#[cfg(feature = "voice-playback")]
pub(super) const VOICE_MIC_MAX_FRAME_AGE: Duration = Duration::from_millis(60);
#[cfg(feature = "voice-playback")]
pub(super) const VOICE_TRANSMIT_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);
#[cfg(feature = "voice-playback")]
pub(super) const VOICE_MIC_PREFERRED_BUFFER_FRAMES: u32 = 480;
#[cfg(feature = "voice-playback")]
pub(super) const VOICE_MIC_GATE_HANGOVER_FRAMES: u8 = 8;
#[cfg(feature = "voice-playback")]
pub(super) const VOICE_MIC_OVERLOAD_RECOVERY_FRAMES: u8 = 8;
#[cfg(feature = "voice-playback")]
pub(super) const VOICE_MIC_HANDLING_NOISE_SUPPRESSION_FRAMES: u8 = 12;
#[cfg(any(test, feature = "voice-playback"))]
pub(super) const VOICE_MIC_OVERLOAD_MIN_CLIPPED_SAMPLES: usize = 8;
#[cfg(any(test, feature = "voice-playback"))]
pub(super) const VOICE_MIC_OVERLOAD_SEVERE_CLIPPED_SAMPLES: usize =
    DISCORD_OPUS_20MS_STEREO_SAMPLES / 20;
#[cfg(any(test, feature = "voice-playback"))]
pub(super) const VOICE_MIC_OVERLOAD_EXTREME_CLIPPED_SAMPLES: usize =
    DISCORD_OPUS_20MS_STEREO_SAMPLES / 8;
#[cfg(any(test, feature = "voice-playback"))]
pub(super) const VOICE_MIC_HANDLING_NOISE_DELTA: i32 = 42_000;
#[cfg(any(test, feature = "voice-playback"))]
pub(super) const VOICE_MIC_OVERLOAD_CLIPPED_STEP_DELTA: i32 = 32_000;
#[cfg(any(test, feature = "voice-playback"))]
pub(super) const VOICE_MIC_OVERLOAD_IMPULSE_DELTA: i32 = 36_000;
#[cfg(any(test, feature = "voice-playback"))]
pub(super) const VOICE_MIC_OVERLOAD_ATTENUATION_GAIN: f32 = 0.35;
#[cfg(any(test, feature = "voice-playback"))]
pub(super) const VOICE_MIC_HANDLING_NOISE_GAIN: f32 = 0.0;
#[cfg(any(test, feature = "voice-playback"))]
pub(super) const VOICE_MIC_OVERLOAD_TRANSIENT_GAIN: f32 = 0.03;
#[cfg(feature = "voice-playback")]
pub(super) const VOICE_MIC_OVERLOAD_RECOVERY_START_GAIN: f32 = 0.15;
#[allow(dead_code)]
#[cfg(any(test, feature = "voice-playback"))]
pub(super) const VOICE_MIC_TRANSMIT_BOOST_GAIN: f32 = 1.5;
#[cfg(any(test, feature = "voice-playback"))]
pub(super) const VOICE_SOFT_LIMIT_THRESHOLD: f32 = 0.85;
#[cfg(any(test, feature = "voice-playback"))]
pub(super) const VOICE_SOFT_LIMIT_CEILING: f32 = 0.95;
#[cfg(any(test, feature = "voice-playback"))]
pub(super) const VOICE_SOFT_LIMIT_CURVE: f32 = 4.0;
pub(super) const OPUS_MAX_FRAME_SAMPLES_PER_CHANNEL: usize = 5760;
pub(super) const VOICE_PLAYBACK_FRAME_QUEUE: usize = 256;
#[cfg(test)]
pub(super) const VOICE_PLAYBACK_FRAME_DURATION: Duration = Duration::from_millis(20);
pub(super) const VOICE_PLAYBACK_POLL_DURATION: Duration = Duration::from_millis(10);
pub(super) const VOICE_OUTPUT_STATS_LOG_INTERVAL: Duration = Duration::from_secs(5);
pub(super) const VOICE_PLAYBACK_POLL_SAMPLES_PER_CHANNEL: usize = 480;
#[cfg(feature = "voice-playback")]
pub(super) const VOICE_TRANSMIT_STATS_LOG_INTERVAL: Duration = Duration::from_secs(5);
pub(super) const VOICE_PLAYBACK_JITTER_BUFFER_DELAY: Duration = Duration::from_millis(60);
pub(super) const VOICE_PLAYBACK_MAX_BUFFERED_FRAMES_PER_SSRC: usize = 32;
pub(super) const VOICE_PLAYBACK_MAX_CONSECUTIVE_PLC_FRAMES: usize = 5;
#[cfg(feature = "voice-playback")]
pub(super) const VOICE_OUTPUT_UNDERRUN_FADE_MILLIS: u32 = 5;

#[cfg(any(test, feature = "voice-playback"))]
pub(super) fn soft_limit_voice_sample(sample: f32) -> f32 {
    let magnitude = sample.abs();
    if magnitude <= VOICE_SOFT_LIMIT_THRESHOLD {
        return sample;
    }

    let excess = (magnitude - VOICE_SOFT_LIMIT_THRESHOLD) / (1.0 - VOICE_SOFT_LIMIT_THRESHOLD);
    let shaped = VOICE_SOFT_LIMIT_THRESHOLD
        + (VOICE_SOFT_LIMIT_CEILING - VOICE_SOFT_LIMIT_THRESHOLD)
            * (1.0 - 1.0 / (1.0 + VOICE_SOFT_LIMIT_CURVE * excess));
    sample.signum() * shaped.min(VOICE_SOFT_LIMIT_CEILING)
}

pub(super) const VOICE_OUTPUT_LOW_PASS_CUTOFF_HZ: f32 = 8_000.0;
#[cfg(feature = "voice-playback")]
pub(super) const VOICE_AUDIO_OUTPUT_QUEUE: usize = 64;
#[cfg(feature = "voice-playback")]
pub(super) const VOICE_AUDIO_OUTPUT_PREBUFFER_FRAMES: u64 =
    DISCORD_VOICE_SAMPLE_RATE as u64 * 60 / 1_000;
#[cfg(feature = "voice-playback")]
pub(super) const VOICE_PULSE_OUTPUT_BUFFER_FRAMES: u32 = 2_400;
pub(super) const AEAD_AES256_GCM_RTPSIZE: &str = "aead_aes256_gcm_rtpsize";
pub(super) const AEAD_XCHACHA20_POLY1305_RTPSIZE: &str = "aead_xchacha20_poly1305_rtpsize";
pub(super) const VOICE_REMOTE_SPEAKING_TTL: Duration = Duration::from_millis(500);
pub(super) const VOICE_REMOTE_SPEAKING_SWEEP_INTERVAL: Duration = Duration::from_millis(250);

pub(super) const VOICE_OP_READY: u8 = 2;
pub(super) const VOICE_OP_HEARTBEAT: u8 = 3;
pub(super) const VOICE_OP_SESSION_DESCRIPTION: u8 = 4;
pub(super) const VOICE_OP_SPEAKING: u8 = 5;
pub(super) const VOICE_OP_HEARTBEAT_ACK: u8 = 6;
pub(super) const VOICE_OP_RESUME: u8 = 7;
pub(super) const VOICE_OP_HELLO: u8 = 8;
pub(super) const VOICE_OP_RESUMED: u8 = 9;
pub(super) const VOICE_OP_CLIENTS_CONNECT: u8 = 11;
pub(super) const VOICE_OP_VIDEO: u8 = 12;
pub(super) const VOICE_OP_CLIENT_DISCONNECT: u8 = 13;
pub(super) const VOICE_OP_MEDIA_SINK_WANTS: u8 = 15;
pub(super) const VOICE_OP_CLIENT_FLAGS: u8 = 18;
pub(super) const VOICE_OP_CLIENT_PLATFORM: u8 = 20;
pub(super) const VOICE_OP_DAVE_PREPARE_TRANSITION: u8 = 21;
pub(super) const VOICE_OP_DAVE_EXECUTE_TRANSITION: u8 = 22;
pub(super) const VOICE_OP_DAVE_TRANSITION_READY: u8 = 23;
pub(super) const VOICE_OP_DAVE_PREPARE_EPOCH: u8 = 24;
pub(super) const VOICE_OP_DAVE_MLS_EXTERNAL_SENDER: u8 = 25;
pub(super) const VOICE_OP_DAVE_MLS_KEY_PACKAGE: u8 = 26;
pub(super) const VOICE_OP_DAVE_MLS_PROPOSALS: u8 = 27;
pub(super) const VOICE_OP_DAVE_MLS_COMMIT_WELCOME: u8 = 28;
pub(super) const VOICE_OP_DAVE_MLS_ANNOUNCE_COMMIT_TRANSITION: u8 = 29;
pub(super) const VOICE_OP_DAVE_MLS_WELCOME: u8 = 30;
pub(super) const VOICE_OP_DAVE_MLS_INVALID_COMMIT_WELCOME: u8 = 31;
