use std::{
    collections::VecDeque,
    net::{Ipv4Addr, SocketAddrV4},
    sync::atomic::Ordering,
};

use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};

use super::media::{annex_b_nals, packetize_h264_payloads};
use super::*;

pub fn build_transport_wide_feedback(
    sender_ssrc: u32,
    media_ssrc: u32,
    base_sequence: u16,
    reference_time: i64,
    feedback_packet_count: u8,
    statuses: &[u8],
    deltas: &[StreamTransportReceiveDelta],
) -> Vec<u8> {
    let mut packet = Vec::with_capacity(20 + statuses.len() / 3 + deltas.len() * 2);
    packet.extend_from_slice(&[
        (RTP_VERSION << 6) | RTCP_TRANSPORT_WIDE_FEEDBACK_FORMAT,
        RTCP_TRANSPORT_LAYER_FEEDBACK,
        0,
        0,
    ]);
    packet.extend_from_slice(&sender_ssrc.to_be_bytes());
    packet.extend_from_slice(&media_ssrc.to_be_bytes());
    packet.extend_from_slice(&base_sequence.to_be_bytes());
    packet.extend_from_slice(
        &u16::try_from(statuses.len())
            .expect("transport feedback status count fits u16")
            .to_be_bytes(),
    );
    let reference_time = (reference_time as u32) & 0x00ff_ffff;
    packet.extend_from_slice(&reference_time.to_be_bytes()[1..]);
    packet.push(feedback_packet_count);

    // Two-bit status vectors are slightly larger than mixed RLE chunks, but
    // their fixed seven-packet shape keeps loss and reordered arrivals clear.
    for statuses in statuses.chunks(7) {
        let mut chunk = 0xc000u16;
        for (index, status) in statuses.iter().enumerate() {
            chunk |= u16::from(*status & 0x03) << (12 - index * 2);
        }
        packet.extend_from_slice(&chunk.to_be_bytes());
    }
    for delta in deltas {
        match delta {
            StreamTransportReceiveDelta::Small(delta) => packet.push(*delta),
            StreamTransportReceiveDelta::Large(delta) => {
                packet.extend_from_slice(&delta.to_be_bytes());
            }
        }
    }
    while !packet.len().is_multiple_of(4) {
        packet.push(0);
    }
    let length_words_minus_one =
        u16::try_from(packet.len() / 4 - 1).expect("transport feedback length fits u16");
    packet[2..4].copy_from_slice(&length_words_minus_one.to_be_bytes());
    packet
}

pub fn reset_stream_h264_pipeline(
    h264: &mut H264Depacketizer,
    startup: &mut H264StartupGate,
    startup_buffer: &mut H264StartupBuffer,
) {
    *h264 = H264Depacketizer::default();
    *startup = H264StartupGate::default();
    startup_buffer.clear();
}

pub async fn log_stream_player_output(
    kind: &'static str,
    output: &'static str,
    stream: impl AsyncRead + Unpin,
    last_error: Arc<Mutex<Option<String>>>,
    player_ready: Option<StreamPlayerReadySignal>,
) {
    let mut lines = BufReader::new(stream).lines();
    loop {
        match lines.next_line().await {
            Ok(Some(line)) if !line.trim().is_empty() => {
                let input_ready = stream_player_input_is_ready(&line);
                logging::debug("stream", format!("mpv {kind} {output}: {line}"));
                *last_error.lock().await = Some(line);
                if let Some(player_ready) = player_ready.as_ref()
                    && input_ready
                    && !player_ready.player_ready.swap(true, Ordering::AcqRel)
                {
                    logging::debug("stream", format!("stream {kind} mpv input ready"));
                    let _ = player_ready.ready_tx.send(player_ready.media_generation);
                }
            }
            Ok(Some(_)) => {}
            Ok(None) => break,
            Err(error) => {
                logging::debug(
                    "stream",
                    format!("read mpv {kind} {output} failed: {error}"),
                );
                break;
            }
        }
    }
}

pub fn stream_player_ready_is_current(
    ready_generation: Option<u64>,
    media_generation: u64,
) -> bool {
    ready_generation == Some(media_generation)
}

pub fn stream_player_input_is_ready(line: &str) -> bool {
    line.contains("[cplayer] Opening done:")
}

pub struct ReservedLocalUdpPortPair {
    pub _rtp_socket: std::net::UdpSocket,
    pub _rtcp_socket: std::net::UdpSocket,
    pub rtp_port: u16,
    pub rtcp_port: u16,
}

