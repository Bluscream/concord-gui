use std::{net::SocketAddrV4, path::Path, process::Stdio};

use tokio::process::Command;

use crate::support::media_player::MediaPlayerIpcEndpoint;

use super::*;

pub fn stream_player_spawn_failure(error: std::io::Error) -> StreamConnectionFailure {
    let message = if error.kind() == std::io::ErrorKind::NotFound {
        "mpv is required to watch Discord streams; install mpv and make sure it is on PATH"
            .to_owned()
    } else {
        format!("start mpv for stream failed: {error}")
    };
    StreamConnectionFailure::stop(message)
}

pub fn stream_player_command(
    sdp_path: &Path,
    input_config_path: &Path,
    display_name: &str,
    ipc_server: &str,
) -> Command {
    let mut player = Command::new("mpv");
    player
        // Keep playback deterministic and prevent user cache settings from
        // turning this live input into a delayed stream.
        .arg("--no-config")
        // mpv disables terminal logs when its output is redirected. Force the
        // line-based log stream on so Concord can capture player lifecycle
        // events through stdout.
        .arg("--terminal=yes")
        // Built-in UI scripts delay SDP socket creation and are not needed for
        // the dedicated stream window.
        .arg("--load-scripts=no")
        // RTP has no seekable live edge. Remove pause controls that would leave
        // the viewer replaying buffered history after the broadcast resumes.
        .arg("--osc=no")
        // Keep normal output quiet, but include the lifecycle stages needed to
        // separate SDP, decoder, and display startup delay in a live log.
        .arg("--msg-level=all=warn,cplayer=v,lavf=v,vd=v,ad=v")
        // An SDP audio track can remain empty for a video-only broadcast. Do
        // not let that selected track block video startup. The first real Opus
        // packet selects it through JSON IPC after mpv has loaded the SDP.
        .arg("--aid=no")
        .arg(format!("--input-ipc-server={ipc_server}"))
        .arg(format!("--input-conf={}", input_config_path.display()))
        // Prefer mpv's safe hardware allowlist and let libavcodec use the
        // available CPU cores when it falls back.
        .arg("--hwdec=auto-safe")
        .arg("--vd-lavc-threads=0")
        // A high-resolution encoded frame can arrive as a short UDP burst.
        // Keep enough byte capacity for that burst and only 150ms of forward
        // media, which smooths arrival jitter without retaining stale history.
        .arg("--stream-buffer-size=1MiB")
        .arg("--audio-buffer=0.05")
        .arg("--cache=yes")
        .arg("--cache-pause=no")
        .arg("--cache-pause-initial=no")
        .arg("--cache-secs=0.15")
        .arg("--demuxer-readahead-secs=0.15")
        .arg("--demuxer-max-bytes=16MiB")
        .arg("--demuxer-max-back-bytes=0")
        .arg("--demuxer=lavf")
        .arg("--demuxer-lavf-format=sdp")
        .arg("--demuxer-lavf-probe-info=nostreams")
        .arg("--demuxer-lavf-analyzeduration=0.1")
        .arg("--demuxer-lavf-probesize=32")
        .arg("--demuxer-lavf-buffersize=262144")
        // mpv uses commas between lavf key/value options. Square brackets keep
        // the protocol list together as one value.
        .arg("--demuxer-lavf-o=protocol_whitelist=[file,udp,rtp],buffer_size=4194304,max_delay=50000,reorder_queue_size=512")
        .arg("--force-window=immediate")
        // Discord can change the encoded resolution when the broadcaster
        // resizes the captured window. Keep the native player window stable
        // and scale the video inside it instead of following every change.
        .arg("--auto-window-resize=no")
        .arg("--geometry=1280x720")
        .arg(format!("--title=Concord - {display_name}'s stream"))
        // Audio and video share one player so its volume and mute controls
        // apply to the complete broadcast.
        .arg("--video-latency-hacks=no")
        .arg("--video-sync=audio")
        // Skip late output frames instead of preserving live delay.
        // Decoder dropping can discard H264 reference frames.
        .arg("--framedrop=vo")
        .arg("--video-timing-offset=0")
        .arg("--")
        .arg(sdp_path)
        // Both output streams remain enabled so startup stages and failures
        // reach the Concord log. stdin is closed so mpv cannot consume TUI
        // input.
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    player
}

