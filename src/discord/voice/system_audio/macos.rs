use std::sync::{Mutex, PoisonError};

use flexaudio_core::backend::{CaptureBackend, RawSink};
use flexaudio_core::types::{Error, ProcessMode, Result};
use screencapturekit::AudioBufferList;
use screencapturekit::prelude::{
    CMSampleBuffer, CMSampleBufferExt, SCContentFilter, SCError, SCShareableContent, SCStream,
    SCStreamConfiguration, SCStreamOutputTrait, SCStreamOutputType,
};

use crate::logging;

use super::{StreamCaptureTarget, StreamCaptureTargetKind};

pub(super) const BACKEND_NAME: &str = "screencapturekit-audio";

const SAMPLE_RATE: u32 = 48_000;
const CHANNELS: u16 = 2;
const AUDIO_SCRATCH_SAMPLES: usize = 8_192;

pub(super) fn capture_backend(
    target: &StreamCaptureTarget,
    _target_pid: u32,
    _mode: ProcessMode,
) -> Box<dyn CaptureBackend> {
    Box::new(ScreenCaptureKitAudioBackend {
        target: target.clone(),
        stream: None,
    })
}

struct ScreenCaptureKitAudioBackend {
    target: StreamCaptureTarget,
    stream: Option<SCStream>,
}

impl CaptureBackend for ScreenCaptureKitAudioBackend {
    fn native_format(&self) -> (u32, u16) {
        (SAMPLE_RATE, CHANNELS)
    }

    fn start(&mut self, sink: RawSink) -> Result<()> {
        if self.stream.is_some() {
            return Ok(());
        }

        let content = SCShareableContent::get().map_err(map_screen_capture_error)?;
        let filter = content_filter(&content, &self.target)?;
        // ScreenCaptureKit filters window audio at the owning app level.
        // Excluding Concord prevents received voice audio from being broadcast again.
        let configuration = SCStreamConfiguration::new()
            .with_captures_audio(true)
            .with_sample_rate(SAMPLE_RATE as i32)
            .with_channel_count(i32::from(CHANNELS))
            .with_excludes_current_process_audio(true);
        let mut stream = SCStream::new(&filter, &configuration);

        if stream
            .add_output_handler(MacAudioOutput::new(sink), SCStreamOutputType::Audio)
            .is_none()
        {
            return Err(Error::Backend(
                "ScreenCaptureKit rejected the audio output handler".to_owned(),
            ));
        }

        stream.start_capture().map_err(map_screen_capture_error)?;
        self.stream = Some(stream);
        Ok(())
    }

    fn stop(&mut self) {
        let Some(stream) = self.stream.take() else {
            return;
        };
        if let Err(error) = stream.stop_capture() {
            logging::debug(
                "stream",
                format!("ScreenCaptureKit audio stop failed: {error}"),
            );
        }
    }
}

impl Drop for ScreenCaptureKitAudioBackend {
    fn drop(&mut self) {
        self.stop();
    }
}

fn content_filter(
    content: &SCShareableContent,
    target: &StreamCaptureTarget,
) -> Result<SCContentFilter> {
    match target.kind {
        StreamCaptureTargetKind::Display => {
            let display = content
                .displays()
                .into_iter()
                .find(|display| display.display_id() == target.id)
                .ok_or(Error::DeviceNotFound)?;
            Ok(SCContentFilter::create()
                .with_display(&display)
                .with_excluding_windows(&[])
                .build())
        }
        StreamCaptureTargetKind::Window => {
            let window = content
                .windows()
                .into_iter()
                .find(|window| window.window_id() == target.id)
                .ok_or(Error::DeviceNotFound)?;
            Ok(SCContentFilter::create().with_window(&window).build())
        }
    }
}

struct MacAudioOutput {
    state: Mutex<MacAudioOutputState>,
}

impl MacAudioOutput {
    fn new(sink: RawSink) -> Self {
        Self {
            state: Mutex::new(MacAudioOutputState {
                sink,
                scratch: Vec::with_capacity(AUDIO_SCRATCH_SAMPLES),
            }),
        }
    }
}

impl SCStreamOutputTrait for MacAudioOutput {
    fn did_output_sample_buffer(&self, sample_buffer: CMSampleBuffer, of_type: SCStreamOutputType) {
        if of_type != SCStreamOutputType::Audio {
            return;
        }
        let Some(buffers) = sample_buffer.audio_buffer_list() else {
            return;
        };
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(&buffers);
    }
}

struct MacAudioOutputState {
    sink: RawSink,
    scratch: Vec<f32>,
}

impl MacAudioOutputState {
    fn push(&mut self, buffers: &AudioBufferList) {
        let Some(first) = buffers.get(0) else {
            return;
        };

        self.scratch.clear();
        if first.number_channels >= u32::from(CHANNELS) {
            append_f32_samples(&mut self.scratch, first.data());
        } else if buffers.num_buffers() >= usize::from(CHANNELS) {
            let Some(second) = buffers.get(1) else {
                return;
            };
            append_planar_stereo(&mut self.scratch, first.data(), second.data());
        } else {
            append_mono_as_stereo(&mut self.scratch, first.data());
        }

        if !self.scratch.is_empty() {
            self.sink.push(&self.scratch, 0);
        }
    }
}

fn append_f32_samples(output: &mut Vec<f32>, bytes: &[u8]) {
    output.extend(
        bytes
            .chunks_exact(size_of::<f32>())
            .map(|sample| f32::from_ne_bytes([sample[0], sample[1], sample[2], sample[3]])),
    );
}

fn append_planar_stereo(output: &mut Vec<f32>, left: &[u8], right: &[u8]) {
    for (left, right) in left
        .chunks_exact(size_of::<f32>())
        .zip(right.chunks_exact(size_of::<f32>()))
    {
        output.push(f32::from_ne_bytes([left[0], left[1], left[2], left[3]]));
        output.push(f32::from_ne_bytes([right[0], right[1], right[2], right[3]]));
    }
}

fn append_mono_as_stereo(output: &mut Vec<f32>, bytes: &[u8]) {
    for sample in bytes.chunks_exact(size_of::<f32>()) {
        let sample = f32::from_ne_bytes([sample[0], sample[1], sample[2], sample[3]]);
        output.extend_from_slice(&[sample, sample]);
    }
}

fn map_screen_capture_error(error: SCError) -> Error {
    match error {
        SCError::PermissionDenied(_) | SCError::NoShareableContent(_) => Error::PermissionDenied,
        SCError::DisplayNotFound(_) | SCError::WindowNotFound(_) => Error::DeviceNotFound,
        SCError::FeatureNotAvailable { .. } => Error::UnsupportedOsVersion,
        error => Error::Backend(error.to_string()),
    }
}
