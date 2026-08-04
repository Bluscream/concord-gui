use std::{
    os::fd::OwnedFd,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender, SyncSender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use ashpd::desktop::{
    PersistMode, Session,
    screencast::{
        CursorMode, Screencast, SelectSourcesOptions, SourceType, Stream as PortalStream,
    },
};
use pipewire as pw;
use pw::{properties::properties, spa};
use spa::pod::Pod;

use super::{
    CaptureFrame, CaptureFrameBufferPool, CaptureOutput, STREAM_CAPTURE_FPS, send_capture_result,
};
use crate::{
    discord::voice::{StreamCaptureTarget, StreamCaptureTargetKind},
    logging,
};

const FRAME_QUEUE_CAPACITY: usize = 2;
const START_TIMEOUT: Duration = Duration::from_secs(5);
const CANCEL_CLOSE_TIMEOUT: Duration = Duration::from_secs(1);

pub(super) struct CaptureSession {
    stop_tx: pw::channel::Sender<()>,
    stopping: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    portal_runtime: tokio::runtime::Runtime,
    portal_session: Option<Session<Screencast>>,
}

struct PipeWireState {
    format: spa::param::video::VideoInfoRaw,
    frames_tx: SyncSender<CaptureFrame>,
    errors_tx: Sender<String>,
    buffer_pool: CaptureFrameBufferPool,
    ready_tx: Option<SyncSender<Result<(), String>>>,
}

impl PipeWireState {
    fn report_error(&mut self, error: String) {
        logging::debug(
            "stream",
            format!("PipeWire video capture reported an error: {error}"),
        );
        let _ = self.errors_tx.send(error.clone());
        if let Some(ready_tx) = self.ready_tx.take() {
            let _ = ready_tx.send(Err(error));
        }
    }

    fn queue_frame(&mut self, frame: CaptureFrame) {
        send_capture_result(&self.frames_tx, &self.errors_tx, Ok(frame));
        if let Some(ready_tx) = self.ready_tx.take() {
            logging::debug(
                "stream",
                "PipeWire video capture produced its first valid frame",
            );
            let _ = ready_tx.send(Ok(()));
        }
    }
}

struct PortalCapture {
    session: Session<Screencast>,
    stream: PortalStream,
    remote_fd: OwnedFd,
}

struct PipeWirePortal {
    stream: PortalStream,
    remote_fd: OwnedFd,
}

pub(super) fn list_targets() -> Result<Vec<StreamCaptureTarget>, String> {
    Ok(vec![StreamCaptureTarget {
        kind: StreamCaptureTargetKind::Portal,
        id: 0,
        title: "Screen or window...".to_owned(),
    }])
}

pub(super) fn start_capture(
    target: &StreamCaptureTarget,
    stop: &AtomicBool,
) -> Result<(CaptureSession, CaptureOutput), String> {
    if target.kind != StreamCaptureTargetKind::Portal {
        return Err("Linux screen sharing requires a portal capture target".to_owned());
    }

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("screen cast portal runtime creation failed: {error}"))?;
    let portal = runtime.block_on(open_portal(stop))?;
    let PortalCapture {
        session: portal_session,
        stream,
        remote_fd,
    } = portal;
    let pipewire_portal = PipeWirePortal { stream, remote_fd };

    let (frames_tx, frames_rx) = mpsc::sync_channel(FRAME_QUEUE_CAPACITY);
    let (errors_tx, errors_rx) = mpsc::channel();
    let buffer_pool = CaptureFrameBufferPool::default();
    let (stop_tx, stop_rx) = pw::channel::channel();
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let stopping = Arc::new(AtomicBool::new(false));
    let worker_stopping = Arc::clone(&stopping);
    let worker = thread::Builder::new()
        .name("stream-pipewire-video".to_owned())
        .spawn(move || {
            let result = run_pipewire_capture(
                pipewire_portal,
                frames_tx.clone(),
                errors_tx.clone(),
                buffer_pool,
                stop_rx,
                ready_tx.clone(),
            );
            match result {
                Ok(()) if worker_stopping.load(Ordering::Acquire) => {
                    logging::debug("stream", "PipeWire video worker stopped on request");
                }
                Ok(()) => {
                    let error = "PipeWire video worker stopped unexpectedly".to_owned();
                    logging::debug("stream", &error);
                    let _ = ready_tx.try_send(Err(error.clone()));
                    let _ = errors_tx.send(error);
                }
                Err(error) if worker_stopping.load(Ordering::Acquire) => {
                    logging::debug(
                        "stream",
                        format!("PipeWire video worker stopped during shutdown: {error}"),
                    );
                }
                Err(error) => {
                    logging::debug("stream", format!("PipeWire video worker failed: {error}"));
                    let _ = ready_tx.try_send(Err(error.clone()));
                    let _ = errors_tx.send(error);
                }
            }
        })
        .map_err(|error| format!("PipeWire video worker spawn failed: {error}"));
    let worker = match worker {
        Ok(worker) => worker,
        Err(error) => {
            let _ = runtime.block_on(portal_session.close());
            return Err(error);
        }
    };

    match wait_for_pipewire_start(&ready_rx, stop) {
        Ok(()) => Ok((
            CaptureSession {
                stop_tx,
                stopping,
                worker: Some(worker),
                portal_runtime: runtime,
                portal_session: Some(portal_session),
            },
            CaptureOutput {
                frames: frames_rx,
                errors: errors_rx,
            },
        )),
        Err(error) => {
            stopping.store(true, Ordering::Release);
            let _ = stop_tx.send(());
            let _ = worker.join();
            let _ = runtime.block_on(portal_session.close());
            Err(error)
        }
    }
}