#[derive(Default)]
pub struct StreamPlayerAudioState {
    pub real_packet_observed: bool,
    pub enable_requested: bool,
}

impl StreamPlayerAudioState {
    pub fn observe_real_packet(&mut self) {
        self.real_packet_observed = true;
    }

    pub fn take_enable_request(&mut self, player_ready: bool) -> bool {
        if !self.real_packet_observed || !player_ready || self.enable_requested {
            return false;
        }
        self.enable_requested = true;
        true
    }
}

pub async fn maybe_enable_stream_player_audio(
    state: &mut StreamPlayerAudioState,
    player_ready: bool,
    endpoint: &MediaPlayerIpcEndpoint,
) {
    if !state.take_enable_request(player_ready) {
        return;
    }

    match timeout(
        STREAM_PLAYER_AUDIO_ENABLE_TIMEOUT,
        endpoint.set_property("aid", "auto"),
    )
    .await
    {
        Ok(Ok(())) => logging::debug("stream", "stream mpv audio enabled"),
        Ok(Err(error)) => {
            logging::error("stream", format!("enable stream mpv audio failed: {error}"))
        }
        Err(_) => logging::error(
            "stream",
            format!(
                "enable stream mpv audio timed out after {} second",
                STREAM_PLAYER_AUDIO_ENABLE_TIMEOUT.as_secs()
            ),
        ),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StreamAudioClockDiscontinuity {
    pub source_delta_ticks: i32,
    pub elapsed_delta_ticks: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StreamAudioTimestamp {
    pub local_timestamp: u32,
    pub discontinuity: Option<StreamAudioClockDiscontinuity>,
}

#[derive(Default)]
pub struct LocalStreamAudioClock {
    pub last_source_timestamp: Option<u32>,
    pub last_local_timestamp: Option<u32>,
    pub last_elapsed: Option<Duration>,
}

impl LocalStreamAudioClock {
    pub fn is_recent_replay(&self, source_timestamp: u32) -> bool {
        let Some(last_source_timestamp) = self.last_source_timestamp else {
            return false;
        };
        let source_delta_ticks = source_timestamp.wrapping_sub(last_source_timestamp) as i32;
        source_delta_ticks <= 0
            && source_delta_ticks.unsigned_abs() <= STREAM_AUDIO_CLOCK_DRIFT_TOLERANCE_TICKS
    }

    // Discord can replay an old packet or restart an audio clock when a stream
    // subscription settles. Valid source deltas retain their media timing. A
    // discontinuity starts a new source epoch on the existing local timeline.
    pub fn rebase(
        &mut self,
        source_timestamp: u32,
        elapsed: Duration,
        synchronized_timestamp: Option<u32>,
    ) -> StreamAudioTimestamp {
        let Some(last_source_timestamp) = self.last_source_timestamp else {
            let fallback_timestamp = elapsed_rtp_timestamp(elapsed, OPUS_RTP_CLOCK_RATE);
            let local_timestamp = select_synchronized_rtp_timestamp(
                fallback_timestamp,
                synchronized_timestamp,
                None,
                OPUS_RTP_CLOCK_RATE,
            );
            self.record(source_timestamp, local_timestamp, elapsed);
            return StreamAudioTimestamp {
                local_timestamp,
                discontinuity: None,
            };
        };
        let last_local_timestamp = self
            .last_local_timestamp
            .expect("an anchored audio clock has a local timestamp");
        let last_elapsed = self
            .last_elapsed
            .expect("an anchored audio clock has an elapsed timestamp");
        let elapsed_delta_ticks =
            elapsed_rtp_timestamp(elapsed.saturating_sub(last_elapsed), OPUS_RTP_CLOCK_RATE);
        let source_delta_ticks = source_timestamp.wrapping_sub(last_source_timestamp) as i32;
        let drift_ticks = i64::from(source_delta_ticks).abs_diff(i64::from(elapsed_delta_ticks));
        let source_is_continuous = source_delta_ticks > 0
            && drift_ticks <= u64::from(STREAM_AUDIO_CLOCK_DRIFT_TOLERANCE_TICKS);

        let (fallback_timestamp, discontinuity) = if source_is_continuous {
            (
                last_local_timestamp.wrapping_add(source_delta_ticks as u32),
                None,
            )
        } else {
            let elapsed_timestamp = elapsed_rtp_timestamp(elapsed, OPUS_RTP_CLOCK_RATE);
            let minimum_timestamp =
                last_local_timestamp.wrapping_add(DISCORD_OPUS_TIMESTAMP_INCREMENT);
            (
                later_rtp_timestamp(elapsed_timestamp, minimum_timestamp),
                Some(StreamAudioClockDiscontinuity {
                    source_delta_ticks,
                    elapsed_delta_ticks,
                }),
            )
        };
        let local_timestamp = select_synchronized_rtp_timestamp(
            fallback_timestamp,
            synchronized_timestamp,
            Some(last_local_timestamp),
            OPUS_RTP_CLOCK_RATE,
        );

        self.record(source_timestamp, local_timestamp, elapsed);
        StreamAudioTimestamp {
            local_timestamp,
            discontinuity,
        }
    }

    pub fn timestamp_at(&self, elapsed: Duration) -> Option<u32> {
        let last_local_timestamp = self.last_local_timestamp?;
        let last_elapsed = self.last_elapsed?;
        Some(last_local_timestamp.wrapping_add(elapsed_rtp_timestamp(
            elapsed.saturating_sub(last_elapsed),
            OPUS_RTP_CLOCK_RATE,
        )))
    }

    pub fn record(&mut self, source_timestamp: u32, local_timestamp: u32, elapsed: Duration) {
        self.last_source_timestamp = Some(source_timestamp);
        self.last_local_timestamp = Some(local_timestamp);
        self.last_elapsed = Some(elapsed);
    }
}

#[derive(Default)]
pub struct LocalStreamAudioForwarder {
    pub sequence: u16,
    pub packets: u32,
    pub octets: u32,
    pub clock: LocalStreamAudioClock,
    pub logged_first_packet: bool,
}

pub struct LocalStreamAudioDestination<'a> {
    pub socket: &'a UdpSocket,
    pub target: SocketAddrV4,
    pub ssrc: u32,
    pub media_started_at: Instant,
    pub presentation_clock: &'a StreamPresentationClock,
}

impl LocalStreamAudioForwarder {
    pub async fn forward(
        &mut self,
        packets: Vec<RecoveredStreamAudioPacket>,
        skipped_sequences: u16,
        destination: &LocalStreamAudioDestination<'_>,
        player_audio: &mut StreamPlayerAudioState,
    ) -> u64 {
        self.sequence = self.sequence.wrapping_add(skipped_sequences);
        let mut dropped_replays = 0u64;
        for packet in packets {
            // A short backward source timestamp is a delayed replay, not a new
            // clock epoch. Leave a local RTP sequence gap so mpv can conceal
            // it instead of decoding old Opus after newer audio.
            if self.clock.is_recent_replay(packet.timestamp) {
                self.sequence = self.sequence.wrapping_add(1);
                dropped_replays = dropped_replays.wrapping_add(1);
                continue;
            }
            player_audio.observe_real_packet();
            let elapsed = destination.media_started_at.elapsed();
            let synchronized_timestamp = destination.presentation_clock.map_timestamp(
                destination.ssrc,
                packet.timestamp,
                OPUS_RTP_CLOCK_RATE,
            );
            let audio_timestamp =
                self.clock
                    .rebase(packet.timestamp, elapsed, synchronized_timestamp);
            let local_timestamp = audio_timestamp.local_timestamp;
            if let Some(discontinuity) = audio_timestamp.discontinuity {
                logging::debug(
                    "stream",
                    format!(
                        "stream audio RTP clock re-anchored: source_timestamp={} source_delta_ticks={} elapsed_delta_ticks={} local_timestamp={local_timestamp}",
                        packet.timestamp,
                        discontinuity.source_delta_ticks,
                        discontinuity.elapsed_delta_ticks,
                    ),
                );
            }
            let local_packet = build_local_rtp_packet(
                LOCAL_STREAM_AUDIO_PAYLOAD_TYPE,
                packet.marker,
                self.sequence,
                local_timestamp,
                destination.ssrc,
                &packet.opus,
            );
            self.sequence = self.sequence.wrapping_add(1);
            let _ = destination
                .socket
                .send_to(&local_packet, destination.target)
                .await;
            self.packets = self.packets.wrapping_add(1);
            self.octets = self.octets.wrapping_add(packet.opus.len() as u32);
            if !self.logged_first_packet {
                self.logged_first_packet = true;
                logging::debug(
                    "stream",
                    format!(
                        "first stream audio forwarded: elapsed_ms={} source_timestamp={} local_timestamp={local_timestamp}",
                        elapsed.as_millis(),
                        packet.timestamp,
                    ),
                );
            }
        }
        dropped_replays
    }

    pub fn reset_source(&mut self) {
        self.clock = LocalStreamAudioClock::default();
    }

    pub fn timestamp_at(&self, elapsed: Duration) -> Option<u32> {
        self.clock.timestamp_at(elapsed)
    }
}

pub async fn forward_recovered_stream_audio(
    update: StreamAudioRecoveryUpdate,
    pending_packets: usize,
    forwarder: &mut LocalStreamAudioForwarder,
    destination: &LocalStreamAudioDestination<'_>,
    player_audio: &mut StreamPlayerAudioState,
    counters: &mut StreamMediaCounters,
) {
    counters.audio_stale_packets = counters
        .audio_stale_packets
        .wrapping_add(update.dropped_stale_packets);
    counters.audio_skipped_packets = counters
        .audio_skipped_packets
        .wrapping_add(u64::from(update.skipped_sequences));
    if update.skipped_sequences != 0 {
        logging::debug(
            "stream",
            format!(
                "stream audio packet gap expired: skipped_packets={} pending_packets={pending_packets}",
                update.skipped_sequences,
            ),
        );
    }
    let dropped_replays = forwarder
        .forward(
            update.ready,
            update.skipped_sequences,
            destination,
            player_audio,
        )
        .await;
    counters.audio_stale_packets = counters.audio_stale_packets.wrapping_add(dropped_replays);
}

#[derive(Default)]
pub struct LocalRtpClock {
    pub source_origin: Option<u32>,
    pub local_origin: u32,
}

impl LocalRtpClock {
    pub fn anchor(&mut self, source_timestamp: u32, local_timestamp: u32) {
        self.source_origin = Some(source_timestamp);
        self.local_origin = local_timestamp;
    }

    pub fn rebase(
        &mut self,
        source_timestamp: u32,
        elapsed: Duration,
        clock_rate: u32,
        synchronized_timestamp: Option<u32>,
    ) -> u32 {
        let previous_timestamp = self.source_origin.map(|_| self.local_origin);
        let source_origin = *self.source_origin.get_or_insert_with(|| {
            self.local_origin = elapsed_rtp_timestamp(elapsed, clock_rate);
            source_timestamp
        });
        let fallback_timestamp = self
            .local_origin
            .wrapping_add(source_timestamp.wrapping_sub(source_origin));
        let local_timestamp = select_synchronized_rtp_timestamp(
            fallback_timestamp,
            synchronized_timestamp,
            previous_timestamp,
            clock_rate,
        );
        self.anchor(source_timestamp, local_timestamp);
        local_timestamp
    }
}

pub fn select_synchronized_rtp_timestamp(
    fallback_timestamp: u32,
    synchronized_timestamp: Option<u32>,
    previous_timestamp: Option<u32>,
    clock_rate: u32,
) -> u32 {
    let Some(synchronized_timestamp) = synchronized_timestamp else {
        return fallback_timestamp;
    };
    let correction = synchronized_timestamp
        .wrapping_sub(fallback_timestamp)
        .cast_signed()
        .unsigned_abs();
    let maximum_correction =
        elapsed_rtp_timestamp(STREAM_PRESENTATION_CLOCK_MAX_CORRECTION, clock_rate);
    if correction > maximum_correction
        || previous_timestamp.is_some_and(|previous| {
            synchronized_timestamp.wrapping_sub(previous).cast_signed() <= 0
        })
    {
        fallback_timestamp
    } else {
        synchronized_timestamp
    }
}

#[derive(Default)]
pub struct LocalStreamVideoForwarder {
    pub sequence: u16,
    pub packets: u32,
    pub octets: u32,
    pub frames: u64,
    pub clock: LocalRtpClock,
    pub logged_first_frame: bool,
}

pub struct LocalStreamVideoDestination<'a> {
    pub socket: &'a UdpSocket,
    pub target: SocketAddrV4,
    pub ssrc: u32,
    pub media_started_at: Instant,
}

impl LocalStreamVideoForwarder {
    pub async fn replay_startup(
        &mut self,
        startup_buffer: &mut H264StartupBuffer,
        destination: &LocalStreamVideoDestination<'_>,
    ) {
        if startup_buffer.is_empty() {
            return;
        }

        let buffered_frames = startup_buffer.len();
        let replay_origin =
            elapsed_rtp_timestamp(destination.media_started_at.elapsed(), VIDEO_RTP_CLOCK_RATE);
        let mut replay_anchor = None;
        for (index, buffered) in startup_buffer.take().into_iter().enumerate() {
            let local_timestamp = replay_origin.wrapping_add(
                u32::try_from(index)
                    .expect("startup buffer length is bounded")
                    .wrapping_mul(STREAM_STARTUP_REPLAY_FRAME_TICKS),
            );
            self.forward_at_timestamp(destination, &buffered, local_timestamp, true)
                .await;
            replay_anchor = Some((buffered.source_timestamp, local_timestamp));
        }
        if let Some((source_timestamp, local_timestamp)) = replay_anchor {
            self.clock.anchor(source_timestamp, local_timestamp);
        }
        logging::debug(
            "stream",
            format!(
                "buffered H264 startup replayed: elapsed_ms={} frames={buffered_frames}",
                destination.media_started_at.elapsed().as_millis()
            ),
        );
    }

    pub async fn forward_live(
        &mut self,
        destination: &LocalStreamVideoDestination<'_>,
        frame: &BufferedH264Frame,
        synchronized_timestamp: Option<u32>,
    ) {
        let local_timestamp = self.clock.rebase(
            frame.source_timestamp,
            destination.media_started_at.elapsed(),
            VIDEO_RTP_CLOCK_RATE,
            synchronized_timestamp,
        );
        self.forward_at_timestamp(destination, frame, local_timestamp, false)
            .await;
    }

    pub async fn forward_at_timestamp(
        &mut self,
        destination: &LocalStreamVideoDestination<'_>,
        frame: &BufferedH264Frame,
        local_timestamp: u32,
        buffered: bool,
    ) {
        let (packet_count, octet_count) = send_local_h264_frame(
            destination.socket,
            destination.target,
            &frame.encoded,
            local_timestamp,
            destination.ssrc,
            &mut self.sequence,
        )
        .await;
        self.packets = self.packets.wrapping_add(packet_count);
        self.octets = self.octets.wrapping_add(octet_count);
        self.frames = self.frames.wrapping_add(1);
        if !self.logged_first_frame {
            self.logged_first_frame = true;
            logging::debug(
                "stream",
                format!(
                    "first {}stream video forwarded: elapsed_ms={} source_timestamp={} local_timestamp={local_timestamp}",
                    if buffered { "buffered " } else { "" },
                    destination.media_started_at.elapsed().as_millis(),
                    frame.source_timestamp,
                ),
            );
        }
    }
}

pub fn elapsed_rtp_timestamp(elapsed: Duration, clock_rate: u32) -> u32 {
    let whole = elapsed.as_secs().wrapping_mul(u64::from(clock_rate));
    let fractional =
        u64::from(elapsed.subsec_nanos()).wrapping_mul(u64::from(clock_rate)) / 1_000_000_000;
    whole.wrapping_add(fractional) as u32
}

pub fn later_rtp_timestamp(left: u32, right: u32) -> u32 {
    if left.wrapping_sub(right) as i32 >= 0 {
        left
    } else {
        right
    }
}

pub fn build_rtcp_receiver_report(
    sender_ssrc: u32,
    block: Option<StreamRtcpReportBlock>,
) -> Vec<u8> {
    let report_count = u8::from(block.is_some());
    let mut packet = Vec::with_capacity(if block.is_some() { 32 } else { 8 });
    packet.extend_from_slice(&[
        (RTP_VERSION << 6) | report_count,
        RTCP_RECEIVER_REPORT,
        0,
        0,
    ]);
    packet.extend_from_slice(&sender_ssrc.to_be_bytes());
    if let Some(block) = block {
        packet.extend_from_slice(&block.source_ssrc.to_be_bytes());
        packet.push(block.fraction_lost);
        let cumulative_lost = block
            .cumulative_lost
            .clamp(-0x80_0000, 0x7f_ffff)
            .to_be_bytes();
        packet.extend_from_slice(&cumulative_lost[1..]);
        packet.extend_from_slice(&block.extended_highest_sequence.to_be_bytes());
        packet.extend_from_slice(&block.interarrival_jitter.to_be_bytes());
        packet.extend_from_slice(&block.last_sender_report.to_be_bytes());
        packet.extend_from_slice(&block.delay_since_last_sender_report.to_be_bytes());
    }
    let length_words_minus_one =
        u16::try_from(packet.len() / 4 - 1).expect("RTCP receiver report length fits u16");
    packet[2..4].copy_from_slice(&length_words_minus_one.to_be_bytes());
    packet
}

pub fn build_rtcp_sdes_cname(sender_ssrc: u32) -> Vec<u8> {
    let cname = format!("concord-{sender_ssrc}");
    let cname_len = u8::try_from(cname.len()).expect("stream RTCP CNAME fits u8");
    let mut packet = Vec::with_capacity(12 + cname.len());
    packet.extend_from_slice(&[(RTP_VERSION << 6) | 1, RTCP_SOURCE_DESCRIPTION, 0, 0]);
    packet.extend_from_slice(&sender_ssrc.to_be_bytes());
    packet.extend_from_slice(&[RTCP_SDES_CNAME, cname_len]);
    packet.extend_from_slice(cname.as_bytes());
    packet.push(0);
    while !packet.len().is_multiple_of(4) {
        packet.push(0);
    }
    let length_words_minus_one =
        u16::try_from(packet.len() / 4 - 1).expect("RTCP SDES length fits u16");
    packet[2..4].copy_from_slice(&length_words_minus_one.to_be_bytes());
    packet
}

pub fn build_stream_rtcp_compound(
    sender_ssrc: u32,
    block: Option<StreamRtcpReportBlock>,
    feedback: Option<&[u8]>,
) -> Vec<u8> {
    let mut packet = build_rtcp_receiver_report(sender_ssrc, block);
    packet.extend_from_slice(&build_rtcp_sdes_cname(sender_ssrc));
    if let Some(feedback) = feedback {
        packet.extend_from_slice(feedback);
    }
    packet
}

pub fn build_rtcp_pli(sender_ssrc: u32, media_ssrc: u32) -> [u8; 12] {
    let mut packet = [0u8; 12];
    packet[0] = (RTP_VERSION << 6) | RTCP_PLI_FORMAT;
    packet[1] = RTCP_PAYLOAD_SPECIFIC_FEEDBACK;
    packet[2..4].copy_from_slice(&RTCP_PLI_LENGTH_WORDS_MINUS_ONE.to_be_bytes());
    packet[4..8].copy_from_slice(&sender_ssrc.to_be_bytes());
    packet[8..12].copy_from_slice(&media_ssrc.to_be_bytes());
    packet
}

pub fn build_rtcp_nack(sender_ssrc: u32, media_ssrc: u32, missing: &[u16]) -> Vec<u8> {
    let mut feedback_control = Vec::new();
    let mut index = 0usize;
    while let Some(&packet_id) = missing.get(index) {
        index += 1;
        let mut bitmask = 0u16;
        while let Some(&sequence) = missing.get(index) {
            let distance = sequence.wrapping_sub(packet_id);
            if !(1..=16).contains(&distance) {
                break;
            }
            bitmask |= 1 << (distance - 1);
            index += 1;
        }
        feedback_control.extend_from_slice(&packet_id.to_be_bytes());
        feedback_control.extend_from_slice(&bitmask.to_be_bytes());
    }

    let mut packet = Vec::with_capacity(12 + feedback_control.len());
    packet.push((RTP_VERSION << 6) | RTCP_GENERIC_NACK_FORMAT);
    packet.push(RTCP_TRANSPORT_LAYER_FEEDBACK);
    let length_words_minus_one =
        u16::try_from((12 + feedback_control.len()) / 4 - 1).expect("RTCP NACK length fits u16");
    packet.extend_from_slice(&length_words_minus_one.to_be_bytes());
    packet.extend_from_slice(&sender_ssrc.to_be_bytes());
    packet.extend_from_slice(&media_ssrc.to_be_bytes());
    packet.extend_from_slice(&feedback_control);
    packet
}
