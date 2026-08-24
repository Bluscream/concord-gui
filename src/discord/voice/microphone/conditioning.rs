use super::*;

#[cfg(feature = "voice-playback")]
pub fn select_fresh_voice_microphone_frame(
    mut frame: VoiceMicrophoneFrame,
    pcm_rx: &mut mpsc::Receiver<VoiceMicrophoneFrame>,
    now: Instant,
) -> (Option<VoiceMicrophoneFrame>, u64) {
    let mut dropped = 0u64;
    while now.saturating_duration_since(frame.captured_at) > VOICE_MIC_MAX_FRAME_AGE
        || pcm_rx.len().saturating_add(1) > VOICE_MIC_MAX_LIVE_FRAMES
    {
        dropped = dropped.saturating_add(1);
        let Ok(next) = pcm_rx.try_recv() else {
            return (None, dropped);
        };
        frame = next;
    }
    (Some(frame), dropped)
}

#[cfg(feature = "voice-playback")]
pub fn voice_microphone_frame_is_active(
    gate: VoiceCaptureGate,
    microphone_gate: &mut VoiceMicrophoneGateState,
    frame: &[i16],
) -> bool {
    gate.transmit_enabled
        && (!gate.use_voice_activity
            || microphone_gate.allows_frame(frame, gate.microphone_sensitivity))
}

#[cfg(feature = "voice-playback")]
pub fn condition_voice_microphone_frame(
    frame: &mut [i16],
    gate: VoiceCaptureGate,
    microphone_gate: &mut VoiceMicrophoneGateState,
    transmit_stats: &mut VoiceUdpTransmitStats,
) {
    let raw_overload_decision = voice_microphone_overload_decision(frame);
    let overload_decision =
        if voice_microphone_clipped_frame_needs_blank(frame, raw_overload_decision) {
            Some(VoiceMicrophoneOverloadDecision {
                kind: VoiceMicrophoneOverloadKind::HandlingNoise,
                gain: VOICE_MIC_HANDLING_NOISE_GAIN,
            })
        } else {
            microphone_gate.overload_decision(frame)
        };
    let overload_gain = if let Some(decision) = overload_decision {
        transmit_stats.overload_smoothed_frames += 1;
        decision.gain
    } else {
        1.0
    };
    let combined_gain =
        overload_gain * gate.microphone_volume.gain() * VOICE_MIC_TRANSMIT_BOOST_GAIN;
    transmit_stats.limited_samples += apply_voice_microphone_gain_and_limit(frame, combined_gain);
}

#[cfg(any(test, feature = "voice-playback"))]
pub fn apply_voice_microphone_gain_and_limit(frame: &mut [i16], gain: f32) -> u64 {
    let mut limited = 0u64;
    for sample in frame {
        let amplified = f32::from(*sample) * gain;
        if amplified.abs() > f32::from(i16::MAX) * VOICE_SOFT_LIMIT_THRESHOLD {
            limited += 1;
        }
        let normalized = amplified / f32::from(i16::MAX);
        *sample = (soft_limit_voice_sample(normalized) * f32::from(i16::MAX))
            .round()
            .clamp(f32::from(i16::MIN), f32::from(i16::MAX)) as i16;
    }
    limited
}

#[cfg(any(test, feature = "voice-playback"))]
pub fn voice_pcm_frame_reaches_sensitivity(
    frame: &[i16],
    sensitivity: MicrophoneSensitivityDb,
) -> bool {
    let threshold = sensitivity.peak_threshold();
    threshold == 0 || voice_pcm_peak(frame) >= threshold
}

#[cfg(any(test, feature = "voice-playback"))]
#[allow(dead_code)]
pub fn voice_microphone_frame_is_overloaded(frame: &[i16]) -> bool {
    voice_microphone_clipped_sample_count(frame) >= VOICE_MIC_OVERLOAD_MIN_CLIPPED_SAMPLES
}

#[cfg(any(test, feature = "voice-playback"))]
#[allow(dead_code)]
pub fn voice_microphone_overload_gain(frame: &[i16]) -> Option<f32> {
    voice_microphone_overload_decision(frame).map(|decision| decision.gain)
}

#[cfg(any(test, feature = "voice-playback"))]
pub fn voice_microphone_clipped_frame_needs_blank(
    frame: &[i16],
    raw_decision: Option<VoiceMicrophoneOverloadDecision>,
) -> bool {
    voice_microphone_clipped_sample_count(frame) > 0
        && !matches!(
            raw_decision.map(|decision| decision.kind),
            Some(VoiceMicrophoneOverloadKind::HandlingNoise)
        )
}