fn wait_for_pipewire_start(
    ready_rx: &Receiver<Result<(), String>>,
    stop: &AtomicBool,
) -> Result<(), String> {
    let deadline = Instant::now() + START_TIMEOUT;
    loop {
        if stop.load(Ordering::Acquire) {
            return Err("screen cast portal selection was cancelled".to_owned());
        }
        let now = Instant::now();
        if now >= deadline {
            return Err("PipeWire video capture did not start in time".to_owned());
        }
        let wait = (deadline - now).min(Duration::from_millis(20));
        match ready_rx.recv_timeout(wait) {
            Ok(result) => return result,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err("PipeWire video capture stopped during startup".to_owned());
            }
        }
    }
}

impl CaptureSession {
    pub(super) fn stop(&mut self) -> Result<(), String> {
        self.stopping.store(true, Ordering::Release);
        let _ = self.stop_tx.send(());
        let worker_result = self.worker.take().map_or(Ok(()), |worker| {
            worker
                .join()
                .map_err(|error| format!("PipeWire video worker panicked: {error:?}"))
        });
        let portal_result = self.portal_session.take().map_or(Ok(()), |session| {
            logging::debug("stream", "closing screen cast portal session");
            let result = self
                .portal_runtime
                .block_on(session.close())
                .map_err(|error| format!("screen cast portal session close failed: {error}"));
            if result.is_ok() {
                logging::debug("stream", "screen cast portal session closed");
            }
            result
        });
        worker_result.and(portal_result)
    }
}

