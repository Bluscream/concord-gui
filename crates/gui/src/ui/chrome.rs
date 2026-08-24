//! Reusable primitives shared by every view.
//!
//! These exist so that views describe *intent* ("a sidebar row that is
//! selected") rather than repeating colour and spacing literals. Consistency
//! here is what keeps the interface coherent as surfaces are added.

use gpui::{Div, prelude::*, px, rgb};

use crate::theme::{Presence, active, layout, scaled, space, text};

/// A vertical divider-free column that fills its parent.
pub fn column() -> Div {
    gpui::div().flex().flex_col()
}

/// A horizontal row, vertically centred.
pub fn row() -> Div {
    gpui::div().flex().flex_row().items_center()
}

/// Sunken panel used for the sidebars.
pub fn panel_sunken(width: f32) -> Div {
    column()
        .w(px(width))
        .h_full()
        .bg(rgb(active().surface_sunken))
        .border_r_1()
        .border_color(rgb(active().border))
}

/// A section label above a group of sidebar rows.
pub fn section_label(title: impl Into<gpui::SharedString>) -> Div {
    gpui::div()
        .px(px(space::MD))
        .pt(px(space::MD))
        .pb(px(space::XS))
        .text_size(px(scaled(text::XS)))
        .text_color(rgb(active().text_subtle))
        .child(title.into())
}

/// A selectable row in a sidebar (channel, DM, guild).
pub fn sidebar_row(selected: bool) -> Div {
    let base = row()
        .w_full()
        .h(px(32.))
        .px(px(space::SM))
        .mx(px(space::XS))
        .gap(px(space::SM))
        .rounded(px(layout::RADIUS))
        .text_size(px(scaled(text::SM)));

    if selected {
        base.bg(rgb(active().surface_active))
            .text_color(rgb(active().text))
    } else {
        base.text_color(rgb(active().text_muted)).hover(|s| {
            s.bg(rgb(active().surface_hover))
                .text_color(rgb(active().text))
        })
    }
}

/// A small circular presence dot.
pub fn presence_dot(presence: Presence) -> Div {
    gpui::div()
        .w(px(8.))
        .h(px(8.))
        .rounded_full()
        .bg(rgb(presence.color(active())))
}

/// Circular avatar.
///
/// `url` renders the real image; without one a deterministic tinted initial is
/// drawn instead. The fallback is always laid out first so a slow or failed
/// fetch leaves the layout unchanged rather than collapsing the row.
pub fn avatar_with_url(
    size: f32,
    seed: &str,
    url: Option<&str>,
    circular: bool,
) -> gpui::AnyElement {
    let radius = if circular { size / 2. } else { layout::RADIUS };
    let fallback = avatar(size, seed).rounded(px(radius));

    match url {
        Some(url) => gpui::div()
            .w(px(size))
            .h(px(size))
            .rounded(px(radius))
            .overflow_hidden()
            .child(
                gpui::img(gpui::SharedUri::from(url.to_string()))
                    .w(px(size))
                    .h(px(size))
                    .rounded(px(radius)),
            )
            .into_any_element(),
        None => fallback.into_any_element(),
    }
}

/// Deterministic tinted initial, used when no avatar URL is available.
pub fn avatar(size: f32, seed: &str) -> Div {
    // Deterministic hue per user so adjacent avatars stay distinguishable
    // without any network fetch.
    let hue = seed.bytes().fold(0u32, |a, b| a.wrapping_add(b as u32)) % 360;
    let tint = match hue % 6 {
        0 => 0x4f6d9a,
        1 => 0x5a5f8a,
        2 => 0x6b5a8a,
        3 => 0x8a5a6b,
        4 => 0x5a8a75,
        _ => 0x8a7a5a,
    };

    gpui::div()
        .w(px(size))
        .h(px(size))
        .rounded_full()
        .bg(rgb(tint))
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(size * 0.4))
        .text_color(rgb(0xffffff))
        .child(
            if seed == "Direct Messages" {
                "DM".to_string()
            } else {
                seed.chars()
                    .next()
                    .unwrap_or('?')
                    .to_uppercase()
                    .to_string()
            },
        )
}

/// Top bar for the main content area.
pub fn header() -> Div {
    row()
        .w_full()
        .h(px(layout::HEADER))
        .px(px(space::LG))
        .gap(px(space::SM))
        .bg(rgb(active().surface))
        .border_b_1()
        .border_color(rgb(active().border))
}

/// What a voice participant row displays.
pub struct VoiceRow<'a> {
    pub name: &'a str,
    pub muted: bool,
    pub deafened: bool,
    pub streaming: bool,
    /// Whether their camera is on. Separate from `streaming`, which is a
    /// shared screen - someone can be doing both at once, and one badge for
    /// both would say the wrong thing about either.
    pub on_camera: bool,
    pub speaking: bool,
    /// Distinguishes this row's element ids from every other participant's.
    pub id_seed: u64,
}

