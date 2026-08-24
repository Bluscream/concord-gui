//! User settings and profile overlays.

use gpui::{prelude::*, px, rgb, Div};

use concord::t;

use crate::theme::{active, layout, scaled, space, text};
use crate::ui::chrome::{column, row};
use crate::ui::overlay::{button, panel};

pub struct ActivityField {
    pub label: String,
    pub placeholder: String,
    pub value: String,
    pub focused: bool,
}

pub fn activity_editor_view(
    kinds: &[(String, bool)],
    fields: &[ActivityField],
    on_kind: impl Fn(usize, &mut gpui::App) + Clone + 'static,
    on_field: impl Fn(usize, &mut gpui::App) + Clone + 'static,
    on_save: impl Fn(&mut gpui::App) + 'static,
    on_clear: impl Fn(&mut gpui::App) + 'static,
    on_cancel: impl Fn(&mut gpui::App) + 'static,
) -> Div {
    let mut kind_row = row()
        .w_full()
        .px(px(space::LG))
        .py(px(space::SM))
        .gap(px(space::XS))
        .flex_wrap();

    for (index, (label, selected)) in kinds.iter().enumerate() {
        let pick = on_kind.clone();
        kind_row = kind_row.child(
            gpui::div()
                .id(("activity-kind", index))
                .px(px(space::SM))
                .py(px(space::XS))
                .rounded(px(4.))
                .cursor_pointer()
                .text_size(px(scaled(text::SM)))
                .bg(rgb(if *selected {
                    active().accent
                } else {
                    active().surface
                }))
                .text_color(rgb(if *selected {
                    active().text
                } else {
                    active().text_muted
                }))
                .hover(|style| style.bg(rgb(active().surface_hover)))
                .on_click(move |_event, _window, cx| pick(index, cx))
                .child(label.clone()),
        );
    }

    let mut form = column().w_full();
    for (index, field) in fields.iter().enumerate() {
        let focus = on_field.clone();
        form = form.child(
            column()
                .id(("activity-field", index))
                .w_full()
                .px(px(space::LG))
                .py(px(space::XS))
                .gap(px(space::XS))
                .cursor_pointer()
                .on_click(move |_event, _window, cx| focus(index, cx))
                .child(
                    gpui::div()
                        .text_size(px(scaled(text::XS)))
                        .text_color(rgb(active().text_subtle))
                        .child(field.label.clone()),
                )
                .child(
                    gpui::div()
                        .w_full()
                        .px(px(space::SM))
                        .py(px(space::XS))
                        .rounded(px(4.))
                        .border_1()
                        .border_color(rgb(if field.focused {
                            active().accent
                        } else {
                            active().border
                        }))
                        .text_size(px(scaled(text::SM)))
                        .text_color(rgb(if field.value.is_empty() {
                            active().text_subtle
                        } else {
                            active().text
                        }))
                        .child(if field.value.is_empty() {
                            field.placeholder.clone()
                        } else {
                            field.value.clone()
                        }),
                ),
        );
    }

    panel(&t!("label-activity"), 420.)
        .child(kind_row)
        .child(form)
        .child(
            row()
                .w_full()
                .px(px(space::LG))
                .py(px(space::MD))
                .gap(px(space::SM))
                .justify_end()
                .child(button(
                    "activity-clear",
                    &t!("action-clear-activity"),
                    false,
                    on_clear,
                ))
                .child(button(
                    "activity-cancel",
                    &t!("action-cancel"),
                    false,
                    on_cancel,
                ))
                .child(button("activity-save", &t!("action-save"), true, on_save)),
        )
}

