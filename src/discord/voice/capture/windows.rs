use std::{
    ffi::c_void,
    mem, slice,
    sync::{
        Arc, LazyLock, Mutex,
        atomic::AtomicBool,
        mpsc::{self, Receiver, SyncSender, TrySendError},
    },
};

use windows::{
    Foundation::TypedEventHandler,
    Graphics::{
        Capture::{
            Direct3D11CaptureFrame, Direct3D11CaptureFramePool, GraphicsCaptureItem,
            GraphicsCaptureSession,
        },
        DirectX::{Direct3D11::IDirect3DDevice, DirectXPixelFormat},
        SizeInt32,
    },
    Win32::{
        Foundation::{HMODULE, HWND, LPARAM, RECT, TRUE},
        Graphics::{
            Direct3D::D3D_DRIVER_TYPE_HARDWARE,
            Direct3D11::{
                D3D11_BOX, D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAP_READ,
                D3D11_MAPPED_SUBRESOURCE, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC,
                D3D11_USAGE_STAGING, D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext,
                ID3D11Resource, ID3D11Texture2D,
            },
            Dwm::{DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute},
            Dxgi::IDXGIDevice,
            Gdi::{
                EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO, MONITORINFOEXW,
            },
        },
        System::WinRT::{
            Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess},
            Graphics::Capture::IGraphicsCaptureItemInterop,
            RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize,
        },
        UI::WindowsAndMessaging::{
            EnumWindows, GetWindowTextLengthW, GetWindowTextW, IsIconic, IsWindowVisible,
        },
    },
    core::{BOOL, IInspectable, Interface, factory},
};

use super::CaptureFrame;
use crate::discord::voice::{StreamCaptureTarget, StreamCaptureTargetKind};

const FRAME_QUEUE_CAPACITY: usize = 2;

static D3D_DEVICE: LazyLock<Result<ID3D11Device, String>> = LazyLock::new(create_d3d_device);
static D3D_CONTEXT: LazyLock<Result<ID3D11DeviceContext, String>> = LazyLock::new(|| unsafe {
    d3d_device()?
        .GetImmediateContext()
        .map_err(|error| format!("D3D11 context creation failed: {error}"))
});

pub(super) struct CaptureSession {
    runtime: Option<WgcRuntime>,
    _apartment: WinRtApartment,
}

struct WinRtApartment;

struct WgcRuntime {
    frame_pool: Direct3D11CaptureFramePool,
    session: GraphicsCaptureSession,
    frame_arrived_token: i64,
    closed: bool,
}

pub(super) fn list_targets() -> Result<Vec<StreamCaptureTarget>, String> {
    let mut targets = Vec::new();

    for (index, monitor) in enumerate_monitors()?.into_iter().enumerate() {
        let info = monitor_info(monitor)?;
        let name = utf16_string(&info.szDevice);
        let rect = info.monitorInfo.rcMonitor;
        let width = (rect.right - rect.left).max(0);
        let height = (rect.bottom - rect.top).max(0);
        let name = if name.is_empty() {
            format!("Display {}", index + 1)
        } else {
            name
        };
        targets.push(StreamCaptureTarget {
            kind: StreamCaptureTargetKind::Display,
            id: monitor.0 as usize as u64,
            title: format!("Screen: {name} ({width}x{height})"),
        });
    }

    for window in enumerate_windows()? {
        let Ok(title) = window_title(window) else {
            continue;
        };
        if title.trim().is_empty() {
            continue;
        }
        targets.push(StreamCaptureTarget {
            kind: StreamCaptureTargetKind::Window,
            id: window.0 as usize as u64,
            title: format!("Window: {}", title.trim()),
        });
    }

    Ok(targets)
}

