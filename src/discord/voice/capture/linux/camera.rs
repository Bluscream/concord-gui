//! Camera capture on Linux, through V4L2.
//!
//! Deliberately feeds the same channel the screen capture does. A camera and a
//! shared window differ only in where the frames come from - both end up
//! scaled, encoded and sent down the same RTP path - so making this a second
//! pipeline would mean maintaining two of everything.
//!
//! Format conversion is done here rather than by libv4l: the two formats worth
//! supporting are the two every webcam offers, and pulling in a C library to
//! convert them would be a dependency for a hundred lines of arithmetic.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::thread::{self, JoinHandle};

use v4l::buffer::Type as BufferType;
use v4l::io::traits::CaptureStream;
use v4l::prelude::*;
use v4l::video::Capture;

use crate::discord::voice::{StreamCaptureTarget, StreamCaptureTargetKind};
use crate::logging;

use super::super::{
    CaptureFrame, CaptureFrameBufferPool, CaptureOutput, STREAM_CAPTURE_FPS, STREAM_CAPTURE_HEIGHT,
    STREAM_CAPTURE_WIDTH,
};

/// How many frames may queue before the oldest is dropped.
///
/// One: a camera frame is worthless the moment a newer one exists, and a
/// backlog only adds latency to a live picture.
const FRAME_QUEUE_CAPACITY: usize = 1;

/// Where V4L2 devices live. Cameras are `/dev/videoN`.
const DEVICE_DIRECTORY: &str = "/dev";

pub(in crate::discord::voice::capture) struct CameraSession {
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl CameraSession {
    pub(in crate::discord::voice::capture) fn stop(&mut self) -> Result<(), String> {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        Ok(())
    }
}

/// Every camera this account can open.
///
/// A device node that cannot be opened or does not report video capture is
/// skipped rather than listed: `/dev/video1` is very often the metadata half
/// of a camera whose picture is on `/dev/video0`, and offering it would give
/// the user a choice that silently produces nothing.
pub(super) fn list_cameras() -> Vec<StreamCaptureTarget> {
    let Ok(entries) = std::fs::read_dir(DEVICE_DIRECTORY) else {
        return Vec::new();
    };

    let mut cameras = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let Some(index) = name
            .strip_prefix("video")
            .and_then(|n| n.parse::<u64>().ok())
        else {
            continue;
        };

        let Ok(device) = Device::new(index as usize) else {
            continue;
        };
        let Ok(capabilities) = device.query_caps() else {
            continue;
        };
        if !capabilities
            .capabilities
            .contains(v4l::capability::Flags::VIDEO_CAPTURE)
        {
            continue;
        }
        // A device that reports capture but offers no format is the metadata
        // node, which opens and then never produces a picture.
        if device.enum_formats().map(|f| f.is_empty()).unwrap_or(true) {
            continue;
        }

        cameras.push(StreamCaptureTarget {
            kind: StreamCaptureTargetKind::Camera,
            id: index,
            title: capabilities.card.clone(),
        });
    }
    cameras
}

pub(super) fn start_camera_capture(
    target: &StreamCaptureTarget,
    buffer_pool: CaptureFrameBufferPool,
) -> Result<(CameraSession, CaptureOutput), String> {
    let device = Device::new(target.id as usize)
        .map_err(|error| format!("opening camera {} failed: {error}", target.title))?;

    let mut format = device
        .format()
        .map_err(|error| format!("reading camera format failed: {error}"))?;
    format.width = STREAM_CAPTURE_WIDTH;
    format.height = STREAM_CAPTURE_HEIGHT;
    // Asked for, not required: V4L2 answers with what it will actually give,
    // which is why the negotiated format is read back rather than assumed.
    format.fourcc = v4l::FourCC::new(b"YUYV");
    let format = device
        .set_format(&format)
        .map_err(|error| format!("setting camera format failed: {error}"))?;

    let fourcc = format.fourcc;
    if !matches!(&fourcc.repr, b"YUYV" | b"MJPG") {
        return Err(format!(
            "camera offers {fourcc}, which this client cannot decode - it needs YUYV or MJPEG"
        ));
    }

    let mut parameters = device
        .params()
        .map_err(|error| format!("reading camera parameters failed: {error}"))?;
    parameters.interval = v4l::Fraction::new(1, STREAM_CAPTURE_FPS);
    // Best effort: a camera that will not run at 30fps still works, just at
    // whatever rate it does offer.
    let _ = device.set_params(&parameters);

    let (frames_tx, frames_rx) = mpsc::sync_channel(FRAME_QUEUE_CAPACITY);
    let (errors_tx, errors_rx) = mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));

    let worker_stop = Arc::clone(&stop);
    let title = target.title.clone();
    let worker = thread::Builder::new()
        .name("stream-camera-video".to_owned())
        .spawn(move || {
            if let Err(error) =
                run_camera_capture(device, format, frames_tx, buffer_pool, &worker_stop)
                && !worker_stop.load(Ordering::Acquire)
            {
                logging::debug("stream", format!("camera {title} failed: {error}"));
                let _ = errors_tx.send(error);
            }
        })
        .map_err(|error| format!("camera worker spawn failed: {error}"))?;

    Ok((
        CameraSession {
            stop,
            worker: Some(worker),
        },
        CaptureOutput {
            frames: frames_rx,
            errors: errors_rx,
        },
    ))
}

