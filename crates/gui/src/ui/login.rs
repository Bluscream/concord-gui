//! Login screen — multi-method picker.
//!
//! Flow:
//!   Picker  →  Password  →  (MfaSelect → MfaCode)  → session
//!           →  Token     →                          → session
//!           →  QrScan    →                          → session
//!           →  Demo      →                          → session (fixture)
//!
//! Key routing is done entirely in `workspace.rs`; this module owns only
//! state and rendering.

use concord::discord::{
    password_auth::{MfaChallenge, MfaMethod},
    qr_auth::QrEvent,
};
use gpui::{Context, Div, prelude::*, px, rgb};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::theme::{active, layout, space, text};
use crate::ui::chrome::{column, row};
use crate::ui::composer::Composer;
use crate::ui::workspace::{LoginAction, Workspace};

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// Which sub-screen is currently showing.
#[derive(Default, PartialEq, Eq, Clone, Copy)]
pub enum LoginScreen {
    #[default]
    Picker,
    Password,
    MfaSelect,
    MfaCode,
    Token,
    QrScan,
}

/// Password-method sub-state.
#[derive(Default)]
pub struct PasswordState {
    pub login: Composer,
    pub password: Composer,
    pub focused_field: PasswordField,
    pub mfa: Option<MfaChallenge>,
    pub mfa_method: Option<MfaMethod>,
    pub mfa_code: Composer,
    pub in_progress: bool,
    pub status: String,
}

#[derive(Default, PartialEq, Eq, Clone, Copy)]
pub enum PasswordField {
    #[default]
    Login,
    Password,
}

impl PasswordField {
    pub fn next(self) -> Self {
        match self {
            Self::Login => Self::Password,
            Self::Password => Self::Login,
        }
    }
}

impl PasswordState {
    pub fn is_submittable(&self) -> bool {
        !self.in_progress && !self.login.is_empty() && !self.password.is_empty()
    }

    pub fn is_mfa_submittable(&self) -> bool {
        !self.in_progress && !self.mfa_code.is_empty()
    }

    /// Wipe sensitive fields after they have been consumed.
    pub fn reset_sensitive(&mut self) {
        self.password.clear();
        self.mfa_code.clear();
        self.mfa = None;
        self.mfa_method = None;
        self.in_progress = false;
        self.status.clear();
    }
}

/// QR-method sub-state.
#[derive(Default)]
pub struct QrState {
    pub bitmap: Option<Vec<Vec<bool>>>,
    pub status: String,
    pub pending_user: Option<String>,
}

impl QrState {
    pub fn reset(&mut self) {
        self.bitmap = None;
        self.status.clear();
        self.pending_user = None;
    }
}

/// Active async handles for in-flight auth tasks.
pub struct LoginHandle {
    pub rx: mpsc::Receiver<LoginEvent>,
    pub join: JoinHandle<()>,
}

/// Unified event type that the UI drains from `LoginHandle`.
pub enum LoginEvent {
    /// Password / MFA auth events.
    Password(concord::discord::password_auth::PasswordAuthEvent),
    /// QR auth events.
    Qr(QrEvent),
}

/// Top-level login state, owned by `Workspace`.
#[derive(Default)]
pub struct Login {
    pub screen: LoginScreen,

    // Per-method state.
    pub password: PasswordState,
    pub token: Composer,
    pub qr: QrState,

    /// Whether to persist the successfully-obtained token.
    pub remember: bool,

    /// Error shown at the bottom of any sub-screen.
    pub error: Option<String>,

    /// Active async task. `None` when idle.
    pub handle: Option<LoginHandle>,
}

impl Login {
    /// True when the token input has enough characters to attempt a connection.
    pub fn token_submittable(&self) -> bool {
        let t = self.token.text().trim();
        if t.is_empty() {
            return false;
        }
        if crate::demo::is_demo_token(t) {
            return true;
        }
        t.len() >= 20
    }
}

// ---------------------------------------------------------------------------
// Rendering helpers
// ---------------------------------------------------------------------------

