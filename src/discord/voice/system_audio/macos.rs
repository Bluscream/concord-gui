use std::ffi::c_void;
use std::ptr::{self, NonNull};
use std::slice;
use std::sync::{Mutex, PoisonError, mpsc};

use block2::{DynBlock, RcBlock};
use dispatch2::{DispatchQueue, DispatchQueueAttr, DispatchRetained};
use flexaudio_core::backend::{CaptureBackend, RawSink};
use flexaudio_core::types::{Error, ProcessMode, Result};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{AllocAnyThread, DefinedClass, define_class, msg_send};
use objc2_core_audio_types::{AudioBuffer, AudioBufferList};
use objc2_core_foundation::CFRetained;
use objc2_core_media::{CMBlockBuffer, CMSampleBuffer};
use objc2_foundation::{NSArray, NSError, NSObject, NSObjectProtocol};
use objc2_screen_capture_kit::{
    SCContentFilter, SCShareableContent, SCStream, SCStreamConfiguration, SCStreamErrorCode,
    SCStreamOutput, SCStreamOutputType, SCWindow,
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
        active_stream: None,
    })
}

pub(super) fn target_process_id(
    _target: &StreamCaptureTarget,
) -> std::result::Result<Option<u32>, String> {
    Ok(None)
}

struct ScreenCaptureKitAudioBackend {
    target: StreamCaptureTarget,
    active_stream: Option<ActiveStream>,
}

struct ActiveStream {
    stream: Retained<SCStream>,
    _output: Retained<MacAudioOutput>,
    _queue: DispatchRetained<DispatchQueue>,
}

// SAFETY: ScreenCaptureKit streams accept control calls from any thread, and
// callbacks run on the retained dispatch queue. The output's mutable state is
// protected by a mutex. The backend moves this group as one unit and stops the
// stream before releasing the output or queue.
unsafe impl Send for ActiveStream {}

impl CaptureBackend for ScreenCaptureKitAudioBackend {
    fn native_format(&self) -> (u32, u16) {
        (SAMPLE_RATE, CHANNELS)
    }

    fn start(&mut self, sink: RawSink) -> Result<()> {
        if self.active_stream.is_some() {
            return Ok(());
        }

        let content = shareable_content()?;
        let filter = content_filter(&content, &self.target)?;
        let configuration = unsafe { SCStreamConfiguration::new() };
        unsafe {
            configuration.setCapturesAudio(true);
            configuration.setSampleRate(SAMPLE_RATE as isize);
            configuration.setChannelCount(CHANNELS as isize);
            // ScreenCaptureKit filters window audio at the owning app level.
            // Excluding Concord prevents received voice audio from being broadcast again.
            configuration.setExcludesCurrentProcessAudio(true);
        }

        let stream = unsafe {
            SCStream::initWithFilter_configuration_delegate(
                SCStream::alloc(),
                &filter,
                &configuration,
                None,
            )
        };
        let output = MacAudioOutput::new(sink);
        let output_protocol = ProtocolObject::<dyn SCStreamOutput>::from_ref(&*output);
        let queue = DispatchQueue::new("concord.stream-system-audio", DispatchQueueAttr::SERIAL);
        unsafe {
            stream.addStreamOutput_type_sampleHandlerQueue_error(
                output_protocol,
                SCStreamOutputType::Audio,
                Some(&queue),
            )
        }
        .map_err(|error| map_screen_capture_error(&error))?;

        let active_stream = ActiveStream {
            stream,
            _output: output,
            _queue: queue,
        };
        wait_for_completion(|completion| unsafe {
            active_stream
                .stream
                .startCaptureWithCompletionHandler(Some(completion));
        })?;
        self.active_stream = Some(active_stream);
        Ok(())
    }

