use flexaudio_core::backend::CaptureBackend;
use flexaudio_core::types::ProcessMode;
use flexaudio_os_windows::WasapiProcessBackend;

use super::StreamCaptureTarget;

pub(super) const BACKEND_NAME: &str = "wasapi-process-loopback";

pub(super) fn capture_backend(
    _target: &StreamCaptureTarget,
    target_pid: u32,
    mode: ProcessMode,
) -> Box<dyn CaptureBackend> {
    Box::new(WasapiProcessBackend::new(target_pid, mode))
}