pub fn image_viewer_view(
    image: gpui::ImageSource,
    position: Option<String>,
    max_width: f32,
    max_height: f32,
    on_step: impl Fn(bool, &mut gpui::App) + Clone + 'static,
    on_zoom: impl Fn(bool, &mut gpui::App) + Clone + 'static,
) -> Div {
    let back = on_step.clone();
    let zoom_out = on_zoom.clone();

    column()
        .items_center()
        .gap(px(space::SM))
        .child(
            gpui::img(image)
                .max_w(px(max_width))
                .max_h(px(max_height))
                .rounded(px(layout::RADIUS)),
        )
        .child(
            row()
                .gap(px(space::SM))
                .items_center()
                .child(
                    gpui::div()
                        .id("image-prev")
                        .px(px(space::SM))
                        .cursor_pointer()
                        .text_color(rgb(active().text_muted))
                        .hover(|style| style.text_color(rgb(active().text)))
                        .on_click(move |_event, _window, cx| back(false, cx))
                        .child("\u{25C0}"),
                )
                .child(
                    gpui::div()
                        .id("image-zoom-out")
                        .px(px(space::SM))
                        .cursor_pointer()
                        .text_color(rgb(active().text_muted))
                        .hover(|style| style.text_color(rgb(active().text)))
                        .on_click(move |_event, _window, cx| zoom_out(false, cx))
                        .child("\u{2212}"),
                )
                .children(position.map(|label| {
                    gpui::div()
                        .text_size(px(scaled(text::XS)))
                        .text_color(rgb(active().text_subtle))
                        .child(label)
                }))
                .child(
                    gpui::div()
                        .id("image-zoom-in")
                        .px(px(space::SM))
                        .cursor_pointer()
                        .text_color(rgb(active().text_muted))
                        .hover(|style| style.text_color(rgb(active().text)))
                        .on_click(move |_event, _window, cx| on_zoom(true, cx))
                        .child("\u{002B}"),
                )
                .child(
                    gpui::div()
                        .id("image-next")
                        .px(px(space::SM))
                        .cursor_pointer()
                        .text_color(rgb(active().text_muted))
                        .hover(|style| style.text_color(rgb(active().text)))
                        .on_click(move |_event, _window, cx| on_step(true, cx))
                        .child("\u{25B6}"),
                ),
        )
}

pub struct SoundRow {
    pub label: String,
    pub name: String,
    pub available: bool,
}

pub fn soundboard_view(
    sounds: &[SoundRow],
    loading: bool,
    error: Option<&str>,
    on_play: impl Fn(usize, &mut gpui::App) + Clone + 'static,
    on_close: impl Fn(&mut gpui::App) + 'static,
) -> Div {
    let mut grid = row()
        .id("soundboard-grid")
        .w_full()
        .px(px(space::LG))
        .py(px(space::SM))
        .gap(px(space::XS))
        .flex_wrap()
        .max_h(px(320.))
        .overflow_y_scroll();

    let notice = if loading {
        Some(t!("status-loading"))
    } else if let Some(error) = error {
        Some(error.to_owned())
    } else if sounds.is_empty() {
        Some(t!("status-no-sounds"))
    } else {
        None
    };

    if let Some(notice) = notice {
        grid = grid.child(
            gpui::div()
                .px(px(space::SM))
                .py(px(space::MD))
                .text_size(px(scaled(text::SM)))
                .text_color(rgb(if error.is_some() {
                    active().danger
                } else {
                    active().text_subtle
                }))
                .child(notice),
        );
    }

    for (index, sound) in sounds.iter().enumerate() {
        let play = on_play.clone();
        grid = grid.child(
            column()
                .id(("sound", index))
                .w(px(84.))
                .h(px(72.))
                .items_center()
                .justify_center()
                .gap(px(space::XS))
                .rounded(px(layout::RADIUS))
                .bg(rgb(active().surface))
                .when(sound.available, |button| {
                    button
                        .cursor_pointer()
                        .hover(|style| style.bg(rgb(active().surface_hover)))
                        .on_click(move |_event, _window, cx| play(index, cx))
                })
                .child(
                    gpui::div()
                        .text_size(px(scaled(text::LG)))
                        .text_color(rgb(if sound.available {
                            active().text
                        } else {
                            active().text_subtle
                        }))
                        .child(sound.label.clone()),
                )
                .child(
                    gpui::div()
                        .text_size(px(scaled(text::XS)))
                        .text_color(rgb(active().text_subtle))
                        .child(sound.name.clone()),
                ),
        );
    }

    panel(&t!("label-soundboard"), 420.).child(grid).child(
        row()
            .w_full()
            .px(px(space::LG))
            .py(px(space::MD))
            .justify_end()
            .child(button(
                "soundboard-close",
                &t!("action-close"),
                true,
                on_close,
            )),
    )
}