pub fn reserve_local_udp_port_pair() -> Result<ReservedLocalUdpPortPair, String> {
    for _ in 0..64 {
        let rtp_socket = std::net::UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
            .map_err(|error| format!("reserve local stream RTP port failed: {error}"))?;
        let rtp_port = rtp_socket
            .local_addr()
            .map_err(|error| format!("read local stream RTP port failed: {error}"))?
            .port();
        let Some(rtcp_port) = rtp_port
            .checked_add(1)
            .filter(|_| rtp_port.is_multiple_of(2))
        else {
            continue;
        };
        let Ok(rtcp_socket) = std::net::UdpSocket::bind((Ipv4Addr::LOCALHOST, rtcp_port)) else {
            continue;
        };
        return Ok(ReservedLocalUdpPortPair {
            _rtp_socket: rtp_socket,
            _rtcp_socket: rtcp_socket,
            rtp_port,
            rtcp_port,
        });
    }
    Err("reserve adjacent local stream RTP and RTCP ports failed".to_owned())
}

pub fn stream_sdp(
    audio_port: u16,
    audio_rtcp_port: u16,
    video_port: u16,
    video_rtcp_port: u16,
) -> String {
    format!(
        "v=0\r\n\
         o=- 0 0 IN IP4 127.0.0.1\r\n\
         s=-\r\n\
         c=IN IP4 127.0.0.1\r\n\
         t=0 0\r\n\
         m=audio {audio_port} RTP/AVP {LOCAL_STREAM_AUDIO_PAYLOAD_TYPE}\r\n\
         a=rtcp:{audio_rtcp_port} IN IP4 127.0.0.1\r\n\
         a=rtpmap:{LOCAL_STREAM_AUDIO_PAYLOAD_TYPE} opus/48000/2\r\n\
         a=recvonly\r\n\
         m=video {video_port} RTP/AVP {LOCAL_STREAM_VIDEO_PAYLOAD_TYPE}\r\n\
         a=rtcp:{video_rtcp_port} IN IP4 127.0.0.1\r\n\
         a=rtpmap:{LOCAL_STREAM_VIDEO_PAYLOAD_TYPE} H264/90000\r\n\
         a=fmtp:{LOCAL_STREAM_VIDEO_PAYLOAD_TYPE} packetization-mode=1\r\n\
         a=recvonly\r\n"
    )
}

pub fn build_local_rtp_packet(
    payload_type: u8,
    marker: bool,
    sequence: u16,
    timestamp: u32,
    ssrc: u32,
    payload: &[u8],
) -> Vec<u8> {
    let mut packet = Vec::with_capacity(RTP_HEADER_MIN_LEN + payload.len());
    packet.push(RTP_VERSION << 6);
    packet.push((u8::from(marker) << 7) | payload_type);
    packet.extend_from_slice(&sequence.to_be_bytes());
    packet.extend_from_slice(&timestamp.to_be_bytes());
    packet.extend_from_slice(&ssrc.to_be_bytes());
    packet.extend_from_slice(payload);
    packet
}

#[derive(Default)]
pub struct H264Depacketizer {
    pub timestamp: Option<u32>,
    pub expected_sequence: Option<u16>,
    pub frame: Vec<u8>,
    pub packet_count: usize,
    pub fragment_open: bool,
}

pub enum H264DepacketizerOutput {
    Pending,
    Frame(Vec<u8>),
    BudgetExceeded,
}

pub enum H264AppendError {
    Invalid,
    BudgetExceeded,
}

#[derive(Default)]
pub struct H264StartupGate {
    pub started: bool,
    pub sps: Option<Vec<u8>>,
    pub pps: Option<Vec<u8>>,
}

pub struct BufferedH264Frame {
    pub encoded: Vec<u8>,
    pub source_timestamp: u32,
}

#[derive(Default)]
pub struct H264StartupBuffer {
    pub frames: VecDeque<BufferedH264Frame>,
    pub bytes: usize,
}

impl H264StartupGate {
    pub fn is_started(&self) -> bool {
        self.started
    }