#[cfg(any(test, feature = "voice-playback"))]
pub fn voice_microphone_overload_decision(
    frame: &[i16],
) -> Option<VoiceMicrophoneOverloadDecision> {
    let max_adjacent_delta = voice_microphone_max_adjacent_delta(frame);
    let clipped_samples = voice_microphone_clipped_sample_count(frame);
    if max_adjacent_delta >= VOICE_MIC_HANDLING_NOISE_DELTA {
        return Some(VoiceMicrophoneOverloadDecision {
            kind: VoiceMicrophoneOverloadKind::HandlingNoise,
            gain: VOICE_MIC_HANDLING_NOISE_GAIN,
        });
    }

    if clipped_samples >= VOICE_MIC_OVERLOAD_EXTREME_CLIPPED_SAMPLES {
        return Some(VoiceMicrophoneOverloadDecision {
            kind: VoiceMicrophoneOverloadKind::HandlingNoise,
            gain: VOICE_MIC_HANDLING_NOISE_GAIN,
        });
    }

    if clipped_samples > 0
        && clipped_samples < VOICE_MIC_OVERLOAD_MIN_CLIPPED_SAMPLES
        && max_adjacent_delta >= VOICE_MIC_OVERLOAD_CLIPPED_STEP_DELTA
    {
        return Some(VoiceMicrophoneOverloadDecision {
            kind: VoiceMicrophoneOverloadKind::HandlingNoise,
            gain: VOICE_MIC_HANDLING_NOISE_GAIN,
        });
    }

    if clipped_samples > 0 && max_adjacent_delta >= VOICE_MIC_OVERLOAD_IMPULSE_DELTA {
        return Some(VoiceMicrophoneOverloadDecision {
            kind: VoiceMicrophoneOverloadKind::HandlingNoise,
            gain: VOICE_MIC_HANDLING_NOISE_GAIN,
        });
    }

    if clipped_samples < VOICE_MIC_OVERLOAD_MIN_CLIPPED_SAMPLES {
        return None;
    }

    if clipped_samples >= VOICE_MIC_OVERLOAD_SEVERE_CLIPPED_SAMPLES {
        return Some(VoiceMicrophoneOverloadDecision {
            kind: VoiceMicrophoneOverloadKind::Transient,
            gain: VOICE_MIC_OVERLOAD_TRANSIENT_GAIN,
        });
    }

    Some(VoiceMicrophoneOverloadDecision {
        kind: VoiceMicrophoneOverloadKind::Attenuated,
        gain: VOICE_MIC_OVERLOAD_ATTENUATION_GAIN,
    })
}

#[cfg(feature = "voice-playback")]
pub fn voice_microphone_overload_recovery_gain(frames_remaining: u8) -> f32 {
    let recovery_frames = f32::from(VOICE_MIC_OVERLOAD_RECOVERY_FRAMES.max(1));
    let elapsed_frames = f32::from(VOICE_MIC_OVERLOAD_RECOVERY_FRAMES - frames_remaining);
    VOICE_MIC_OVERLOAD_RECOVERY_START_GAIN
        + (1.0 - VOICE_MIC_OVERLOAD_RECOVERY_START_GAIN) * (elapsed_frames / recovery_frames)
}

#[cfg(any(test, feature = "voice-playback"))]
pub fn voice_microphone_clipped_sample_count(frame: &[i16]) -> usize {
    frame
        .iter()
        .filter(|sample| i32::from(**sample).abs() >= i32::from(i16::MAX) - 1)
        .count()
}

#[cfg(any(test, feature = "voice-playback"))]
pub fn voice_microphone_max_adjacent_delta(frame: &[i16]) -> i32 {
    frame
        .windows(2)
        .map(|samples| (i32::from(samples[1]) - i32::from(samples[0])).abs())
        .max()
        .unwrap_or(0)
}

#[cfg(any(test, feature = "voice-playback"))]
pub fn voice_pcm_peak(frame: &[i16]) -> i32 {
    frame
        .iter()
        .map(|sample| i32::from(*sample).abs())
        .max()
        .unwrap_or(0)
}

#[cfg(feature = "voice-playback")]
impl VoiceMicrophoneGateState {
    pub fn overload_decision(&mut self, frame: &[i16]) -> Option<VoiceMicrophoneOverloadDecision> {
        if let Some(decision) = voice_microphone_overload_decision(frame) {
            if decision.kind == VoiceMicrophoneOverloadKind::HandlingNoise {
                self.handling_noise_suppression_frames =
                    VOICE_MIC_HANDLING_NOISE_SUPPRESSION_FRAMES;
                self.overload_recovery_frames = 0;
                return Some(decision);
            }
            if self.handling_noise_suppression_frames > 0 {
                self.handling_noise_suppression_frames -= 1;
                return Some(VoiceMicrophoneOverloadDecision {
                    kind: VoiceMicrophoneOverloadKind::Recovery,
                    gain: VOICE_MIC_HANDLING_NOISE_GAIN,
                });
            }
            self.overload_recovery_frames = if decision.gain <= VOICE_MIC_OVERLOAD_TRANSIENT_GAIN {
                VOICE_MIC_OVERLOAD_RECOVERY_FRAMES
            } else {
                0
            };
            return Some(decision);
        }
        if self.handling_noise_suppression_frames > 0 {
            self.handling_noise_suppression_frames -= 1;
            return Some(VoiceMicrophoneOverloadDecision {
                kind: VoiceMicrophoneOverloadKind::Recovery,
                gain: VOICE_MIC_HANDLING_NOISE_GAIN,
            });
        }
        if self.overload_recovery_frames > 0 {
            let recovery_gain =
                voice_microphone_overload_recovery_gain(self.overload_recovery_frames);
            self.overload_recovery_frames -= 1;
            return Some(VoiceMicrophoneOverloadDecision {
                kind: VoiceMicrophoneOverloadKind::Recovery,
                gain: recovery_gain,
            });
        }
        None
    }

    pub fn allows_frame(&mut self, frame: &[i16], sensitivity: MicrophoneSensitivityDb) -> bool {
        if voice_pcm_frame_reaches_sensitivity(frame, sensitivity) {
            self.hangover_frames = VOICE_MIC_GATE_HANGOVER_FRAMES;
            return true;
        }
        if self.hangover_frames > 0 {
            self.hangover_frames -= 1;
            return true;
        }
        false
    }

    pub fn reset(&mut self) {
        self.hangover_frames = 0;
        self.overload_recovery_frames = 0;
        self.handling_noise_suppression_frames = 0;
    }
}
