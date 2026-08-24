use std::collections::BTreeMap;

use super::*;

impl StreamRtcpControl {
    pub fn set_source(&mut self, source_ssrc: u32) {
        if self.source_ssrc == source_ssrc {
            return;
        }
        let nonce = self.nonce;
        *self = Self {
            nonce,
            source_ssrc,
            ..Self::default()
        };
    }

    pub fn observe_rtp(&mut self, sequence: u16, timestamp: u32, arrival: Duration) {
        if self.source_ssrc == 0 {
            return;
        }
        let extended = extend_transport_sequence(sequence, self.highest_extended_sequence);
        self.base_extended_sequence.get_or_insert(extended);
        if self
            .highest_extended_sequence
            .is_none_or(|highest| extended > highest)
        {
            self.highest_extended_sequence = Some(extended);
        }
        self.received_packets = self.received_packets.wrapping_add(1);

        let arrival_timestamp = elapsed_rtp_timestamp(arrival, VIDEO_RTP_CLOCK_RATE);
        let origin = *self.jitter_origin.get_or_insert(StreamRtcpJitterOrigin {
            arrival_timestamp,
            source_timestamp: timestamp,
        });
        let arrival_delta = arrival_timestamp.wrapping_sub(origin.arrival_timestamp) as i32;
        let source_delta = timestamp.wrapping_sub(origin.source_timestamp) as i32;
        let transit = i64::from(arrival_delta) - i64::from(source_delta);
        if let Some(previous_transit) = self.previous_transit {
            let delta = transit.abs_diff(previous_transit) as i64;
            self.jitter_q4 += delta - ((self.jitter_q4 + 8) >> 4);
        }
        self.previous_transit = Some(transit);
    }

    pub fn observe_sender_report(&mut self, report: StreamRtcpSenderReport, received_at: Duration) {
        if report.sender_ssrc != self.source_ssrc {
            return;
        }
        self.last_sender_report = Some(StreamRtcpLastSenderReport {
            middle_ntp_timestamp: (report.ntp_timestamp >> 16) as u32,
            received_at,
        });
    }

    pub fn report_block(&mut self, now: Duration) -> Option<StreamRtcpReportBlock> {
        let base = self.base_extended_sequence?;
        let highest = self.highest_extended_sequence?;
        let expected = highest.wrapping_sub(base).wrapping_add(1);
        let expected_interval = expected.wrapping_sub(self.expected_prior);
        let received_interval = self.received_packets.wrapping_sub(self.received_prior);
        let lost_interval = i64::from(expected_interval) - i64::from(received_interval);
        let fraction_lost = if expected_interval == 0 || lost_interval <= 0 {
            0
        } else {
            u8::try_from(((lost_interval << 8) / i64::from(expected_interval)).min(255))
                .expect("RTCP fraction loss is bounded to u8")
        };
        self.expected_prior = expected;
        self.received_prior = self.received_packets;

        let cumulative_lost = (i64::from(expected) - i64::from(self.received_packets))
            .clamp(-0x80_0000, 0x7f_ffff) as i32;
        let (last_sender_report, delay_since_last_sender_report) = self
            .last_sender_report
            .map(|report| {
                (
                    report.middle_ntp_timestamp,
                    duration_to_rtcp_delay(now.saturating_sub(report.received_at)),
                )
            })
            .unwrap_or_default();
        Some(StreamRtcpReportBlock {
            source_ssrc: self.source_ssrc,
            fraction_lost,
            cumulative_lost,
            extended_highest_sequence: highest,
            interarrival_jitter: u32::try_from((self.jitter_q4.max(0) + 8) >> 4)
                .unwrap_or(u32::MAX),
            last_sender_report,
            delay_since_last_sender_report,
        })
    }

    pub async fn send_feedback(
        &mut self,
        socket: &UdpSocket,
        encryptor: &VoiceRtpEncryptor,
        sender_ssrc: u32,
        feedback: &[u8],
        kind: &str,
        elapsed: Duration,
    ) -> Result<(), String> {
        let report = self.report_block(elapsed);
        let packet = build_stream_rtcp_compound(sender_ssrc, report, Some(feedback));
        self.send_packet(socket, encryptor, &packet, kind).await
    }

    pub async fn send_report(
        &mut self,
        socket: &UdpSocket,
        encryptor: &VoiceRtpEncryptor,
        sender_ssrc: u32,
        elapsed: Duration,
    ) -> Result<(), String> {
        let report = self.report_block(elapsed);
        let packet = build_stream_rtcp_compound(sender_ssrc, report, None);
        self.send_packet(socket, encryptor, &packet, "receiver report")
            .await
    }