    pub fn accept(&mut self, frame: Vec<u8>) -> Option<Vec<u8>> {
        let has_idr = {
            let mut has_idr = false;
            for nal in annex_b_nals(&frame) {
                match nal.first().map(|byte| byte & 0x1f) {
                    Some(5) => has_idr = true,
                    Some(7) => {
                        self.sps = Some(nal.to_vec());
                    }
                    Some(8) => {
                        self.pps = Some(nal.to_vec());
                    }
                    _ => {}
                }
            }
            has_idr
        };

        if self.started {
            return Some(frame);
        }
        if !has_idr || self.sps.is_none() || self.pps.is_none() {
            return None;
        }

        self.started = true;
        let mut startup_frame = Vec::new();
        append_annex_b_nal(
            &mut startup_frame,
            self.sps.as_deref().expect("startup requires an SPS"),
        );
        append_annex_b_nal(
            &mut startup_frame,
            self.pps.as_deref().expect("startup requires a PPS"),
        );
        for nal in annex_b_nals(&frame) {
            if !matches!(nal.first().map(|byte| byte & 0x1f), Some(7 | 8)) {
                append_annex_b_nal(&mut startup_frame, nal);
            }
        }
        Some(startup_frame)
    }
}

impl H264StartupBuffer {
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    pub fn len(&self) -> usize {
        self.frames.len()
    }

    pub fn push(&mut self, frame: BufferedH264Frame) -> bool {
        let Some(next_bytes) = self.bytes.checked_add(frame.encoded.len()) else {
            self.clear();
            return false;
        };
        if self.frames.len() >= STREAM_STARTUP_BUFFER_MAX_FRAMES
            || next_bytes > STREAM_STARTUP_BUFFER_MAX_BYTES
        {
            self.clear();
            return false;
        }
        self.bytes = next_bytes;
        self.frames.push_back(frame);
        true
    }

    pub fn take(&mut self) -> VecDeque<BufferedH264Frame> {
        self.bytes = 0;
        std::mem::take(&mut self.frames)
    }

    pub fn clear(&mut self) {
        self.frames.clear();
        self.bytes = 0;
    }
}

pub fn accept_or_buffer_h264(
    player_ready: bool,
    startup: &mut H264StartupGate,
    startup_buffer: &mut H264StartupBuffer,
    frame: Vec<u8>,
    source_timestamp: u32,
) -> Option<BufferedH264Frame> {
    let frame = BufferedH264Frame {
        encoded: startup.accept(frame)?,
        source_timestamp,
    };
    if player_ready {
        return Some(frame);
    }
    if !startup_buffer.push(frame) {
        *startup = H264StartupGate::default();
    }
    None
}

pub fn append_annex_b_nal(frame: &mut Vec<u8>, nal: &[u8]) {
    frame.extend_from_slice(&[0, 0, 0, 1]);
    frame.extend_from_slice(nal);
}

pub fn h264_nal_types(frame: &[u8]) -> Vec<u8> {
    annex_b_nals(frame)
        .filter_map(|nal| nal.first().map(|byte| byte & 0x1f))
        .collect()
}

impl H264Depacketizer {
    pub fn push(&mut self, header: &RtpHeader, payload: &[u8]) -> H264DepacketizerOutput {
        if self.timestamp != Some(header.timestamp)
            || self
                .expected_sequence
                .is_some_and(|expected| expected != header.sequence)
        {
            self.reset(header.timestamp);
        }
        if self.packet_count >= STREAM_H264_ACCESS_UNIT_MAX_PACKETS {
            self.reset(header.timestamp);
            return H264DepacketizerOutput::BudgetExceeded;
        }
        self.packet_count += 1;
        self.expected_sequence = Some(header.sequence.wrapping_add(1));
        let Some(nal_type) = payload.first().map(|byte| byte & 0x1f) else {
            self.reset(header.timestamp);
            return H264DepacketizerOutput::Pending;
        };
        let accepted = match nal_type {
            1..=23 => self.append_nal(payload),
            24 => self.append_stap_a(payload),
            28 => self.append_fu_a(payload),
            _ => Err(H264AppendError::Invalid),
        };
        match accepted {
            Ok(()) => {}
            Err(H264AppendError::Invalid) => {
                self.reset(header.timestamp);
                return H264DepacketizerOutput::Pending;
            }
            Err(H264AppendError::BudgetExceeded) => {
                self.reset(header.timestamp);
                return H264DepacketizerOutput::BudgetExceeded;
            }
        }
        if header.marker {
            if self.fragment_open || self.frame.is_empty() {
                self.reset(header.timestamp);
                return H264DepacketizerOutput::Pending;
            }
            self.timestamp = None;
            self.expected_sequence = None;
            self.packet_count = 0;
            return H264DepacketizerOutput::Frame(std::mem::take(&mut self.frame));
        }
        H264DepacketizerOutput::Pending
    }

    pub fn reset(&mut self, timestamp: u32) {
        self.timestamp = Some(timestamp);
        self.expected_sequence = None;
        self.frame.clear();
        self.packet_count = 0;
        self.fragment_open = false;
    }

