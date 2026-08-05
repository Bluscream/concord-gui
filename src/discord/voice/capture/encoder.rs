use openh264::{
    OpenH264API,
    encoder::{
        BitRate, Encoder, EncoderConfig, FrameRate, FrameType, IntraFramePeriod, Level, Profile,
        RateControlMode, UsageType, VuiConfig,
    },
    formats::YUVSlices,
};

use super::{STREAM_CAPTURE_BITRATE, STREAM_CAPTURE_FPS, STREAM_INTRA_FRAME_PERIOD_FRAMES};
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
use super::{STREAM_CAPTURE_HEIGHT, STREAM_CAPTURE_WIDTH};
use crate::logging;

#[derive(Clone, Copy)]
pub(super) struct I420Frame<'a> {
    y: &'a [u8],
    u: &'a [u8],
    v: &'a [u8],
    width: usize,
    height: usize,
    y_stride: usize,
    u_stride: usize,
    v_stride: usize,
}

impl<'a> I420Frame<'a> {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        y: &'a [u8],
        u: &'a [u8],
        v: &'a [u8],
        width: usize,
        height: usize,
        y_stride: usize,
        u_stride: usize,
        v_stride: usize,
    ) -> Self {
        Self {
            y,
            u,
            v,
            width,
            height,
            y_stride,
            u_stride,
            v_stride,
        }
    }
}

pub(super) struct EncodedH264Frame {
    pub(super) annex_b: Vec<u8>,
    pub(super) is_keyframe: bool,
}

impl EncodedH264Frame {
    fn new(mut annex_b: Vec<u8>, is_keyframe: bool) -> Result<Self, String> {
        h264::normalize_annex_b_for_webrtc(&mut annex_b)?;
        Ok(Self {
            annex_b,
            is_keyframe,
        })
    }
}

pub(super) enum StreamEncoder {
    OpenH264(Box<OpenH264Encoder>),
    #[cfg(target_os = "linux")]
    VaApi(Box<vaapi::VaApiEncoder>),
    #[cfg(target_os = "macos")]
    VideoToolbox(Box<macos::VideoToolboxEncoder>),
    #[cfg(target_os = "windows")]
    MediaFoundation(Box<windows::MediaFoundationEncoder>),
}

impl StreamEncoder {
    pub(super) fn new_auto() -> Result<Self, String> {
        #[cfg(target_os = "linux")]
        return Self::new_with_hardware(
            "VA API",
            vaapi::VaApiEncoder::new().map(|encoder| Self::VaApi(Box::new(encoder))),
        );

        #[cfg(target_os = "macos")]
        return Self::new_with_hardware(
            "VideoToolbox",
            macos::VideoToolboxEncoder::new().map(|encoder| Self::VideoToolbox(Box::new(encoder))),
        );

        #[cfg(target_os = "windows")]
        return Self::new_with_hardware(
            "Media Foundation",
            windows::MediaFoundationEncoder::new()
                .map(|encoder| Self::MediaFoundation(Box::new(encoder))),
        );

        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        let encoder = Self::OpenH264(Box::new(OpenH264Encoder::new()?));
        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        logging::debug(
            "stream",
            format!("stream H264 encoder selected: backend={}", encoder.name()),
        );
        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        return Ok(encoder);
    }

    fn new_with_hardware(
        hardware_name: &'static str,
        hardware: Result<Self, String>,
    ) -> Result<Self, String> {
        let selection = select_with_software_fallback(hardware_name, hardware, || {
            OpenH264Encoder::new().map(Box::new).map(Self::OpenH264)
        })?;
        if let Some(error) = selection.hardware_failure.as_deref() {
            logging::debug(
                "stream",
                format!(
                    "{hardware_name} H264 encoder is unavailable; falling back to OpenH264: {error}"
                ),
            );
        }
        logging::debug(
            "stream",
            format!(
                "stream H264 encoder selected: backend={}",
                selection.encoder.name()
            ),
        );
        Ok(selection.encoder)
    }