async fn open_portal(stop: &AtomicBool) -> Result<PortalCapture, String> {
    let cancellation = wait_for_capture_cancellation(stop);
    tokio::pin!(cancellation);
    logging::debug("stream", "connecting to screen cast portal");
    // The default ashpd proxy caches its D-Bus connection process-wide. Our
    // capture runtime is per session, so that cached connection stops being
    // driven after the first runtime is dropped. Bind a fresh connection to
    // each capture runtime so later broadcasts can open another portal.
    let connection = tokio::select! {
        _ = &mut cancellation => return Err("screen cast portal selection was cancelled".to_owned()),
        result = ashpd::zbus::Connection::session() => result
            .map_err(|error| format!("screen cast portal connection failed: {error}"))?,
    };
    let proxy = tokio::select! {
        _ = &mut cancellation => return Err("screen cast portal selection was cancelled".to_owned()),
        result = Screencast::with_connection(connection) => result
            .map_err(|error| format!("screen cast portal proxy creation failed: {error}"))?,
    };
    logging::debug("stream", "screen cast portal connected");
    let session = tokio::select! {
        _ = &mut cancellation => return Err("screen cast portal selection was cancelled".to_owned()),
        result = proxy.create_session(Default::default()) => result
            .map_err(|error| format!("screen cast portal session creation failed: {error}"))?,
    };
    logging::debug("stream", "screen cast portal session created");
    let select_sources = proxy.select_sources(
        &session,
        SelectSourcesOptions::default()
            .set_cursor_mode(CursorMode::Embedded)
            .set_sources(SourceType::Monitor | SourceType::Window)
            .set_multiple(false)
            .set_persist_mode(PersistMode::DoNot),
    );
    tokio::select! {
        _ = &mut cancellation => {
            close_cancelled_portal_session(&session).await;
            return Err("screen cast portal selection was cancelled".to_owned());
        }
        result = select_sources => result
            .map_err(|error| format!("screen cast source selection failed: {error}"))?,
    };
    logging::debug("stream", "waiting for screen cast portal source selection");

    let start = proxy.start(&session, None, Default::default());
    let response = tokio::select! {
        _ = &mut cancellation => {
            close_cancelled_portal_session(&session).await;
            return Err("screen cast portal selection was cancelled".to_owned());
        }
        result = start => result
            .map_err(|error| format!("screen cast portal start request failed: {error}"))?
            .response()
            .map_err(|error| format!("screen cast portal start failed: {error}"))?,
    };
    logging::debug("stream", "screen cast portal source selected");
    let stream = response
        .streams()
        .first()
        .cloned()
        .ok_or_else(|| "screen cast portal returned no selected source".to_owned())?;
    let remote_fd = tokio::select! {
        _ = &mut cancellation => {
            close_cancelled_portal_session(&session).await;
            return Err("screen cast portal selection was cancelled".to_owned());
        }
        result = proxy.open_pipe_wire_remote(&session, Default::default()) => result
            .map_err(|error| format!("screen cast PipeWire remote open failed: {error}"))?,
    };
    logging::debug("stream", "screen cast PipeWire remote opened");

    Ok(PortalCapture {
        session,
        stream,
        remote_fd,
    })
}

