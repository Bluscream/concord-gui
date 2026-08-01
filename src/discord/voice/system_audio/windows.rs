use flexaudio_core::backend::CaptureBackend;
use flexaudio_core::types::ProcessMode;
use flexaudio_os_windows::WasapiProcessBackend;
use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;

use super::StreamCaptureTarget;

pub(super) const BACKEND_NAME: &str = "wasapi-process-loopback";

pub(super) fn capture_backend(
    _target: &StreamCaptureTarget,
    target_pid: u32,
    mode: ProcessMode,
) -> Box<dyn CaptureBackend> {
    Box::new(WasapiProcessBackend::new(target_pid, mode))
}

pub(super) fn target_process_id(target: &StreamCaptureTarget) -> Result<Option<u32>, String> {
    let mut process_id = 0;
    unsafe {
        GetWindowThreadProcessId(target.id as usize as *mut _, &mut process_id);
    }
    if process_id == 0 {
        return Err(format!(
            "window process lookup failed for capture target: {}",
            target.title
        ));
    }
    Ok(Some(process_id))
}