    fn name(&self) -> &'static str {
        match self {
            Self::OpenH264(_) => "openh264",
            #[cfg(target_os = "linux")]
            Self::VaApi(_) => "vaapi",
            #[cfg(target_os = "macos")]
            Self::VideoToolbox(_) => "videotoolbox",
            #[cfg(target_os = "windows")]
            Self::MediaFoundation(_) => "media-foundation",
        }
    }

    pub(super) fn encode(
        &mut self,
        frame: I420Frame<'_>,
        force_keyframe: bool,
    ) -> Result<Option<EncodedH264Frame>, String> {
        match self {
            Self::OpenH264(encoder) => encoder.encode(frame, force_keyframe),
            #[cfg(target_os = "linux")]
            Self::VaApi(encoder) => encoder.encode(frame, force_keyframe),
            #[cfg(target_os = "macos")]
            Self::VideoToolbox(encoder) => encoder.encode(frame, force_keyframe),
            #[cfg(target_os = "windows")]
            Self::MediaFoundation(encoder) => encoder.encode(frame, force_keyframe),
        }
    }
}

pub(super) struct OpenH264Encoder {
    encoder: Encoder,
}

impl OpenH264Encoder {
    fn new() -> Result<Self, String> {
        let encoder =
            Encoder::with_api_config(OpenH264API::from_source(), openh264_encoder_config())
                .map_err(|error| format!("OpenH264 encoder creation failed: {error}"))?;
        Ok(Self { encoder })
    }

    fn encode(
        &mut self,
        frame: I420Frame<'_>,
        force_keyframe: bool,
    ) -> Result<Option<EncodedH264Frame>, String> {
        if force_keyframe {
            self.encoder.force_intra_frame();
        }
        let yuv = YUVSlices::new(
            (frame.y, frame.u, frame.v),
            (frame.width, frame.height),
            (frame.y_stride, frame.u_stride, frame.v_stride),
        );
        let encoded = self
            .encoder
            .encode(&yuv)
            .map_err(|error| format!("OpenH264 frame encoding failed: {error}"))?;
        let is_keyframe = matches!(encoded.frame_type(), FrameType::IDR | FrameType::I);
        let annex_b = encoded.to_vec();

        (!annex_b.is_empty())
            .then(|| EncodedH264Frame::new(annex_b, is_keyframe))
            .transpose()
    }
}

pub(super) fn openh264_encoder_config() -> EncoderConfig {
    // OpenH264 enables these camera-oriented tools by default, but its
    // screen-content mode rejects them and writes warnings directly to stderr.
    EncoderConfig::new()
        .usage_type(UsageType::ScreenContentRealTime)
        .skip_frames(true)
        .adaptive_quantization(false)
        .background_detection(false)
        .rate_control_mode(RateControlMode::Bitrate)
        .bitrate(BitRate::from_bps(STREAM_CAPTURE_BITRATE))
        .max_frame_rate(FrameRate::from_hz(STREAM_CAPTURE_FPS as f32))
        .profile(Profile::Baseline)
        .level(Level::Level_3_1)
        .intra_frame_period(IntraFramePeriod::from_num_frames(
            STREAM_INTRA_FRAME_PERIOD_FRAMES,
        ))
        .vui(VuiConfig::bt709())
}

struct EncoderSelection<T> {
    encoder: T,
    hardware_failure: Option<String>,
}

fn select_with_software_fallback<T>(
    hardware_name: &str,
    hardware: Result<T, String>,
    software: impl FnOnce() -> Result<T, String>,
) -> Result<EncoderSelection<T>, String> {
    match hardware {
        Ok(encoder) => Ok(EncoderSelection {
            encoder,
            hardware_failure: None,
        }),
        Err(hardware_error) => software()
            .map(|encoder| EncoderSelection {
                encoder,
                hardware_failure: Some(hardware_error.clone()),
            })
            .map_err(|software_error| {
                format!(
                    "H264 encoder creation failed: {hardware_name}: {hardware_error}; OpenH264: {software_error}"
                )
            }),
    }
}

#[path = "encoder/h264.rs"]
mod h264;

#[cfg(target_os = "macos")]
#[path = "encoder/macos.rs"]
mod macos;

#[cfg(target_os = "windows")]
#[path = "encoder/windows.rs"]
mod windows;

#[cfg(target_os = "linux")]
mod vaapi {
    use std::{
        collections::VecDeque,
        path::{Path, PathBuf},
        rc::Rc,
        sync::Arc,
    };