async fn wait_for_capture_cancellation(stop: &AtomicBool) {
    while !stop.load(Ordering::Acquire) {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn close_cancelled_portal_session(session: &Session<Screencast>) {
    match tokio::time::timeout(CANCEL_CLOSE_TIMEOUT, session.close()).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => logging::debug(
            "stream",
            format!("cancelled screen cast portal session close failed: {error}"),
        ),
        Err(_) => logging::debug(
            "stream",
            "cancelled screen cast portal session close timed out",
        ),
    }
}

fn run_pipewire_capture(
    portal: PipeWirePortal,
    frames_tx: SyncSender<CaptureFrame>,
    errors_tx: Sender<String>,
    buffer_pool: CaptureFrameBufferPool,
    stop_rx: pw::channel::Receiver<()>,
    ready_tx: SyncSender<Result<(), String>>,
) -> Result<(), String> {
    pw::init();
    let mainloop = pw::main_loop::MainLoopRc::new(None)
        .map_err(|error| format!("PipeWire main loop creation failed: {error}"))?;
    let context = pw::context::ContextRc::new(&mainloop, None).map_err(|error| {
        format!(
            "PipeWire context creation failed: {error}. Ensure the PipeWire client configuration is installed"
        )
    })?;
    let core = context
        .connect_fd_rc(portal.remote_fd, None)
        .map_err(|error| format!("PipeWire portal connection failed: {error}"))?;
    let stream = pw::stream::StreamRc::new(
        core,
        "concord-screen-capture",
        properties! {
            *pw::keys::MEDIA_TYPE => "Video",
            *pw::keys::MEDIA_CATEGORY => "Capture",
            *pw::keys::MEDIA_ROLE => "Screen",
        },
    )
    .map_err(|error| format!("PipeWire video stream creation failed: {error}"))?;

    let stop_mainloop = mainloop.clone();
    let _stop_listener = stop_rx.attach(mainloop.loop_(), move |_| stop_mainloop.quit());
    let state_error_mainloop = mainloop.clone();
    let format_error_mainloop = mainloop.clone();
    let frame_error_mainloop = mainloop.clone();
    let state = PipeWireState {
        format: Default::default(),
        frames_tx,
        errors_tx,
        buffer_pool,
        ready_tx: Some(ready_tx.clone()),
    };
    let _stream_listener = stream
        .add_local_listener_with_user_data(state)
        .state_changed(move |_, state, old, new| {
            logging::debug(
                "stream",
                format!("PipeWire video stream state changed: {old:?} -> {new:?}"),
            );
            if let pw::stream::StreamState::Error(error) = new {
                state.report_error(format!("PipeWire video stream failed: {error}"));
                state_error_mainloop.quit();
            }
        })
        .param_changed(move |_, state, id, param| {
            let Some(param) = param else {
                return;
            };
            if id != spa::param::ParamType::Format.as_raw() {
                return;
            }
            let (media_type, media_subtype) = match spa::param::format_utils::parse_format(param) {
                Ok(format) => format,
                Err(error) => {
                    state.report_error(format!(
                        "PipeWire video media format parse failed: {error}"
                    ));
                    format_error_mainloop.quit();
                    return;
                }
            };
            if media_type != spa::param::format::MediaType::Video
                || media_subtype != spa::param::format::MediaSubtype::Raw
            {
                state.report_error(format!(
                    "PipeWire negotiated an unsupported media format: {media_type:?}/{media_subtype:?}"
                ));
                format_error_mainloop.quit();
                return;
            }
            match state.format.parse(param) {
                Ok(_) => {
                    let size = state.format.size();
                    let framerate = state.format.framerate();
                    logging::debug(
                        "stream",
                        format!(
                            "PipeWire video format negotiated: format={:?} width={} height={} framerate={}/{}",
                            state.format.format(),
                            size.width,
                            size.height,
                            framerate.num,
                            framerate.denom,
                        ),
                    );
                }
                Err(error) => {
                    state.report_error(format!("PipeWire video format parse failed: {error}"));
                    format_error_mainloop.quit();
                }
            }
        })
        .process(move |stream, state| {
            let Some(mut buffer) = stream.dequeue_buffer() else {
                return;
            };
            let Some(data) = buffer.datas_mut().first_mut() else {
                return;
            };
            match pipewire_frame(data, state.format, &state.buffer_pool) {
                Some(Ok(frame)) => state.queue_frame(frame),
                Some(Err(error)) => {
                    state.report_error(error);
                    frame_error_mainloop.quit();
                }
                None => {}
            }
        })
        .register()
        .map_err(|error| format!("PipeWire video listener setup failed: {error}"))?;

    let size = portal.stream.size().unwrap_or((1280, 720));
    let width = u32::try_from(size.0.max(2)).unwrap_or(1280);
    let height = u32::try_from(size.1.max(2)).unwrap_or(720);
    let maximum_width = width.max(8192);
    let maximum_height = height.max(4320);
    let format = spa::pod::object!(
        spa::utils::SpaTypes::ObjectParamFormat,
        spa::param::ParamType::EnumFormat,
        spa::pod::property!(
            spa::param::format::FormatProperties::MediaType,
            Id,
            spa::param::format::MediaType::Video
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::MediaSubtype,
            Id,
            spa::param::format::MediaSubtype::Raw
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::VideoFormat,
            Choice,
            Enum,
            Id,
            spa::param::video::VideoFormat::RGBA,
            spa::param::video::VideoFormat::RGBA,
            spa::param::video::VideoFormat::RGBx,
            spa::param::video::VideoFormat::BGRA,
            spa::param::video::VideoFormat::BGRx,
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::VideoSize,
            Choice,
            Range,
            Rectangle,
            spa::utils::Rectangle { width, height },
            spa::utils::Rectangle {
                width: 2,
                height: 2,
            },
            spa::utils::Rectangle {
                width: maximum_width,
                height: maximum_height,
            }
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::VideoFramerate,
            Choice,
            Range,
            Fraction,
            spa::utils::Fraction {
                num: STREAM_CAPTURE_FPS,
                denom: 1,
            },
            spa::utils::Fraction { num: 0, denom: 1 },
            spa::utils::Fraction { num: 360, denom: 1 }
        ),
    );
    let values = spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &spa::pod::Value::Object(format),
    )
    .map_err(|error| format!("PipeWire video format serialization failed: {error}"))?
    .0
    .into_inner();
    let mut params = [Pod::from_bytes(&values).expect("serialized video format is valid")];
    stream
        .connect(
            spa::utils::Direction::Input,
            Some(portal.stream.pipe_wire_node_id()),
            pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS,
            &mut params,
        )
        .map_err(|error| format!("PipeWire video stream connection failed: {error}"))?;

    logging::debug(
        "stream",
        format!(
            "PipeWire video stream connection requested: node_id={} preferred_size={}x{}",
            portal.stream.pipe_wire_node_id(),
            width,
            height,
        ),
    );
    mainloop.run();
    logging::debug("stream", "PipeWire video main loop stopped");
    let _ = ready_tx.try_send(Err(
        "PipeWire video capture stopped before producing its first frame".to_owned(),
    ));
    Ok(())
}