/// A full-width method button.
fn method_btn(
    label: &'static str,
    subtitle: &str,
    accent_color: u32,
    action: LoginAction,
    cx: &mut Context<Workspace>,
) -> gpui::Stateful<Div> {
    row()
        .id(label)
        .w_full()
        .min_h(px(56.))
        .px(px(space::LG))
        .gap(px(space::MD))
        .rounded(px(layout::RADIUS_LG))
        .bg(rgb(active().surface_sunken))
        .border_1()
        .border_color(rgb(active().border))
        .cursor_pointer()
        .hover(|d| {
            d.bg(rgb(active().surface_hover))
                .border_color(rgb(accent_color))
        })
        .on_click(cx.listener(move |this, _event, window, cx| {
            this.handle_login_action(action, window, cx);
        }))
        .child(
            gpui::div()
                .w(px(4.))
                .h(px(32.))
                .rounded_full()
                .bg(rgb(accent_color)),
        )
        .child(
            column()
                .gap(px(2.))
                .child(
                    gpui::div()
                        .text_size(px(text::BASE))
                        .text_color(rgb(active().text))
                        .child(label.to_string()),
                )
                .child(
                    gpui::div()
                        .text_size(px(text::XS))
                        .text_color(rgb(active().text_subtle))
                        .child(subtitle.to_string()),
                ),
        )
}

/// A labelled text input row (used in both password and token screens).
/// How much of a field's value to show.
///
/// The right answer differs per secret, so it is stated explicitly rather than
/// left to a single `masked` flag:
///
/// * `Full` - passwords. Human-chosen and short, so revealing even a prefix
///   materially helps anyone reading over a shoulder or a screenshot.
/// * `Prefix` - tokens. Long and opaque; showing four characters confirms the
///   right value was pasted while leaking almost nothing.
/// * `None` - MFA codes. Short, single-use and typed from another screen, so
///   masking only makes typos harder to spot without protecting anything.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mask {
    None,
    Prefix,
    Full,
}

/// Apply a mask policy to a value.
fn apply_mask(value: &str, mask: Mask) -> String {
    match mask {
        Mask::None => value.to_string(),
        Mask::Full => "•".repeat(value.chars().count()),
        Mask::Prefix if value.is_empty() => String::new(),
        Mask::Prefix => {
            let visible: String = value.chars().take(4).collect();
            format!(
                "{}{}",
                visible,
                "•".repeat(value.chars().count().saturating_sub(4))
            )
        }
    }
}

#[cfg(test)]
fn mask_for_test(value: &str, mask: Mask) -> String {
    apply_mask(value, mask)
}

fn input_row(label: &str, value: &str, focused: bool, mask: Mask, error: bool) -> Div {
    let display = apply_mask(value, mask);

    column()
        .w_full()
        .gap(px(space::XS))
        .child(
            gpui::div()
                .text_size(px(text::XS))
                .text_color(rgb(active().text_muted))
                .child(label.to_string()),
        )
        .child(
            row()
                .w_full()
                .min_h(px(38.))
                .px(px(space::MD))
                .py(px(space::SM))
                .rounded(px(layout::RADIUS))
                .bg(rgb(active().surface_sunken))
                .border_1()
                .border_color(rgb(if error {
                    active().danger
                } else if focused {
                    active().accent
                } else {
                    active().border
                }))
                .text_size(px(text::BASE))
                .child(if value.is_empty() {
                    gpui::div()
                        .text_color(rgb(active().text_subtle))
                        .child(label.to_string())
                } else {
                    gpui::div().text_color(rgb(active().text)).child(display)
                }),
        )
}

/// Back button shown at the top-left of every sub-screen.
fn back_button(cx: &mut Context<Workspace>) -> gpui::Stateful<Div> {
    row()
        .id("back_btn")
        .gap(px(space::XS))
        .py(px(space::XS))
        .px(px(space::SM))
        .rounded(px(layout::RADIUS))
        .text_size(px(text::SM))
        .text_color(rgb(active().text_muted))
        .cursor_pointer()
        .hover(|d| {
            d.bg(rgb(active().surface_hover))
                .text_color(rgb(active().text))
        })
        .on_click(cx.listener(move |this, _event, window, cx| {
            this.handle_login_action(LoginAction::Back, window, cx);
        }))
        .child(gpui::div().child("← Back"))
}

/// Primary action button.
fn action_btn(
    label: &'static str,
    enabled: bool,
    action: LoginAction,
    cx: &mut Context<Workspace>,
) -> gpui::Stateful<Div> {
    let base = row()
        .id(label)
        .w_full()
        .h(px(40.))
        .items_center()
        .justify_center()
        .rounded(px(layout::RADIUS_LG))
        .bg(rgb(if enabled {
            active().accent
        } else {
            active().surface_active
        }))
        .text_size(px(text::BASE))
        .text_color(rgb(if enabled {
            active().on_accent
        } else {
            active().text_subtle
        }))
        .child(label.to_string());

    if enabled {
        base.cursor_pointer()
            .hover(|s| s.bg(rgb(active().accent_hover)))
            .on_click(cx.listener(move |this, _event, window, cx| {
                this.handle_login_action(action, window, cx);
            }))
    } else {
        base
    }
}

