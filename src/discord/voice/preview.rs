use std::time::Duration;

#[cfg(feature = "stream-broadcast")]
use std::time::Instant;

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use image::{
    DynamicImage, ExtendedColorType, Rgba, RgbaImage,
    codecs::jpeg::JpegEncoder,
    imageops::{FilterType, replace, resize},
};
use tokio::{sync::mpsc, task::JoinHandle, time::timeout};

use crate::{discord::rest::DiscordRest, logging};

#[cfg(feature = "stream-broadcast")]
const STREAM_PREVIEW_INTERVAL: Duration = Duration::from_secs(60);
const STREAM_PREVIEW_UPLOAD_TIMEOUT: Duration = Duration::from_secs(10);
const STREAM_PREVIEW_WIDTH: u32 = 640;
const STREAM_PREVIEW_HEIGHT: u32 = 360;
const STREAM_PREVIEW_JPEG_QUALITY: u8 = 80;

#[derive(Debug)]
pub(super) struct StreamPreviewFrame {
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) rgba: Vec<u8>,
}

#[derive(Default)]
#[cfg(feature = "stream-broadcast")]
pub(super) struct StreamPreviewCadence {
    last_queued_at: Option<Instant>,
}

#[cfg(feature = "stream-broadcast")]
impl StreamPreviewCadence {
    pub(super) fn is_due(&self, now: Instant) -> bool {
        self.last_queued_at.is_none_or(|last_queued_at| {
            now.checked_duration_since(last_queued_at)
                .is_some_and(|elapsed| elapsed >= STREAM_PREVIEW_INTERVAL)
        })
    }

    pub(super) fn record_queued(&mut self, now: Instant) {
        self.last_queued_at = Some(now);
    }
}

pub(super) fn encode_stream_preview_data_uri(frame: StreamPreviewFrame) -> Result<String, String> {
    if frame.width == 0 || frame.height == 0 {
        return Err("stream preview frame is empty".to_owned());
    }
    let image = RgbaImage::from_raw(frame.width, frame.height, frame.rgba)
        .ok_or_else(|| "stream preview RGBA frame has an invalid length".to_owned())?;
    let scale = (STREAM_PREVIEW_WIDTH as f64 / frame.width as f64)
        .min(STREAM_PREVIEW_HEIGHT as f64 / frame.height as f64);
    let scaled_width = ((frame.width as f64 * scale).round() as u32).clamp(1, STREAM_PREVIEW_WIDTH);
    let scaled_height =
        ((frame.height as f64 * scale).round() as u32).clamp(1, STREAM_PREVIEW_HEIGHT);
    let resized = resize(&image, scaled_width, scaled_height, FilterType::Triangle);
    let mut letterboxed = RgbaImage::from_pixel(
        STREAM_PREVIEW_WIDTH,
        STREAM_PREVIEW_HEIGHT,
        Rgba([0, 0, 0, 255]),
    );
    replace(
        &mut letterboxed,
        &resized,
        i64::from((STREAM_PREVIEW_WIDTH - scaled_width) / 2),
        i64::from((STREAM_PREVIEW_HEIGHT - scaled_height) / 2),
    );
    let rgb = DynamicImage::ImageRgba8(letterboxed).to_rgb8();
    let mut jpeg = Vec::new();
    JpegEncoder::new_with_quality(&mut jpeg, STREAM_PREVIEW_JPEG_QUALITY)
        .encode(
            rgb.as_raw(),
            STREAM_PREVIEW_WIDTH,
            STREAM_PREVIEW_HEIGHT,
            ExtendedColorType::Rgb8,
        )
        .map_err(|error| format!("stream preview JPEG encoding failed: {error}"))?;

    Ok(format!(
        "data:image/jpeg;base64,{}",
        BASE64_STANDARD.encode(jpeg)
    ))
}

#[derive(Clone)]
pub(crate) struct StreamPreviewUploader {
    rest: DiscordRest,
}