pub(super) fn start_capture(
    target: &StreamCaptureTarget,
    _stop: &AtomicBool,
) -> Result<(CaptureSession, Receiver<Result<CaptureFrame, String>>), String> {
    let apartment = WinRtApartment::initialize()?;
    let item = capture_item(target)?;
    let (frames_tx, frames_rx) = mpsc::sync_channel(FRAME_QUEUE_CAPACITY);
    let runtime = WgcRuntime::start(item, frames_tx)?;
    Ok((
        CaptureSession {
            runtime: Some(runtime),
            _apartment: apartment,
        },
        frames_rx,
    ))
}

impl CaptureSession {
    pub(super) fn stop(&mut self) -> Result<(), String> {
        let Some(mut runtime) = self.runtime.take() else {
            return Ok(());
        };
        runtime.close()
    }
}

impl WinRtApartment {
    fn initialize() -> Result<Self, String> {
        unsafe { RoInitialize(RO_INIT_MULTITHREADED) }
            .map_err(|error| format!("Windows Runtime initialization failed: {error}"))?;
        Ok(Self)
    }
}

impl Drop for WinRtApartment {
    fn drop(&mut self) {
        unsafe { RoUninitialize() };
    }
}

impl WgcRuntime {
    fn start(
        item: GraphicsCaptureItem,
        frames_tx: SyncSender<Result<CaptureFrame, String>>,
    ) -> Result<Self, String> {
        let size = item
            .Size()
            .map_err(|error| format!("WGC capture size lookup failed: {error}"))?;
        if size.Width <= 0 || size.Height <= 0 {
            return Err("WGC capture source has invalid dimensions".to_owned());
        }

        let winrt_d3d_device = direct3d_device()?;
        let frame_pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
            &winrt_d3d_device,
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            2,
            size,
        )
        .map_err(|error| format!("WGC frame pool creation failed: {error}"))?;

        let current_size = Arc::new(Mutex::new(size));
        let callback_size = Arc::clone(&current_size);
        let frame_arrived_token = frame_pool
            .FrameArrived(
                &TypedEventHandler::<Direct3D11CaptureFramePool, IInspectable>::new(
                    move |frame_pool, _| {
                        let Some(frame_pool) = frame_pool.as_ref() else {
                            return Ok(());
                        };
                        let result = capture_next_frame(frame_pool);
                        let next_size = result.as_ref().ok().map(|(_, size)| *size);
                        send_latest(&frames_tx, result.map(|(frame, _)| frame));

                        if let Some(next_size) = next_size {
                            let mut current_size = callback_size
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner);
                            if current_size.Width != next_size.Width
                                || current_size.Height != next_size.Height
                            {
                                let recreate = direct3d_device().and_then(|device| {
                                    frame_pool
                                        .Recreate(
                                            &device,
                                            DirectXPixelFormat::B8G8R8A8UIntNormalized,
                                            2,
                                            next_size,
                                        )
                                        .map_err(|error| error.to_string())
                                });
                                if let Err(error) = recreate {
                                    send_latest(
                                        &frames_tx,
                                        Err(format!("WGC frame pool resize failed: {error}")),
                                    );
                                } else {
                                    *current_size = next_size;
                                }
                            }
                        }
                        Ok(())
                    },
                ),
            )
            .map_err(|error| format!("WGC frame callback setup failed: {error}"))?;
        let session = frame_pool
            .CreateCaptureSession(&item)
            .map_err(|error| format!("WGC capture session creation failed: {error}"))?;
        let _ = session.SetIsBorderRequired(false);
        let _ = session.SetIsCursorCaptureEnabled(true);
        session
            .StartCapture()
            .map_err(|error| format!("WGC capture start failed: {error}"))?;

        Ok(Self {
            frame_pool,
            session,
            frame_arrived_token,
            closed: false,
        })
    }

    fn close(&mut self) -> Result<(), String> {
        if self.closed {
            return Ok(());
        }
        self.closed = true;
        let mut errors = Vec::new();
        if let Err(error) = self.frame_pool.RemoveFrameArrived(self.frame_arrived_token) {
            errors.push(format!("remove frame callback: {error}"));
        }
        if let Err(error) = self.session.Close() {
            errors.push(format!("close capture session: {error}"));
        }
        if let Err(error) = self.frame_pool.Close() {
            errors.push(format!("close frame pool: {error}"));
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(format!("WGC shutdown failed: {}", errors.join(", ")))
        }
    }
}

