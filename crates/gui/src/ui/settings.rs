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
    ShowCustomEmoji,
    MediaPlayback,
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
            Toggle::ShowCustomEmoji => "Show custom emoji",
            Toggle::MediaPlayback => "External media playback",
            Toggle::CircularAvatars => "Circular avatars",
            Toggle::DesktopNotifications => "Desktop notifications",
            Toggle::NoiseSuppression => "Noise suppression",
            Toggle::ShareRichPresence => "Share rich presence",
        }
    }

    fn hint(self) -> Option<&'static str> {
        match self {
            Toggle::LightMode => Some("Switch between Dark and Light mode interface themes"),
            Toggle::ShareRichPresence => Some("Lets others see what you are playing"),
            Toggle::NoiseSuppression => Some("Applied when joining a voice channel"),
            Toggle::MediaPlayback => Some("Opens video and audio in an external player"),
            _ => None,
        }
    }

    fn slot(self) -> usize {
        match self {
            Toggle::LightMode => 0,
            Toggle::Hour24 => 1,
            Toggle::ShowAvatars => 2,
            Toggle::ShowCustomEmoji => 3,
            Toggle::MediaPlayback => 8,
            Toggle::CircularAvatars => 4,
            Toggle::DesktopNotifications => 5,
            Toggle::NoiseSuppression => 6,
            Toggle::ShareRichPresence => 7,
        }
    }
}

/// Notified whenever a setting changes, so the live client can adopt it.
///
/// Without this the window edits a detached clone: it would save correctly to
/// config.toml while the running UI kept stale values until restart.
pub type OnChange = std::rc::Rc<dyn Fn(&concord::config::AppOptions, &mut gpui::App)>;

pub struct SettingsWindow {
    pub options: concord::config::AppOptions,
    pub settings_note: Option<String>,
    on_change: Option<OnChange>,
    focus: FocusHandle,
}

impl SettingsWindow {
    pub fn new(options: concord::config::AppOptions, cx: &mut Context<Self>) -> Self {
        Self {
            options,
            settings_note: None,
            on_change: None,
            focus: cx.focus_handle(),
        }
    }

    /// Register the callback that propagates changes to the live client.
    pub fn on_change(mut self, callback: OnChange) -> Self {
        self.on_change = Some(callback);
        self
    }

    /// Persist, apply the theme, and notify the live client.
    ///
    /// `cx` is required so the change can reach the opener; saving without it
    /// would leave the running UI showing stale settings.
    pub fn save_options(&mut self, cx: &mut gpui::App) {
        // Applied here rather than at next launch so the palette flips while
        // the window is still open.
        crate::theme::set_light_mode(self.options.display.light_mode);

        self.settings_note = match concord::config::save_options(&self.options) {
            Ok(()) => Some("Saved to config.toml".to_string()),
            Err(err) => Some(format!("Failed to save: {err}")),
        };

        if let Some(callback) = self.on_change.clone() {
            callback(&self.options, cx);
        }

        cx.refresh_windows();
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

        row()
            .id("settings-window-view")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, cx| {
                if event.keystroke.key == "escape" {
                    this.save_options(cx);
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
                                                this.save_options(cx);
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
                                                this.save_options(cx);
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
                            // --- Section 3: Interface & Display ---
                            .child(section_title("Interface & Display", theme))
                            .child(toggle_row(
                                Toggle::ShowAvatars,
                                options.display.show_avatars,
                                theme,
                                cx.listener(|this, _, _, cx| {
                                    this.options.display.show_avatars = !this.options.display.show_avatars;
                                    this.save_options(cx);
                                    cx.notify();
                                }),
                            ))
                            .child(toggle_row(
                                Toggle::CircularAvatars,
                                options.display.circular_avatars,
                                theme,
                                cx.listener(|this, _, _, cx| {
                                    this.options.display.circular_avatars = !this.options.display.circular_avatars;
                                    this.save_options(cx);
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
                                    this.save_options(cx);
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
                                    this.save_options(cx);
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
                                    this.save_options(cx);
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