    pub async fn send_packet(
        &mut self,
        socket: &UdpSocket,
        encryptor: &VoiceRtpEncryptor,
        packet: &[u8],
        kind: &str,
    ) -> Result<(), String> {
        let encrypted = encryptor.encrypt_rtcp_feedback(packet, self.nonce.to_be_bytes())?;
        self.nonce = self
            .nonce
            .checked_add(1)
            .ok_or_else(|| "stream RTCP nonce exhausted".to_owned())?;
        socket
            .send(&encrypted)
            .await
            .map_err(|error| format!("send stream RTCP {kind} failed: {error}"))?;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StreamTransportReceiveDelta {
    Small(u8),
    Large(i16),
}

impl StreamTransportReceiveDelta {
    pub fn status(self) -> u8 {
        match self {
            Self::Small(_) => TRANSPORT_PACKET_RECEIVED_SMALL_DELTA,
            Self::Large(_) => TRANSPORT_PACKET_RECEIVED_LARGE_DELTA,
        }
    }
}

#[derive(Default)]
pub struct StreamTransportFeedback {
    pub arrivals: BTreeMap<u32, Duration>,
    pub highest_extended_sequence: Option<u32>,
    pub next_unreported_sequence: Option<u32>,
    pub last_reported_sequence: Option<u32>,
    pub feedback_packet_count: u8,
}

impl StreamTransportFeedback {
    pub fn reset(&mut self) {
        self.arrivals.clear();
        self.highest_extended_sequence = None;
        self.next_unreported_sequence = None;
        self.last_reported_sequence = None;
    }

    pub fn observe(&mut self, sequence: u16, arrival: Duration) {
        let extended = extend_transport_sequence(sequence, self.highest_extended_sequence);
        if self
            .last_reported_sequence
            .is_some_and(|reported| extended <= reported)
        {
            return;
        }

        if self.next_unreported_sequence.is_some_and(|base| {
            extended.saturating_sub(base) >= STREAM_TRANSPORT_FEEDBACK_MAX_STATUSES as u32
        }) {
            self.arrivals.clear();
            self.next_unreported_sequence = Some(extended);
            self.last_reported_sequence = extended.checked_sub(1);
        } else {
            self.next_unreported_sequence = Some(
                self.next_unreported_sequence
                    .map_or(extended, |base| base.min(extended)),
            );
        }

        self.highest_extended_sequence = Some(
            self.highest_extended_sequence
                .map_or(extended, |highest| highest.max(extended)),
        );
        self.arrivals.entry(extended).or_insert(arrival);
    }

    pub fn take_feedback(&mut self, sender_ssrc: u32, media_ssrc: u32) -> Option<Vec<u8>> {
        let base = self.next_unreported_sequence?;
        let highest = self.highest_extended_sequence?;
        if base > highest {
            return None;
        }
        let end =
            highest.min(base.saturating_add(STREAM_TRANSPORT_FEEDBACK_MAX_STATUSES as u32 - 1));
        let first_received = self.arrivals.range(base..=end).next()?.1;
        let first_arrival_micros = duration_micros(*first_received);
        let reference_time =
            first_arrival_micros.div_euclid(TRANSPORT_FEEDBACK_REFERENCE_TIME_MICROS);
        let reference_micros = reference_time * TRANSPORT_FEEDBACK_REFERENCE_TIME_MICROS;

        let mut statuses = Vec::with_capacity((end - base + 1) as usize);
        let mut deltas = Vec::new();
        let mut previous_received_micros = reference_micros;
        for extended in base..=end {
            let Some(arrival) = self.arrivals.get(&extended) else {
                statuses.push(TRANSPORT_PACKET_NOT_RECEIVED);
                continue;
            };
            let arrival_micros = duration_micros(*arrival);
            let delta = rounded_divide(
                arrival_micros - previous_received_micros,
                TRANSPORT_FEEDBACK_DELTA_MICROS,
            );
            let encoded = if let Ok(delta) = u8::try_from(delta) {
                StreamTransportReceiveDelta::Small(delta)
            } else if let Ok(delta) = i16::try_from(delta) {
                StreamTransportReceiveDelta::Large(delta)
            } else {
                break;
            };
            statuses.push(encoded.status());
            deltas.push(encoded);
            previous_received_micros = arrival_micros;
        }
        if statuses.is_empty() || !statuses.iter().any(|status| *status != 0) {
            return None;
        }

        let status_count =
            u16::try_from(statuses.len()).expect("transport status count is bounded");
        let actual_end = base + u32::from(status_count) - 1;
        let packet = build_transport_wide_feedback(
            sender_ssrc,
            media_ssrc,
            base as u16,
            reference_time,
            self.feedback_packet_count,
            &statuses,
            &deltas,
        );
        self.feedback_packet_count = self.feedback_packet_count.wrapping_add(1);
        self.last_reported_sequence = Some(actual_end);
        self.next_unreported_sequence = actual_end.checked_add(1);
        self.arrivals.retain(|sequence, _| *sequence > actual_end);
        Some(packet)
    }
}

pub fn extend_transport_sequence(sequence: u16, highest: Option<u32>) -> u32 {
    let Some(highest) = highest else {
        return u32::from(sequence);
    };
    let delta = i32::from(sequence.wrapping_sub(highest as u16) as i16);
    if delta >= 0 {
        highest.wrapping_add(delta as u32)
    } else {
        highest.saturating_sub(delta.unsigned_abs())
    }
}

pub fn duration_micros(duration: Duration) -> i64 {
    i64::try_from(duration.as_micros()).unwrap_or(i64::MAX)
}

pub fn rounded_divide(value: i64, divisor: i64) -> i64 {
    if value >= 0 {
        (value + divisor / 2) / divisor
    } else {
        (value - divisor / 2) / divisor
    }
}

pub fn duration_to_rtcp_delay(duration: Duration) -> u32 {
    let whole = duration.as_secs().saturating_mul(1 << 16);
    let fraction = u64::from(duration.subsec_nanos()).saturating_mul(1 << 16) / 1_000_000_000;
    u32::try_from(whole.saturating_add(fraction)).unwrap_or(u32::MAX)
}

impl StreamAudioRecovery {
    pub fn push(
        &mut self,
        packet: RecoveredStreamAudioPacket,
        now: Instant,
    ) -> StreamAudioRecoveryUpdate {
        let sequence = packet.sequence;
        if let Some(next_sequence) = self.next_sequence {
            let distance = sequence.wrapping_sub(next_sequence);
            if distance >= 0x8000 {
                if self.started {
                    return StreamAudioRecoveryUpdate {
                        dropped_stale_packets: 1,
                        ..StreamAudioRecoveryUpdate::default()
                    };
                }
                self.next_sequence = Some(sequence);
            }
        } else {
            self.next_sequence = Some(sequence);
            self.first_buffered_at = Some(now);
        }
        if self.pending.contains_key(&sequence) {
            return StreamAudioRecoveryUpdate {
                dropped_stale_packets: 1,
                ..StreamAudioRecoveryUpdate::default()
            };
        }
        self.pending.insert(
            sequence,
            PendingStreamAudioPacket {
                packet,
                arrived_at: now,
            },
        );
        self.poll(now)
    }

    pub fn poll(&mut self, now: Instant) -> StreamAudioRecoveryUpdate {
        if self.pending.is_empty() {
            return StreamAudioRecoveryUpdate::default();
        }
        if !self.started {
            let buffered_long_enough = self.first_buffered_at.is_some_and(|first| {
                now.saturating_duration_since(first) >= STREAM_AUDIO_REORDER_DELAY
            });
            if !buffered_long_enough && self.pending.len() < STREAM_AUDIO_MAX_PENDING_PACKETS {
                return StreamAudioRecoveryUpdate::default();
            }
            self.started = true;
            self.first_buffered_at = None;
        }

        let mut update = StreamAudioRecoveryUpdate::default();
        loop {
            while let Some(expected) = self.next_sequence {
                let Some(pending) = self.pending.remove(&expected) else {
                    break;
                };
                self.next_sequence = Some(expected.wrapping_add(1));
                update.ready.push(pending.packet);
            }
            if self.pending.is_empty() {
                break;
            }

            let expected = self
                .next_sequence
                .expect("started audio recovery has a next sequence");
            let Some((next_sequence, distance)) = self
                .pending
                .keys()
                .filter_map(|sequence| {
                    let distance = sequence.wrapping_sub(expected);
                    (distance < 0x8000).then_some((*sequence, distance))
                })
                .min_by_key(|(_, distance)| *distance)
            else {
                break;
            };
            let gap_started_at = self
                .pending
                .values()
                .map(|pending| pending.arrived_at)
                .min()
                .expect("non-empty audio recovery has a packet arrival time");
            let gap_expired =
                now.saturating_duration_since(gap_started_at) >= STREAM_AUDIO_REORDER_DELAY;
            if !gap_expired && self.pending.len() < STREAM_AUDIO_MAX_PENDING_PACKETS {
                break;
            }
            self.next_sequence = Some(next_sequence);
            update.skipped_sequences = update.skipped_sequences.wrapping_add(distance);
        }
        update
    }

    pub fn reset(&mut self) {
        self.next_sequence = None;
        self.pending.clear();
        self.first_buffered_at = None;
        self.started = false;
    }

    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }
}

impl StreamVideoRecovery {
    pub fn push(
        &mut self,
        packet: RecoveredStreamVideoPacket,
        now: Instant,
    ) -> StreamVideoRecoveryUpdate {
        let sequence = packet.header.sequence;
        let expected = *self.next_sequence.get_or_insert(sequence);
        let distance = sequence.wrapping_sub(expected);
        if distance >= 0x8000 {
            return StreamVideoRecoveryUpdate::default();
        }

        let is_new = !self.pending.contains_key(&sequence);
        if distance != 0 {
            self.gap_started_at.get_or_insert(now);
        }
        let pending_packets = self.pending.len() + usize::from(is_new);
        let pending_bytes =
            self.pending_bytes
                .saturating_add(if is_new { packet.payload.len() } else { 0 });
        let reset = if distance != 0
            && (pending_packets > STREAM_VIDEO_MAX_PENDING_PACKETS
                || pending_bytes > STREAM_VIDEO_MAX_PENDING_BYTES)
        {
            let context = StreamVideoRecoveryReset {
                distance,
                pending_packets,
                pending_bytes,
                gap_age: self
                    .gap_started_at
                    .map(|started| now.saturating_duration_since(started)),
            };
            self.reset();
            self.next_sequence = Some(sequence);
            Some(context)
        } else {
            None
        };
        if !self.pending.contains_key(&sequence) {
            self.pending_bytes = self.pending_bytes.saturating_add(packet.payload.len());
            self.pending.insert(sequence, packet);
        }

        let mut ready = Vec::new();
        while let Some(expected) = self.next_sequence {
            let Some(packet) = self.pending.remove(&expected) else {
                break;
            };
            self.pending_bytes = self.pending_bytes.saturating_sub(packet.payload.len());
            self.next_sequence = Some(expected.wrapping_add(1));
            ready.push(packet);
        }
        if self.pending.is_empty() {
            self.gap_started_at = None;
            self.last_nack_at = None;
        } else {
            self.gap_started_at.get_or_insert(now);
        }

        StreamVideoRecoveryUpdate { ready, reset }
    }