    use cros_codecs::{
        BlockingMode, DecodedFormat, Fourcc, FrameLayout, PlaneLayout, Resolution,
        backend::vaapi::encoder::VaapiBackend,
        codec::h264::parser::{Level, Profile},
        decoder::StreamInfo,
        encoder::{
            FrameMetadata, PredictionStructure, RateControl, Tunings, VideoEncoder,
            h264::EncoderConfig, stateless::h264::StatelessEncoder as H264StatelessEncoder,
        },
        libva::{Display, Surface, VAEntrypoint, VAProfile},
        video_frame::{
            VideoFrame,
            frame_pool::{FramePool, PooledVideoFrame},
            gbm_video_frame::{GbmDevice, GbmExternalBufferDescriptor, GbmUsage, GbmVideoFrame},
        },
    };

    use super::{
        EncodedH264Frame, I420Frame, STREAM_CAPTURE_BITRATE, STREAM_CAPTURE_FPS,
        STREAM_CAPTURE_HEIGHT, STREAM_CAPTURE_WIDTH, STREAM_INTRA_FRAME_PERIOD_FRAMES,
        annex_b_contains_idr, copy_i420_to_nv12,
    };

    const VA_INPUT_FRAME_COUNT: usize = 3;

    type HardwareEncoder = H264StatelessEncoder<
        PooledVideoFrame<GbmVideoFrame>,
        VaapiBackend<GbmExternalBufferDescriptor, Surface<GbmExternalBufferDescriptor>>,
    >;

    pub(in crate::discord::voice::capture) struct VaApiEncoder {
        encoder: HardwareEncoder,
        frames: FramePool<GbmVideoFrame>,
        frame_layout: FrameLayout,
        frame_index: u64,
    }

    impl VaApiEncoder {
        pub(super) fn new() -> Result<Self, String> {
            let paths = render_device_paths()?;
            let mut failures = Vec::new();

            for path in paths {
                match Self::for_device(&path) {
                    Ok(encoder) => return Ok(encoder),
                    Err(error) => failures.push(format!("{}: {error}", path.display())),
                }
            }

            Err(format!(
                "no compatible VA API render device was found ({})",
                failures.join("; ")
            ))
        }

        fn for_device(path: &Path) -> Result<Self, String> {
            let display = Display::open_drm_display(path)
                .map_err(|error| format!("VA display initialization failed: {error}"))?;
            let gbm_device = GbmDevice::open(path)
                .map_err(|error| format!("GBM device initialization failed: {error}"))?;
            let entrypoints = display
                .query_config_entrypoints(VAProfile::VAProfileH264ConstrainedBaseline)
                .map_err(|error| format!("VA H264 entrypoint query failed: {error}"))?;
            let power_modes = encoder_power_modes(&entrypoints)?;
            let resolution = Resolution {
                width: STREAM_CAPTURE_WIDTH,
                height: STREAM_CAPTURE_HEIGHT,
            };
            let fourcc = Fourcc::from(b"NV12");
            let frame_layout = nv12_frame_layout(fourcc, resolution);
            let config = hardware_encoder_config(resolution);
            let mut encoder_errors = Vec::new();

            for low_power in power_modes {
                match Self::for_power_mode(
                    Rc::clone(&display),
                    Arc::clone(&gbm_device),
                    config.clone(),
                    fourcc,
                    resolution,
                    frame_layout.clone(),
                    low_power,
                ) {
                    Ok(encoder) => return Ok(encoder),
                    Err(error) => encoder_errors.push(format!(
                        "{} entrypoint: {error}",
                        if low_power { "low-power" } else { "standard" }
                    )),
                }
            }

            Err(format!(
                "VA H264 encoder initialization failed ({})",
                encoder_errors.join("; ")
            ))
        }

