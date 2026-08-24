use std::{collections::HashMap, sync::atomic::AtomicBool};

use uuid::Uuid;

use super::media::GatewayChildTasks;
use super::*;

pub async fn connect_stream_gateway(
    session: &StreamGatewaySession,
    events_tx: &mpsc::UnboundedSender<VoiceRuntimeEvent>,
    status_publisher: &VoiceStatusPublisher,
) -> Result<VoiceConnectionEnd, StreamConnectionFailure> {
    let url = gateway::voice_gateway_url(&session.endpoint)?;
    logging::debug("stream", format!("connecting stream websocket: {url}"));
    let (ws, response) = timeout(VOICE_WEBSOCKET_CONNECT_TIMEOUT, connect_async(&url))
        .await
        .map_err(|_| "stream websocket connect timed out after 10s".to_owned())?
        .map_err(|error| format!("stream websocket connect failed: {error}"))?;
    logging::debug(
        "stream",
        format!("stream websocket connected: status={}", response.status()),
    );
    let (writer, mut reader) = ws.split();
    let writer = Arc::new(Mutex::new(writer));
    let mut gateway_control = gateway::StreamVoiceGatewayControl::new(
        Arc::clone(&writer),
        session.current_user_id,
        &session.rtc_server_id,
    )?;
    let dave_state = gateway_control.dave_state();
    let mut child_tasks = GatewayChildTasks::default();
    let (media_finished_tx, mut media_finished_rx) =
        mpsc::unbounded_channel::<Result<(), StreamConnectionFailure>>();
    let (player_ready_tx, mut player_ready_rx) = mpsc::unbounded_channel::<u64>();
    let (video_source_tx, video_source_rx) = watch::channel(StreamVideoSource::default());
    let mut udp_socket: Option<Arc<UdpSocket>> = None;
    let mut local_ssrc: Option<u32> = None;
    let mut current_description: Option<VoiceSessionDescription> = None;
    let mut media_generation = 0u64;
    let mut connection_stable_deadline: Option<Instant> = None;

    gateway::send_voice_text(&writer, stream_identify_payload(session)).await?;
    logging::debug("stream", "stream identify sent");

    let result = loop {
        let frame = tokio::select! {
            _ = gateway_control.heartbeat_timed_out() => {
                break Ok(VoiceConnectionEnd::Reconnect);
            }
            media_result = media_finished_rx.recv(), if child_tasks.has_media() => {
                match media_result {
                    Some(Ok(())) => break Ok(VoiceConnectionEnd::Stop),
                    Some(Err(error)) => break Err(error),
                    None => break Ok(VoiceConnectionEnd::Reconnect),
                }
            }
            ready_generation = player_ready_rx.recv(), if child_tasks.has_media() => {
                if stream_player_ready_is_current(ready_generation, media_generation) {
                    status_publisher
                        .publish_stream_playback_ready(
                            session.request.scope,
                            session.request.channel_id,
                            session.request.owner_id,
                        )
                        .await;
                    connection_stable_deadline =
                        Some(Instant::now() + STREAM_CONNECTION_STABLE_INTERVAL);
                }
                continue;
            }
            _ = tokio::time::sleep_until(tokio::time::Instant::from_std(
                connection_stable_deadline.unwrap_or_else(Instant::now)
            )), if connection_stable_deadline.is_some() => {
                connection_stable_deadline = None;
                let _ = events_tx.send(VoiceRuntimeEvent::StreamConnectionEstablished {
                    connection_id: session.connection_id,
                    stream_key: session.request.stream_key.clone(),
                });
                continue;
            }
            frame = reader.next() => frame,
        };
        let Some(frame) = frame else {
            break Ok(VoiceConnectionEnd::Reconnect);
        };
        let frame = match frame {
            Ok(frame) => frame,
            Err(error) => {
                break Err(StreamConnectionFailure::reconnect(format!(
                    "stream websocket read failed: {error}"
                )));
            }
        };
        match gateway_control.frame_action(&frame).await? {
            gateway::StreamVoiceGatewayFrameAction::Payload => {}
            gateway::StreamVoiceGatewayFrameAction::Continue => continue,
            gateway::StreamVoiceGatewayFrameAction::End(outcome) => break Ok(outcome),
        }
        match frame {
            WsMessage::Text(text) => {
                let value: Value = serde_json::from_str(&text)
                    .map_err(|error| format!("stream websocket JSON parse failed: {error}"))?;
                gateway_control.record_sequence(&value).await;
                let opcode = value.get("op").and_then(Value::as_u64).unwrap_or_default() as u8;
                match opcode {
                    VOICE_OP_READY => {
                        let ready = gateway::parse_voice_ready_payload(&value)?;
                        let mode = gateway::choose_encryption_mode(&ready.modes)?;
                        let (socket, discovered) =
                            gateway::discover_voice_udp_address(&ready).await?;
                        gateway::send_voice_text(
                            &writer,
                            stream_select_protocol_payload(&discovered, &mode),
                        )
                        .await?;
                        gateway::send_voice_text(
                            &writer,
                            stream_receive_only_video_payload(ready.ssrc),
                        )
                        .await?;
                        local_ssrc = Some(ready.ssrc);
                        udp_socket = Some(socket);
                    }
                    VOICE_OP_SESSION_DESCRIPTION => {
                        let description = gateway::parse_voice_session_description(&value)?;
                        if description
                            .video_codec
                            .as_deref()
                            .is_some_and(|codec| !codec.eq_ignore_ascii_case("H264"))
                        {
                            break Err(StreamConnectionFailure::reconnect(format!(
                                "stream selected unsupported video codec: {}",
                                description.video_codec.as_deref().unwrap_or("none")
                            )));
                        }
                        dave_state
                            .lock()
                            .await
                            .apply_protocol_version(description.dave_protocol_version)?;
                        let Some(socket) = udp_socket.as_ref() else {
                            break Err(StreamConnectionFailure::reconnect(
                                "stream session description arrived before UDP ready",
                            ));
                        };
                        let Some(local_ssrc) = local_ssrc else {
                            break Err(StreamConnectionFailure::reconnect(
                                "stream session description arrived before local SSRC",
                            ));
                        };
                        if current_description.as_ref() == Some(&description) {
                            continue;
                        }
                        let socket_for_media = Arc::clone(socket);
                        let description_for_media = description.clone();
                        let dave_for_media = Arc::clone(&dave_state);
                        let source_for_media = video_source_rx.clone();
                        let finished = media_finished_tx.clone();
                        let owner_id = session.request.owner_id;
                        media_generation = media_generation.wrapping_add(1);
                        connection_stable_deadline = None;
                        let stream_player_ready = StreamPlayerReadySignal {
                            player_ready: Arc::new(AtomicBool::new(false)),
                            ready_tx: player_ready_tx.clone(),
                            media_generation,
                            display_name: session.request.display_name.clone(),
                        };
                        child_tasks
                            .replace_media(tokio::spawn(async move {
                                let result = run_stream_media(
                                    socket_for_media,
                                    description_for_media,
                                    dave_for_media,
                                    source_for_media,
                                    owner_id,
                                    local_ssrc,
                                    stream_player_ready,
                                )
                                .await;
                                let _ = finished.send(result);
                            }))
                            .await;
                        child_tasks
                            .replace_keepalive(tokio::spawn(gateway::run_voice_udp_keepalive(
                                Arc::clone(socket),
                            )))
                            .await;
                        current_description = Some(description);
                    }
                    VOICE_OP_VIDEO => {
                        if let Some(source) =
                            parse_stream_video_source(&value, session.request.owner_id)
                        {
                            logging::debug(
                                "stream",
                                format!(
                                    "stream video source selected: audio_ssrc={} video_ssrc={} rtx_ssrc={:?} pixel_count={:?}",
                                    source.audio_ssrc,
                                    source.video_ssrc,
                                    source.rtx_ssrc,
                                    source.pixel_count,
                                ),
                            );
                            {
                                let mut dave = dave_state.lock().await;
                                dave.record_ssrc_user(source.audio_ssrc, session.request.owner_id);
                                dave.record_ssrc_user(source.video_ssrc, session.request.owner_id);
                            }
                            gateway::send_voice_text(
                                &writer,
                                stream_media_sink_wants_payload(
                                    source.audio_ssrc,
                                    source.video_ssrc,
                                    source.pixel_count,
                                ),
                            )
                            .await?;
                            video_source_tx.send_replace(source);
                        }
                    }
                    other => {
                        if !gateway_control
                            .handle_json_op(other, &value, &mut child_tasks)
                            .await?
                        {
                            logging::debug(
                                "stream",
                                format!("unhandled stream gateway op={other}"),
                            );
                        }
                    }
                }
            }
            WsMessage::Binary(payload) => {
                gateway_control.handle_binary(&payload).await?;
            }
            WsMessage::Ping(_) | WsMessage::Pong(_) | WsMessage::Close(_) | WsMessage::Frame(_) => {
                unreachable!("gateway control frames are handled first")
            }
        }
    };

    child_tasks.shutdown().await;
    result
}

