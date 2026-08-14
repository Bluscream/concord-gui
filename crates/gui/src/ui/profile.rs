//! User profile panel.
//!
//! Opened by clicking a member row or a message author. The panel shows what
//! the core caches: display name, handle, pronouns, bio, roles and mutual
//! guilds. Fields Discord has not supplied are omitted entirely rather than
//! rendered empty, so a sparse profile reads as sparse instead of broken.

use gpui::{Div, prelude::*, px, rgb};

use crate::theme::{DARK, layout, space, text};
use crate::ui::chrome::{avatar_with_url, column, row};

/// A profile projected for rendering.
pub struct ProfileView {
    pub display_name: String,
    pub handle: Option<String>,
    pub avatar: Option<String>,
    pub pronouns: Option<String>,
    pub bio: Option<String>,
    pub roles: Vec<(String, Option<u32>)>,
    pub mutual_guilds: Vec<String>,
    /// False while the fetch is in flight, so the panel can say so rather than
    /// looking like an empty profile.
    pub loaded: bool,
}

pub fn profile_view(profile: &ProfileView) -> Div {
    let mut panel = column()
        .w(px(layout::MEMBERS + 80.))
        .h_full()
        .bg(rgb(DARK.surface_sunken))
        .border_l_1()
        .border_color(rgb(DARK.border))
        .overflow_hidden();

    // Header: avatar, name, handle.
    panel = panel.child(
        column()
            .w_full()
            .p(px(space::LG))
            .gap(px(space::SM))
            .items_center()
            .border_b_1()
            .border_color(rgb(DARK.border))
            .child(avatar_with_url(
                64.,
                &profile.display_name,
                profile.avatar.as_deref(),
            ))
            .child(
                gpui::div()
                    .text_size(px(text::LG))
                    .text_color(rgb(DARK.text))
                    .child(profile.display_name.clone()),
            ),
    );

    if let Some(handle) = &profile.handle {
        panel = panel.child(section("Username", handle.clone()));
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
                    .bg(rgb(DARK.surface_hover))
                    .border_1()
                    .border_color(rgb(color.unwrap_or(DARK.border)))
                    .text_size(px(text::XS))
                    .text_color(rgb(color.unwrap_or(DARK.text_muted)))
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
                    .text_size(px(text::SM))
                    .text_color(rgb(DARK.text_muted))
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
                .text_size(px(text::XS))
                .text_color(rgb(DARK.text_subtle))
                .child("Loading profile…"),
        );
    }

    panel
}

fn label(text_value: &'static str) -> Div {
    gpui::div()
        .text_size(px(text::XS))
        .text_color(rgb(DARK.text_subtle))
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
                .text_size(px(text::SM))
                .text_color(rgb(DARK.text))
                .child(body),
        )
}