fn pipewire_frame(
    data: &mut spa::buffer::Data,
    format: spa::param::video::VideoInfoRaw,
    buffer_pool: &CaptureFrameBufferPool,
) -> Option<Result<CaptureFrame, String>> {
    let width = format.size().width;
    let height = format.size().height;
    let video_format = format.format();
    if width == 0 || height == 0 || video_format == spa::param::video::VideoFormat::Unknown {
        return None;
    }

    let chunk = data.chunk();
    let offset = isize::try_from(chunk.offset()).ok()?;
    let stride = if chunk.stride() == 0 {
        isize::try_from(width.checked_mul(4)?).ok()?
    } else {
        isize::try_from(chunk.stride()).ok()?
    };
    let row_length = usize::try_from(width.checked_mul(4)?).ok()?;
    if stride.unsigned_abs() < row_length {
        return Some(Err(
            "PipeWire video frame has an invalid row stride".to_owned()
        ));
    }
    let bytes = data.data()?;
    let output_length = row_length.checked_mul(height as usize)?;
    let mut rgba = buffer_pool.take(output_length);

    for row in 0..height as usize {
        let source_offset = offset.checked_add(stride.checked_mul(row as isize)?)?;
        let source_offset = match usize::try_from(source_offset) {
            Ok(offset) => offset,
            Err(_) => {
                return Some(Err(
                    "PipeWire video frame has a negative row offset".to_owned()
                ));
            }
        };
        let source_end = source_offset.checked_add(row_length)?;
        if source_end > bytes.len() {
            return Some(Err(
                "PipeWire video frame is shorter than expected".to_owned()
            ));
        }
        let source = &bytes[source_offset..source_end];
        let destination = &mut rgba[row * row_length..(row + 1) * row_length];
        match video_format {
            spa::param::video::VideoFormat::RGBA => destination.copy_from_slice(source),
            spa::param::video::VideoFormat::RGBx => {
                destination.copy_from_slice(source);
                for alpha in destination.iter_mut().skip(3).step_by(4) {
                    *alpha = 255;
                }
            }
            spa::param::video::VideoFormat::BGRA | spa::param::video::VideoFormat::BGRx => {
                for (source, destination) in
                    source.chunks_exact(4).zip(destination.chunks_exact_mut(4))
                {
                    destination.copy_from_slice(&[
                        source[2],
                        source[1],
                        source[0],
                        if video_format == spa::param::video::VideoFormat::BGRA {
                            source[3]
                        } else {
                            255
                        },
                    ]);
                }
            }
            _ => {
                return Some(Err(format!(
                    "PipeWire negotiated an unsupported video format: {video_format:?}"
                )));
            }
        }
    }

    Some(Ok(CaptureFrame::new(
        width,
        height,
        rgba,
        buffer_pool.clone(),
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn pending_portal_wait_observes_capture_cancellation() {
        let stop = AtomicBool::new(true);

        tokio::time::timeout(
            Duration::from_millis(100),
            wait_for_capture_cancellation(&stop),
        )
        .await
        .expect("capture cancellation should wake the portal wait");
    }

    #[test]
    fn first_valid_pipewire_frame_completes_startup_once() {
        let (frames_tx, frames_rx) = mpsc::sync_channel(2);
        let (errors_tx, _errors_rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let buffer_pool = CaptureFrameBufferPool::default();
        let mut state = PipeWireState {
            format: Default::default(),
            frames_tx,
            errors_tx,
            buffer_pool: buffer_pool.clone(),
            ready_tx: Some(ready_tx),
        };

        state.queue_frame(CaptureFrame::new(
            1,
            1,
            vec![1, 2, 3, 255],
            buffer_pool.clone(),
        ));
        assert_eq!(
            ready_rx
                .recv()
                .expect("first frame should report readiness"),
            Ok(())
        );
        assert_eq!(
            frames_rx
                .recv()
                .expect("first frame should remain queued")
                .rgba,
            vec![1, 2, 3, 255]
        );

        state.queue_frame(CaptureFrame::new(1, 1, vec![4, 5, 6, 255], buffer_pool));
        assert!(matches!(
            ready_rx.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));
    }
}
