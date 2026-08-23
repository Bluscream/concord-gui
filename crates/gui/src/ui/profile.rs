//! User profile panel.
//!
//! Opened by clicking a member row or a message author. The panel shows what
//! the core caches: display name, handle, pronouns, bio, roles and mutual
//! guilds. Fields Discord has not supplied are omitted entirely rather than
//! rendered empty, so a sparse profile reads as sparse instead of broken.

use gpui::{Div, prelude::*, px, rgb};

use crate::theme::{active, layout, scaled, space, text};
use crate::ui::chrome::{avatar_with_url, column, row};

/// A profile projected for rendering.
pub struct ProfileView {
    pub display_name: String,
    pub handle: Option<String>,
    pub avatar: Option<String>,
    pub pronouns: Option<String>,
    pub bio: Option<String>,
    /// What they are doing, as the core words it. Ordered as Discord orders
    /// it, custom status first.
    pub activities: Vec<String>,
    pub roles: Vec<(String, Option<u32>)>,
    pub mutual_guilds: Vec<String>,
    /// False while the fetch is in flight, so the panel can say so rather than
    /// looking like an empty profile.
    pub loaded: bool,
}

pub fn profile_view<F>(profile: &ProfileView, circular_avatars: bool, on_close: Option<F>) -> Div
where
    F: Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
{
    let mut panel = column()
        .w(px(layout::MEMBERS + 80.))
        .h_full()
        .bg(rgb(active().surface_sunken))
        .border_l_1()
        .border_color(rgb(active().border))
        .overflow_hidden();

    let mut header_col = column()
        .w_full()
        .p(px(space::LG))
        .gap(px(space::SM))
        .items_center()
        .border_b_1()
        .border_color(rgb(active().border));

    if let Some(close_fn) = on_close {
        header_col = header_col.child(
            row()
                .w_full()
                .justify_end()
                .child(
                    gpui::div()
                        .id("profile-close-btn")
                        .w(px(24.))
                        .h(px(24.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(layout::RADIUS))
                        .cursor_pointer()
                        .text_size(px(14.))
                        .text_color(rgb(active().text_muted))
                        .hover(|s| s.bg(rgb(active().surface_hover)).text_color(rgb(active().text)))
                        .on_click(close_fn)
                        .child("✕")
                        .tooltip(|_window, cx| cx.new(|_| crate::ui::chrome::Tooltip::new("Close profile")).into())
                )
        );
    }

    // Header: avatar, name, handle.
    panel = panel.child(
        header_col
            .child(avatar_with_url(
                64.,
                &profile.display_name,
                profile.avatar.as_deref(),
                circular_avatars,
            ))
            .child(
                gpui::div()
                    .text_size(px(scaled(text::LG)))
                    .text_color(rgb(active().text))
                    .child(profile.display_name.clone()),
            ),
    );

    if let Some(handle) = &profile.handle {
        panel = panel.child(section("Username", handle.clone()));
    }

    // Above the bio: what someone is doing right now is the thing most worth
    // seeing, and it is the reason to open a profile at all.
    for activity in &profile.activities {
        panel = panel.child(
            gpui::div()
                .px(px(space::LG))
                .py(px(space::XS))
                .text_size(px(scaled(text::SM)))
                .text_color(rgb(active().text_muted))
                .child(activity.clone()),
        );
    }

    if let Some(pronouns) = &profile.pronouns {
        panel = panel.child(section("Pronouns", pronouns.clone()));
    }

    if let Some(bio) = &profile.bio {
        panel = panel.child(section("About", bio.clone()));
    }

    if !profile.roles.is_empty() {
        let mut chips = row().flex_wrap().gap(px(space::XS));

        for (name, color) in &profile.roles {
            chips = chips.child(
                row()
                    .gap(px(space::XS))
                    .px(px(6.))
                    .py(px(2.))
                    .rounded(px(layout::RADIUS))
                    .bg(rgb(active().surface_hover))
                    .border_1()
                    .border_color(rgb(color.unwrap_or(active().border)))
                    .text_size(px(scaled(text::XS)))
                    .text_color(rgb(color.unwrap_or(active().text_muted)))
                    .child(name.clone()),
            );
        }

        panel = panel.child(
            column()
                .w_full()
                .px(px(space::MD))
                .py(px(space::SM))
                .gap(px(space::XS))
                .child(label("Roles"))
                .child(chips),
        );
    }

    if !profile.mutual_guilds.is_empty() {
        let mut list = column().gap(px(2.));
        for guild in &profile.mutual_guilds {
            list = list.child(
                gpui::div()
                    .text_size(px(scaled(text::SM)))
                    .text_color(rgb(active().text_muted))
                    .child(guild.clone()),
            );
        }

        panel = panel.child(
            column()
                .w_full()
                .px(px(space::MD))
                .py(px(space::SM))
                .gap(px(space::XS))
                .child(label("Mutual servers"))
                .child(list),
        );
    }

    if !profile.loaded {
        panel = panel.child(
            gpui::div()
                .px(px(space::MD))
                .py(px(space::SM))
                .text_size(px(scaled(text::XS)))
                .text_color(rgb(active().text_subtle))
                .child("Loading profile…"),
        );
    }

    panel
}

fn label(text_value: &'static str) -> Div {
    gpui::div()
        .text_size(px(scaled(text::XS)))
        .text_color(rgb(active().text_subtle))
        .child(text_value)
}

fn section(title: &'static str, body: String) -> Div {
    column()
        .w_full()
        .px(px(space::MD))
        .py(px(space::SM))
        .gap(px(space::XS))
        .child(label(title))
        .child(
            gpui::div()
                .text_size(px(scaled(text::SM)))
                .text_color(rgb(active().text))
                .child(body),
        )
}
