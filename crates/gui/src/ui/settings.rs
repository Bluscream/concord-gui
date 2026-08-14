//! Separate Settings OS Window view.
//!
//! Backed by `config::AppOptions`, running in its own native window, allowing
//! configuration of client theme (Light/Dark mode), custom Discord API/gateway
//! base URLs (for self-hosted Discord instances such as Spacebar / protocol-server),
//! display options, notifications, and voice settings.

use gpui::{Context, Div, FocusHandle, KeyDownEvent, Render, Window, prelude::*, px, rgb};

use crate::theme::{DARK, LIGHT, Palette, active, layout, space, text};
use crate::ui::chrome::{column, row};
use crate::ui::composer::Composer;

/// Toggleable boolean settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Toggle {
    Hour24,
    ShowAvatars,
    CircularAvatars,
    DesktopNotifications,
    NoiseSuppression,
    ShareRichPresence,
    LightMode,
}

impl Toggle {
    pub fn label(self) -> &'static str {
        match self {
            Toggle::LightMode => "Light theme",
            Toggle::Hour24 => "24-hour timestamps",
            Toggle::ShowAvatars => "Show avatars",
            Toggle::CircularAvatars => "Circular avatars",
            Toggle::DesktopNotifications => "Desktop notifications",
            Toggle::NoiseSuppression => "Noise suppression",
            Toggle::ShareRichPresence => "Share rich presence",
            Toggle::LightMode => "Light Mode Theme",
        }
    }

    fn hint(self) -> Option<&'static str> {
        match self {
            Toggle::LightMode => Some("Switch between Dark and Light mode interface themes"),
            Toggle::ShareRichPresence => Some("Lets others see what you are playing"),
            Toggle::NoiseSuppression => Some("Applied when joining a voice channel"),
            _ => None,
        }
    }

    fn slot(self) -> usize {
        match self {
            Toggle::LightMode => 0,
            Toggle::Hour24 => 1,
            Toggle::ShowAvatars => 2,
            Toggle::CircularAvatars => 3,
            Toggle::DesktopNotifications => 4,
            Toggle::NoiseSuppression => 5,
            Toggle::ShareRichPresence => 6,
            Toggle::LightMode => 5,
        }
    }
}

pub struct SettingsWindow {
    pub options: concord::config::AppOptions,
    pub settings_note: Option<String>,
    pub url_composer: Composer,
    focus: FocusHandle,
}

impl SettingsWindow {
    pub fn new(options: concord::config::AppOptions, cx: &mut Context<Self>) -> Self {
        let mut url_composer = Composer::default();
        url_composer.set_text(&options.server.discord_base_url);
        Self {
            options,
            settings_note: None,
            url_composer,
            focus: cx.focus_handle(),
        }
    }

    pub fn save_options(&mut self) {
        match concord::config::save_options(&self.options) {
            Ok(()) => {
                self.settings_note = Some("Saved to config.toml".to_string());
            }
            Err(err) => {
                self.settings_note = Some(format!("Failed to save: {err}"));
            }
        }
    }
}

