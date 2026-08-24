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

use super::super::media::{
    GatewayChildTasks, build_rtcp_sender_report, current_unix_time, packetize_h264_payloads,
};
use super::super::runtime::MAX_VOICE_RECONNECT_ATTEMPTS;
use super::super::{
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
use super::*;


pub fn duration_for_bitrate(bytes: usize, bits_per_second: u64) -> Duration {
    let nanoseconds = (bytes as u128)
        .saturating_mul(8)
        .saturating_mul(1_000_000_000)
        .div_ceil(u128::from(bits_per_second.max(1)));
    Duration::from_nanos(u64::try_from(nanoseconds).unwrap_or(u64::MAX))
}

pub fn duration_div_ceil(duration: Duration, divisor: u32) -> Duration {
    let nanoseconds = duration.as_nanos().div_ceil(u128::from(divisor.max(1)));
    Duration::from_nanos(u64::try_from(nanoseconds).unwrap_or(u64::MAX))
}

pub fn receiver_report_has_new_loss(
    previous: &mut HashMap<u32, i32>,
    reporter_ssrc: u32,
    current: i32,
) -> bool {
    let previous = previous.insert(reporter_ssrc, current);
    current > 0 && previous.is_none_or(|previous| current > previous)
}

pub fn parse_broadcast_rtcp_feedback(
    packet: &[u8],
    video_ssrc: u32,
) -> Result<BroadcastRtcpFeedback, String> {
    let mut feedback = BroadcastRtcpFeedback::default();
    let mut offset = 0usize;

    while offset < packet.len() {
        let packet_len = rtcp_packet_len(&packet[offset..])?;
        let rtcp = &packet[offset..offset + packet_len];
        let feedback_format = rtcp[0] & 0x1f;

        match rtcp[1] {
            201 => parse_receiver_report(rtcp, video_ssrc, &mut feedback)?,
            205 if feedback_format == 1 => parse_generic_nack(rtcp, video_ssrc, &mut feedback)?,
            206 if feedback_format == 1 => {
                if rtcp.len() < 12 {
                    return Err("RTCP PLI packet is too short".to_owned());
                }
                let media_ssrc =
                    u32::from_be_bytes(rtcp[8..12].try_into().expect("validated PLI media SSRC"));
                feedback.request_keyframe |= media_ssrc == video_ssrc;
            }
            206 if feedback_format == 4 => {
                parse_full_intra_request(rtcp, video_ssrc, &mut feedback)?
            }
            _ => {}
        }
        offset += packet_len;
    }

    feedback.nack_sequences.sort_unstable();
    feedback.nack_sequences.dedup();
    Ok(feedback)
}

pub fn rtcp_packet_len(packet: &[u8]) -> Result<usize, String> {
    if packet.len() < 4 {
        return Err("RTCP packet is shorter than its header".to_owned());
    }
    if packet[0] >> 6 != RTP_VERSION {
        return Err("RTCP packet has unsupported version".to_owned());
    }
    if !(192..=223).contains(&packet[1]) {
        return Err("RTCP packet has invalid packet type".to_owned());
    }
    let packet_len = (usize::from(u16::from_be_bytes([packet[2], packet[3]])) + 1) * 4;
    if packet_len < 4 || packet_len > packet.len() {
        return Err("RTCP packet length exceeds received data".to_owned());
    }
    Ok(packet_len)
}

pub fn parse_receiver_report(
    packet: &[u8],
    video_ssrc: u32,
    feedback: &mut BroadcastRtcpFeedback,
) -> Result<(), String> {
    let report_count = usize::from(packet[0] & 0x1f);
    let expected_len = 8usize.saturating_add(report_count.saturating_mul(24));
    if packet.len() < expected_len {
        return Err("RTCP receiver report is shorter than its report blocks".to_owned());
    }

    let reporter_ssrc = u32::from_be_bytes(
        packet[4..8]
            .try_into()
            .expect("validated receiver report sender SSRC"),
    );
    for index in 0..report_count {
        let start = 8 + index * 24;
        let reported_ssrc = u32::from_be_bytes(
            packet[start..start + 4]
                .try_into()
                .expect("validated receiver report SSRC"),
        );
        if reported_ssrc != video_ssrc {
            continue;
        }

        let fraction_lost = packet[start + 4];
        let cumulative = u32::from(packet[start + 5]) << 16
            | u32::from(packet[start + 6]) << 8
            | u32::from(packet[start + 7]);
        let cumulative_lost = if cumulative & 0x80_0000 != 0 {
            (cumulative | 0xff00_0000) as i32
        } else {
            cumulative as i32
        };
        feedback.receiver_reports.push(BroadcastReceiverReport {
            reporter_ssrc,
            fraction_lost,
            cumulative_lost,
        });
    }
    Ok(())
}

pub fn parse_generic_nack(
    packet: &[u8],
    video_ssrc: u32,
    feedback: &mut BroadcastRtcpFeedback,
) -> Result<(), String> {
    if packet.len() < 12 || !(packet.len() - 12).is_multiple_of(4) {
        return Err("RTCP NACK packet has invalid feedback length".to_owned());
    }
    let media_ssrc =
        u32::from_be_bytes(packet[8..12].try_into().expect("validated NACK media SSRC"));
    if media_ssrc != video_ssrc {
        return Ok(());
    }

    for entry in packet[12..].chunks_exact(4) {
        let packet_id = u16::from_be_bytes([entry[0], entry[1]]);
        let bitmask = u16::from_be_bytes([entry[2], entry[3]]);
        feedback.nack_sequences.push(packet_id);
        for bit in 0..16 {
            if bitmask & (1 << bit) != 0 {
                feedback
                    .nack_sequences
                    .push(packet_id.wrapping_add(bit + 1));
            }
        }
    }
    Ok(())
}

pub fn parse_full_intra_request(
    packet: &[u8],
    video_ssrc: u32,
    feedback: &mut BroadcastRtcpFeedback,
) -> Result<(), String> {
    if packet.len() < 12 || !(packet.len() - 12).is_multiple_of(8) {
        return Err("RTCP FIR packet has invalid feedback length".to_owned());
    }
    feedback.request_keyframe |= packet[12..].chunks_exact(8).any(|entry| {
        u32::from_be_bytes(entry[..4].try_into().expect("validated FIR media SSRC")) == video_ssrc
    });
    Ok(())
}

pub fn packetize_discord_h264_frame(
    frame: &[u8],
    timestamp: u32,
    ssrc: u32,
    sequence: &mut u16,
    transport_sequence: &mut u16,
) -> Vec<Vec<u8>> {
    let payloads = packetize_h264_payloads(frame, STREAM_RTP_MAX_PAYLOAD_BYTES);
    let payload_count = payloads.len();
    payloads
        .into_iter()
        .enumerate()
        .map(|(index, payload)| {
            let packet = build_discord_video_rtp_packet(
                *sequence,
                timestamp,
                ssrc,
                index + 1 == payload_count,
                *transport_sequence,
                &payload,
            );
            *sequence = sequence.wrapping_add(1);
            *transport_sequence = transport_sequence.wrapping_add(1);
            packet
        })
        .collect()
}

pub fn build_discord_video_rtp_packet(
    sequence: u16,
    timestamp: u32,
    ssrc: u32,
    marker: bool,
    transport_sequence: u16,
    payload: &[u8],
) -> Vec<u8> {
    build_discord_video_rtp_packet_with_payload_type(
        sequence,
        timestamp,
        ssrc,
        marker,
        transport_sequence,
        DISCORD_STREAM_VIDEO_PAYLOAD_TYPE,
        RTP_EXTENSION_RID,
        payload,
    )
}

pub fn build_discord_video_rtx_packet(
    original: &[u8],
    rtx_ssrc: u32,
    rtx_sequence: u16,
    transport_sequence: u16,
) -> Result<Vec<u8>, String> {
    let header = parse_rtp_header(original)?;
    let original_payload = original
        .get(header.payload_offset..)
        .ok_or_else(|| "original RTP packet is missing media payload".to_owned())?;
    let mut rtx_payload =
        Vec::with_capacity(STREAM_RTX_ORIGINAL_SEQUENCE_BYTES + original_payload.len());
    rtx_payload.extend_from_slice(&header.sequence.to_be_bytes());
    rtx_payload.extend_from_slice(original_payload);
    Ok(build_discord_video_rtp_packet_with_payload_type(
        rtx_sequence,
        header.timestamp,
        rtx_ssrc,
        header.marker,
        transport_sequence,
        DISCORD_STREAM_VIDEO_RTX_PAYLOAD_TYPE,
        RTP_EXTENSION_REPAIRED_RID,
        &rtx_payload,
    ))
}

#[allow(clippy::too_many_arguments)]
pub fn build_discord_video_rtp_packet_with_payload_type(
    sequence: u16,
    timestamp: u32,
    ssrc: u32,
    marker: bool,
    transport_sequence: u16,
    payload_type: u8,
    rid_extension: u8,
    payload: &[u8],
) -> Vec<u8> {
    let mut extensions = Vec::with_capacity(STREAM_RTP_EXTENSION_BODY_BYTES);
    push_one_byte_extension(
        &mut extensions,
        RTP_EXTENSION_TRANSPORT_SEQUENCE,
        &transport_sequence.to_be_bytes(),
    );
    push_one_byte_extension(&mut extensions, RTP_EXTENSION_PLAYOUT_DELAY, &[0, 0, 0]);
    push_one_byte_extension(
        &mut extensions,
        RTP_EXTENSION_VIDEO_CONTENT_TYPE,
        &[VIDEO_CONTENT_TYPE_SCREEN],
    );
    push_one_byte_extension(&mut extensions, rid_extension, STREAM_RID.as_bytes());
    while extensions.len() % 4 != 0 {
        extensions.push(0);
    }

    let mut packet = Vec::with_capacity(
        RTP_HEADER_MIN_LEN + RTP_HEADER_EXTENSION_BYTES + extensions.len() + payload.len(),
    );
    packet.push((RTP_VERSION << 6) | 0x10);
    packet.push((u8::from(marker) << 7) | payload_type);
    packet.extend_from_slice(&sequence.to_be_bytes());
    packet.extend_from_slice(&timestamp.to_be_bytes());
    packet.extend_from_slice(&ssrc.to_be_bytes());
    packet.extend_from_slice(&RTP_EXTENSION_PROFILE_ONE_BYTE.to_be_bytes());
    packet.extend_from_slice(
        &u16::try_from(extensions.len() / 4)
            .expect("RTP extension word count fits u16")
            .to_be_bytes(),
    );
    packet.extend_from_slice(&extensions);
    packet.extend_from_slice(payload);
    packet
}

pub fn push_one_byte_extension(output: &mut Vec<u8>, id: u8, value: &[u8]) {
    debug_assert!((1..=14).contains(&id));
    debug_assert!((1..=16).contains(&value.len()));
    output.push((id << 4) | (u8::try_from(value.len()).expect("extension length fits u8") - 1));
    output.extend_from_slice(value);
}

