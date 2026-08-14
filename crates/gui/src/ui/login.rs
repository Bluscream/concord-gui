//! Login screen.
//!
//! Only token entry is implemented. Upstream also supports email/password and
//! QR (`concord::discord::qr_auth`), but password login involves captcha and
//! MFA flows that need dedicated UI; offering a half-working password form
//! would be worse than offering none.
//!
//! The token is held in memory only for as long as it takes to start the
//! session, and is echoed masked so it cannot be read off a shared screen or
//! a screenshot.

use gpui::{Div, prelude::*, px, rgb};

use crate::theme::{DARK, layout, space, text};
use crate::ui::chrome::{column, row};
use crate::ui::composer::Composer;

/// Login screen state.
#[derive(Default)]
pub struct Login {
    pub input: Composer,
    pub error: Option<String>,
    /// Set while a session is starting, to prevent duplicate submissions.
    pub connecting: bool,
    /// Whether to persist the token to the configured credential store.
    pub remember: bool,
}

impl Login {
    pub fn token(&self) -> &str {
        self.input.text()
    }

    /// Discord tokens are opaque, but an empty or obviously-truncated value is
    /// worth rejecting before a round trip.
    pub fn is_submittable(&self) -> bool {
        !self.connecting && self.input.text().trim().len() >= 20
    }
}

/// Mask a token for display: enough to confirm what was pasted, not enough to
/// leak it.
fn masked(token: &str) -> String {
    let visible = token.chars().take(4).collect::<String>();
    if token.is_empty() {
        String::new()
    } else if token.len() <= 4 {
        "•".repeat(token.len())
    } else {
        format!("{visible}{}", "•".repeat(token.chars().count() - 4))
    }
}

pub fn login_view(login: &Login) -> Div {
    let card = column()
        .w(px(420.))
        .gap(px(space::LG))
        .p(px(space::XL))
        .rounded(px(layout::RADIUS_LG))
        .bg(rgb(DARK.surface))
        .border_1()
        .border_color(rgb(DARK.border))
        .child(
            gpui::div()
                .text_size(px(text::XL))
                .text_color(rgb(DARK.text))
                .child("concord"),
        )
        .child(
            gpui::div()
                .text_size(px(text::SM))
                .text_color(rgb(DARK.text_muted))
                .child("Paste a user token to connect."),
        )
        .child(
            // Input field.
            row()
                .w_full()
                .min_h(px(40.))
                .px(px(space::MD))
                .py(px(space::SM))
                .rounded(px(layout::RADIUS))
                .bg(rgb(DARK.surface_sunken))
                .border_1()
                .border_color(rgb(if login.error.is_some() {
                    DARK.danger
                } else {
                    DARK.accent
                }))
                .text_size(px(text::BASE))
                .child(if login.input.text().is_empty() {
                    gpui::div().text_color(rgb(DARK.text_subtle)).child("Token")
                } else {
                    gpui::div()
                        .text_color(rgb(DARK.text))
                        .child(masked(login.input.text()))
                }),
        );

    let card = if let Some(error) = &login.error {
        card.child(
            gpui::div()
                .text_size(px(text::SM))
                .text_color(rgb(DARK.danger))
                .child(error.clone()),
        )
    } else {
        card
    };

    let card = card
        .child(
            gpui::div()
                .text_size(px(text::XS))
                .text_color(rgb(DARK.text_subtle))
                .child(if login.remember {
                    "Enter to connect  ·  ctrl-r: will be saved to the credential store"
                } else {
                    "Enter to connect  ·  ctrl-r: remember this token"
                }),
        )
        .child(
            gpui::div()
                .text_size(px(text::XS))
                .text_color(rgb(DARK.text_subtle))
                .child(if login.connecting {
                    "Connecting…"
                } else {
                    "Third-party clients are against Discord's ToS; use at your own risk."
                }),
        );

    column()
        .size_full()
        .items_center()
        .justify_center()
        .bg(rgb(DARK.bg))
        .child(card)
}