    pub fn take_nack_if_due(&mut self, now: Instant) -> Option<Vec<u16>> {
        if self
            .last_nack_at
            .is_some_and(|last| now.saturating_duration_since(last) < STREAM_VIDEO_NACK_INTERVAL)
        {
            return None;
        }
        let missing = self.missing_sequences();
        if missing.is_empty() {
            return None;
        }
        self.last_nack_at = Some(now);
        Some(missing)
    }

    pub fn take_expired_gap(&mut self, now: Instant) -> Option<StreamVideoRecoveryReset> {
        let started = self.gap_started_at?;
        let gap_age = now.saturating_duration_since(started);
        if gap_age < STREAM_VIDEO_GAP_TIMEOUT {
            return None;
        }
        let expected = self.next_sequence?;
        let distance = self
            .pending
            .keys()
            .map(|sequence| sequence.wrapping_sub(expected))
            .filter(|distance| *distance < 0x8000)
            .max()
            .unwrap_or_default();
        let context = StreamVideoRecoveryReset {
            distance,
            pending_packets: self.pending.len(),
            pending_bytes: self.pending_bytes,
            gap_age: Some(gap_age),
        };
        self.reset();
        Some(context)
    }

    pub fn reset(&mut self) {
        self.next_sequence = None;
        self.pending.clear();
        self.pending_bytes = 0;
        self.gap_started_at = None;
        self.last_nack_at = None;
    }