pub(super) struct StreamPreviewUploadTask {
    task: Option<JoinHandle<()>>,
}

impl StreamPreviewUploader {
    pub(crate) fn new(rest: DiscordRest) -> Self {
        Self { rest }
    }

    pub(super) fn start(
        &self,
        stream_key: String,
        frames_rx: mpsc::Receiver<StreamPreviewFrame>,
    ) -> StreamPreviewUploadTask {
        let uploader = self.clone();
        let task = tokio::spawn(async move {
            uploader.run(stream_key, frames_rx).await;
        });
        StreamPreviewUploadTask { task: Some(task) }
    }

    async fn run(self, stream_key: String, mut frames_rx: mpsc::Receiver<StreamPreviewFrame>) {
        while let Some(frame) = frames_rx.recv().await {
            let thumbnail =
                match tokio::task::spawn_blocking(move || encode_stream_preview_data_uri(frame))
                    .await
                {
                    Ok(Ok(thumbnail)) => thumbnail,
                    Ok(Err(error)) => {
                        logging::debug("stream", error);
                        continue;
                    }
                    Err(error) => {
                        logging::debug(
                            "stream",
                            format!("stream preview encoder task failed: {error}"),
                        );
                        continue;
                    }
                };

            match timeout(
                STREAM_PREVIEW_UPLOAD_TIMEOUT,
                self.rest.upload_stream_preview(&stream_key, &thumbnail),
            )
            .await
            {
                Ok(Ok(())) => {
                    logging::debug("stream", "stream preview uploaded");
                }
                Ok(Err(error)) => {
                    logging::debug("stream", format!("stream preview upload failed: {error}"));
                }
                Err(_) => {
                    logging::debug("stream", "stream preview upload timed out");
                }
            }
        }
    }
}

impl StreamPreviewUploadTask {
    pub(super) async fn shutdown(mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
            let _ = task.await;
        }
    }
}

impl Drop for StreamPreviewUploadTask {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use image::GenericImageView;

    use super::*;

    #[test]
    #[cfg(feature = "stream-broadcast")]
    fn stream_preview_cadence_queues_immediately_then_waits_for_interval() {
        let started_at = Instant::now();
        let mut cadence = StreamPreviewCadence::default();

        assert!(cadence.is_due(started_at));
        cadence.record_queued(started_at);
        assert!(!cadence.is_due(started_at + STREAM_PREVIEW_INTERVAL - Duration::from_millis(1)));
        assert!(cadence.is_due(started_at + STREAM_PREVIEW_INTERVAL));
    }

    #[test]
    fn stream_preview_data_uri_is_letterboxed_at_preview_size() {
        let frame = StreamPreviewFrame {
            width: 2,
            height: 2,
            rgba: [255, 0, 0, 255].repeat(4),
        };

        let data_uri = encode_stream_preview_data_uri(frame).expect("preview frame should encode");
        let encoded = data_uri
            .strip_prefix("data:image/jpeg;base64,")
            .expect("preview should use a JPEG data URI");
        let bytes = BASE64_STANDARD
            .decode(encoded)
            .expect("preview base64 should decode");
        let image = image::load_from_memory(&bytes).expect("preview JPEG should decode");

        assert_eq!(
            image.dimensions(),
            (STREAM_PREVIEW_WIDTH, STREAM_PREVIEW_HEIGHT)
        );
        let left_bar = image.get_pixel(0, STREAM_PREVIEW_HEIGHT / 2);
        assert!(
            left_bar.0[..3].iter().all(|channel| *channel < 16),
            "left preview bar should be black: {left_bar:?}"
        );
        let center = image.get_pixel(STREAM_PREVIEW_WIDTH / 2, STREAM_PREVIEW_HEIGHT / 2);
        assert!(
            center.0[0] > 220 && center.0[1] < 32 && center.0[2] < 32,
            "preview center should remain red: {center:?}"
        );
    }
}