impl Drop for WgcRuntime {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

fn capture_item(target: &StreamCaptureTarget) -> Result<GraphicsCaptureItem, String> {
    let interop = factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()
        .map_err(|error| format!("WGC capture item factory failed: {error}"))?;
    match target.kind {
        StreamCaptureTargetKind::Display => {
            let monitor = HMONITOR(target.id as usize as *mut c_void);
            unsafe { interop.CreateForMonitor::<GraphicsCaptureItem>(monitor) }
                .map_err(|error| format!("WGC screen capture item creation failed: {error}"))
        }
        StreamCaptureTargetKind::Window => {
            let window = HWND(target.id as usize as *mut c_void);
            unsafe { interop.CreateForWindow::<GraphicsCaptureItem>(window) }
                .map_err(|error| format!("WGC window capture item creation failed: {error}"))
        }
        StreamCaptureTargetKind::Portal => {
            Err("portal capture targets are only valid on Linux".to_owned())
        }
    }
}

fn capture_next_frame(
    frame_pool: &Direct3D11CaptureFramePool,
) -> Result<(CaptureFrame, SizeInt32), String> {
    let frame = frame_pool
        .TryGetNextFrame()
        .map_err(|error| format!("WGC frame retrieval failed: {error}"))?;
    let result = copy_wgc_frame(&frame);
    let close_result = frame.Close();
    match (result, close_result) {
        (Ok(frame), Ok(())) => Ok(frame),
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(format!("WGC frame close failed: {error}")),
    }
}

fn copy_wgc_frame(frame: &Direct3D11CaptureFrame) -> Result<(CaptureFrame, SizeInt32), String> {
    let content_size = frame
        .ContentSize()
        .map_err(|error| format!("WGC frame size lookup failed: {error}"))?;
    let width =
        u32::try_from(content_size.Width).map_err(|_| "WGC frame width is invalid".to_owned())?;
    let height =
        u32::try_from(content_size.Height).map_err(|_| "WGC frame height is invalid".to_owned())?;
    if width == 0 || height == 0 {
        return Err("WGC returned an empty frame".to_owned());
    }

    let surface = frame
        .Surface()
        .map_err(|error| format!("WGC frame surface lookup failed: {error}"))?;
    let access = surface
        .cast::<IDirect3DDxgiInterfaceAccess>()
        .map_err(|error| format!("WGC DXGI surface conversion failed: {error}"))?;
    let source_texture = unsafe { access.GetInterface::<ID3D11Texture2D>() }
        .map_err(|error| format!("WGC D3D11 texture lookup failed: {error}"))?;
    let rgba = texture_to_rgba(
        d3d_device()?,
        d3d_context()?,
        &source_texture,
        width,
        height,
    )?;
    Ok((
        CaptureFrame {
            width,
            height,
            rgba,
        },
        content_size,
    ))
}

fn create_d3d_device() -> Result<ID3D11Device, String> {
    let mut device = None;
    unsafe {
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            None,
        )
    }
    .map_err(|error| format!("D3D11 device creation failed: {error}"))?;
    device.ok_or_else(|| "D3D11 returned no device".to_owned())
}

fn d3d_device() -> Result<&'static ID3D11Device, String> {
    D3D_DEVICE.as_ref().map_err(Clone::clone)
}

fn d3d_context() -> Result<&'static ID3D11DeviceContext, String> {
    D3D_CONTEXT.as_ref().map_err(Clone::clone)
}

