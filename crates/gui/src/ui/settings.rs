//! Separate Settings OS Window view.
//!
//! Backed by `config::AppOptions`, running in its own native window, allowing
//! configuration of client theme (Light/Dark mode), custom Discord API/gateway
//! base URLs (for self-hosted Discord instances such as Spacebar / protocol-server),
//! display options, notifications, and voice settings.

use gpui::{Context, Div, FocusHandle, KeyDownEvent, Render, Window, prelude::*, px, rgb};

use crate::theme::{DARK, LIGHT, Palette, layout, scaled, space, text};
use crate::ui::chrome::{column, row};

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
}

impl Toggle {
    pub fn label(self) -> String {
        concord::t!(match self {
            Toggle::Hour24 => "toggle-hour24",
            Toggle::ShowAvatars => "toggle-show-avatars",
            Toggle::ShowCustomEmoji => "toggle-show-custom-emoji",
            Toggle::MediaPlayback => "toggle-media-playback",
            Toggle::CircularAvatars => "toggle-circular-avatars",
            Toggle::DesktopNotifications => "toggle-desktop-notifications",
            Toggle::NoiseSuppression => "toggle-noise-suppression",
            Toggle::ShareRichPresence => "toggle-share-rich-presence",
        })
    }

    /// The explanatory line under a toggle, where one earns its place.
    fn hint(self) -> Option<String> {
        let key = match self {
            Toggle::ShareRichPresence => "hint-share-rich-presence",
            Toggle::NoiseSuppression => "hint-noise-suppression",
            Toggle::MediaPlayback => "hint-media-playback",
            _ => return None,
        };
        Some(concord::t!(key))
    }

    fn slot(self) -> usize {
        match self {
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
            Ok(()) => Some(concord::t!("settings-saved-to").to_string()),
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
                            .text_size(px(scaled(text::XS)))
                            .text_color(rgb(theme.text_subtle))
                            .child(concord::t!("settings-app-settings")),
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
                                    .text_size(px(scaled(text::XS)))
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
                                    .text_size(px(scaled(text::LG)))
                                    .text_color(rgb(theme.text))
                                    .child(concord::t!("settings-title")),
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
                                                    .text_size(px(scaled(text::BASE)))
                                                    .text_color(rgb(theme.text))
                                                    .child("🌙 Dark Mode"),
                                            )
                                            .child(
                                                gpui::div()
                                                    .text_size(px(scaled(text::XS)))
                                                    .text_color(rgb(theme.text_subtle))
                                                    .child(concord::t!("settings-theme-dark")),
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
                                                    .text_size(px(scaled(text::BASE)))
                                                    .text_color(rgb(theme.text))
                                                    .child("☀️ Light Mode"),
                                            )
                                            .child(
                                                gpui::div()
                                                    .text_size(px(scaled(text::XS)))
                                                    .text_color(rgb(theme.text_subtle))
                                                    .child(concord::t!("settings-theme-light")),
                                            ),
                                    ),
                            )
                            // --- Section 3: Interface & Display ---
                            .child(section_title(
                                concord::t!("settings-interface-display"),
                                theme,
                            ))
                            // Language, first in this section because it
                            // changes every other label under it.
                            .child(language_row(options.display.language, theme, cx))
                            .child(toggle_row(
                                Toggle::ShowAvatars,
                                options.display.show_avatars,
                                theme,
                                cx.listener(|this, _, _, cx| {
                                    this.options.display.show_avatars =
                                        !this.options.display.show_avatars;
                                    this.save_options(cx);
                                    cx.notify();
                                }),
                            ))
                            .child(toggle_row(
                                Toggle::CircularAvatars,
                                options.display.circular_avatars,
                                theme,
                                cx.listener(|this, _, _, cx| {
                                    this.options.display.circular_avatars =
                                        !this.options.display.circular_avatars;
                                    this.save_options(cx);
                                    cx.notify();
                                }),
                            ))
                            .child(toggle_row(
                                Toggle::ShowCustomEmoji,
                                options.display.show_custom_emoji,
                                theme,
                                cx.listener(|this, _, _, cx| {
                                    this.options.display.show_custom_emoji =
                                        !this.options.display.show_custom_emoji;
                                    this.save_options(cx);
                                    cx.notify();
                                }),
                            ))
                            .child(toggle_row(
                                Toggle::Hour24,
                                options.display.hour_format_24,
                                theme,
                                cx.listener(|this, _, _, cx| {
                                    this.options.display.hour_format_24 =
                                        !this.options.display.hour_format_24;
                                    this.save_options(cx);
                                    cx.notify();
                                }),
                            ))
                            .child(toggle_row(
                                Toggle::MediaPlayback,
                                options.display.media_playback,
                                theme,
                                cx.listener(|this, _, _, cx| {
                                    this.options.display.media_playback =
                                        !this.options.display.media_playback;
                                    this.save_options(cx);
                                    cx.notify();
                                }),
                            ))
                            // --- Section 4: Notifications ---
                            .child(section_title(concord::t!("settings-notifications"), theme))
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
                                    this.options.voice.noise_suppression =
                                        !this.options.voice.noise_suppression;
                                    this.save_options(cx);
                                    cx.notify();
                                }),
                            ))
                            // --- Section 6: Presence ---
                            .child(section_title(concord::t!("settings-presence"), theme))
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
                                    .text_size(px(scaled(text::XS)))
                                    .text_color(rgb(theme.text_subtle))
                                    .child(saved_note.map(str::to_owned).unwrap_or_else(|| {
                                        concord::t!("settings-saved-automatically")
                                    })),
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
        .text_size(px(scaled(text::SM)))
        .text_color(rgb(if active { theme.text } else { theme.text_muted }))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(theme.surface_hover)).text_color(rgb(theme.text)))
        .child(label)
}