    pub fn missing_sequences(&self) -> Vec<u16> {
        let Some(expected) = self.next_sequence else {
            return Vec::new();
        };
        let Some(farthest) = self
            .pending
            .keys()
            .map(|sequence| sequence.wrapping_sub(expected))
            .filter(|distance| *distance < 0x8000)
            .max()
        else {
            return Vec::new();
        };
        (0..=farthest)
            .map(|distance| expected.wrapping_add(distance))
            .filter(|sequence| !self.pending.contains_key(sequence))
            .take(STREAM_VIDEO_MAX_NACK_SEQUENCES)
            .collect()
    }
}

pub fn recover_stream_video_packet(
    header: RtpHeader,
    mut payload: Vec<u8>,
    source: StreamVideoSource,
) -> Option<RecoveredStreamVideoPacket> {
    if header.payload_type == DISCORD_STREAM_VIDEO_PAYLOAD_TYPE && header.ssrc == source.video_ssrc
    {
        return Some(RecoveredStreamVideoPacket { header, payload });
    }
    if header.payload_type != DISCORD_STREAM_VIDEO_RTX_PAYLOAD_TYPE
        || source.rtx_ssrc != Some(header.ssrc)
    {
        return None;
    }
    let original_sequence = u16::from_be_bytes([*payload.first()?, *payload.get(1)?]);
    if payload.len() <= 2 {
        return None;
    }
    payload.drain(..2);
    Some(RecoveredStreamVideoPacket {
        header: RtpHeader {
            payload_type: DISCORD_STREAM_VIDEO_PAYLOAD_TYPE,
            sequence: original_sequence,
            ssrc: source.video_ssrc,
            ..header
        },
        payload,
    })
}

pub fn parse_stream_transport_sequence(
    extension_profile: Option<u16>,
    extension_body: &[u8],
) -> Option<u16> {
    if extension_profile != Some(RTP_ONE_BYTE_EXTENSION_PROFILE) {
        return None;
    }

    let mut offset = 0usize;
    while offset < extension_body.len() {
        let descriptor = extension_body[offset];
        offset += 1;
        let extension_id = descriptor >> 4;
        if extension_id == 0 {
            continue;
        }
        if extension_id == 15 {
            break;
        }
        let extension_len = usize::from(descriptor & 0x0f) + 1;
        let end = offset.checked_add(extension_len)?;
        let value = extension_body.get(offset..end)?;
        if extension_id == DISCORD_TRANSPORT_SEQUENCE_EXTENSION_ID {
            return matches!(value.len(), 2 | 4).then(|| u16::from_be_bytes([value[0], value[1]]));
        }
        offset = end;
    }
    None
}

pub fn parse_stream_video_source(
    value: &Value,
    owner_id: Id<UserMarker>,
) -> Option<StreamVideoSource> {
    let data = value.get("d")?;
    if data.get("user_id").and_then(Value::as_str) != Some(owner_id.to_string().as_str()) {
        return None;
    }
    let audio_ssrc = data
        .get("audio_ssrc")
        .and_then(Value::as_u64)
        .and_then(|ssrc| u32::try_from(ssrc).ok())?;
    let fallback_video_ssrc = data
        .get("video_ssrc")
        .and_then(Value::as_u64)
        .and_then(|ssrc| u32::try_from(ssrc).ok())
        .filter(|ssrc| *ssrc != 0);
    let selected = data
        .get("streams")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|stream| {
            stream
                .get("active")
                .and_then(Value::as_bool)
                .unwrap_or(true)
        })
        .filter_map(|stream| {
            let ssrc = stream
                .get("ssrc")
                .and_then(Value::as_u64)
                .and_then(|ssrc| u32::try_from(ssrc).ok())?;
            let quality = stream
                .get("quality")
                .and_then(Value::as_u64)
                .unwrap_or_default();
            let rtx_ssrc = stream
                .get("rtx_ssrc")
                .and_then(Value::as_u64)
                .and_then(|ssrc| u32::try_from(ssrc).ok());
            let pixel_count = stream.get("max_resolution").and_then(|resolution| {
                let width = resolution.get("width")?.as_u64()?;
                let height = resolution.get("height")?.as_u64()?;
                width.checked_mul(height).filter(|pixels| *pixels != 0)
            });
            Some((quality, ssrc, rtx_ssrc, pixel_count))
        })
        .max_by_key(|(quality, _, _, _)| *quality);
    let (video_ssrc, rtx_ssrc, pixel_count) = selected
        .map(|(_, ssrc, rtx, pixels)| (ssrc, rtx, pixels))
        .or_else(|| fallback_video_ssrc.map(|ssrc| (ssrc, Some(ssrc.wrapping_add(1)), None)))?;
    Some(StreamVideoSource {
        audio_ssrc,
        video_ssrc,
        rtx_ssrc,
        pixel_count,
    })
}
