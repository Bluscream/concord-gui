use std::{
    io::Cursor,
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
};

use image::{
    AnimationDecoder as _, DynamicImage, Frame, ImageDecoder as _, ImageFormat, ImageReader,
    Limits,
    codecs::{gif::GifDecoder, webp::WebPDecoder},
};
use tokio::{sync::mpsc, task};

use super::preview::ImagePreviewKey;

const MAX_CONCURRENT_MEDIA_IMAGE_DECODES: usize = 2;
pub(super) const MAX_DECODED_IMAGE_WIDTH: u32 = 4096;
pub(super) const MAX_DECODED_IMAGE_HEIGHT: u32 = 4096;
const MAX_DECODED_IMAGE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_DECODED_ANIMATION_FRAMES: usize = 256;
const MIN_ANIMATION_FRAME_DELAY: Duration = Duration::from_millis(50);
const MAX_ANIMATION_FRAME_DELAY: Duration = Duration::from_secs(10);

struct DecodedMediaFrame {
    image: DynamicImage,
    delay: Duration,
}

pub(in crate::tui) struct DecodedMediaImage {
    frames: Vec<DecodedMediaFrame>,
    current_frame_index: usize,
    next_frame_deadline: Option<Instant>,
}

impl DecodedMediaImage {
    fn still(image: DynamicImage) -> Self {
        Self {
            frames: vec![DecodedMediaFrame {
                image,
                delay: MIN_ANIMATION_FRAME_DELAY,
            }],
            current_frame_index: 0,
            next_frame_deadline: None,
        }
    }

    pub(in crate::tui) fn current_frame(&self) -> &DynamicImage {
        &self
            .frames
            .get(self.current_frame_index)
            .expect("decoded media always has a current frame")
            .image
    }

    pub(in crate::tui) fn current_frame_index(&self) -> usize {
        self.current_frame_index
    }

    pub(in crate::tui) fn frame_count(&self) -> usize {
        self.frames.len()
    }

    pub(in crate::tui) fn is_animated(&self) -> bool {
        self.frame_count() > 1
    }

    pub(in crate::tui) fn start_animation(&mut self, now: Instant) {
        if !self.is_animated() || self.next_frame_deadline.is_some() {
            return;
        }
        self.next_frame_deadline = now.checked_add(self.current_frame_delay());
    }

    pub(in crate::tui) fn pause_animation(&mut self) {
        self.next_frame_deadline = None;
    }

    pub(in crate::tui) fn next_frame_deadline(&self) -> Option<Instant> {
        self.next_frame_deadline
    }

    pub(in crate::tui) fn advance_frame(&mut self, now: Instant) -> bool {
        let Some(deadline) = self.next_frame_deadline else {
            return false;
        };
        if now < deadline || !self.is_animated() {
            return false;
        }

        self.current_frame_index = (self.current_frame_index + 1) % self.frames.len();
        self.next_frame_deadline = now.checked_add(self.current_frame_delay());
        true
    }

    fn current_frame_delay(&self) -> Duration {
        self.frames
            .get(self.current_frame_index)
            .expect("decoded media always has a current frame")
            .delay
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::tui) enum MediaImageDecodeKey {
    Preview(ImagePreviewKey),
    Avatar(String),
    Emoji(String),
}

pub(in crate::tui) struct MediaImageDecodeJob {
    pub(super) key: MediaImageDecodeKey,
    pub(super) generation: u64,
    pub(super) bytes: Arc<[u8]>,
}

pub(in crate::tui) struct MediaImageDecodeResult {
    pub(in crate::tui) key: MediaImageDecodeKey,
    pub(in crate::tui) generation: u64,
    pub(in crate::tui) result: std::result::Result<DecodedMediaImage, String>,
}

pub(in crate::tui) fn spawn_media_image_decode(
    job: MediaImageDecodeJob,
    tx: mpsc::UnboundedSender<MediaImageDecodeResult>,
) {
    let decode_permits = media_image_decode_permits().clone();
    task::spawn(async move {
        let Ok(_permit) = decode_permits.acquire_owned().await else {
            return;
        };
        if let Ok(result) = task::spawn_blocking(move || decode_media_image(job)).await {
            let _ = tx.send(result);
        }
    });
}

fn decode_media_image(job: MediaImageDecodeJob) -> MediaImageDecodeResult {
    let result = decode_media_image_bytes(&job.bytes);
    MediaImageDecodeResult {
        key: job.key,
        generation: job.generation,
        result,
    }
}

fn media_image_decode_permits() -> &'static Arc<tokio::sync::Semaphore> {
    static PERMITS: OnceLock<Arc<tokio::sync::Semaphore>> = OnceLock::new();
    PERMITS.get_or_init(|| {
        Arc::new(tokio::sync::Semaphore::new(
            MAX_CONCURRENT_MEDIA_IMAGE_DECODES,
        ))
    })
}