    pub fn append_nal(&mut self, nal: &[u8]) -> Result<(), H264AppendError> {
        self.ensure_capacity(4usize.saturating_add(nal.len()))?;
        self.frame.extend_from_slice(&[0, 0, 0, 1]);
        self.frame.extend_from_slice(nal);
        self.fragment_open = false;
        Ok(())
    }

    pub fn append_stap_a(&mut self, payload: &[u8]) -> Result<(), H264AppendError> {
        let mut cursor = 1usize;
        let mut required_bytes = 0usize;
        let mut nal_count = 0usize;
        while cursor + 2 <= payload.len() {
            let size = usize::from(u16::from_be_bytes([payload[cursor], payload[cursor + 1]]));
            cursor += 2;
            let Some(nal) = payload.get(cursor..cursor.saturating_add(size)) else {
                return Err(H264AppendError::Invalid);
            };
            if nal.is_empty() {
                return Err(H264AppendError::Invalid);
            }
            required_bytes = required_bytes
                .checked_add(4)
                .and_then(|bytes| bytes.checked_add(nal.len()))
                .ok_or(H264AppendError::BudgetExceeded)?;
            cursor += size;
            nal_count += 1;
        }
        if nal_count == 0 || cursor != payload.len() {
            return Err(H264AppendError::Invalid);
        }
        self.ensure_capacity(required_bytes)?;

        cursor = 1;
        while cursor + 2 <= payload.len() {
            let size = usize::from(u16::from_be_bytes([payload[cursor], payload[cursor + 1]]));
            cursor += 2;
            let nal = &payload[cursor..cursor + size];
            self.frame.extend_from_slice(&[0, 0, 0, 1]);
            self.frame.extend_from_slice(nal);
            cursor += size;
        }
        self.fragment_open = false;
        Ok(())
    }

    pub fn append_fu_a(&mut self, payload: &[u8]) -> Result<(), H264AppendError> {
        if payload.len() < 3 {
            return Err(H264AppendError::Invalid);
        }
        let indicator = payload[0];
        let fu_header = payload[1];
        let start = fu_header & 0x80 != 0;
        let end = fu_header & 0x40 != 0;
        let header_bytes = if start { 5 } else { 0 };
        self.ensure_capacity(header_bytes + payload.len() - 2)?;
        if start {
            self.frame.extend_from_slice(&[0, 0, 0, 1]);
            self.frame.push((indicator & 0xe0) | (fu_header & 0x1f));
            self.fragment_open = !end;
        } else if !self.fragment_open {
            return Err(H264AppendError::Invalid);
        } else if end {
            self.fragment_open = false;
        }
        self.frame.extend_from_slice(&payload[2..]);
        Ok(())
    }

    pub fn ensure_capacity(&self, additional_bytes: usize) -> Result<(), H264AppendError> {
        if self
            .frame
            .len()
            .checked_add(additional_bytes)
            .is_some_and(|bytes| bytes <= STREAM_H264_ACCESS_UNIT_MAX_BYTES)
        {
            Ok(())
        } else {
            Err(H264AppendError::BudgetExceeded)
        }
    }
}

pub fn packetize_h264_frame(
    frame: &[u8],
    timestamp: u32,
    ssrc: u32,
    sequence: &mut u16,
) -> Vec<Vec<u8>> {
    let payloads = packetize_h264_payloads(frame, LOCAL_H264_MAX_PAYLOAD_BYTES);
    let payload_count = payloads.len();
    payloads
        .into_iter()
        .enumerate()
        .map(|(index, payload)| {
            let packet = build_local_rtp_packet(
                LOCAL_STREAM_VIDEO_PAYLOAD_TYPE,
                index + 1 == payload_count,
                *sequence,
                timestamp,
                ssrc,
                &payload,
            );
            *sequence = sequence.wrapping_add(1);
            packet
        })
        .collect()
}

pub async fn send_local_h264_frame(
    socket: &UdpSocket,
    target: SocketAddrV4,
    frame: &[u8],
    timestamp: u32,
    ssrc: u32,
    sequence: &mut u16,
) -> (u32, u32) {
    let packets = packetize_h264_frame(frame, timestamp, ssrc, sequence);
    let packet_count = u32::try_from(packets.len()).expect("H264 packet count fits u32");
    let octet_count = packets.iter().fold(0u32, |total, packet| {
        total.wrapping_add(packet.len().saturating_sub(RTP_HEADER_MIN_LEN) as u32)
    });
    for packet in packets {
        let _ = socket.send_to(&packet, target).await;
    }
    (packet_count, octet_count)
}
