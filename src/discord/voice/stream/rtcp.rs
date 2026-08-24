use std::{
    io::Write,
    net::{Ipv4Addr, SocketAddrV4},
    sync::atomic::Ordering,
};

use tempfile::NamedTempFile;

use crate::support::media_player::MediaPlayerIpcEndpoint;

use super::media::{build_rtcp_sender_report, current_unix_time};
use super::*;

pub async fn run_stream_media(
    discord_socket: Arc<UdpSocket>,
    description: VoiceSessionDescription,
    dave_state: Arc<Mutex<VoiceDaveState>>,
    video_source_rx: watch::Receiver<StreamVideoSource>,
    owner_id: Id<UserMarker>,
    local_ssrc: u32,
    stream_player_ready: StreamPlayerReadySignal,
) -> Result<(), StreamConnectionFailure> {
    let audio_ports = reserve_local_udp_port_pair()?;
    let video_ports = reserve_local_udp_port_pair()?;
    let audio_port = audio_ports.rtp_port;
    let audio_rtcp_port = audio_ports.rtcp_port;
    let video_port = video_ports.rtp_port;
    let video_rtcp_port = video_ports.rtcp_port;
    let mut sdp =
        NamedTempFile::new().map_err(|error| format!("create stream SDP failed: {error}"))?;
    sdp.write_all(stream_sdp(audio_port, audio_rtcp_port, video_port, video_rtcp_port).as_bytes())
        .map_err(|error| format!("write stream SDP failed: {error}"))?;
    sdp.flush()
        .map_err(|error| format!("flush stream SDP failed: {error}"))?;
    let mut player_input_config = NamedTempFile::new()
        .map_err(|error| format!("create stream mpv input config failed: {error}"))?;
    player_input_config
        .write_all(STREAM_PLAYER_INPUT_CONFIG.as_bytes())
        .map_err(|error| format!("write stream mpv input config failed: {error}"))?;
    player_input_config
        .flush()
        .map_err(|error| format!("flush stream mpv input config failed: {error}"))?;

    // Keep both RTP/RTCP pairs reserved until the SDP is complete, then
    // release them immediately before mpv binds its receive sockets.
    drop(audio_ports);
    drop(video_ports);
    let player_ipc = MediaPlayerIpcEndpoint::unique();
    player_ipc
        .prepare()
        .map_err(|error| format!("prepare stream mpv IPC failed: {error}"))?;
    let mut player = stream_player_command(
        sdp.path(),
        player_input_config.path(),
        &stream_player_ready.display_name,
        player_ipc.server_arg(),
    );
    let mut player = player.spawn().map_err(stream_player_spawn_failure)?;
    let player_id = player.id();
    let player_stdout = player
        .stdout
        .take()
        .ok_or_else(|| StreamConnectionFailure::stop("capture stream mpv stdout failed"))?;
    let player_stderr = player
        .stderr
        .take()
        .ok_or_else(|| StreamConnectionFailure::stop("capture stream mpv stderr failed"))?;
    let last_player_error = Arc::new(Mutex::new(None));
    let video_player_ready = Arc::clone(&stream_player_ready.player_ready);
    let player_log_tasks = StreamPlayerLogTasks::new([
        tokio::spawn(log_stream_player_output(
            "stream",
            "stdout",
            player_stdout,
            Arc::clone(&last_player_error),
            Some(stream_player_ready),
        )),
        tokio::spawn(log_stream_player_output(
            "stream",
            "stderr",
            player_stderr,
            Arc::clone(&last_player_error),
            None,
        )),
    ]);
    logging::debug(
        "stream",
        format!(
            "stream mpv started: pid={player_id:?} audio_port={audio_port} audio_rtcp_port={audio_rtcp_port} video_port={video_port} video_rtcp_port={video_rtcp_port}"
        ),
    );

    let local_socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .map_err(|error| format!("bind local stream RTP socket failed: {error}"))?;
    let audio_target = SocketAddrV4::new(Ipv4Addr::LOCALHOST, audio_port);
    let audio_rtcp_target = SocketAddrV4::new(Ipv4Addr::LOCALHOST, audio_rtcp_port);
    let video_target = SocketAddrV4::new(Ipv4Addr::LOCALHOST, video_port);
    let video_rtcp_target = SocketAddrV4::new(Ipv4Addr::LOCALHOST, video_rtcp_port);
    let decryptor = VoiceRtpDecryptor::new(&description.mode, &description.secret_key)?;
    let encryptor = VoiceRtpEncryptor::new(&description.mode, &description.secret_key)?;
    let mut packet = [0u8; STREAM_RTP_PACKET_BYTES];
    let mut h264 = H264Depacketizer::default();
    let mut h264_startup = H264StartupGate::default();
    let mut h264_startup_buffer = H264StartupBuffer::default();
    let mut audio_recovery = StreamAudioRecovery::default();
    let mut video_recovery = StreamVideoRecovery::default();
    let mut active_audio_source = 0u32;
    let mut active_video_source = (0u32, None);
    let mut local_audio = LocalStreamAudioForwarder::default();
    let mut local_video = LocalStreamVideoForwarder::default();
    let mut discord_rtcp = StreamRtcpControl::default();
    let mut pli_throttle = StreamPliThrottle::default();
    let mut transport_feedback = StreamTransportFeedback::default();
    let mut presentation_clock = StreamPresentationClock::default();
    let mut media_counters = StreamMediaCounters::default();
    let mut previous_media_counters = StreamMediaCounters::default();
    let mut previous_local_video_frames = 0u64;
    let mut previous_stats_elapsed = Duration::ZERO;
    let media_started_at = Instant::now();
    let mut player_audio = StreamPlayerAudioState::default();
    let mut logged_first_video_frame = false;
    let mut logged_video_before_player_ready = false;
    let mut logged_keyframe_request = false;
    let mut logged_local_sender_reports = false;
    let mut logged_discord_sender_report = false;
    let mut local_rtcp_report_ticks = 0u64;
    let mut keyframe_request_interval = tokio::time::interval(STREAM_KEYFRAME_REQUEST_INTERVAL);
    keyframe_request_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut video_recovery_interval = tokio::time::interval(STREAM_VIDEO_NACK_INTERVAL);
    video_recovery_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut audio_recovery_interval = tokio::time::interval(STREAM_AUDIO_REORDER_INTERVAL);
    audio_recovery_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut local_rtcp_report_interval = tokio::time::interval(LOCAL_RTCP_REPORT_INTERVAL);
    local_rtcp_report_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut discord_rtcp_report_interval =
        tokio::time::interval(STREAM_RTCP_RECEIVER_REPORT_INTERVAL);
    discord_rtcp_report_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut transport_feedback_interval = tokio::time::interval(STREAM_TRANSPORT_FEEDBACK_INTERVAL);
    transport_feedback_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let player_ready_timeout = tokio::time::sleep(STREAM_PLAYER_READY_TIMEOUT);
    tokio::pin!(player_ready_timeout);

    loop {
        maybe_enable_stream_player_audio(
            &mut player_audio,
            video_player_ready.load(Ordering::Acquire),
            &player_ipc,
        )
        .await;
        tokio::select! {
            _ = &mut player_ready_timeout,
                if !video_player_ready.load(Ordering::Acquire) =>
            {
                let last_error = last_player_error.lock().await.clone();
                return Err(StreamConnectionFailure::stop(match last_error {
                    Some(error) => format!(
                        "stream mpv did not open its SDP input within {} seconds: {error}",
                        STREAM_PLAYER_READY_TIMEOUT.as_secs(),
                    ),
                    None => format!(
                        "stream mpv did not open its SDP input within {} seconds; local RTP ports may no longer be available",
                        STREAM_PLAYER_READY_TIMEOUT.as_secs(),
                    ),
                }));
            }
            _ = local_rtcp_report_interval.tick() => {
                let elapsed = media_started_at.elapsed();
                let source = *video_source_rx.borrow();
                let unix_time = current_unix_time();
                let mut sent_report = false;
                if source.audio_ssrc != 0
                    && local_audio.packets != 0
                    && let Some(audio_timestamp) = local_audio.timestamp_at(elapsed)
                {
                    let report = build_rtcp_sender_report(
                        source.audio_ssrc,
                        unix_time,
                        audio_timestamp,
                        local_audio.packets,
                        local_audio.octets,
                    );
                    let _ = local_socket.send_to(&report, audio_rtcp_target).await;
                    sent_report = true;
                }
                if source.video_ssrc != 0 && local_video.packets != 0 {
                    let report = build_rtcp_sender_report(
                        source.video_ssrc,
                        unix_time,
                        elapsed_rtp_timestamp(elapsed, VIDEO_RTP_CLOCK_RATE),
                        local_video.packets,
                        local_video.octets,
                    );
                    let _ = local_socket.send_to(&report, video_rtcp_target).await;
                    sent_report = true;
                }
                if sent_report {
                    local_rtcp_report_ticks = local_rtcp_report_ticks.wrapping_add(1);
                    if !logged_local_sender_reports {
                        logged_local_sender_reports = true;
                        logging::debug(
                            "stream",
                            format!(
                                "local RTCP sender reports started: elapsed_ms={}",
                                elapsed.as_millis()
                            ),
                        );
                    } else if local_rtcp_report_ticks.is_multiple_of(10) {
                        let interval_elapsed = elapsed.saturating_sub(previous_stats_elapsed);
                        let interval_local_video_frames = local_video
                            .frames
                            .wrapping_sub(previous_local_video_frames);
                        logging::debug(
                            "stream",
                            format!(
                                "stream media stats: elapsed_ms={} interval_ms={} interval_video_packets={} interval_rtx_packets={} interval_h264_frames={} interval_h264_bytes={} interval_output_video_frames={} audio_pending_packets={} audio_stale_packets={} audio_skipped_packets={} decoder_resets={} transport_feedbacks={} nacks={} plis={} suppressed_plis={}",
                                elapsed.as_millis(),
                                interval_elapsed.as_millis(),
                                media_counters.primary_video_packets.wrapping_sub(
                                    previous_media_counters.primary_video_packets,
                                ),
                                media_counters
                                    .rtx_video_packets
                                    .wrapping_sub(previous_media_counters.rtx_video_packets),
                                media_counters
                                    .h264_frames
                                    .wrapping_sub(previous_media_counters.h264_frames),
                                media_counters
                                    .h264_bytes
                                    .wrapping_sub(previous_media_counters.h264_bytes),
                                interval_local_video_frames,
                                audio_recovery.pending_len(),
                                media_counters.audio_stale_packets,
                                media_counters.audio_skipped_packets,
                                media_counters.decoder_resets,
                                media_counters.transport_feedbacks,
                                media_counters.nacks,
                                media_counters.plis,
                                media_counters.suppressed_plis,
                            ),
                        );
                        previous_media_counters = media_counters;
                        previous_local_video_frames = local_video.frames;
                        previous_stats_elapsed = elapsed;
                    }
                }
            }
            _ = discord_rtcp_report_interval.tick() => {
                let source = *video_source_rx.borrow();
                if source.video_ssrc != 0 {
                    discord_rtcp.set_source(source.video_ssrc);
                    discord_rtcp
                        .send_report(
                            &discord_socket,
                            &encryptor,
                            local_ssrc,
                            media_started_at.elapsed(),
                        )
                        .await?;
                }
            }
            _ = transport_feedback_interval.tick() => {
                let source = *video_source_rx.borrow();
                if source.video_ssrc != 0
                    && let Some(feedback) = transport_feedback
                        .take_feedback(local_ssrc, source.video_ssrc)
                {
                    discord_rtcp
                        .send_feedback(
                            &discord_socket,
                            &encryptor,
                            local_ssrc,
                            &feedback,
                            "transport-wide feedback",
                            media_started_at.elapsed(),
                        )
                        .await?;
                    media_counters.transport_feedbacks =
                        media_counters.transport_feedbacks.wrapping_add(1);
                }
            }
            _ = audio_recovery_interval.tick() => {
                let source = *video_source_rx.borrow();
                if source.audio_ssrc != active_audio_source {
                    active_audio_source = source.audio_ssrc;
                    audio_recovery.reset();
                    local_audio.reset_source();
                }
                if source.audio_ssrc != 0 {
                    let update = audio_recovery.poll(Instant::now());
                    let destination = LocalStreamAudioDestination {
                        socket: &local_socket,
                        target: audio_target,
                        ssrc: source.audio_ssrc,
                        media_started_at,
                        presentation_clock: &presentation_clock,
                    };
                    forward_recovered_stream_audio(
                        update,
                        audio_recovery.pending_len(),
                        &mut local_audio,
                        &destination,
                        &mut player_audio,
                        &mut media_counters,
                    )
                    .await;
                }
            }
            _ = video_recovery_interval.tick() => {
                let source = *video_source_rx.borrow();
                let source_identity = (source.video_ssrc, source.rtx_ssrc);
                if source_identity != active_video_source {
                    active_video_source = source_identity;
                    discord_rtcp.set_source(source.video_ssrc);
                    transport_feedback.reset();
                    video_recovery.reset();
                    reset_stream_h264_pipeline(
                        &mut h264,
                        &mut h264_startup,
                        &mut h264_startup_buffer,
                    );
                }
                let now = Instant::now();
                if let Some(reset) = video_recovery.take_expired_gap(now) {
                    media_counters.decoder_resets =
                        media_counters.decoder_resets.wrapping_add(1);
                    reset_stream_h264_pipeline(
                        &mut h264,
                        &mut h264_startup,
                        &mut h264_startup_buffer,
                    );
                    let mut pli_sent = None;
                    if source.video_ssrc != 0 {
                        let sent = pli_throttle
                            .send_if_due(
                                &mut discord_rtcp,
                                &discord_socket,
                                &encryptor,
                                local_ssrc,
                                source.video_ssrc,
                                media_started_at.elapsed(),
                            )
                            .await?;
                        media_counters.observe_pli_request(sent);
                        pli_sent = Some(sent);
                    }
                    logging::debug(
                        "stream",
                        format!(
                            "stream video packet gap expired: distance={} pending_packets={} pending_bytes={} gap_age_ms={:?}; reset the H264 pipeline and {}",
                            reset.distance,
                            reset.pending_packets,
                            reset.pending_bytes,
                            reset.gap_age.map(|age| age.as_millis()),
                            match pli_sent {
                                Some(true) => "requested a new keyframe",
                                Some(false) => "kept the recent keyframe request",
                                None => "had no active video source for a keyframe request",
                            }
                        ),
                    );
                } else if let Some(missing) = video_recovery.take_nack_if_due(now)
                    && source.video_ssrc != 0
                {
                    let feedback = build_rtcp_nack(local_ssrc, source.video_ssrc, &missing);
                    discord_rtcp
                        .send_feedback(
                            &discord_socket,
                            &encryptor,
                            local_ssrc,
                            &feedback,
                            "NACK",
                            media_started_at.elapsed(),
                        )
                        .await?;
                    media_counters.nacks = media_counters.nacks.wrapping_add(1);
                }
            }
            _ = keyframe_request_interval.tick(), if !h264_startup.is_started() => {
                let source = *video_source_rx.borrow();
                if source.video_ssrc != 0 {
                    discord_rtcp.set_source(source.video_ssrc);
                    let sent = pli_throttle
                        .send_if_due(
                            &mut discord_rtcp,
                            &discord_socket,
                            &encryptor,
                            local_ssrc,
                            source.video_ssrc,
                            media_started_at.elapsed(),
                        )
                        .await?;
                    media_counters.observe_pli_request(sent);
                    if sent && !logged_keyframe_request {
                        logged_keyframe_request = true;
                        logging::debug(
                            "stream",
                            format!(
                                "stream compound RTCP keyframe request sent: sender_ssrc={local_ssrc} media_ssrc={}",
                                source.video_ssrc
                            ),
                        );
                    }
                }
            }
            status = player.wait() => {
                let status = status.map_err(|error| {
                    StreamConnectionFailure::stop(format!(
                        "wait for stream mpv failed: {error}"
                    ))
                })?;
                player_log_tasks.finish().await;
                let last_error = last_player_error.lock().await.clone();
                logging::debug("stream", format!("stream mpv exited: status={status}"));
                if status.success() {
                    return Ok(());
                }
                return Err(StreamConnectionFailure::stop(match last_error {
                    Some(error) => format!("stream mpv exited with {status}: {error}"),
                    None => format!("stream mpv exited with {status}"),
                }));
            }
            received = discord_socket.recv(&mut packet) => {
                let received =
                    received.map_err(|error| format!("stream UDP receive failed: {error}"))?;
                let packet_arrival = media_started_at.elapsed();
                let packet = &packet[..received];
                if looks_like_rtcp_packet(packet) {
                    let decrypted = match decryptor.decrypt_rtcp_feedback(packet) {
                        Ok(decrypted) => decrypted,
                        Err(error) => {
                            logging::debug(
                                "stream",
                                format!("stream RTCP decrypt failed: {error}"),
                            );
                            continue;
                        }
                    };
                    let reports = match parse_stream_rtcp_sender_reports(&decrypted) {
                        Ok(reports) => reports,
                        Err(error) => {
                            logging::debug(
                                "stream",
                                format!("stream RTCP parse failed: {error}"),
                            );
                            continue;
                        }
                    };
                    let source = *video_source_rx.borrow();
                    let elapsed = media_started_at.elapsed();
                    for report in reports {
                        if report.sender_ssrc != source.audio_ssrc
                            && report.sender_ssrc != source.video_ssrc
                        {
                            continue;
                        }
                        if report.sender_ssrc == source.video_ssrc {
                            discord_rtcp.set_source(source.video_ssrc);
                            discord_rtcp.observe_sender_report(report, elapsed);
                        }
                        presentation_clock.observe_sender_report(report, elapsed);
                        if !logged_discord_sender_report {
                            logged_discord_sender_report = true;
                            logging::debug(
                                "stream",
                                format!(
                                    "Discord RTCP sender clock started: elapsed_ms={} sender_ssrc={} rtp_timestamp={} packets={} octets={}",
                                    elapsed.as_millis(),
                                    report.sender_ssrc,
                                    report.rtp_timestamp,
                                    report.packet_count,
                                    report.octet_count,
                                ),
                            );
                        }
                    }
                    continue;
                }
                let header = match parse_rtp_header(packet) {
                    Ok(header) => header,
                    Err(_) => continue,
                };
                let source = *video_source_rx.borrow();
                if source.audio_ssrc != active_audio_source {
                    active_audio_source = source.audio_ssrc;
                    audio_recovery.reset();
                    local_audio.reset_source();
                }
                let source_identity = (source.video_ssrc, source.rtx_ssrc);
                if source_identity != active_video_source {
                    active_video_source = source_identity;
                    discord_rtcp.set_source(source.video_ssrc);
                    transport_feedback.reset();
                    video_recovery.reset();
                    reset_stream_h264_pipeline(
                        &mut h264,
                        &mut h264_startup,
                        &mut h264_startup_buffer,
                    );
                }
                if header.payload_type != DISCORD_VOICE_PAYLOAD_TYPE
                    && header.payload_type != DISCORD_STREAM_VIDEO_PAYLOAD_TYPE
                    && header.payload_type != DISCORD_STREAM_VIDEO_RTX_PAYLOAD_TYPE
                {
                    continue;
                }
                let decrypted = match decryptor.decrypt_packet_any(packet, &header) {
                    Ok(decrypted) => decrypted,
                    Err(error) => {
                        logging::debug("stream", format!("stream RTP decrypt failed: {error}"));
                        continue;
                    }
                };
                if let Some(sequence) = parse_stream_transport_sequence(
                    decrypted.extension_profile,
                    &decrypted.extension_body,
                ) {
                    transport_feedback.observe(sequence, packet_arrival);
                }
                if header.payload_type == DISCORD_VOICE_PAYLOAD_TYPE
                    && source.audio_ssrc != 0
                    && header.ssrc == source.audio_ssrc
                {
                    let media = dave_state
                        .lock()
                        .await
                        .unwrap_media_payload_for_ssrc(header.ssrc, &decrypted.media_payload);
                    let opus = match media {
                        VoiceMediaPayload::Plain(opus)
                        | VoiceMediaPayload::DaveDecrypted { opus, .. } => opus,
                        _ => continue,
                    };
                    let update = audio_recovery.push(
                        RecoveredStreamAudioPacket {
                            marker: header.marker,
                            sequence: header.sequence,
                            timestamp: header.timestamp,
                            opus,
                        },
                        Instant::now(),
                    );
                    let destination = LocalStreamAudioDestination {
                        socket: &local_socket,
                        target: audio_target,
                        ssrc: source.audio_ssrc,
                        media_started_at,
                        presentation_clock: &presentation_clock,
                    };
                    forward_recovered_stream_audio(
                        update,
                        audio_recovery.pending_len(),
                        &mut local_audio,
                        &destination,
                        &mut player_audio,
                        &mut media_counters,
                    )
                    .await;
                } else if source.video_ssrc != 0 {
                    let received_payload_type = header.payload_type;
                    if received_payload_type == DISCORD_STREAM_VIDEO_PAYLOAD_TYPE
                        && header.ssrc == source.video_ssrc
                    {
                        discord_rtcp.observe_rtp(
                            header.sequence,
                            header.timestamp,
                            packet_arrival,
                        );
                    }
                    let Some(video_packet) =
                        recover_stream_video_packet(header, decrypted.media_payload, source)
                    else {
                        continue;
                    };
                    if received_payload_type == DISCORD_STREAM_VIDEO_RTX_PAYLOAD_TYPE {
                        media_counters.rtx_video_packets =
                            media_counters.rtx_video_packets.wrapping_add(1);
                    } else {
                        media_counters.primary_video_packets =
                            media_counters.primary_video_packets.wrapping_add(1);
                    }
                    let now = Instant::now();
                    let recovery = video_recovery.push(video_packet, now);
                    if let Some(reset) = recovery.reset {
                        media_counters.decoder_resets =
                            media_counters.decoder_resets.wrapping_add(1);
                        reset_stream_h264_pipeline(
                            &mut h264,
                            &mut h264_startup,
                            &mut h264_startup_buffer,
                        );
                        let pli_sent = pli_throttle
                            .send_if_due(
                                &mut discord_rtcp,
                                &discord_socket,
                                &encryptor,
                                local_ssrc,
                                source.video_ssrc,
                                media_started_at.elapsed(),
                            )
                            .await?;
                        media_counters.observe_pli_request(pli_sent);
                        logging::debug(
                            "stream",
                            format!(
                                "stream video recovery budget exceeded: distance={} pending_packets={} packet_limit={} pending_bytes={} byte_limit={} gap_age_ms={:?}; reset the H264 pipeline and {}",
                                reset.distance,
                                reset.pending_packets,
                                STREAM_VIDEO_MAX_PENDING_PACKETS,
                                reset.pending_bytes,
                                STREAM_VIDEO_MAX_PENDING_BYTES,
                                reset.gap_age.map(|age| age.as_millis()),
                                if pli_sent {
                                    "requested a new keyframe"
                                } else {
                                    "kept the recent keyframe request"
                                }
                            ),
                        );
                    }
                    if let Some(missing) = video_recovery.take_nack_if_due(now) {
                        let feedback = build_rtcp_nack(local_ssrc, source.video_ssrc, &missing);
                        discord_rtcp
                            .send_feedback(
                                &discord_socket,
                                &encryptor,
                                local_ssrc,
                                &feedback,
                                "NACK",
                                media_started_at.elapsed(),
                            )
                            .await?;
                        media_counters.nacks = media_counters.nacks.wrapping_add(1);
                    }
                    for video_packet in recovery.ready {
                        let header = video_packet.header;
                        let frame = match h264.push(&header, &video_packet.payload) {
                            H264DepacketizerOutput::Pending => continue,
                            H264DepacketizerOutput::Frame(frame) => {
                                media_counters.h264_frames =
                                    media_counters.h264_frames.wrapping_add(1);
                                media_counters.h264_bytes = media_counters
                                    .h264_bytes
                                    .wrapping_add(
                                        u64::try_from(frame.len())
                                            .expect("bounded H264 frame length fits u64"),
                                    );
                                frame
                            }
                            H264DepacketizerOutput::BudgetExceeded => {
                                media_counters.decoder_resets =
                                    media_counters.decoder_resets.wrapping_add(1);
                                reset_stream_h264_pipeline(
                                    &mut h264,
                                    &mut h264_startup,
                                    &mut h264_startup_buffer,
                                );
                                let pli_sent = pli_throttle
                                    .send_if_due(
                                        &mut discord_rtcp,
                                        &discord_socket,
                                        &encryptor,
                                        local_ssrc,
                                        source.video_ssrc,
                                        media_started_at.elapsed(),
                                    )
                                    .await?;
                                media_counters.observe_pli_request(pli_sent);
                                logging::debug(
                                    "stream",
                                    format!(
                                        "stream H264 access unit exceeded its safety budget; {}",
                                        if pli_sent {
                                            "requested a new keyframe"
                                        } else {
                                            "kept the recent keyframe request"
                                        }
                                    ),
                                );
                                continue;
                            }
                        };
                        let frame = match dave_state
                            .lock()
                            .await
                            .decrypt_video_frame(owner_id, &frame)
                        {
                            Ok(Some(frame)) => frame,
                            Ok(None) => continue,
                            Err(error) => {
                                logging::debug("stream", error);
                                continue;
                            }
                        };
                        if !logged_first_video_frame {
                            logged_first_video_frame = true;
                            logging::debug(
                                "stream",
                                format!(
                                    "first stream video frame decrypted: elapsed_ms={} nal_types={:?}",
                                    media_started_at.elapsed().as_millis(),
                                    h264_nal_types(&frame),
                                ),
                            );
                        }
                        let player_ready = video_player_ready.load(Ordering::Acquire);
                        let local_video_destination = LocalStreamVideoDestination {
                            socket: &local_socket,
                            target: video_target,
                            ssrc: source.video_ssrc,
                            media_started_at,
                        };
                        if player_ready {
                            local_video
                                .replay_startup(
                                    &mut h264_startup_buffer,
                                    &local_video_destination,
                                )
                                .await;
                        }
                        if !player_ready && !logged_video_before_player_ready {
                            logged_video_before_player_ready = true;
                            logging::debug(
                                "stream",
                                "buffering stream video until mpv opens its SDP input",
                            );
                        }
                        let waiting_for_keyframe = !h264_startup.is_started();
                        let frame = accept_or_buffer_h264(
                            player_ready,
                            &mut h264_startup,
                            &mut h264_startup_buffer,
                            frame,
                            header.timestamp,
                        );
                        if waiting_for_keyframe && h264_startup.is_started() {
                            logging::debug(
                                "stream",
                                format!(
                                    "stream H264 keyframe {}: elapsed_ms={}",
                                    if player_ready { "accepted" } else { "buffered" },
                                    media_started_at.elapsed().as_millis()
                                ),
                            );
                        }
                        let Some(frame) = frame else {
                            continue;
                        };
                        local_video
                            .forward_live(
                                &local_video_destination,
                                &frame,
                                presentation_clock.map_timestamp(
                                    source.video_ssrc,
                                    frame.source_timestamp,
                                    VIDEO_RTP_CLOCK_RATE,
                                ),
                            )
                            .await;
                    }
                }
            }
        }
    }
}
