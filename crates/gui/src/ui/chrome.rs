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
            seed.chars()
                .next()
                .unwrap_or('?')
                .to_uppercase()
                .to_string(),
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

/// A voice participant nested under its channel in the sidebar.
pub fn voice_participant_row(
    name: &str,
    muted: bool,
    deafened: bool,
    streaming: bool,
    speaking: bool,
    // Distinguishes this row's element ids from every other participant's.
    id_seed: u64,
    on_watch: impl Fn(&mut gpui::App) + 'static,
    on_toggle_mute: impl Fn(&mut gpui::App) + 'static,
) -> Div {
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