/// A voice participant nested under its channel in the sidebar.
pub fn voice_participant_row(
    row_data: VoiceRow<'_>,
    on_watch: impl Fn(&mut gpui::App) + 'static,
    on_toggle_mute: impl Fn(&mut gpui::App) + 'static,
) -> Div {
    let VoiceRow {
        name,
        muted,
        deafened,
        streaming,
        on_camera,
        speaking,
        id_seed,
    } = row_data;

    row()
        .w_full()
        .h(px(24.))
        .pl(px(space::XL))
        .pr(px(space::SM))
        .gap(px(space::SM))
        .text_size(px(scaled(text::XS)))
        .child(avatar(layout::AVATAR_SM, name))
        .child(
            gpui::div()
                .flex_1()
                // Speaking is shown by brightening the name rather than by a
                // ring, which would reflow the row on every voice activity.
                .text_color(rgb(if speaking {
                    active().success
                } else {
                    active().text_muted
                }))
                .child(name.to_string()),
        )
        .when(on_camera, |d| {
            // A word rather than a camera glyph: the obvious one is outside
            // the Basic Multilingual Plane and draws as an empty box in the
            // font this ships, which a test here checks for.
            d.child(
                gpui::div()
                    .px(px(space::XS))
                    .text_color(rgb(active().text_muted))
                    .child("cam"),
            )
        })
        .when(streaming, |d| {
            // "live" is the label; the click target is the whole badge, since
            // watching is the only thing a viewer wants from a live marker.
            d.child(
                gpui::div()
                    .id(("watch-stream", id_seed))
                    .px(px(space::XS))
                    .rounded(px(layout::RADIUS))
                    .cursor_pointer()
                    .text_color(rgb(active().accent))
                    .hover(|style| style.bg(rgb(active().surface_hover)))
                    .child("live")
                    .on_click(move |_event, _window, cx| on_watch(cx)),
            )
        })
        .when(muted || deafened, |d| {
            d.child(
                gpui::div()
                    .text_color(rgb(active().danger))
                    .child(if deafened { "deaf" } else { "mute" }),
            )
        })
        // Local mute, offered on every participant: it is about what this
        // client plays, so it applies whether or not they muted themselves.
        .child(
            gpui::div()
                .id(("local-mute", id_seed))
                .px(px(space::XS))
                .rounded(px(layout::RADIUS))
                .cursor_pointer()
                .text_color(rgb(active().text_subtle))
                .hover(|style| style.text_color(rgb(active().text)))
                .child("vol")
                .on_click(move |_event, _window, cx| on_toggle_mute(cx)),
        )
}

/// A hover tooltip.
///
/// GPUI supplies the mechanism but no view, so this is the one every tooltip
/// in the client uses - one place to style them, and one place to make sure
/// the text goes through translation.
pub struct Tooltip {
    text: gpui::SharedString,
}

impl Tooltip {
    pub fn new(text: impl Into<gpui::SharedString>) -> Self {
        Self { text: text.into() }
    }
}

impl gpui::Render for Tooltip {
    fn render(
        &mut self,
        _window: &mut gpui::Window,
        _cx: &mut gpui::Context<Self>,
    ) -> impl IntoElement {
        gpui::div()
            .px(px(space::SM))
            .py(px(space::XS))
            .rounded(px(layout::RADIUS))
            .bg(rgb(active().surface_sunken))
            .border_1()
            .border_color(rgb(active().border))
            .text_size(px(scaled(text::XS)))
            .text_color(rgb(active().text))
            .child(self.text.clone())
    }
}

/// An icon button: a glyph, a tooltip, and an active state.
///
/// Glyphs must come from the Basic Multilingual Plane. Emoji such as U+1F3A4
/// render as empty boxes here - the shipped fonts have no colour-emoji
/// coverage - which is what the first version of the voice controls did.
/// Geometric and technical symbols (U+25xx, U+26xx, U+27xx) are safe.
pub fn icon_button(
    id: &'static str,
    glyph: &'static str,
    tooltip: impl Into<gpui::SharedString>,
    active_state: bool,
) -> gpui::Stateful<Div> {
    let tooltip = tooltip.into();
    gpui::div()
        .id(id)
        .w(px(28.))
        .h(px(28.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(layout::RADIUS))
        .cursor_pointer()
        .text_size(px(scaled(text::BASE)))
        .bg(rgb(if active_state {
            active().danger
        } else {
            active().surface_sunken
        }))
        .text_color(rgb(if active_state {
            active().on_accent
        } else {
            active().text_muted
        }))
        .hover(|style| style.bg(rgb(active().surface_hover)))
        .child(glyph)
        .tooltip(move |_window, cx| cx.new(|_| Tooltip::new(tooltip.clone())).into())
}

/// A presence swatch with a tooltip, for picking a status.
///
/// Drawn rather than lettered: a filled circle always renders, where the
/// obvious emoji for it does not.
pub fn presence_swatch(
    id: &'static str,
    presence: Presence,
    tooltip: impl Into<gpui::SharedString>,
    selected: bool,
) -> gpui::Stateful<Div> {
    let tooltip = tooltip.into();
    gpui::div()
        .id(id)
        .w(px(20.))
        .h(px(20.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(layout::RADIUS))
        .cursor_pointer()
        .when(selected, |swatch| swatch.bg(rgb(active().surface_active)))
        .hover(|style| style.bg(rgb(active().surface_hover)))
        .child(
            gpui::div()
                .w(px(10.))
                .h(px(10.))
                .rounded_full()
                .bg(rgb(presence.color(active())))
                // An unselected status is dimmed rather than hidden, so the
                // row reads as a set of choices with one of them current.
                .when(!selected, |dot| dot.opacity(0.55)),
        )
        .tooltip(move |_window, cx| cx.new(|_| Tooltip::new(tooltip.clone())).into())
}