/// Render the QR bitmap using half-block Unicode so each pair of rows becomes
/// one line (upper-half ▀, lower-half ▄, full █, space).
fn qr_bitmap_view(bitmap: &[Vec<bool>]) -> Div {
    let n = bitmap.len();
    let mut lines: Vec<String> = Vec::new();

    let mut y = 0usize;
    while y < n {
        let row_a = &bitmap[y];
        let row_b = if y + 1 < n {
            Some(&bitmap[y + 1])
        } else {
            None
        };
        let mut line = String::new();
        for x in 0..row_a.len() {
            let top = row_a[x];
            let bot = row_b.map(|r| r[x]).unwrap_or(false);
            line.push(match (top, bot) {
                (true, true) => '█',
                (true, false) => '▀',
                (false, true) => '▄',
                (false, false) => ' ',
            });
        }
        lines.push(line);
        y += 2;
    }

    let mut col = column()
        .p(px(space::MD))
        .rounded(px(layout::RADIUS))
        .bg(rgb(0xffffff)); // QR codes need a white background
    for line in lines {
        col = col.child(
            gpui::div()
                .text_size(px(9.))
                .text_color(rgb(0x000000))
                .child(line),
        );
    }
    col
}

// ---------------------------------------------------------------------------
// Main render entry
// ---------------------------------------------------------------------------

pub fn login_view(login: &Login, cx: &mut Context<Workspace>) -> Div {
    let content = match login.screen {
        LoginScreen::Picker => picker_view(login, cx),
        LoginScreen::Password => password_view(login, cx),
        LoginScreen::MfaSelect => mfa_select_view(login, cx),
        LoginScreen::MfaCode => mfa_code_view(login, cx),
        LoginScreen::Token => token_view(login, cx),
        LoginScreen::QrScan => qr_view(login, cx),
    };

    column()
        .size_full()
        .items_center()
        .justify_center()
        .bg(rgb(active().bg))
        .child(content)
}

// -- Picker ------------------------------------------------------------------

fn picker_view(login: &Login, cx: &mut Context<Workspace>) -> Div {
    let card = column()
        .w(px(400.))
        .gap(px(space::LG))
        .p(px(space::XL))
        .rounded(px(layout::RADIUS_LG))
        .bg(rgb(active().surface))
        .border_1()
        .border_color(rgb(active().border))
        // Logo + title
        .child(
            column()
                .gap(px(space::XS))
                .child(
                    gpui::div()
                        .text_size(px(24.))
                        .text_color(rgb(active().text))
                        .child("concord"),
                )
                .child(
                    gpui::div()
                        .text_size(px(text::SM))
                        .text_color(rgb(active().text_muted))
                        .child("Choose how to log in"),
                ),
        )
        // Method buttons
        .child(
            column()
                .w_full()
                .gap(px(space::SM))
                .child(method_btn(
                    "Username + Password",
                    "Email, phone, or username",
                    active().accent,
                    LoginAction::PickPassword,
                    cx,
                ))
                .child(method_btn(
                    "User / Bot Token",
                    "Paste a user or bot token directly",
                    0x7c6fe0,
                    LoginAction::PickToken,
                    cx,
                ))
                .child(method_btn(
                    "QR Code",
                    "Scan with the Discord mobile app",
                    active().success,
                    LoginAction::PickQr,
                    cx,
                ))
                .child(method_btn(
                    "Demo Mode",
                    "Offline fixture data — no account needed",
                    active().warning,
                    LoginAction::PickDemo,
                    cx,
                )),
        );

    let card = maybe_error(card, &login.error);

    card.child(
        gpui::div()
            .text_size(px(text::XS))
            .text_color(rgb(active().text_subtle))
            .child("Third-party clients are against Discord's ToS; use at your own risk."),
    )
}

// -- Password ----------------------------------------------------------------