fn section_title(title: impl Into<gpui::SharedString>, theme: &Palette) -> Div {
    gpui::div()
        .pt(px(space::SM))
        .pb(px(space::XS))
        .border_b_1()
        .border_color(rgb(theme.border))
        .text_size(px(scaled(text::SM)))
        .text_color(rgb(theme.accent))
        .child(title.into())
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
                        .text_size(px(scaled(text::SM)))
                        .text_color(rgb(theme.text))
                        .child(toggle.label()),
                )
                .when_some(toggle.hint(), |c, hint| {
                    c.child(
                        gpui::div()
                            .text_size(px(scaled(text::XS)))
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

/// The language picker.
///
/// Languages are listed by their own name - somebody looking for German is
/// looking for "Deutsch", not for whatever the current interface calls it -
/// so these are deliberately not translated.
fn language_row(
    current: Option<concord::i18n::Language>,
    theme: &Palette,
    cx: &mut Context<SettingsWindow>,
) -> Div {
    let mut choices = row().gap(px(space::XS)).flex_wrap();

    // "Follow system" is a real choice rather than an absence, so it is
    // offered as its own option.
    choices = choices.child(language_choice(
        "language-system",
        concord::t!("settings-language-follow-system"),
        current.is_none(),
        theme,
        cx.listener(|this, _, _, cx| {
            this.options.display.language = None;
            concord::i18n::set_language(concord::i18n::language_from_system());
            this.save_options(cx);
            cx.notify();
        }),
    ));

    for language in concord::i18n::Language::ALL {
        let language = *language;
        choices = choices.child(language_choice(
            language.tag(),
            language.endonym().to_owned(),
            current == Some(language),
            theme,
            cx.listener(move |this, _, _, cx| {
                this.options.display.language = Some(language);
                concord::i18n::set_language(language);
                this.save_options(cx);
                cx.notify();
            }),
        ));
    }

    column()
        .w_full()
        .px(px(space::LG))
        .py(px(space::SM))
        .gap(px(space::XS))
        .child(
            gpui::div()
                .text_size(px(scaled(text::SM)))
                .text_color(rgb(theme.text))
                .child(concord::t!("label-language")),
        )
        .child(choices)
}

fn language_choice(
    id: &'static str,
    label: String,
    selected: bool,
    theme: &Palette,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> gpui::Stateful<Div> {
    gpui::div()
        .id(id)
        .px(px(space::SM))
        .py(px(space::XS))
        .rounded(px(layout::RADIUS))
        .cursor_pointer()
        .text_size(px(scaled(text::XS)))
        .bg(rgb(if selected {
            theme.surface_active
        } else {
            theme.surface_sunken
        }))
        .text_color(rgb(if selected {
            theme.text
        } else {
            theme.text_muted
        }))
        .child(label)
        .on_click(on_click)
}