pub struct ConnectionRow {
    pub primary: String,
    pub secondary: String,
    pub visibility_action: String,
    pub activity_action: String,
}

impl ConnectionRow {
    pub fn new(connection: &concord::discord::Connection) -> Self {
        use concord::discord::ConnectionVisibility;
        Self {
            primary: format!("{} - {}", connection.kind, connection.name),
            secondary: connection.summary(),
            visibility_action: if connection.visibility == ConnectionVisibility::Everyone {
                t!("action-connection-hide")
            } else {
                t!("action-connection-show")
            },
            activity_action: if connection.show_activity {
                t!("action-connection-activity-off")
            } else {
                t!("action-connection-activity-on")
            },
        }
    }
}

pub fn connections_view(
    rows: &[ConnectionRow],
    loading: bool,
    error: Option<&str>,
    on_visibility: impl Fn(usize, &mut gpui::App) + Clone + 'static,
    on_activity: impl Fn(usize, &mut gpui::App) + Clone + 'static,
    on_unlink: impl Fn(usize, &mut gpui::App) + Clone + 'static,
    on_close: impl Fn(&mut gpui::App) + 'static,
) -> Div {
    let mut list = column()
        .id("connection-rows")
        .max_h(px(360.))
        .overflow_y_scroll();

    let notice = if loading {
        Some(t!("status-loading"))
    } else if let Some(error) = error {
        Some(error.to_owned())
    } else if rows.is_empty() {
        Some(t!("status-no-connections"))
    } else {
        None
    };

    if let Some(notice) = notice {
        list = list.child(
            gpui::div()
                .px(px(space::LG))
                .py(px(space::MD))
                .text_size(px(scaled(text::SM)))
                .text_color(rgb(if error.is_some() {
                    active().danger
                } else {
                    active().text_subtle
                }))
                .child(notice),
        );
    }

    for (index, entry) in rows.iter().enumerate() {
        let visibility = on_visibility.clone();
        let activity = on_activity.clone();
        let unlink = on_unlink.clone();
        list = list.child(
            row()
                .id(("connection-row", index))
                .w_full()
                .px(px(space::LG))
                .py(px(space::XS))
                .gap(px(space::SM))
                .items_center()
                .child(
                    column()
                        .flex_1()
                        .child(
                            gpui::div()
                                .text_size(px(scaled(text::SM)))
                                .text_color(rgb(active().text))
                                .child(entry.primary.clone()),
                        )
                        .child(
                            gpui::div()
                                .text_size(px(scaled(text::XS)))
                                .text_color(rgb(active().text_subtle))
                                .child(entry.secondary.clone()),
                        ),
                )
                .child(
                    gpui::div()
                        .id(("connection-visibility", index))
                        .px(px(space::SM))
                        .py(px(space::XS))
                        .rounded(px(4.))
                        .cursor_pointer()
                        .text_size(px(scaled(text::XS)))
                        .text_color(rgb(active().text_muted))
                        .hover(|style| style.bg(rgb(active().surface_hover)))
                        .on_click(move |_event, _window, cx| visibility(index, cx))
                        .child(entry.visibility_action.clone()),
                )
                .child(
                    gpui::div()
                        .id(("connection-activity", index))
                        .px(px(space::SM))
                        .py(px(space::XS))
                        .rounded(px(4.))
                        .cursor_pointer()
                        .text_size(px(scaled(text::XS)))
                        .text_color(rgb(active().text_muted))
                        .hover(|style| style.bg(rgb(active().surface_hover)))
                        .on_click(move |_event, _window, cx| activity(index, cx))
                        .child(entry.activity_action.clone()),
                )
                .child(
                    gpui::div()
                        .id(("connection-unlink", index))
                        .px(px(space::SM))
                        .py(px(space::XS))
                        .rounded(px(4.))
                        .cursor_pointer()
                        .text_size(px(scaled(text::XS)))
                        .text_color(rgb(active().danger))
                        .hover(|style| style.bg(rgb(active().surface_hover)))
                        .on_click(move |_event, _window, cx| unlink(index, cx))
                        .child(t!("action-unlink")),
                ),
        );
    }

    panel(&t!("label-connections"), 520.)
        .child(list)
        .child(
            gpui::div()
                .px(px(space::LG))
                .py(px(space::XS))
                .text_size(px(scaled(text::XS)))
                .text_color(rgb(active().text_subtle))
                .child(t!("hint-connections-add")),
        )
        .child(
            row()
                .w_full()
                .px(px(space::LG))
                .py(px(space::MD))
                .justify_end()
                .child(button(
                    "connections-close",
                    &t!("action-close"),
                    true,
                    on_close,
                )),
        )
}

