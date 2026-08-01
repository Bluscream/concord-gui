use std::sync::mpsc::Receiver;

use super::CaptureFrame;
use crate::discord::voice::StreamCaptureTarget;

pub(super) struct CaptureSession;

pub(super) fn list_targets() -> Result<Vec<StreamCaptureTarget>, String> {
    Err("screen capture is unsupported on this operating system".to_owned())
}

pub(super) fn start_capture(
    _target: &StreamCaptureTarget,
) -> Result<(CaptureSession, Receiver<Result<CaptureFrame, String>>), String> {
    Err("screen capture is unsupported on this operating system".to_owned())
}

impl CaptureSession {
    pub(super) fn stop(&mut self) -> Result<(), String> {
        Ok(())
    }
}