        fn for_power_mode(
            display: Rc<Display>,
            gbm_device: Arc<GbmDevice>,
            config: EncoderConfig,
            fourcc: Fourcc,
            resolution: Resolution,
            frame_layout: FrameLayout,
            low_power: bool,
        ) -> Result<Self, String> {
            let frames = create_frame_pool(Rc::clone(&display), gbm_device, fourcc, resolution)?;
            let encoder = HardwareEncoder::new_vaapi(
                Rc::clone(&display),
                config.clone(),
                fourcc,
                resolution,
                low_power,
                BlockingMode::Blocking,
            )
            .map_err(|error| format!("VA H264 encoder creation failed: {error}"))?;
            let mut candidate = Self {
                encoder,
                frames,
                frame_layout: frame_layout.clone(),
                frame_index: 0,
            };
            candidate.probe()?;

            // The probe starts an H264 sequence. Recreate only the codec state so
            // the first frame sent to Discord remains the first IDR of the stream.
            let frames = candidate.frames;
            drop(candidate.encoder);
            let encoder = HardwareEncoder::new_vaapi(
                display,
                config,
                fourcc,
                resolution,
                low_power,
                BlockingMode::Blocking,
            )
            .map_err(|error| format!("VA H264 encoder recreation failed after probe: {error}"))?;
            Ok(Self {
                encoder,
                frames,
                frame_layout,
                frame_index: 0,
            })
        }

        fn probe(&mut self) -> Result<(), String> {
            let width = STREAM_CAPTURE_WIDTH as usize;
            let height = STREAM_CAPTURE_HEIGHT as usize;
            let y = vec![16; width * height];
            let u = vec![128; width * height / 4];
            let v = vec![128; width * height / 4];
            let frame = I420Frame::new(&y, &u, &v, width, height, width, width / 2, width / 2);
            let encoded = self
                .encode(frame, false)?
                .ok_or_else(|| "VA H264 probe produced no encoded frame".to_owned())?;
            if !encoded.is_keyframe {
                return Err("VA H264 probe did not produce an initial keyframe".to_owned());
            }
            Ok(())
        }

        pub(super) fn encode(
            &mut self,
            input: I420Frame<'_>,
            force_keyframe: bool,
        ) -> Result<Option<EncodedH264Frame>, String> {
            let mut frame = self
                .frames
                .alloc()
                .ok_or_else(|| "VA API input surface pool is exhausted".to_owned())?;
            upload_i420_frame(&mut frame, input)?;

            let metadata = FrameMetadata {
                timestamp: self.frame_index,
                layout: self.frame_layout.clone(),
                force_keyframe,
            };
            self.frame_index = self.frame_index.wrapping_add(1);
            self.encoder
                .encode(metadata, frame)
                .map_err(|error| format!("VA API H264 frame submission failed: {error}"))?;
            let coded = self
                .encoder
                .poll()
                .map_err(|error| format!("VA API H264 frame completion failed: {error}"))?;
            let Some(coded) = coded else {
                return Ok(None);
            };
            let annex_b = coded.bitstream;
            let is_keyframe = coded.metadata.force_keyframe || annex_b_contains_idr(&annex_b);

            (!annex_b.is_empty())
                .then(|| EncodedH264Frame::new(annex_b, is_keyframe))
                .transpose()
        }
    }