fn direct3d_device() -> Result<IDirect3DDevice, String> {
    let dxgi_device = d3d_device()?
        .cast::<IDXGIDevice>()
        .map_err(|error| format!("D3D11 DXGI device conversion failed: {error}"))?;
    let inspectable = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi_device) }
        .map_err(|error| format!("WinRT D3D11 device creation failed: {error}"))?;
    inspectable
        .cast::<IDirect3DDevice>()
        .map_err(|error| format!("WinRT D3D11 device conversion failed: {error}"))
}

fn texture_to_rgba(
    d3d_device: &ID3D11Device,
    d3d_context: &ID3D11DeviceContext,
    source_texture: &ID3D11Texture2D,
    width: u32,
    height: u32,
) -> Result<Vec<u8>, String> {
    let mut source_description = D3D11_TEXTURE2D_DESC::default();
    unsafe { source_texture.GetDesc(&mut source_description) };
    if width > source_description.Width || height > source_description.Height {
        return Err("WGC frame dimensions exceed its D3D11 texture".to_owned());
    }

    let mut staging_description = source_description;
    staging_description.Width = width;
    staging_description.Height = height;
    staging_description.BindFlags = 0;
    staging_description.MiscFlags = 0;
    staging_description.Usage = D3D11_USAGE_STAGING;
    staging_description.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
    let mut staging_texture = None;
    unsafe { d3d_device.CreateTexture2D(&staging_description, None, Some(&mut staging_texture)) }
        .map_err(|error| format!("WGC staging texture creation failed: {error}"))?;
    let staging_texture =
        staging_texture.ok_or_else(|| "D3D11 returned no staging texture".to_owned())?;

    let region = D3D11_BOX {
        left: 0,
        top: 0,
        right: width,
        bottom: height,
        front: 0,
        back: 1,
    };
    unsafe {
        d3d_context.CopySubresourceRegion(
            Some(
                &staging_texture
                    .cast()
                    .map_err(|error| format!("WGC staging resource conversion failed: {error}"))?,
            ),
            0,
            0,
            0,
            0,
            Some(
                &source_texture
                    .cast()
                    .map_err(|error| format!("WGC source resource conversion failed: {error}"))?,
            ),
            0,
            Some(&region),
        );
    }

    let resource = staging_texture
        .cast::<ID3D11Resource>()
        .map_err(|error| format!("WGC mapped resource conversion failed: {error}"))?;
    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    unsafe { d3d_context.Map(Some(&resource), 0, D3D11_MAP_READ, 0, Some(&mut mapped)) }
        .map_err(|error| format!("WGC staging texture map failed: {error}"))?;
    let result = copy_mapped_bgra(&mapped, width, height);
    unsafe { d3d_context.Unmap(Some(&resource), 0) };
    result
}

fn copy_mapped_bgra(
    mapped: &D3D11_MAPPED_SUBRESOURCE,
    width: u32,
    height: u32,
) -> Result<Vec<u8>, String> {
    if mapped.pData.is_null() || mapped.RowPitch < width.saturating_mul(4) {
        return Err("WGC returned invalid mapped texture data".to_owned());
    }
    let row_length = usize::try_from(width)
        .ok()
        .and_then(|width| width.checked_mul(4))
        .ok_or_else(|| "WGC frame row size overflowed".to_owned())?;
    let output_length = row_length
        .checked_mul(height as usize)
        .ok_or_else(|| "WGC frame size overflowed".to_owned())?;
    let mut rgba = vec![0; output_length];
    let source = mapped.pData.cast::<u8>();
    for row in 0..height as usize {
        let source = unsafe {
            slice::from_raw_parts(source.add(row * mapped.RowPitch as usize), row_length)
        };
        let destination = &mut rgba[row * row_length..(row + 1) * row_length];
        for (bgra, rgba) in source.chunks_exact(4).zip(destination.chunks_exact_mut(4)) {
            rgba.copy_from_slice(&[bgra[2], bgra[1], bgra[0], bgra[3]]);
        }
    }
    Ok(rgba)
}

