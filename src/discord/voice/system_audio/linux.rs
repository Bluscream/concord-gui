use flexaudio_core::backend::CaptureBackend;
use flexaudio_core::types::ProcessMode;
use flexaudio_os_linux::PwProcessBackend;

use super::StreamCaptureTarget;

pub(super) const BACKEND_NAME: &str = "pipewire-process";

pub(super) fn capture_backend(
    _target: &StreamCaptureTarget,
    target_pid: u32,
    mode: ProcessMode,
) -> Box<dyn CaptureBackend> {
    Box::new(PwProcessBackend::new(target_pid, mode))
}