    fn render_device_paths() -> Result<Vec<PathBuf>, String> {
        let entries = std::fs::read_dir("/dev/dri")
            .map_err(|error| format!("cannot inspect /dev/dri: {error}"))?;
        let mut paths = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| {
                        name.strip_prefix("renderD").is_some_and(|suffix| {
                            suffix.chars().all(|value| value.is_ascii_digit())
                        })
                    })
            })
            .collect::<Vec<_>>();
        paths.sort();
        if paths.is_empty() {
            return Err("no DRM render devices are available under /dev/dri".to_owned());
        }
        Ok(paths)
    }

    fn encoder_power_modes(entrypoints: &[VAEntrypoint::Type]) -> Result<Vec<bool>, String> {
        let mut modes = Vec::new();
        if entrypoints.contains(&VAEntrypoint::VAEntrypointEncSliceLP) {
            modes.push(true);
        }
        if entrypoints.contains(&VAEntrypoint::VAEntrypointEncSlice) {
            modes.push(false);
        }
        if modes.is_empty() {
            return Err("the VA driver does not expose an H264 encode entrypoint".to_owned());
        }
        Ok(modes)
    }

    fn create_frame_pool(
        display: Rc<Display>,
        gbm_device: Arc<GbmDevice>,
        fourcc: Fourcc,
        resolution: Resolution,
    ) -> Result<FramePool<GbmVideoFrame>, String> {
        let mut allocated = VecDeque::with_capacity(VA_INPUT_FRAME_COUNT);
        for _ in 0..VA_INPUT_FRAME_COUNT {
            allocated.push_back(
                Arc::clone(&gbm_device)
                    .new_frame(fourcc, resolution, resolution, GbmUsage::Encode)
                    .map_err(|error| format!("VA API input surface allocation failed: {error}"))?,
            );
        }

        let probe = allocated
            .front_mut()
            .expect("VA API input surface count is non-zero");
        validate_mappable_frame(probe)?;
        let native = probe
            .to_native_handle(&display)
            .map_err(|error| format!("VA API input surface import failed: {error}"))?;
        drop(native);

        let mut frames = FramePool::new(move |_| {
            allocated
                .pop_front()
                .expect("VA API frame pool uses only preallocated surfaces")
        });
        frames.resize(&StreamInfo {
            format: DecodedFormat::NV12,
            coded_resolution: resolution,
            display_resolution: resolution,
            min_num_frames: VA_INPUT_FRAME_COUNT,
        });
        Ok(frames)
    }

    fn validate_mappable_frame(frame: &mut GbmVideoFrame) -> Result<(), String> {
        let strides = frame.get_plane_pitch();
        if strides.len() < 2 {
            return Err("VA API NV12 input surface does not expose two planes".to_owned());
        }
        let mapping = frame
            .map_mut()
            .map_err(|error| format!("VA API input surface mapping failed: {error}"))?;
        let planes = mapping.get();
        if planes.len() < 2 {
            return Err("VA API NV12 input mapping does not expose two planes".to_owned());
        }
        Ok(())
    }

    fn upload_i420_frame(
        frame: &mut PooledVideoFrame<GbmVideoFrame>,
        input: I420Frame<'_>,
    ) -> Result<(), String> {
        let strides = frame.get_plane_pitch();
        if strides.len() < 2 {
            return Err("VA API NV12 input surface does not expose two strides".to_owned());
        }
        let mapping = frame
            .map_mut()
            .map_err(|error| format!("VA API input surface mapping failed: {error}"))?;
        let planes = mapping.get();
        if planes.len() < 2 {
            return Err("VA API NV12 input mapping does not expose two planes".to_owned());
        }
        let mut y = planes[0].borrow_mut();
        let mut uv = planes[1].borrow_mut();
        copy_i420_to_nv12(input, &mut y, &mut uv, strides[0], strides[1])
            .map_err(|error| format!("VA API NV12 upload failed: {error}"))
    }

    fn hardware_encoder_config(resolution: Resolution) -> EncoderConfig {
        EncoderConfig {
            resolution,
            profile: Profile::Baseline,
            level: Level::L3_1,
            pred_structure: PredictionStructure::LowDelay {
                limit: u16::try_from(STREAM_INTRA_FRAME_PERIOD_FRAMES)
                    .expect("stream intra period fits in u16"),
            },
            initial_tunings: Tunings {
                rate_control: RateControl::ConstantBitrate(u64::from(STREAM_CAPTURE_BITRATE)),
                framerate: STREAM_CAPTURE_FPS,
                ..Default::default()
            },
        }
    }

    fn nv12_frame_layout(fourcc: Fourcc, resolution: Resolution) -> FrameLayout {
        let stride = resolution.width as usize;
        FrameLayout {
            format: (fourcc, 0),
            size: resolution,
            planes: vec![
                PlaneLayout {
                    buffer_index: 0,
                    offset: 0,
                    stride,
                },
                PlaneLayout {
                    buffer_index: 0,
                    offset: stride * resolution.height as usize,
                    stride,
                },
            ],
        }
    }
}