pub fn stream_identify_payload(session: &StreamGatewaySession) -> String {
    json!({
        "op": 0,
        "d": {
            "server_id": session.rtc_server_id,
            "user_id": session.current_user_id.to_string(),
            "channel_id": session.rtc_channel_id.to_string(),
            "session_id": session.session_id,
            "token": session.token,
            "video": true,
            "max_dave_protocol_version": davey::DAVE_PROTOCOL_VERSION,
        },
    })
    .to_string()
}

pub fn stream_select_protocol_payload(discovered: &DiscoveredVoiceAddress, mode: &str) -> String {
    json!({
        "op": 1,
        "d": {
            "protocol": "udp",
            "data": {
                "address": discovered.address,
                "port": discovered.port,
                "mode": mode,
            },
            "codecs": [
                {
                    "name": "opus",
                    "type": "audio",
                    "priority": 1000,
                    "payload_type": DISCORD_VOICE_PAYLOAD_TYPE,
                    "encode": false,
                    "decode": true,
                },
                {
                    "name": "H264",
                    "type": "video",
                    "priority": 1000,
                    "payload_type": DISCORD_STREAM_VIDEO_PAYLOAD_TYPE,
                    "rtx_payload_type": DISCORD_STREAM_VIDEO_RTX_PAYLOAD_TYPE,
                    "encode": false,
                    "decode": true,
                },
            ],
            "rtc_connection_id": Uuid::new_v4().to_string(),
        },
    })
    .to_string()
}