fn password_view(login: &Login, cx: &mut Context<Workspace>) -> Div {
    let pw = &login.password;
    let submittable = pw.is_submittable();

    let card = column()
        .w(px(400.))
        .gap(px(space::LG))
        .p(px(space::XL))
        .rounded(px(layout::RADIUS_LG))
        .bg(rgb(active().surface))
        .border_1()
        .border_color(rgb(active().border))
        .child(
            row()
                .w_full()
                .gap(px(space::SM))
                .child(back_button(cx))
                .child(
                    gpui::div()
                        .flex_1()
                        .text_size(px(text::LG))
                        .text_color(rgb(active().text))
                        .child("Username + Password"),
                ),
        )
        .child(
            column()
                .w_full()
                .gap(px(space::MD))
                .child(input_row(
                    "Email / Phone / Username",
                    pw.login.text(),
                    pw.focused_field == PasswordField::Login,
                    Mask::None,
                    false,
                ))
                .child(input_row(
                    "Password",
                    pw.password.text(),
                    pw.focused_field == PasswordField::Password,
                    Mask::Full,
                    false,
                )),
        )
        .child(action_btn(
            if pw.in_progress {
                "Connecting…"
            } else {
                "Log In"
            },
            submittable,
            LoginAction::SubmitPassword,
            cx,
        ));

    let card = maybe_error(card, &login.error);

    card.child(
        gpui::div()
            .text_size(px(text::XS))
            .text_color(rgb(active().text_subtle))
            .child("Tab to switch fields · Enter to submit · ctrl-r to save token"),
    )
    .when(!pw.status.is_empty(), |d| {
        d.child(
            gpui::div()
                .text_size(px(text::XS))
                .text_color(rgb(active().text_muted))
                .child(pw.status.clone()),
        )
    })
}

// -- MFA method select -------------------------------------------------------

fn mfa_select_view(login: &Login, cx: &mut Context<Workspace>) -> Div {
    let pw = &login.password;
    let challenge = pw.mfa.as_ref();

    let card = column()
        .w(px(400.))
        .gap(px(space::LG))
        .p(px(space::XL))
        .rounded(px(layout::RADIUS_LG))
        .bg(rgb(active().surface))
        .border_1()
        .border_color(rgb(active().border))
        .child(
            column()
                .gap(px(space::XS))
                .child(
                    gpui::div()
                        .text_size(px(text::LG))
                        .text_color(rgb(active().text))
                        .child("Two-Factor Authentication"),
                )
                .child(
                    gpui::div()
                        .text_size(px(text::SM))
                        .text_color(rgb(active().text_muted))
                        .child("Choose a verification method"),
                ),
        );

    let card = if let Some(challenge) = challenge {
        let mut methods_col = column().w_full().gap(px(space::SM));
        for method in &challenge.methods {
            let (label, subtitle) = match method {
                MfaMethod::Totp => (
                    "Authenticator App (TOTP)",
                    "Enter the 6-digit code from your authenticator",
                ),
                MfaMethod::Sms => ("SMS Code", "Receive a code by text message"),
            };
            methods_col = methods_col.child(method_btn(
                label,
                subtitle,
                active().accent,
                LoginAction::PickMfaMethod(*method),
                cx,
            ));
        }
        card.child(methods_col)
    } else {
        card.child(
            gpui::div()
                .text_color(rgb(active().text_muted))
                .child("No MFA methods available"),
        )
    };

    maybe_error(card, &login.error)
}

// -- MFA code entry ----------------------------------------------------------

fn mfa_code_view(login: &Login, cx: &mut Context<Workspace>) -> Div {
    let pw = &login.password;
    let submittable = pw.is_mfa_submittable();
    let method_name = match pw.mfa_method {
        Some(MfaMethod::Sms) => "SMS Code",
        Some(MfaMethod::Totp) => "Authenticator Code",
        None => "Verification Code",
    };

    let card = column()
        .w(px(400.))
        .gap(px(space::LG))
        .p(px(space::XL))
        .rounded(px(layout::RADIUS_LG))
        .bg(rgb(active().surface))
        .border_1()
        .border_color(rgb(active().border))
        .child(
            row()
                .w_full()
                .gap(px(space::SM))
                .child(back_button(cx))
                .child(
                    gpui::div()
                        .flex_1()
                        .text_size(px(text::LG))
                        .text_color(rgb(active().text))
                        .child(method_name),
                ),
        )
        .when(!pw.status.is_empty(), |d| {
            d.child(
                gpui::div()
                    .text_size(px(text::SM))
                    .text_color(rgb(active().text_muted))
                    .child(pw.status.clone()),
            )
        })
        .child(input_row(
            "Verification code",
            pw.mfa_code.text(),
            true,
            Mask::None,
            false,
        ))
        .child(action_btn(
            if pw.in_progress {
                "Verifying…"
            } else {
                "Verify"
            },
            submittable,
            LoginAction::SubmitMfaCode,
            cx,
        ));

    maybe_error(card, &login.error)
}

// -- Token -------------------------------------------------------------------