impl Render for SettingsWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme: &Palette = if self.options.display.light_mode {
            &LIGHT
        } else {
            &DARK
        };

        let options = &self.options;
        let saved_note = self.settings_note.as_deref();

        let display_url = if self.url_composer.is_empty() {
            if options.server.discord_base_url.is_empty() {
                "https://discord.com"
            } else {
                options.server.discord_base_url.as_str()
            }
        } else {
            self.url_composer.text()
        };

        row()
            .id("settings-window-view")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, cx| {
                let pasted = (event.keystroke.key == "v"
                    && (event.keystroke.modifiers.control
                        || event.keystroke.modifiers.platform))
                    .then(|| cx.read_from_clipboard().and_then(|item| item.text()))
                    .flatten();
                if this.url_composer.handle_key_with_clipboard(event, pasted) {
                    this.options.server.discord_base_url = this.url_composer.text().to_string();
                    this.save_options();
                    cx.notify();
                }
            }))
            .size_full()
            .bg(rgb(theme.bg))
            .overflow_hidden()
            // --- Left Navigation Sidebar Column ---
            .child(
                column()
                    .w(px(220.))
                    .h_full()
                    .bg(rgb(theme.surface_sunken))
                    .border_r_1()
                    .border_color(rgb(theme.border))
                    .px(px(space::MD))
                    .py(px(space::LG))
                    .gap(px(space::XS))
                    .child(
                        gpui::div()
                            .px(px(space::SM))
                            .pb(px(space::SM))
                            .text_size(px(text::XS))
                            .text_color(rgb(theme.text_subtle))
                            .child("APP SETTINGS"),
                    )
                    .child(sidebar_nav_item("⚙ Appearance", true, theme))
                    .child(sidebar_nav_item("🌐 Server Endpoint", false, theme))
                    .child(sidebar_nav_item("📺 Display & UI", false, theme))
                    .child(sidebar_nav_item("🔔 Notifications", false, theme))
                    .child(sidebar_nav_item("🎤 Voice & Audio", false, theme))
                    .child(sidebar_nav_item("🟢 Presence", false, theme))
                    .child(
                        column()
                            .flex_1()
                            .justify_end()
                            .px(px(space::SM))
                            .pt(px(space::MD))
                            .border_t_1()
                            .border_color(rgb(theme.border))
                            .child(
                                gpui::div()
                                    .text_size(px(text::XS))
                                    .text_color(rgb(theme.text_subtle))
                                    .child("concord v0.1.0"),
                            ),
                    ),
            )
            // --- Right Content Column ---
            .child(
                column()
                    .flex_1()
                    .h_full()
                    .bg(rgb(theme.surface))
                    .overflow_hidden()
                    // Content Header
                    .child(
                        row()
                            .w_full()
                            .h(px(layout::HEADER))
                            .px(px(space::XL))
                            .items_center()
                            .justify_between()
                            .border_b_1()
                            .border_color(rgb(theme.border))
                            .child(
                                gpui::div()
                                    .text_size(px(text::LG))
                                    .text_color(rgb(theme.text))
                                    .child("Client Settings"),
                            ),
                    )
                    // Scrollable Settings Sections Body
                    .child(
                        column()
                            .id("settings-sections")
                            .flex_1()
                            .w_full()
                            .px(px(space::XL))
                            .py(px(space::LG))
                            .gap(px(space::XL))
                            .overflow_y_scroll()
                            // --- Section 1: Appearance & Theme ---
                            .child(section_title("Appearance & Theme", theme))
                            .child(
                                row()
                                    .w_full()
                                    .gap(px(space::MD))
                                    .child(
                                        column()
                                            .id("card-theme-dark")
                                            .flex_1()
                                            .p(px(space::LG))
                                            .gap(px(space::XS))
                                            .rounded(px(layout::RADIUS))
                                            .bg(rgb(if !options.display.light_mode {
                                                theme.surface_active
                                            } else {
                                                theme.surface_sunken
                                            }))
                                            .border_2()
                                            .border_color(rgb(if !options.display.light_mode {
                                                theme.accent
                                            } else {
                                                theme.border
                                            }))
                                            .cursor_pointer()
                                            .hover(|s| s.bg(rgb(theme.surface_hover)))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.options.display.light_mode = false;
                                                this.save_options();
                                                cx.notify();
                                            }))
                                            .child(
                                                gpui::div()
                                                    .text_size(px(text::BASE))
                                                    .text_color(rgb(theme.text))
                                                    .child("🌙 Dark Mode"),
                                            )
                                            .child(
                                                gpui::div()
                                                    .text_size(px(text::XS))
                                                    .text_color(rgb(theme.text_subtle))
                                                    .child("Neutral dark theme tuned for long reading sessions"),
                                            ),
                                    )
                                    .child(
                                        column()
                                            .id("card-theme-light")
                                            .flex_1()
                                            .p(px(space::LG))
                                            .gap(px(space::XS))
                                            .rounded(px(layout::RADIUS))
                                            .bg(rgb(if options.display.light_mode {
                                                theme.surface_active
                                            } else {
                                                theme.surface_sunken
                                            }))
                                            .border_2()
                                            .border_color(rgb(if options.display.light_mode {
                                                theme.accent
                                            } else {
                                                theme.border
                                            }))
                                            .cursor_pointer()
                                            .hover(|s| s.bg(rgb(theme.surface_hover)))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.options.display.light_mode = true;
                                                this.save_options();
                                                cx.notify();
                                            }))
                                            .child(
                                                gpui::div()
                                                    .text_size(px(text::BASE))
                                                    .text_color(rgb(theme.text))
                                                    .child("☀️ Light Mode"),
                                            )
                                            .child(
                                                gpui::div()
                                                    .text_size(px(text::XS))
                                                    .text_color(rgb(theme.text_subtle))
                                                    .child("Bright light mode theme"),
                                            ),
                                    ),
                            )
                            // --- Section 2: Server & API Connection ---
                            .child(section_title("Server & API Connection", theme))
                            .child(
                                column()
                                    .w_full()
                                    .gap(px(space::SM))
                                    .child(
                                        gpui::div()
                                            .text_size(px(text::SM))
                                            .text_color(rgb(theme.text))
                                            .child("Discord API Base URL"),
                                    )
                                    .child(
                                        gpui::div()
                                            .text_size(px(text::XS))
                                            .text_color(rgb(theme.text_subtle))
                                            .child("Default: https://discord.com — set custom URL for self-hosted instances (Spacebar, protocol-server)"),
                                    )
                                    .child(
                                        row()
                                            .w_full()
                                            .h(px(40.))
                                            .px(px(space::MD))
                                            .items_center()
                                            .rounded(px(layout::RADIUS))
                                            .bg(rgb(theme.surface_sunken))
                                            .border_1()
                                            .border_color(rgb(theme.border))
                                            .text_size(px(text::BASE))
                                            .text_color(rgb(theme.text))
                                            .child(display_url.to_string()),
                                    ),
                            )
                            // --- Section 3: Interface & Display ---
                            .child(section_title("Interface & Display", theme))
                            .child(toggle_row(
                                Toggle::ShowAvatars,
                                options.display.show_avatars,
                                theme,
                                cx.listener(|this, _, _, cx| {
                                    this.options.display.show_avatars = !this.options.display.show_avatars;
                                    this.save_options();
                                    cx.notify();
                                }),
                            ))
                            .child(toggle_row(
                                Toggle::CircularAvatars,
                                options.display.circular_avatars,
                                theme,
                                cx.listener(|this, _, _, cx| {
                                    this.options.display.circular_avatars = !this.options.display.circular_avatars;
                                    this.save_options();
                                    cx.notify();
                                }),
                            ))
                            // --- Section 4: Notifications ---
                            .child(section_title("Notifications", theme))
                            .child(toggle_row(
                                Toggle::DesktopNotifications,
                                options.notifications.desktop_notifications,
                                theme,
                                cx.listener(|this, _, _, cx| {
                                    this.options.notifications.desktop_notifications =
                                        !this.options.notifications.desktop_notifications;
                                    this.save_options();
                                    cx.notify();
                                }),
                            ))
                            // --- Section 5: Voice & Audio ---
                            .child(section_title("Voice & Audio", theme))
                            .child(toggle_row(
                                Toggle::NoiseSuppression,
                                options.voice.noise_suppression,
                                theme,
                                cx.listener(|this, _, _, cx| {
                                    this.options.voice.noise_suppression = !this.options.voice.noise_suppression;
                                    this.save_options();
                                    cx.notify();
                                }),
                            ))
                            // --- Section 6: Presence ---
                            .child(section_title("Presence", theme))
                            .child(toggle_row(
                                Toggle::ShareRichPresence,
                                options.presence.share_rich_presence,
                                theme,
                                cx.listener(|this, _, _, cx| {
                                    this.options.presence.share_rich_presence =
                                        !this.options.presence.share_rich_presence;
                                    this.save_options();
                                    cx.notify();
                                }),
                            )),
                    )
                    // Footer Bar
                    .child(
                        row()
                            .w_full()
                            .h(px(44.))
                            .px(px(space::XL))
                            .items_center()
                            .border_t_1()
                            .border_color(rgb(theme.border))
                            .bg(rgb(theme.surface_sunken))
                            .child(
                                gpui::div()
                                    .text_size(px(text::XS))
                                    .text_color(rgb(theme.text_subtle))
                                    .child(saved_note.unwrap_or("Settings saved automatically").to_string()),
                            ),
                    ),
            )
    }
}