    fn stop(&mut self) {
        let Some(active_stream) = self.active_stream.take() else {
            return;
        };
        if let Err(error) = wait_for_completion(|completion| unsafe {
            active_stream
                .stream
                .stopCaptureWithCompletionHandler(Some(completion));
        }) {
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

fn shareable_content() -> Result<Retained<SCShareableContent>> {
    let (result_tx, result_rx) = mpsc::sync_channel(1);
    let completion: RcBlock<dyn Fn(*mut SCShareableContent, *mut NSError)> = RcBlock::new(
        move |content: *mut SCShareableContent, error: *mut NSError| {
            let result = if let Some(error) = unsafe { error.as_ref() } {
                Err(map_screen_capture_error(error))
            } else {
                unsafe { Retained::retain(content) }
                    .map(|content| Retained::into_raw(content) as usize)
                    .ok_or_else(|| {
                        Error::Backend("ScreenCaptureKit returned no shareable content".to_owned())
                    })
            };
            let _ = result_tx.send(result);
        },
    );
    unsafe {
        SCShareableContent::getShareableContentWithCompletionHandler(&completion);
    }

    let content_address = result_rx.recv().map_err(|_| {
        Error::Backend("ScreenCaptureKit shareable content request was cancelled".to_owned())
    })??;
    let content = unsafe { Retained::from_raw(content_address as *mut SCShareableContent) }
        .ok_or_else(|| Error::Backend("ScreenCaptureKit returned invalid content".to_owned()))?;
    Ok(content)
}

fn content_filter(
    content: &SCShareableContent,
    target: &StreamCaptureTarget,
) -> Result<Retained<SCContentFilter>> {
    let target_id = u32::try_from(target.id).map_err(|_| Error::DeviceNotFound)?;
    match target.kind {
        StreamCaptureTargetKind::Display => {
            let displays = unsafe { content.displays() };
            let display = (0..displays.count())
                .map(|index| displays.objectAtIndex(index))
                .find(|display| unsafe { display.displayID() } == target_id)
                .ok_or(Error::DeviceNotFound)?;
            let excluded_windows = NSArray::<SCWindow>::new();
            Ok(unsafe {
                SCContentFilter::initWithDisplay_excludingWindows(
                    SCContentFilter::alloc(),
                    &display,
                    &excluded_windows,
                )
            })
        }
        StreamCaptureTargetKind::Window => {
            let windows = unsafe { content.windows() };
            let window = (0..windows.count())
                .map(|index| windows.objectAtIndex(index))
                .find(|window| unsafe { window.windowID() } == target_id)
                .ok_or(Error::DeviceNotFound)?;
            Ok(unsafe {
                SCContentFilter::initWithDesktopIndependentWindow(SCContentFilter::alloc(), &window)
            })
        }
        StreamCaptureTargetKind::Portal => Err(Error::DeviceNotFound),
    }
}

fn wait_for_completion(operation: impl FnOnce(&DynBlock<dyn Fn(*mut NSError)>)) -> Result<()> {
    let (result_tx, result_rx) = mpsc::sync_channel(1);
    let completion: RcBlock<dyn Fn(*mut NSError)> = RcBlock::new(move |error: *mut NSError| {
        let result =
            unsafe { error.as_ref() }.map_or(Ok(()), |error| Err(map_screen_capture_error(error)));
        let _ = result_tx.send(result);
    });
    operation(&completion);
    result_rx.recv().map_err(|_| {
        Error::Backend("ScreenCaptureKit operation completion was cancelled".to_owned())
    })?
}

struct MacAudioOutputIvars {
    state: Mutex<MacAudioOutputState>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "ConcordScreenCaptureAudioOutput"]
    #[ivars = MacAudioOutputIvars]
    struct MacAudioOutput;

    unsafe impl SCStreamOutput for MacAudioOutput {
        #[unsafe(method(stream:didOutputSampleBuffer:ofType:))]
        #[allow(non_snake_case)]
        unsafe fn stream_didOutputSampleBuffer_ofType(
            &self,
            _stream: &SCStream,
            sample_buffer: &CMSampleBuffer,
            output_type: SCStreamOutputType,
        ) {
            if output_type != SCStreamOutputType::Audio {
                return;
            }
            self.ivars()
                .state
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(sample_buffer);
        }
    }
);

unsafe impl NSObjectProtocol for MacAudioOutput {}

impl MacAudioOutput {
    fn new(sink: RawSink) -> Retained<Self> {
        let this = Self::alloc().set_ivars(MacAudioOutputIvars {
            state: Mutex::new(MacAudioOutputState {
                sink,
                scratch: Vec::with_capacity(AUDIO_SCRATCH_SAMPLES),
            }),
        });
        unsafe { msg_send![super(this), init] }
    }
}

struct MacAudioOutputState {
    sink: RawSink,
    scratch: Vec<f32>,
}

impl MacAudioOutputState {
    fn push(&mut self, sample_buffer: &CMSampleBuffer) {
        let Some(buffers) = CapturedAudioBuffers::new(sample_buffer) else {
            return;
        };
        let buffer_count = usize::try_from(buffers.list.number_buffers)
            .unwrap_or(0)
            .min(buffers.list.buffers.len());
        let Some(first) = buffers.list.buffers.first().filter(|_| buffer_count > 0) else {
            return;
        };
        let Some(first_data) = audio_buffer_bytes(first) else {
            return;
        };

        self.scratch.clear();
        if first.mNumberChannels >= u32::from(CHANNELS) {
            append_f32_samples(&mut self.scratch, first_data);
        } else if buffer_count >= usize::from(CHANNELS) {
            let Some(second_data) = audio_buffer_bytes(&buffers.list.buffers[1]) else {
                return;
            };
            append_planar_stereo(&mut self.scratch, first_data, second_data);
        } else {
            append_mono_as_stereo(&mut self.scratch, first_data);
        }

        if !self.scratch.is_empty() {
            self.sink.push(&self.scratch, 0);
        }
    }
}

#[repr(C)]
struct StereoAudioBufferList {
    number_buffers: u32,
    buffers: [AudioBuffer; 2],
}

struct CapturedAudioBuffers {
    list: StereoAudioBufferList,
    _block_buffer: CFRetained<CMBlockBuffer>,
}

impl CapturedAudioBuffers {
    fn new(sample_buffer: &CMSampleBuffer) -> Option<Self> {
        let empty_buffer = AudioBuffer {
            mNumberChannels: 0,
            mDataByteSize: 0,
            mData: ptr::null_mut::<c_void>(),
        };
        let mut list = StereoAudioBufferList {
            number_buffers: 0,
            buffers: [empty_buffer; 2],
        };
        let mut block_buffer = ptr::null_mut();
        let status = unsafe {
            sample_buffer.audio_buffer_list_with_retained_block_buffer(
                ptr::null_mut(),
                (&mut list as *mut StereoAudioBufferList).cast::<AudioBufferList>(),
                size_of::<StereoAudioBufferList>(),
                None,
                None,
                0,
                &mut block_buffer,
            )
        };
        let block_buffer = NonNull::new(block_buffer)
            .map(|block_buffer| unsafe { CFRetained::from_raw(block_buffer) });
        if status != 0 {
            return None;
        }

        Some(Self {
            list,
            _block_buffer: block_buffer?,
        })
    }
}

fn audio_buffer_bytes(buffer: &AudioBuffer) -> Option<&[u8]> {
    let length = buffer.mDataByteSize as usize;
    if length == 0 {
        return Some(&[]);
    }
    let data = NonNull::new(buffer.mData.cast::<u8>())?;
    Some(unsafe { slice::from_raw_parts(data.as_ptr(), length) })
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

fn map_screen_capture_error(error: &NSError) -> Error {
    match SCStreamErrorCode(error.code()) {
        SCStreamErrorCode::UserDeclined | SCStreamErrorCode::MissingEntitlements => {
            Error::PermissionDenied
        }
        SCStreamErrorCode::NoWindowList
        | SCStreamErrorCode::NoDisplayList
        | SCStreamErrorCode::NoCaptureSource => Error::DeviceNotFound,
        _ => Error::Backend(error.to_string()),
    }
}