fn run_camera_capture(
    device: Device,
    format: v4l::Format,
    frames: SyncSender<CaptureFrame>,
    buffer_pool: CaptureFrameBufferPool,
    stop: &AtomicBool,
) -> Result<(), String> {
    let mut stream = MmapStream::with_buffers(&device, BufferType::VideoCapture, 4)
        .map_err(|error| format!("starting camera stream failed: {error}"))?;

    while !stop.load(Ordering::Acquire) {
        let (buffer, _) = stream
            .next()
            .map_err(|error| format!("reading a camera frame failed: {error}"))?;

        let rgba = match &format.fourcc.repr {
            b"YUYV" => yuyv_to_rgba(buffer, format.width, format.height)?,
            b"MJPG" => mjpeg_to_rgba(buffer, format.width, format.height)?,
            other => {
                return Err(format!(
                    "camera switched to {} mid-stream",
                    String::from_utf8_lossy(other)
                ));
            }
        };

        // Dropped rather than queued when the encoder is behind: a stale
        // camera frame is worse than a missing one.
        if frames
            .try_send(CaptureFrame::new(
                format.width,
                format.height,
                rgba,
                buffer_pool.clone(),
            ))
            .is_err()
            && stop.load(Ordering::Acquire)
        {
            break;
        }
    }
    Ok(())
}

/// YUYV 4:2:2 to RGBA.
///
/// Two pixels share one pair of chroma samples, which is why this steps four
/// input bytes at a time and writes eight output ones.
fn yuyv_to_rgba(buffer: &[u8], width: u32, height: u32) -> Result<Vec<u8>, String> {
    let pixels = (width as usize) * (height as usize);
    let expected = pixels * 2;
    if buffer.len() < expected {
        return Err(format!(
            "camera frame is {} bytes, expected {expected}",
            buffer.len()
        ));
    }

    let mut rgba = vec![0u8; pixels * 4];
    for (index, chunk) in buffer[..expected].chunks_exact(4).enumerate() {
        let (y0, u, y1, v) = (chunk[0], chunk[1], chunk[2], chunk[3]);
        let out = index * 8;
        write_yuv_pixel(&mut rgba[out..out + 4], y0, u, v);
        write_yuv_pixel(&mut rgba[out + 4..out + 8], y1, u, v);
    }
    Ok(rgba)
}

/// BT.601, which is what V4L2 cameras produce.
fn write_yuv_pixel(out: &mut [u8], y: u8, u: u8, v: u8) {
    let y = f32::from(y);
    let u = f32::from(u) - 128.;
    let v = f32::from(v) - 128.;

    out[0] = (y + 1.402 * v).clamp(0., 255.) as u8;
    out[1] = (y - 0.344_136 * u - 0.714_136 * v).clamp(0., 255.) as u8;
    out[2] = (y + 1.772 * u).clamp(0., 255.) as u8;
    out[3] = 255;
}

fn mjpeg_to_rgba(buffer: &[u8], width: u32, height: u32) -> Result<Vec<u8>, String> {
    let decoded = image::load_from_memory_with_format(buffer, image::ImageFormat::Jpeg)
        .map_err(|error| format!("decoding a camera frame failed: {error}"))?
        .to_rgba8();

    // A camera may hand back a different size than it negotiated; scaling here
    // keeps the encoder's assumption about frame size true.
    if decoded.width() != width || decoded.height() != height {
        return Ok(image::imageops::resize(
            &decoded,
            width,
            height,
            image::imageops::FilterType::Triangle,
        )
        .into_raw());
    }
    Ok(decoded.into_raw())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yuyv_converts_two_pixels_per_chroma_pair() {
        // Mid-grey: Y=128 with neutral chroma is grey in RGB, and both pixels
        // of the pair get it.
        let frame = vec![128, 128, 128, 128];
        let rgba = yuyv_to_rgba(&frame, 2, 1).expect("a full frame converts");

        assert_eq!(rgba.len(), 8);
        for pixel in rgba.chunks_exact(4) {
            assert!((126..=130).contains(&pixel[0]), "red was {}", pixel[0]);
            assert!((126..=130).contains(&pixel[1]));
            assert!((126..=130).contains(&pixel[2]));
            assert_eq!(pixel[3], 255, "camera frames are opaque");
        }
    }

    #[test]
    fn a_short_frame_is_an_error_rather_than_a_panic() {
        // Reachable if a camera reports one size and delivers another; the
        // slice arithmetic would otherwise run off the end.
        let frame = vec![128, 128];
        assert!(yuyv_to_rgba(&frame, 4, 4).is_err());
        assert!(yuyv_to_rgba(&[], 1, 1).is_err());
    }

    #[test]
    fn colour_survives_the_conversion() {
        // Full red in YUV: the point is that chroma is not being ignored.
        let frame = vec![81, 90, 81, 240];
        let rgba = yuyv_to_rgba(&frame, 2, 1).expect("a full frame converts");

        assert!(rgba[0] > 200, "red channel was {}", rgba[0]);
        assert!(rgba[1] < 60, "green channel was {}", rgba[1]);
        assert!(rgba[2] < 60, "blue channel was {}", rgba[2]);
    }
}