fn sidebar_nav_item(label: &'static str, active: bool, theme: &Palette) -> Div {
    gpui::div()
        .w_full()
        .px(px(space::MD))
        .py(px(space::SM))
        .rounded(px(layout::RADIUS))
        .bg(rgb(if active {
            theme.surface_active
        } else {
            theme.surface_sunken
        }))
        .text_size(px(text::SM))
        .text_color(rgb(if active { theme.text } else { theme.text_muted }))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(theme.surface_hover)).text_color(rgb(theme.text)))
        .child(label)
}

fn section_title(title: &'static str, theme: &Palette) -> Div {
    gpui::div()
        .pt(px(space::SM))
        .pb(px(space::XS))
        .border_b_1()
        .border_color(rgb(theme.border))
        .text_size(px(text::SM))
        .text_color(rgb(theme.accent))
        .child(title)
}

fn toggle_row(
    toggle: Toggle,
    enabled: bool,
    theme: &Palette,
    listener: impl Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> gpui::Stateful<Div> {
    row()
        .id(("toggle-row", toggle.slot()))
        .w_full()
        .px(px(space::MD))
        .py(px(space::SM))
        .rounded(px(layout::RADIUS))
        .gap(px(space::SM))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(theme.surface_hover)))
        .on_click(listener)
        .child(
            column()
                .flex_1()
                .child(
                    gpui::div()
                        .text_size(px(text::SM))
                        .text_color(rgb(theme.text))
                        .child(toggle.label()),
                )
                .when_some(toggle.hint(), |c, hint| {
                    c.child(
                        gpui::div()
                            .text_size(px(text::XS))
                            .text_color(rgb(theme.text_subtle))
                            .child(hint),
                    )
                }),
        )
        .child(switch(enabled, theme))
}

fn switch(on: bool, theme: &Palette) -> Div {
    let track = gpui::div()
        .w(px(32.))
        .h(px(18.))
        .rounded(px(9.))
        .bg(rgb(if on {
            theme.accent
        } else {
            theme.surface_active
        }))
        .flex()
        .items_center();

    let knob = gpui::div()
        .w(px(14.))
        .h(px(14.))
        .rounded_full()
        .bg(rgb(if on {
            theme.on_accent
        } else {
            theme.text_subtle
        }));

    if on {
        track.justify_end().child(knob.mr(px(2.)))
    } else {
        track.justify_start().child(knob.ml(px(2.)))
    }
}
