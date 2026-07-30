use tokio::sync::mpsc;

use super::{STREAM_BROADCAST_FEATURE_DISABLED, StreamCaptureTarget};

pub(super) const STREAM_CAPTURE_WIDTH: u32 = 1280;
pub(super) const STREAM_CAPTURE_HEIGHT: u32 = 720;
pub(super) const STREAM_CAPTURE_FPS: u32 = 30;
pub(super) const STREAM_CAPTURE_BITRATE: u32 = 6_000_000;

// Keep the broadcast runtime interface stable in reduced builds. Capture
// startup always fails before either of these values can be constructed.
#[derive(Debug)]
pub(super) struct EncodedStreamFrame {
    pub(super) timestamp: u32,
    pub(super) annex_b: Vec<u8>,
    pub(super) is_keyframe: bool,
}

pub(super) struct StreamCaptureHandle;

impl StreamCaptureHandle {
    pub(super) fn request_keyframe(&self) {
        // Disabled builds never construct a capture handle.
    }
}

pub(crate) fn list_stream_capture_targets() -> Result<Vec<StreamCaptureTarget>, String> {
    Err(STREAM_BROADCAST_FEATURE_DISABLED.to_owned())
}

pub(super) fn start_stream_capture(
    _target: StreamCaptureTarget,
    _frames_tx: mpsc::Sender<Result<EncodedStreamFrame, String>>,
) -> Result<StreamCaptureHandle, String> {
    Err(STREAM_BROADCAST_FEATURE_DISABLED.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_targets_report_the_disabled_feature() {
        assert_eq!(
            list_stream_capture_targets(),
            Err(STREAM_BROADCAST_FEATURE_DISABLED.to_owned())
        );
    }
}