fn enumerate_monitors() -> Result<Vec<HMONITOR>, String> {
    extern "system" fn callback(
        monitor: HMONITOR,
        _device_context: HDC,
        _rect: *mut RECT,
        state: LPARAM,
    ) -> BOOL {
        unsafe {
            (*(state.0 as *mut Vec<HMONITOR>)).push(monitor);
        }
        TRUE
    }

    let mut monitors = Vec::new();
    unsafe {
        EnumDisplayMonitors(
            None,
            None,
            Some(callback),
            LPARAM((&mut monitors as *mut Vec<HMONITOR>) as isize),
        )
    }
    .ok()
    .map_err(|error| format!("Windows screen enumeration failed: {error}"))?;
    Ok(monitors)
}

fn monitor_info(monitor: HMONITOR) -> Result<MONITORINFOEXW, String> {
    let mut info = MONITORINFOEXW::default();
    info.monitorInfo.cbSize = mem::size_of::<MONITORINFOEXW>() as u32;
    unsafe {
        GetMonitorInfoW(
            monitor,
            (&mut info as *mut MONITORINFOEXW).cast::<MONITORINFO>(),
        )
    }
    .ok()
    .map_err(|error| format!("Windows screen information lookup failed: {error}"))?;
    Ok(info)
}

fn enumerate_windows() -> Result<Vec<HWND>, String> {
    extern "system" fn callback(window: HWND, state: LPARAM) -> BOOL {
        if is_capturable_window(window) {
            unsafe {
                (*(state.0 as *mut Vec<HWND>)).push(window);
            }
        }
        TRUE
    }

    let mut windows = Vec::new();
    unsafe {
        EnumWindows(
            Some(callback),
            LPARAM((&mut windows as *mut Vec<HWND>) as isize),
        )
    }
    .map_err(|error| format!("Windows window enumeration failed: {error}"))?;
    Ok(windows)
}

fn is_capturable_window(window: HWND) -> bool {
    if !unsafe { IsWindowVisible(window).as_bool() } || unsafe { IsIconic(window).as_bool() } {
        return false;
    }
    if unsafe { GetWindowTextLengthW(window) } <= 0 {
        return false;
    }
    let mut cloaked = 0u32;
    let cloaked_result = unsafe {
        DwmGetWindowAttribute(
            window,
            DWMWA_CLOAKED,
            (&mut cloaked as *mut u32).cast(),
            mem::size_of::<u32>() as u32,
        )
    };
    if cloaked_result.is_ok() && cloaked != 0 {
        return false;
    }
    let mut rect = RECT::default();
    if unsafe {
        DwmGetWindowAttribute(
            window,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            (&mut rect as *mut RECT).cast(),
            mem::size_of::<RECT>() as u32,
        )
    }
    .is_err()
    {
        return false;
    }
    rect.right - rect.left >= 2 && rect.bottom - rect.top >= 2
}

fn window_title(window: HWND) -> Result<String, String> {
    let length = unsafe { GetWindowTextLengthW(window) };
    if length <= 0 {
        return Ok(String::new());
    }
    let mut buffer = vec![0u16; length as usize + 1];
    let written = unsafe { GetWindowTextW(window, &mut buffer) };
    if written <= 0 {
        return Err("Windows window title lookup failed".to_owned());
    }
    buffer.truncate(written as usize);
    Ok(String::from_utf16_lossy(&buffer))
}

fn utf16_string(value: &[u16]) -> String {
    let length = value
        .iter()
        .position(|character| *character == 0)
        .unwrap_or(value.len());
    String::from_utf16_lossy(&value[..length])
}

fn send_latest(
    frames_tx: &SyncSender<Result<CaptureFrame, String>>,
    frame: Result<CaptureFrame, String>,
) {
    if frame.is_err() {
        let _ = frames_tx.send(frame);
        return;
    }
    match frames_tx.try_send(frame) {
        Ok(()) | Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {}
    }
}