pub struct PrivacyRow {
    pub label: String,
    pub detail: String,
    pub enabled: Option<bool>,
    pub value: Option<String>,
}

impl PrivacyRow {
    pub fn new(
        setting: concord::discord::PrivacySetting,
        state: &concord::discord::PrivacyState,
    ) -> Self {
        Self {
            label: setting.label().to_owned(),
            detail: setting.detail().to_owned(),
            enabled: setting.is_on(state),
            value: setting.value(state).map(str::to_owned),
        }
    }
}

pub fn privacy_view(
    rows: &[PrivacyRow],
    on_toggle: impl Fn(usize, &mut gpui::App) + Clone + 'static,
    on_close: impl Fn(&mut gpui::App) + 'static,
) -> Div {
    let mut list = column()
        .id("privacy-rows")
        .max_h(px(360.))
        .overflow_y_scroll();

    for (index, entry) in rows.iter().enumerate() {
        let toggle = on_toggle.clone();
        list = list.child(
            row()
                .id(("privacy-row", index))
                .w_full()
                .px(px(space::LG))
                .py(px(space::XS))
                .gap(px(space::SM))
                .items_center()
                .cursor_pointer()
                .hover(|style| style.bg(rgb(active().surface_hover)))
                .on_click(move |_event, _window, cx| toggle(index, cx))
                .child(
                    column()
                        .flex_1()
                        .child(
                            gpui::div()
                                .text_size(px(scaled(text::SM)))
                                .text_color(rgb(active().text))
                                .child(entry.label.clone()),
                        )
                        .child(
                            gpui::div()
                                .text_size(px(scaled(text::XS)))
                                .text_color(rgb(active().text_subtle))
                                .child(entry.detail.clone()),
                        ),
                )
                .child(
                    gpui::div()
                        .text_size(px(scaled(text::SM)))
                        .text_color(rgb(match entry.enabled {
                            Some(true) => active().accent,
                            Some(false) => active().text_muted,
                            None => active().text_subtle,
                        }))
                        .child(match (&entry.value, entry.enabled) {
                            (Some(value), _) => value.clone(),
                            (None, Some(true)) => t!("state-on"),
                            (None, Some(false)) => t!("state-off"),
                            (None, None) => t!("state-unknown"),
                        }),
                ),
        );
    }

    panel(&t!("label-privacy"), 520.).child(list).child(
        row()
            .w_full()
            .px(px(space::LG))
            .py(px(space::MD))
            .justify_end()
            .child(button("privacy-close", &t!("action-close"), true, on_close)),
    )
}