pub fn stream_receive_only_video_payload(audio_ssrc: u32) -> String {
    json!({
        "op": VOICE_OP_VIDEO,
        "d": {
            "audio_ssrc": audio_ssrc,
            "video_ssrc": 0,
            "rtx_ssrc": 0,
            "streams": [],
        },
    })
    .to_string()
}

pub fn stream_media_sink_wants_payload(
    audio_ssrc: u32,
    video_ssrc: u32,
    video_pixel_count: Option<u64>,
) -> String {
    let mut wants = serde_json::Map::new();
    if audio_ssrc != 0 {
        wants.insert(audio_ssrc.to_string(), Value::from(100));
    }
    if video_ssrc != 0 {
        wants.insert(video_ssrc.to_string(), Value::from(100));
        if let Some(pixel_count) = video_pixel_count {
            let mut pixel_counts = serde_json::Map::new();
            pixel_counts.insert(video_ssrc.to_string(), Value::from(pixel_count));
            wants.insert("pixelCounts".to_owned(), Value::Object(pixel_counts));
        }
    }
    wants.insert("any".to_owned(), Value::from(0));
    json!({
        "op": VOICE_OP_MEDIA_SINK_WANTS,
        "d": Value::Object(wants),
    })
    .to_string()
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StreamVideoSource {
    pub audio_ssrc: u32,
    pub video_ssrc: u32,
    pub rtx_ssrc: Option<u32>,
    pub pixel_count: Option<u64>,
}

#[derive(Debug, Eq, PartialEq)]
pub struct RecoveredStreamAudioPacket {
    pub marker: bool,
    pub sequence: u16,
    pub timestamp: u32,
    pub opus: Vec<u8>,
}

#[derive(Debug)]
pub struct PendingStreamAudioPacket {
    pub packet: RecoveredStreamAudioPacket,
    pub arrived_at: Instant,
}

#[derive(Default)]
pub struct StreamAudioRecovery {
    pub next_sequence: Option<u16>,
    pub pending: HashMap<u16, PendingStreamAudioPacket>,
    pub first_buffered_at: Option<Instant>,
    pub started: bool,
}

#[derive(Default)]
pub struct StreamAudioRecoveryUpdate {
    pub ready: Vec<RecoveredStreamAudioPacket>,
    pub skipped_sequences: u16,
    pub dropped_stale_packets: u64,
}

#[derive(Debug, Eq, PartialEq)]
pub struct RecoveredStreamVideoPacket {
    pub header: RtpHeader,
    pub payload: Vec<u8>,
}

#[derive(Default)]
pub struct StreamVideoRecovery {
    pub next_sequence: Option<u16>,
    pub pending: HashMap<u16, RecoveredStreamVideoPacket>,
    pub pending_bytes: usize,
    pub gap_started_at: Option<Instant>,
    pub last_nack_at: Option<Instant>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StreamVideoRecoveryReset {
    pub distance: u16,
    pub pending_packets: usize,
    pub pending_bytes: usize,
    pub gap_age: Option<Duration>,
}

#[derive(Default)]
pub struct StreamVideoRecoveryUpdate {
    pub ready: Vec<RecoveredStreamVideoPacket>,
    pub reset: Option<StreamVideoRecoveryReset>,
}

#[derive(Default)]
pub struct StreamPliThrottle {
    pub media_ssrc: Option<u32>,
    pub last_sent_at: Option<Instant>,
}

impl StreamPliThrottle {
    pub fn permit(&mut self, media_ssrc: u32, now: Instant) -> bool {
        if self.media_ssrc != Some(media_ssrc) {
            self.media_ssrc = Some(media_ssrc);
            self.last_sent_at = Some(now);
            return true;
        }
        if self.last_sent_at.is_some_and(|last| {
            now.saturating_duration_since(last) < STREAM_KEYFRAME_REQUEST_INTERVAL
        }) {
            return false;
        }
        self.last_sent_at = Some(now);
        true
    }

    pub async fn send_if_due(
        &mut self,
        control: &mut StreamRtcpControl,
        socket: &UdpSocket,
        encryptor: &VoiceRtpEncryptor,
        sender_ssrc: u32,
        media_ssrc: u32,
        elapsed: Duration,
    ) -> Result<bool, String> {
        if !self.permit(media_ssrc, Instant::now()) {
            return Ok(false);
        }
        let feedback = build_rtcp_pli(sender_ssrc, media_ssrc);
        control
            .send_feedback(socket, encryptor, sender_ssrc, &feedback, "PLI", elapsed)
            .await?;
        Ok(true)
    }
}

#[derive(Clone, Copy, Default)]
pub struct StreamMediaCounters {
    pub audio_stale_packets: u64,
    pub audio_skipped_packets: u64,
    pub primary_video_packets: u64,
    pub rtx_video_packets: u64,
    pub h264_frames: u64,
    pub h264_bytes: u64,
    pub decoder_resets: u64,
    pub transport_feedbacks: u64,
    pub nacks: u64,
    pub plis: u64,
    pub suppressed_plis: u64,
}

impl StreamMediaCounters {
    pub fn observe_pli_request(&mut self, sent: bool) {
        if sent {
            self.plis = self.plis.wrapping_add(1);
        } else {
            self.suppressed_plis = self.suppressed_plis.wrapping_add(1);
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StreamRtcpReportBlock {
    pub source_ssrc: u32,
    pub fraction_lost: u8,
    pub cumulative_lost: i32,
    pub extended_highest_sequence: u32,
    pub interarrival_jitter: u32,
    pub last_sender_report: u32,
    pub delay_since_last_sender_report: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StreamRtcpSenderReport {
    pub sender_ssrc: u32,
    pub ntp_timestamp: u64,
    pub rtp_timestamp: u32,
    pub packet_count: u32,
    pub octet_count: u32,
}

#[derive(Clone, Copy)]
pub struct StreamNtpOrigin {
    pub ntp_timestamp: u64,
    pub local_elapsed: Duration,
}

#[derive(Default)]
pub struct StreamPresentationClock {
    pub origin: Option<StreamNtpOrigin>,
    pub reports: HashMap<u32, StreamRtcpSenderReport>,
}

impl StreamPresentationClock {
    pub fn observe_sender_report(&mut self, report: StreamRtcpSenderReport, elapsed: Duration) {
        self.origin.get_or_insert(StreamNtpOrigin {
            ntp_timestamp: report.ntp_timestamp,
            local_elapsed: elapsed,
        });
        self.reports.insert(report.sender_ssrc, report);
    }

    // RTCP Sender Reports connect every media RTP clock to one NTP clock. Map
    // that common sender time onto the local elapsed timeline so audio and
    // video no longer acquire separate offsets from their packet arrival time.
    pub fn map_timestamp(&self, ssrc: u32, source_timestamp: u32, clock_rate: u32) -> Option<u32> {
        let origin = self.origin?;
        let report = self.reports.get(&ssrc)?;
        let ntp_delta = report.ntp_timestamp.wrapping_sub(origin.ntp_timestamp) as i64;
        let ntp_delta_ticks = i128::from(ntp_delta) * i128::from(clock_rate) / (1i128 << 32);
        let source_delta_ticks =
            i128::from(source_timestamp.wrapping_sub(report.rtp_timestamp) as i32);
        let local_origin_ticks =
            i128::from(elapsed_rtp_timestamp(origin.local_elapsed, clock_rate));
        let local_timestamp = local_origin_ticks + ntp_delta_ticks + source_delta_ticks;
        if local_timestamp < 0 {
            return None;
        }
        Some(local_timestamp.rem_euclid(1i128 << 32) as u32)
    }
}

pub fn parse_stream_rtcp_sender_reports(
    compound: &[u8],
) -> Result<Vec<StreamRtcpSenderReport>, String> {
    let mut reports = Vec::new();
    let mut offset = 0usize;
    while offset < compound.len() {
        let remaining = compound.len() - offset;
        if remaining < 4 {
            return Err("RTCP compound packet has a truncated header".to_owned());
        }
        if compound[offset] >> 6 != RTP_VERSION {
            return Err("RTCP packet has an invalid version".to_owned());
        }
        let length_words_minus_one =
            u16::from_be_bytes([compound[offset + 2], compound[offset + 3]]);
        let packet_len = (usize::from(length_words_minus_one) + 1)
            .checked_mul(4)
            .ok_or_else(|| "RTCP packet length overflowed".to_owned())?;
        let packet_end = offset
            .checked_add(packet_len)
            .filter(|end| *end <= compound.len())
            .ok_or_else(|| "RTCP packet length exceeds the compound packet".to_owned())?;

        if compound[offset + 1] == RTCP_SENDER_REPORT {
            let report_count = usize::from(compound[offset] & 0x1f);
            let minimum_len = 28 + report_count * 24;
            if packet_len < minimum_len {
                return Err("RTCP sender report is truncated".to_owned());
            }
            let sender_ssrc = rtcp_u32(compound, offset + 4);
            let ntp_seconds = rtcp_u32(compound, offset + 8);
            let ntp_fraction = rtcp_u32(compound, offset + 12);
            reports.push(StreamRtcpSenderReport {
                sender_ssrc,
                ntp_timestamp: (u64::from(ntp_seconds) << 32) | u64::from(ntp_fraction),
                rtp_timestamp: rtcp_u32(compound, offset + 16),
                packet_count: rtcp_u32(compound, offset + 20),
                octet_count: rtcp_u32(compound, offset + 24),
            });
        }
        offset = packet_end;
    }
    Ok(reports)
}

pub fn rtcp_u32(packet: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes([
        packet[offset],
        packet[offset + 1],
        packet[offset + 2],
        packet[offset + 3],
    ])
}

#[derive(Clone, Copy)]
pub struct StreamRtcpJitterOrigin {
    pub arrival_timestamp: u32,
    pub source_timestamp: u32,
}

#[derive(Clone, Copy)]
pub struct StreamRtcpLastSenderReport {
    pub middle_ntp_timestamp: u32,
    pub received_at: Duration,
}

#[derive(Default)]
pub struct StreamRtcpControl {
    pub nonce: u32,
    pub source_ssrc: u32,
    pub base_extended_sequence: Option<u32>,
    pub highest_extended_sequence: Option<u32>,
    pub received_packets: u32,
    pub expected_prior: u32,
    pub received_prior: u32,
    pub jitter_origin: Option<StreamRtcpJitterOrigin>,
    pub previous_transit: Option<i64>,
    pub jitter_q4: i64,
    pub last_sender_report: Option<StreamRtcpLastSenderReport>,
}
