//! Settings panel.
//!
//! Backed by the same `config::AppOptions` the TUI reads, so a change here
//! shows up there and survives a restart. Only options this front-end actually
//! honours are exposed - listing a toggle that changes nothing would be worse
//! than omitting it.
//!
//! Options deliberately absent: audio device selection and microphone levels
//! (they need device enumeration and a live meter to be usable), push-to-talk
//! binding (needs key capture), notification and voice sounds (they are file
//! paths, so they need a picker rather than a switch), and theming (the
//! palette is compiled in). Inline image and custom-emoji rendering are not
//! implemented, so their options are not shown either - a switch that changes
//! nothing is worse than its absence. All remain editable in `config.toml`.

use gpui::{Div, prelude::*, px, rgb};

use crate::theme::{DARK, layout, space, text};
use crate::ui::chrome::{column, row};

/// One boolean setting, identified so a click can route back to the field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Toggle {
    ShowAvatars,
    CircularAvatars,
    DesktopNotifications,
    NoiseSuppression,
    ShareRichPresence,
}

impl Toggle {
    pub fn label(self) -> &'static str {
        match self {
            Toggle::ShowAvatars => "Show avatars",
            Toggle::CircularAvatars => "Circular avatars",
            Toggle::DesktopNotifications => "Desktop notifications",
            Toggle::NoiseSuppression => "Noise suppression",
            Toggle::ShareRichPresence => "Share rich presence",
        }
    }

    fn hint(self) -> Option<&'static str> {
        match self {
            Toggle::ShareRichPresence => Some("Lets others see what you are playing"),
            Toggle::NoiseSuppression => Some("Applied when joining a voice channel"),
            _ => None,
        }
    }

    /// A stable number for element ids.
    fn slot(self) -> usize {
        match self {
            Toggle::ShowAvatars => 0,
            Toggle::CircularAvatars => 1,
            Toggle::DesktopNotifications => 2,
            Toggle::NoiseSuppression => 3,
            Toggle::ShareRichPresence => 4,
        }
    }
}

/// Grouped for display. Groups mirror the config file's own sections so the
/// panel and `config.toml` stay recognisably the same thing.
pub const SECTIONS: &[(&str, &[Toggle])] = &[
    ("Display", &[Toggle::ShowAvatars, Toggle::CircularAvatars]),
    (
        "Notifications",
        // Sound options are paths, not switches; they need a file picker
        // rather than a toggle, so they are configured in config.toml.
        &[Toggle::DesktopNotifications],
    ),
    ("Voice", &[Toggle::NoiseSuppression]),
    ("Presence", &[Toggle::ShareRichPresence]),
];

/// Render the settings panel.
///
/// `value` reads the current state of a toggle; `saved_note` reports the last
/// persistence attempt, so a failed write is visible rather than silent.
pub fn settings_view(
    value: impl Fn(Toggle) -> bool,
    saved_note: Option<&str>,
    on_toggle: impl Fn(Toggle, &mut gpui::App) + Clone + 'static,
) -> impl IntoElement {
    let mut panel = column()
        .id("settings")
        .w(px(layout::MEMBERS + 120.))
        .h_full()
        .bg(rgb(DARK.surface_sunken))
        .border_l_1()
        .border_color(rgb(DARK.border))
        .overflow_y_scroll();

    panel = panel.child(
        row()
            .w_full()
            .h(px(layout::HEADER))
            .px(px(space::MD))
            .border_b_1()
            .border_color(rgb(DARK.border))
            .text_size(px(text::SM))
            .text_color(rgb(DARK.text))
            .child("Settings"),
    );

    for (title, toggles) in SECTIONS {
        panel = panel.child(
            gpui::div()
                .px(px(space::MD))
                .pt(px(space::MD))
                .pb(px(space::XS))
                .text_size(px(text::XS))
                .text_color(rgb(DARK.text_subtle))
                .child(*title),
        );

        for toggle in *toggles {
            let toggle = *toggle;
            let enabled = value(toggle);
            let handler = on_toggle.clone();

            let mut entry = column()
                .id(("setting", toggle.slot()))
                .w_full()
                .px(px(space::MD))
                .py(px(space::SM))
                .gap(px(2.))
                .cursor_pointer()
                .hover(|style| style.bg(rgb(DARK.surface_hover)))
                .on_click(move |_event, _window, cx| handler(toggle, cx))
                .child(
                    row()
                        .w_full()
                        .gap(px(space::SM))
                        .child(
                            gpui::div()
                                .flex_1()
                                .text_size(px(text::SM))
                                .text_color(rgb(DARK.text))
                                .child(toggle.label()),
                        )
                        .child(switch(enabled)),
                );

            if let Some(hint) = toggle.hint() {
                entry = entry.child(
                    gpui::div()
                        .text_size(px(text::XS))
                        .text_color(rgb(DARK.text_subtle))
                        .child(hint),
                );
            }

            panel = panel.child(entry);
        }
    }

    if let Some(note) = saved_note {
        panel = panel.child(
            gpui::div()
                .px(px(space::MD))
                .py(px(space::SM))
                .text_size(px(text::XS))
                .text_color(rgb(DARK.text_subtle))
                .child(note.to_string()),
        );
    }

    panel
}

/// A small on/off switch. Filled when on, so state reads without comparing
/// against a neighbour.
fn switch(on: bool) -> Div {
    let track = gpui::div()
        .w(px(30.))
        .h(px(16.))
        .rounded(px(8.))
        .bg(rgb(if on { DARK.accent } else { DARK.surface_active }))
        .flex()
        .items_center();

    let knob = gpui::div()
        .w(px(12.))
        .h(px(12.))
        .rounded_full()
        .bg(rgb(if on { DARK.on_accent } else { DARK.text_subtle }));

    if on {
        track.justify_end().child(knob.mr(px(2.)))
    } else {
        track.justify_start().child(knob.ml(px(2.)))
    }
}