pub(in crate::tui) fn decode_image_bytes(
    bytes: &[u8],
) -> std::result::Result<DynamicImage, String> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|error| format!("decode failed: {error}"))?;
    reader.limits(decode_limits());
    reader
        .decode()
        .map_err(|error| format!("decode failed: {error}"))
}

pub(in crate::tui) fn decode_media_image_bytes(
    bytes: &[u8],
) -> std::result::Result<DecodedMediaImage, String> {
    let reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|error| format!("decode failed: {error}"))?;

    match reader.format() {
        Some(ImageFormat::Gif) => decode_gif_animation(bytes),
        Some(ImageFormat::WebP) => decode_webp_animation(bytes),
        _ => decode_image_bytes(bytes).map(DecodedMediaImage::still),
    }
}

fn decode_gif_animation(bytes: &[u8]) -> std::result::Result<DecodedMediaImage, String> {
    let mut decoder =
        GifDecoder::new(Cursor::new(bytes)).map_err(|error| format!("decode failed: {error}"))?;
    decoder
        .set_limits(decode_limits())
        .map_err(|error| format!("decode failed: {error}"))?;
    decode_animation_frames(decoder.into_frames())
}

fn decode_webp_animation(bytes: &[u8]) -> std::result::Result<DecodedMediaImage, String> {
    let mut decoder =
        WebPDecoder::new(Cursor::new(bytes)).map_err(|error| format!("decode failed: {error}"))?;
    decoder
        .set_limits(decode_limits())
        .map_err(|error| format!("decode failed: {error}"))?;
    if !decoder.has_animation() {
        return decode_image_bytes(bytes).map(DecodedMediaImage::still);
    }
    decode_animation_frames(decoder.into_frames())
}

fn decode_animation_frames(
    frames: impl Iterator<Item = image::ImageResult<Frame>>,
) -> std::result::Result<DecodedMediaImage, String> {
    let mut decoded_frames = Vec::new();
    let mut decoded_bytes = 0u64;

    for result in frames {
        let frame = match result {
            Ok(frame) => frame,
            Err(error) if decoded_frames.is_empty() => {
                return Err(format!("decode failed: {error}"));
            }
            Err(_) => return first_frame_fallback(decoded_frames),
        };
        let frame_bytes = u64::try_from(frame.buffer().as_raw().len()).unwrap_or(u64::MAX);
        let next_decoded_bytes = decoded_bytes.saturating_add(frame_bytes);
        if decoded_frames.len() >= MAX_DECODED_ANIMATION_FRAMES
            || next_decoded_bytes > MAX_DECODED_IMAGE_BYTES
        {
            return first_frame_fallback(decoded_frames);
        }

        decoded_bytes = next_decoded_bytes;
        decoded_frames.push(DecodedMediaFrame {
            delay: bounded_frame_delay(frame.delay()),
            image: DynamicImage::ImageRgba8(frame.into_buffer()),
        });
    }

    match decoded_frames.len() {
        0 => Err("decode failed: animated image has no frames".to_owned()),
        1 => first_frame_fallback(decoded_frames),
        _ => Ok(DecodedMediaImage {
            frames: decoded_frames,
            current_frame_index: 0,
            next_frame_deadline: None,
        }),
    }
}

fn first_frame_fallback(
    frames: Vec<DecodedMediaFrame>,
) -> std::result::Result<DecodedMediaImage, String> {
    frames
        .into_iter()
        .next()
        .map(|frame| DecodedMediaImage::still(frame.image))
        .ok_or_else(|| "decode failed: animated image has no frames".to_owned())
}

fn bounded_frame_delay(delay: image::Delay) -> Duration {
    let (numerator, denominator) = delay.numer_denom_ms();
    let nanos = u128::from(numerator)
        .saturating_mul(1_000_000)
        .checked_div(u128::from(denominator))
        .unwrap_or_default();
    let duration = Duration::from_nanos(u64::try_from(nanos).unwrap_or(u64::MAX));
    duration.clamp(MIN_ANIMATION_FRAME_DELAY, MAX_ANIMATION_FRAME_DELAY)
}

fn decode_limits() -> Limits {
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DECODED_IMAGE_WIDTH);
    limits.max_image_height = Some(MAX_DECODED_IMAGE_HEIGHT);
    limits.max_alloc = Some(MAX_DECODED_IMAGE_BYTES);
    limits
}
