//! Settings pop-out window modal.
//!
//! Backed by `config::AppOptions`, allowing configuration of client theme (Light/Dark mode),
//! custom Discord API/gateway base URLs (for self-hosted Discord instances such as Spacebar / protocol-server),
//! display options, notifications, and voice settings.

use gpui::{Context, Div, prelude::*, px, rgb};

use crate::theme::{DARK, LIGHT, Palette, layout, space, text};
use crate::ui::chrome::{column, row};
use crate::ui::workspace::Workspace;

/// Toggleable boolean settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Toggle {
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
            Toggle::ShowAvatars => 0,
            Toggle::CircularAvatars => 1,
            Toggle::DesktopNotifications => 2,
            Toggle::NoiseSuppression => 3,
            Toggle::ShareRichPresence => 4,
            Toggle::LightMode => 5,
        }
    }
}

/// Render the pop-out Settings Modal Window.
pub fn settings_modal_view(workspace: &Workspace, cx: &mut Context<Workspace>) -> impl IntoElement {
    let theme: &Palette = if workspace.options.display.light_mode {
        &LIGHT
    } else {
        &DARK
    };

    let options = &workspace.options;
    let saved_note = workspace.settings_note.as_deref();

    let display_url = if workspace.url_composer.is_empty() {
        if options.server.discord_base_url.is_empty() {
            "https://discord.com"
        } else {
            options.server.discord_base_url.as_str()
        }
    } else {
        workspace.url_composer.text()
    };

    // Full-screen backdrop overlay
    gpui::div()
        .id("settings-modal-backdrop")
        .absolute()
        .inset_0()
        .bg(rgb(0x000000))
        .opacity(0.85)
        .items_center()
        .justify_center()
        .on_click(cx.listener(|this, _event, _window, cx| {
            this.settings_open = false;
            cx.notify();
        }))
        .child(
            // Modal Card Container
            column()
                .id("settings-modal-card")
                .w(px(560.))
                .max_h(px(620.))
                .bg(rgb(theme.surface))
                .rounded(px(layout::RADIUS_LG))
                .border_1()
                .border_color(rgb(theme.border))
                .shadow_lg()
                .overflow_hidden()
                .on_click(|_event, _window, _cx| {
                    // Prevent clicks inside the modal card from closing the backdrop
                })
                // Header Bar
                .child(
                    row()
                        .w_full()
                        .h(px(layout::HEADER))
                        .px(px(space::LG))
                        .items_center()
                        .justify_between()
                        .border_b_1()
                        .border_color(rgb(theme.border))
                        .bg(rgb(theme.surface_sunken))
                        .child(
                            column()
                                .child(
                                    gpui::div()
                                        .text_size(px(text::LG))
                                        .text_color(rgb(theme.text))
                                        .child("⚙ Settings"),
                                )
                                .child(
                                    gpui::div()
                                        .text_size(px(text::XS))
                                        .text_color(rgb(theme.text_subtle))
                                        .child("Appearance, Server Endpoints & Client Options"),
                                ),
                        )
                        .child(
                            gpui::div()
                                .id("close-settings-modal")
                                .px(px(space::SM))
                                .py(px(space::XS))
                                .rounded(px(layout::RADIUS))
                                .cursor_pointer()
                                .hover(|s| s.bg(rgb(theme.surface_hover)))
                                .text_size(px(16.))
                                .text_color(rgb(theme.text_muted))
                                .child("✖")
                                .on_click(cx.listener(|this, _event, _window, cx| {
                                    this.settings_open = false;
                                    cx.notify();
                                })),
                        ),
                )
                // Scrollable Body
                .child(
                    column()
                        .id("settings-modal-body")
                        .flex_1()
                        .w_full()
                        .p(px(space::LG))
                        .gap(px(space::LG))
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
                                        .p(px(space::MD))
                                        .gap(px(2.))
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
                                                .child("Neutral dark theme (default)"),
                                        ),
                                )
                                .child(
                                    column()
                                        .id("card-theme-light")
                                        .flex_1()
                                        .p(px(space::MD))
                                        .gap(px(2.))
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
                        // --- Section 2: Server & Connection Endpoint ---
                        .child(section_title("Server & API Connection", theme))
                        .child(
                            column()
                                .w_full()
                                .gap(px(space::XS))
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
                                        .child("Default: https://discord.com — set custom URL for self-hosted instances (Spacebar, protocol-server, etc.)"),
                                )
                                .child(
                                    row()
                                        .w_full()
                                        .min_h(px(38.))
                                        .px(px(space::MD))
                                        .py(px(space::SM))
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
                // Footer
                .child(
                    row()
                        .w_full()
                        .h(px(48.))
                        .px(px(space::LG))
                        .items_center()
                        .justify_between()
                        .border_t_1()
                        .border_color(rgb(theme.border))
                        .bg(rgb(theme.surface_sunken))
                        .child(
                            gpui::div()
                                .text_size(px(text::XS))
                                .text_color(rgb(theme.text_subtle))
                                .child(saved_note.unwrap_or("Settings saved automatically").to_string()),
                        )
                        .child(
                            gpui::div()
                                .id("done-settings-modal")
                                .px(px(space::LG))
                                .py(px(space::SM))
                                .rounded(px(layout::RADIUS))
                                .bg(rgb(theme.accent))
                                .hover(|s| s.bg(rgb(theme.accent_hover)))
                                .text_size(px(text::SM))
                                .text_color(rgb(theme.on_accent))
                                .cursor_pointer()
                                .child("Done")
                                .on_click(cx.listener(|this, _event, _window, cx| {
                                    this.settings_open = false;
                                    cx.notify();
                                })),
                        ),
                ),
        )
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
        .bg(rgb(if on { theme.accent } else { theme.surface_active }))
        .flex()
        .items_center();

    let knob = gpui::div()
        .w(px(14.))
        .h(px(14.))
        .rounded_full()
        .bg(rgb(if on { theme.on_accent } else { theme.text_subtle }));

    if on {
        track.justify_end().child(knob.mr(px(2.)))
    } else {
        track.justify_start().child(knob.ml(px(2.)))
    }
}