#[cfg(any(test, target_os = "linux", target_os = "macos", target_os = "windows"))]
fn copy_i420_to_nv12(
    input: I420Frame<'_>,
    destination_y: &mut [u8],
    destination_uv: &mut [u8],
    destination_y_stride: usize,
    destination_uv_stride: usize,
) -> Result<(), String> {
    if input.width == 0
        || input.height == 0
        || !input.width.is_multiple_of(2)
        || !input.height.is_multiple_of(2)
    {
        return Err("I420 dimensions must be non-zero and even".to_owned());
    }
    validate_plane(
        input.y,
        input.y_stride,
        input.width,
        input.height,
        "source Y",
    )?;
    let chroma_width = input.width / 2;
    let chroma_height = input.height / 2;
    validate_plane(
        input.u,
        input.u_stride,
        chroma_width,
        chroma_height,
        "source U",
    )?;
    validate_plane(
        input.v,
        input.v_stride,
        chroma_width,
        chroma_height,
        "source V",
    )?;
    validate_plane(
        destination_y,
        destination_y_stride,
        input.width,
        input.height,
        "destination Y",
    )?;
    validate_plane(
        destination_uv,
        destination_uv_stride,
        input.width,
        chroma_height,
        "destination UV",
    )?;

    for row in 0..input.height {
        let source_start = row * input.y_stride;
        let destination_start = row * destination_y_stride;
        destination_y[destination_start..destination_start + input.width]
            .copy_from_slice(&input.y[source_start..source_start + input.width]);
    }
    for row in 0..chroma_height {
        let u_start = row * input.u_stride;
        let v_start = row * input.v_stride;
        let destination_start = row * destination_uv_stride;
        let destination = &mut destination_uv[destination_start..destination_start + input.width];
        for column in 0..chroma_width {
            destination[column * 2] = input.u[u_start + column];
            destination[column * 2 + 1] = input.v[v_start + column];
        }
    }
    Ok(())
}

#[cfg(any(test, target_os = "linux", target_os = "macos", target_os = "windows"))]
fn validate_plane(
    plane: &[u8],
    stride: usize,
    row_width: usize,
    rows: usize,
    name: &str,
) -> Result<(), String> {
    if stride < row_width {
        return Err(format!("{name} stride is shorter than its row width"));
    }
    let required = rows
        .saturating_sub(1)
        .checked_mul(stride)
        .and_then(|length| length.checked_add(row_width))
        .ok_or_else(|| format!("{name} dimensions overflow"))?;
    if plane.len() < required {
        return Err(format!(
            "{name} plane is too short: required={required} available={}",
            plane.len()
        ));
    }
    Ok(())
}

#[cfg(any(test, target_os = "macos", target_os = "windows"))]
fn length_prefixed_h264_to_annex_b(
    frame: &[u8],
    nal_length_bytes: usize,
) -> Result<Vec<u8>, String> {
    if !(1..=4).contains(&nal_length_bytes) {
        return Err(format!(
            "H264 NAL length field must use 1 to 4 bytes, got {nal_length_bytes}"
        ));
    }

    let mut annex_b = Vec::with_capacity(frame.len().saturating_add(16));
    let mut offset = 0usize;
    while offset < frame.len() {
        let length_end = offset
            .checked_add(nal_length_bytes)
            .ok_or_else(|| "H264 NAL length offset overflowed".to_owned())?;
        let length_bytes = frame
            .get(offset..length_end)
            .ok_or_else(|| "H264 access unit ends inside a NAL length field".to_owned())?;
        let mut nal_length = 0usize;
        for byte in length_bytes {
            nal_length = nal_length
                .checked_mul(256)
                .and_then(|length| length.checked_add(usize::from(*byte)))
                .ok_or_else(|| "H264 NAL length overflowed".to_owned())?;
        }
        if nal_length == 0 {
            return Err("H264 access unit contains an empty NAL unit".to_owned());
        }
        let nal_end = length_end
            .checked_add(nal_length)
            .ok_or_else(|| "H264 NAL payload length overflowed".to_owned())?;
        let nal = frame
            .get(length_end..nal_end)
            .ok_or_else(|| "H264 access unit ends inside a NAL payload".to_owned())?;
        annex_b.extend_from_slice(&[0, 0, 0, 1]);
        annex_b.extend_from_slice(nal);
        offset = nal_end;
    }
    if annex_b.is_empty() {
        return Err("H264 access unit contains no NAL units".to_owned());
    }
    Ok(annex_b)
}