pub struct AccessRow {
    pub primary: String,
    pub secondary: String,
    pub action: String,
    pub destructive: bool,
    pub selected: bool,
}

pub struct AccessPanel<'a> {
    pub rows: &'a [AccessRow],
    pub loading: bool,
    pub error: Option<&'a str>,
    pub password: Option<&'a str>,
    pub logout_enabled: bool,
}

pub fn access_view(
    panel_state: AccessPanel<'_>,
    on_row: impl Fn(usize, &mut gpui::App) + Clone + 'static,
    on_password: impl Fn(&str, &mut gpui::App) + Clone + 'static,
    on_logout: impl Fn(&mut gpui::App) + 'static,
    on_close: impl Fn(&mut gpui::App) + 'static,
) -> Div {
    let mut list = column()
        .id("access-rows")
        .max_h(px(320.))
        .overflow_y_scroll();

    let notice = if panel_state.loading {
        Some(t!("status-loading"))
    } else if let Some(error) = panel_state.error {
        Some(error.to_owned())
    } else if panel_state.rows.is_empty() {
        Some(t!("status-no-access"))
    } else {
        None
    };

    if let Some(notice) = notice {
        list = list.child(
            gpui::div()
                .px(px(space::LG))
                .py(px(space::MD))
                .text_size(px(scaled(text::SM)))
                .text_color(rgb(if panel_state.error.is_some() {
                    active().danger
                } else {
                    active().text_subtle
                }))
                .child(notice),
        );
    }

    for (index, entry) in panel_state.rows.iter().enumerate() {
        let act = on_row.clone();
        list = list.child(
            row()
                .id(("access-row", index))
                .w_full()
                .px(px(space::LG))
                .py(px(space::XS))
                .gap(px(space::SM))
                .items_center()
                .when(entry.selected, |r| r.bg(rgb(active().surface_active)))
                .child(
                    column()
                        .flex_1()
                        .child(
                            gpui::div()
                                .text_size(px(scaled(text::SM)))
                                .text_color(rgb(active().text))
                                .child(entry.primary.clone()),
                        )
                        .child(
                            gpui::div()
                                .text_size(px(scaled(text::XS)))
                                .text_color(rgb(active().text_subtle))
                                .child(entry.secondary.clone()),
                        ),
                )
                .child(
                    gpui::div()
                        .id(("access-row-action", index))
                        .px(px(space::SM))
                        .py(px(space::XS))
                        .rounded(px(4.))
                        .cursor_pointer()
                        .text_size(px(scaled(text::XS)))
                        .text_color(rgb(if entry.destructive {
                            active().danger
                        } else {
                            active().text_muted
                        }))
                        .hover(|style| style.bg(rgb(active().surface_hover)))
                        .on_click(move |_event, _window, cx| act(index, cx))
                        .child(entry.action.clone()),
                ),
        );
    }

    let mut panel = panel(&t!("label-access"), 560.).child(list);

    if let Some(password) = panel_state.password {
        let typed = on_password.clone();
        panel = panel.child(
            column()
                .w_full()
                .px(px(space::LG))
                .py(px(space::SM))
                .gap(px(space::XS))
                .child(
                    gpui::div()
                        .text_size(px(scaled(text::XS)))
                        .text_color(rgb(active().text_subtle))
                        .child(t!("hint-session-password")),
                )
                .child(
                    gpui::div()
                        .id("access-password")
                        .w_full()
                        .px(px(space::SM))
                        .py(px(space::XS))
                        .rounded(px(layout::RADIUS))
                        .bg(rgb(active().surface_sunken))
                        .text_size(px(scaled(text::SM)))
                        .text_color(rgb(active().text))
                        .child(password.to_owned())
                        .on_key_down(move |event: &gpui::KeyDownEvent, _window, cx| {
                            typed(&event.keystroke.key, cx);
                        }),
                ),
        );
    }

    panel.child(
        row()
            .w_full()
            .px(px(space::LG))
            .py(px(space::MD))
            .gap(px(space::SM))
            .justify_end()
            .when(panel_state.logout_enabled, |r| {
                let logout = std::rc::Rc::new(on_logout);
                r.child(button(
                    "access-logout",
                    &t!("action-log-out-sessions"),
                    true,
                    move |cx| logout(cx),
                ))
            })
            .child(button("access-close", &t!("action-close"), false, on_close)),
    )
}

