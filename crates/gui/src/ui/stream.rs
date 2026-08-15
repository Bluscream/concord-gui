//! Screenshare broadcast controls.
//!
//! The core implements capture and encoding (PipeWire + xdg-desktop-portal on
//! Linux, with hardware encode on Windows/macOS); this is only the picker and
//! the running-state control.
//!
//! Two constraints are surfaced rather than hidden, because both change what
//! the user should expect:
//!
//! * Capture requires the `stream-broadcast` feature. Without it the core has
//!   no capture path at all, so the button says so instead of failing on click.
//! * Watching someone else's stream opens `mpv` upstream rather than rendering
//!   in-process, so it is not offered here as though it were an in-app view.

use concord::discord::{StreamCaptureTarget, StreamCaptureTargetKind};
use gpui::{Div, prelude::*, px, rgb};

use crate::theme::{active, layout, scaled, space, text};
use crate::ui::chrome::{column, row};

/// Picker state for choosing what to broadcast.
pub struct StreamPicker {
    pub targets: Vec<StreamCaptureTarget>,
    /// Set while the enumeration request is outstanding.
    pub loading: bool,
    /// Reason enumeration failed, if it did.
    pub error: Option<String>,
}

impl StreamPicker {
    pub fn loading() -> Self {
        Self {
            targets: Vec::new(),
            loading: true,
            error: None,
        }
    }
}

fn kind_label(kind: StreamCaptureTargetKind) -> &'static str {
    match kind {
        StreamCaptureTargetKind::Display => "Screen",
        StreamCaptureTargetKind::Window => "Window",
        // The portal picks the source itself, so the label promises nothing
        // about what will be captured.
        StreamCaptureTargetKind::Portal => "Choose…",
    }
}

/// Render the capture-target picker.
pub fn picker_view(
    picker: &StreamPicker,
    on_pick: impl Fn(usize, &mut gpui::App) + Clone + 'static,
    on_cancel: impl Fn(&mut gpui::App) + Clone + 'static,
) -> Div {
    let mut panel = column()
        .w(px(360.))
        .p(px(space::MD))
        .gap(px(space::SM))
        .rounded(px(layout::RADIUS_LG))
        .bg(rgb(active().surface))
        .border_1()
        .border_color(rgb(active().border))
        .child(
            gpui::div()
                .text_size(px(scaled(text::SM)))
                .text_color(rgb(active().text))
                .child("Share a screen or window"),
        );

    if picker.loading {
        return panel.child(
            gpui::div()
                .text_size(px(scaled(text::XS)))
                .text_color(rgb(active().text_subtle))
                .child("Finding capture sources…"),
        );
    }

    if let Some(error) = &picker.error {
        panel = panel.child(
            gpui::div()
                .text_size(px(scaled(text::XS)))
                .text_color(rgb(active().danger))
                .child(error.clone()),
        );
    } else if picker.targets.is_empty() {
        // On Linux this usually means xdg-desktop-portal is missing, which is
        // worth naming: the user can install it and retry.
        panel = panel.child(
            gpui::div()
                .text_size(px(scaled(text::XS)))
                .text_color(rgb(active().text_subtle))
                .child("No capture sources. On Linux this needs xdg-desktop-portal."),
        );
    }

    let mut list = column()
        .id("stream-targets")
        .max_h(px(260.))
        .gap(px(2.))
        .overflow_y_scroll();

    for (index, target) in picker.targets.iter().enumerate() {
        let handler = on_pick.clone();
        list = list.child(
            row()
                .id(("stream-target", index))
                .w_full()
                .gap(px(space::SM))
                .px(px(space::SM))
                .py(px(space::XS))
                .rounded(px(layout::RADIUS))
                .cursor_pointer()
                .hover(|style| style.bg(rgb(active().surface_hover)))
                .child(
                    gpui::div()
                        .w(px(52.))
                        .text_size(px(scaled(text::XS)))
                        .text_color(rgb(active().text_subtle))
                        .child(kind_label(target.kind)),
                )
                .child(
                    gpui::div()
                        .flex_1()
                        .text_size(px(scaled(text::SM)))
                        .text_color(rgb(active().text))
                        .child(target.title.clone()),
                )
                .on_click(move |_event, _window, cx| handler(index, cx)),
        );
    }

    panel.child(list).child(
        gpui::div()
            .id("stream-cancel")
            .px(px(space::SM))
            .py(px(space::XS))
            .rounded(px(layout::RADIUS))
            .cursor_pointer()
            .text_size(px(scaled(text::XS)))
            .text_color(rgb(active().text_muted))
            .hover(|style| style.bg(rgb(active().surface_hover)))
            .child("Cancel")
            .on_click(move |_event, _window, cx| on_cancel(cx)),
    )
}

/// The share control in the voice bar.
///
/// `available` is false when the build lacks capture support, in which case
/// the control explains itself rather than failing on click.
pub fn share_button(broadcasting: bool, available: bool) -> Div {
    let (label, tone) = match (available, broadcasting) {
        (false, _) => ("share n/a", active().text_subtle),
        (true, true) => ("stop share", active().danger),
        (true, false) => ("share", active().text_muted),
    };

    gpui::div()
        .px(px(space::SM))
        .py(px(space::XS))
        .rounded(px(layout::RADIUS))
        .text_size(px(scaled(text::XS)))
        .bg(rgb(if broadcasting {
            active().surface_active
        } else {
            active().surface_hover
        }))
        .text_color(rgb(tone))
        .child(label)
}