fn token_view(login: &Login, cx: &mut Context<Workspace>) -> Div {
    let submittable = login.token_submittable();

    let card = column()
        .w(px(420.))
        .gap(px(space::LG))
        .p(px(space::XL))
        .rounded(px(layout::RADIUS_LG))
        .bg(rgb(active().surface))
        .border_1()
        .border_color(rgb(active().border))
        .child(
            row()
                .w_full()
                .gap(px(space::SM))
                .child(back_button(cx))
                .child(
                    gpui::div()
                        .flex_1()
                        .text_size(px(text::LG))
                        .text_color(rgb(active().text))
                        .child("User / Bot Token"),
                ),
        )
        .child(input_row(
            "Token",
            login.token.text(),
            true,
            Mask::Prefix,
            login.error.is_some(),
        ))
        .child(action_btn(
            "Connect",
            submittable,
            LoginAction::SubmitToken,
            cx,
        ));

    let card = maybe_error(card, &login.error);

    card.child(
        gpui::div()
            .text_size(px(text::XS))
            .text_color(rgb(active().text_subtle))
            .child(if login.remember {
                "Enter to connect  ·  ctrl-r: will be saved to the credential store"
            } else {
                "Enter to connect  ·  ctrl-r: remember this token"
            }),
    )
}

// -- QR scan -----------------------------------------------------------------

fn qr_view(login: &Login, cx: &mut Context<Workspace>) -> Div {
    let qr = &login.qr;

    let mut card = column()
        .w(px(460.))
        .gap(px(space::LG))
        .p(px(space::XL))
        .rounded(px(layout::RADIUS_LG))
        .bg(rgb(active().surface))
        .border_1()
        .border_color(rgb(active().border))
        .child(
            row()
                .w_full()
                .gap(px(space::SM))
                .child(back_button(cx))
                .child(
                    gpui::div()
                        .flex_1()
                        .text_size(px(text::LG))
                        .text_color(rgb(active().text))
                        .child("QR Code"),
                ),
        );

    card = if let Some(bitmap) = &qr.bitmap {
        card.child(
            column()
                .items_center()
                .gap(px(space::SM))
                .child(qr_bitmap_view(bitmap))
                .when(qr.pending_user.is_some(), |d| {
                    d.child(
                        gpui::div()
                            .text_size(px(text::SM))
                            .text_color(rgb(active().success))
                            .child(format!(
                                "Confirm login in the Discord app as {}",
                                qr.pending_user.as_deref().unwrap_or("")
                            )),
                    )
                }),
        )
    } else {
        card.child(
            gpui::div()
                .text_size(px(text::SM))
                .text_color(rgb(active().text_muted))
                .child(if qr.status.is_empty() {
                    "Starting QR login\u{2026}".to_string()
                } else {
                    qr.status.clone()
                }),
        )
    };

    if !qr.status.is_empty() && qr.bitmap.is_some() {
        card = card.child(
            gpui::div()
                .text_size(px(text::XS))
                .text_color(rgb(active().text_muted))
                .child(qr.status.clone()),
        );
    }

    maybe_error(card, &login.error)
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

fn maybe_error(card: Div, error: &Option<String>) -> Div {
    if let Some(msg) = error {
        card.child(
            gpui::div()
                .text_size(px(text::SM))
                .text_color(rgb(active().danger))
                .child(msg.clone()),
        )
    } else {
        card
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The mask policy is security-relevant, so it is asserted rather than
    /// left to inspection: a password must never reveal a prefix.
    #[test]
    fn password_masking_reveals_nothing() {
        let masked = super::mask_for_test("hunter2", Mask::Full);
        assert_eq!(masked, "•".repeat(7));
        assert!(!masked.contains('h'));
    }

    #[test]
    fn token_masking_reveals_only_a_short_prefix() {
        let token = "MTIzNDU2Nzg5MDEyMzQ1Njc4";
        let masked = super::mask_for_test(token, Mask::Prefix);

        assert!(masked.starts_with("MTIz"));
        assert_eq!(masked.chars().count(), token.chars().count());
        // Everything past the prefix must be hidden.
        assert_eq!(
            masked.chars().filter(|c| *c == '•').count(),
            token.len() - 4
        );
    }

    #[test]
    fn mfa_codes_are_shown_so_typos_are_visible() {
        assert_eq!(super::mask_for_test("123456", Mask::None), "123456");
    }

    #[test]
    fn masking_handles_short_and_empty_values() {
        assert_eq!(super::mask_for_test("", Mask::Prefix), "");
        assert_eq!(super::mask_for_test("", Mask::Full), "");
        // Shorter than the prefix window must not panic or over-repeat.
        assert_eq!(super::mask_for_test("ab", Mask::Prefix), "ab");
    }

    #[test]
    fn masking_is_char_based_not_byte_based() {
        // A byte-based implementation would emit the wrong number of bullets
        // for multi-byte input.
        assert_eq!(super::mask_for_test("héllo", Mask::Full).chars().count(), 5);
    }
}