pub struct AccountPanel<'a> {
    pub fields: &'a [(String, String, String, bool)],
    pub problem: Option<&'a str>,
    pub enrolment_uri: Option<&'a str>,
    pub enrolment_code: &'a str,
    pub backup_codes: &'a [(String, bool)],
}

type AccountAction = Box<dyn Fn(&mut gpui::App)>;
type BackupCodesAction = Box<dyn Fn(bool, &mut gpui::App)>;

pub struct AccountActions {
    pub save: AccountAction,
    pub enrol: AccountAction,
    pub submit_enrolment: AccountAction,
    pub disable: AccountAction,
    pub backup_codes: BackupCodesAction,
    pub close: AccountAction,
}

pub fn account_view(
    panel_state: AccountPanel<'_>,
    on_focus: impl Fn(usize, &mut gpui::App) + Clone + 'static,
    on_type: impl Fn(&str, &mut gpui::App) + Clone + 'static,
    on_totp: impl Fn(&str, &mut gpui::App) + Clone + 'static,
    actions: AccountActions,
) -> Div {
    let mut form = column().w_full();
    let backup_codes_action = std::rc::Rc::new(actions.backup_codes);

    for (index, (label, value, hint, focused)) in panel_state.fields.iter().enumerate() {
        let focus = on_focus.clone();
        let typed = on_type.clone();

        form = form.child(
            column()
                .id(("account-field", index))
                .w_full()
                .px(px(space::LG))
                .py(px(space::XS))
                .gap(px(space::XS))
                .cursor_pointer()
                .on_click(move |_event, _window, cx| focus(index, cx))
                .child(
                    gpui::div()
                        .text_size(px(scaled(text::XS)))
                        .text_color(rgb(active().text_subtle))
                        .child(label.clone()),
                )
                .child(
                    gpui::div()
                        .w_full()
                        .px(px(space::SM))
                        .py(px(space::XS))
                        .rounded(px(layout::RADIUS))
                        .bg(rgb(active().surface_sunken))
                        .border_1()
                        .border_color(rgb(if *focused {
                            active().accent
                        } else {
                            active().border
                        }))
                        .text_size(px(scaled(text::SM)))
                        .text_color(rgb(active().text))
                        .child(value.clone())
                        .on_key_down(move |event: &gpui::KeyDownEvent, _window, cx| {
                            typed(&event.keystroke.key, cx);
                        }),
                )
                .child(
                    gpui::div()
                        .text_size(px(scaled(text::XS)))
                        .text_color(rgb(active().text_subtle))
                        .child(hint.clone()),
                ),
        );
    }

    if let Some(uri) = panel_state.enrolment_uri {
        let totp_typed = on_totp;
        form = form.child(
            column()
                .w_full()
                .px(px(space::LG))
                .py(px(space::SM))
                .gap(px(space::XS))
                .child(
                    gpui::div()
                        .text_size(px(scaled(text::XS)))
                        .text_color(rgb(active().text_subtle))
                        .child(t!("hint-authenticator-uri")),
                )
                .child(
                    gpui::div()
                        .w_full()
                        .px(px(space::SM))
                        .py(px(space::XS))
                        .rounded(px(layout::RADIUS))
                        .bg(rgb(active().surface_sunken))
                        .text_size(px(scaled(text::XS)))
                        .text_color(rgb(active().text_muted))
                        .child(uri.to_owned()),
                )
                .child(
                    gpui::div()
                        .text_size(px(scaled(text::XS)))
                        .text_color(rgb(active().text_subtle))
                        .child(t!("hint-authenticator-code")),
                )
                .child(
                    gpui::div()
                        .id("totp-code")
                        .w_full()
                        .px(px(space::SM))
                        .py(px(space::XS))
                        .rounded(px(layout::RADIUS))
                        .bg(rgb(active().surface_sunken))
                        .border_1()
                        .border_color(rgb(active().border))
                        .text_size(px(scaled(text::SM)))
                        .text_color(rgb(active().text))
                        .child(panel_state.enrolment_code.to_owned())
                        .on_key_down(move |event: &gpui::KeyDownEvent, _window, cx| {
                            totp_typed(&event.keystroke.key, cx);
                        }),
                ),
        );
    }

    if !panel_state.backup_codes.is_empty() {
        let mut codes_grid = row()
            .w_full()
            .px(px(space::LG))
            .py(px(space::SM))
            .gap(px(space::SM))
            .flex_wrap();

        for (index, (code, consumed)) in panel_state.backup_codes.iter().enumerate() {
            codes_grid = codes_grid.child(
                gpui::div()
                    .id(("backup-code", index))
                    .px(px(space::SM))
                    .py(px(space::XS))
                    .rounded(px(layout::RADIUS))
                    .bg(rgb(active().surface_sunken))
                    .text_size(px(scaled(text::XS)))
                    .text_color(rgb(if *consumed {
                        active().text_subtle
                    } else {
                        active().text
                    }))
                    .child(code.clone()),
            );
        }

        form = form
            .child(
                row()
                    .w_full()
                    .px(px(space::LG))
                    .justify_between()
                    .items_center()
                    .child(
                        gpui::div()
                            .text_size(px(scaled(text::XS)))
                            .text_color(rgb(active().text_subtle))
                            .child(t!("label-backup-codes")),
                    )
                    .child(
                        gpui::div()
                            .id("regenerate-codes")
                            .px(px(space::SM))
                            .py(px(space::XS))
                            .rounded(px(layout::RADIUS))
                            .cursor_pointer()
                            .text_size(px(scaled(text::XS)))
                            .text_color(rgb(active().danger))
                            .hover(|style| style.bg(rgb(active().surface_hover)))
                            .on_click({
                                let actions_codes = backup_codes_action.clone();
                                move |_, _, cx| actions_codes(true, cx)
                            })
                            .child(t!("action-regenerate")),
                    ),
            )
            .child(codes_grid);
    }

    if let Some(problem) = panel_state.problem {
        form = form.child(
            gpui::div()
                .px(px(space::LG))
                .py(px(space::XS))
                .text_size(px(scaled(text::XS)))
                .text_color(rgb(active().danger))
                .child(problem.to_owned()),
        );
    }

    let panel = panel(&t!("label-account"), 520.).child(form);

    panel.child(
        row()
            .w_full()
            .px(px(space::LG))
            .py(px(space::MD))
            .gap(px(space::SM))
            .justify_end()
            .child(button(
                "account-codes",
                &t!("action-backup-codes"),
                false,
                {
                    let codes = backup_codes_action.clone();
                    move |cx| codes(false, cx)
                },
            ))
            .child(button(
                "account-enrol",
                &t!("action-two-factor"),
                false,
                actions.enrol,
            ))
            .when(panel_state.enrolment_uri.is_some(), |r| {
                r.child(button(
                    "account-enrol-finish",
                    &t!("action-finish-enrolment"),
                    true,
                    actions.submit_enrolment,
                ))
            })
            .when(panel_state.enrolment_uri.is_none(), |r| {
                r.child(button(
                    "account-disable",
                    &t!("action-disable-two-factor"),
                    false,
                    actions.disable,
                ))
            })
            .when(panel_state.problem.is_none(), |r| {
                r.child(button(
                    "account-save",
                    &t!("action-save"),
                    true,
                    actions.save,
                ))
            })
            .child(button(
                "account-close",
                &t!("action-close"),
                false,
                actions.close,
            )),
    )
}