#[cfg(any(test, target_os = "windows"))]
fn normalize_h264_access_unit(frame: &[u8]) -> Result<Vec<u8>, String> {
    let starts_with_annex_b = frame.starts_with(&[0, 0, 1]) || frame.starts_with(&[0, 0, 0, 1]);
    if starts_with_annex_b && contains_only_valid_annex_b_nals(frame) {
        return Ok(frame.to_vec());
    }

    for nal_length_bytes in [4, 2, 1] {
        if let Ok(annex_b) = length_prefixed_h264_to_annex_b(frame, nal_length_bytes)
            && contains_only_valid_annex_b_nals(&annex_b)
        {
            return Ok(annex_b);
        }
    }

    Err("H264 output is neither Annex B nor valid length-prefixed NAL data".to_owned())
}

#[cfg(any(test, target_os = "windows"))]
fn contains_only_valid_annex_b_nals(frame: &[u8]) -> bool {
    let nals = super::super::media::annex_b_nals(frame);
    !nals.is_empty()
        && nals.iter().all(|nal| {
            nal.first()
                .is_some_and(|header| header & 0x80 == 0 && matches!(header & 0x1f, 1..=23))
        })
}

#[cfg(any(test, target_os = "linux", target_os = "macos", target_os = "windows"))]
fn annex_b_contains_idr(frame: &[u8]) -> bool {
    super::super::media::annex_b_nals(frame)
        .into_iter()
        .any(|nal| nal.first().is_some_and(|header| header & 0x1f == 5))
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    #[test]
    fn screen_content_encoder_configuration_initializes_cleanly() {
        let _encoder =
            Encoder::with_api_config(OpenH264API::from_source(), openh264_encoder_config())
                .expect("screen content encoder configuration should initialize");
    }

    #[test]
    fn screen_content_encoder_keeps_bitrate_and_two_second_intra_period() {
        let config = format!("{:?}", openh264_encoder_config());

        assert!(
            config.contains("intra_frame_period: IntraFramePeriod(60)"),
            "unexpected stream encoder configuration: {config}"
        );
        assert!(
            config.contains("bitrate: BitRate(8000000)"),
            "unexpected stream encoder configuration: {config}"
        );
    }

    #[test]
    fn encoder_selection_prefers_hardware_and_falls_back_only_after_failure() {
        let software_called = Cell::new(false);
        let selected = select_with_software_fallback("native", Ok("hardware"), || {
            software_called.set(true);
            Ok("software")
        })
        .expect("hardware selection should succeed");
        assert_eq!(selected.encoder, "hardware");
        assert_eq!(selected.hardware_failure, None);
        assert!(!software_called.get());

        let selected =
            select_with_software_fallback("native", Err("no device".to_owned()), || Ok("software"))
                .expect("software fallback should succeed");
        assert_eq!(selected.encoder, "software");
        assert_eq!(selected.hardware_failure.as_deref(), Some("no device"));

        let error =
            select_with_software_fallback::<&str>("native", Err("no device".to_owned()), || {
                Err("codec initialization failed".to_owned())
            })
            .err()
            .expect("failure of both encoders should be reported");
        assert!(error.contains("native: no device"));
        assert!(error.contains("OpenH264: codec initialization failed"));
    }

    #[test]
    fn i420_to_nv12_interleaves_chroma_and_respects_stride() {
        let y = [1, 2, 3, 4, 99, 5, 6, 7, 8, 99];
        let u = [10, 11, 99];
        let v = [20, 21, 99];
        let input = I420Frame::new(&y, &u, &v, 4, 2, 5, 3, 3);
        let mut destination_y = [0; 12];
        let mut destination_uv = [0; 6];

        copy_i420_to_nv12(input, &mut destination_y, &mut destination_uv, 6, 6)
            .expect("valid I420 should convert to NV12");

        assert_eq!(destination_y, [1, 2, 3, 4, 0, 0, 5, 6, 7, 8, 0, 0]);
        assert_eq!(destination_uv, [10, 20, 11, 21, 0, 0]);
    }

    #[test]
    fn i420_to_nv12_rejects_unsafe_plane_layouts() {
        let odd_width = I420Frame::new(&[0; 6], &[0; 2], &[0; 2], 3, 2, 3, 2, 2);
        let error = copy_i420_to_nv12(odd_width, &mut [0; 6], &mut [0; 3], 3, 3)
            .expect_err("odd I420 dimensions should be rejected");
        assert_eq!(error, "I420 dimensions must be non-zero and even");

        let short_y = I420Frame::new(&[0; 7], &[0; 2], &[0; 2], 4, 2, 4, 2, 2);
        let error = copy_i420_to_nv12(short_y, &mut [0; 8], &mut [0; 4], 4, 4)
            .expect_err("short source planes should be rejected");
        assert!(error.contains("source Y plane is too short"));

        let valid = I420Frame::new(&[0; 8], &[0; 2], &[0; 2], 4, 2, 4, 2, 2);
        let error = copy_i420_to_nv12(valid, &mut [0; 8], &mut [0; 3], 4, 3)
            .expect_err("short destination planes should be rejected");
        assert!(error.contains("destination UV stride is shorter"));
    }

    #[test]
    fn annex_b_keyframe_detection_accepts_three_and_four_byte_start_codes() {
        assert!(annex_b_contains_idr(&[0, 0, 1, 0x65, 1, 2]));
        assert!(annex_b_contains_idr(&[
            0, 0, 0, 1, 0x67, 1, 0, 0, 1, 0x65, 2
        ]));
        assert!(!annex_b_contains_idr(&[0, 0, 0, 1, 0x41, 1, 2]));
    }

    #[test]
    fn length_prefixed_h264_conversion_validates_and_rewrites_each_nal() {
        let cases: &[(&[u8], usize, &[u8])] = &[
            (
                &[3, 0x67, 1, 2, 2, 0x65, 3],
                1,
                &[0, 0, 0, 1, 0x67, 1, 2, 0, 0, 0, 1, 0x65, 3],
            ),
            (
                &[0, 3, 0x67, 1, 2, 0, 2, 0x65, 3],
                2,
                &[0, 0, 0, 1, 0x67, 1, 2, 0, 0, 0, 1, 0x65, 3],
            ),
            (
                &[0, 0, 0, 3, 0x67, 1, 2, 0, 0, 0, 2, 0x65, 3],
                4,
                &[0, 0, 0, 1, 0x67, 1, 2, 0, 0, 0, 1, 0x65, 3],
            ),
        ];
        for &(input, length_size, expected) in cases {
            assert_eq!(
                length_prefixed_h264_to_annex_b(input, length_size)
                    .expect("valid length-prefixed H264 should convert"),
                expected
            );
        }

        let truncated = [0, 0, 0, 4, 0x65, 1];
        assert!(
            length_prefixed_h264_to_annex_b(&truncated, 4)
                .expect_err("truncated NAL units should fail")
                .contains("ends inside a NAL payload")
        );
        assert!(
            length_prefixed_h264_to_annex_b(&[0, 0], 4)
                .expect_err("truncated length fields should fail")
                .contains("ends inside a NAL length field")
        );
        assert!(
            length_prefixed_h264_to_annex_b(&[0, 0, 0, 0], 4)
                .expect_err("empty NAL units should fail")
                .contains("empty NAL unit")
        );
        assert!(length_prefixed_h264_to_annex_b(&[], 4).is_err());
        assert!(length_prefixed_h264_to_annex_b(&[1, 0x65], 0).is_err());
    }

    #[test]
    fn h264_output_normalization_preserves_annex_b_and_converts_length_prefixed_frames() {
        let annex_b = [0, 0, 0, 1, 0x67, 1, 2, 0, 0, 1, 0x65, 3];
        assert_eq!(
            normalize_h264_access_unit(&annex_b).expect("valid Annex B should pass through"),
            annex_b
        );

        let length_prefixed_with_start_code_in_payload = [0, 0, 0, 5, 0x65, 0, 0, 1, 0x22];
        assert_eq!(
            normalize_h264_access_unit(&length_prefixed_with_start_code_in_payload)
                .expect("valid length-prefixed output should convert"),
            [0, 0, 0, 1, 0x65, 0, 0, 1, 0x22]
        );

        assert!(normalize_h264_access_unit(&[0, 0, 1, 0x80]).is_err());
        assert!(normalize_h264_access_unit(&[0, 0, 0, 9, 0x65]).is_err());
    }
}
