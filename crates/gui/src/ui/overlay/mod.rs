//! Modal panels drawn above the workspace.

mod prompts;
mod server;
mod user_settings;

pub use prompts::*;
pub use server::*;
pub use user_settings::*;

use gpui::{Div, Stateful, prelude::*, px, rgb};

use crate::theme::{active, layout, scaled, space, text};
use crate::ui::chrome::column;

/// The dimmed backdrop, which also swallows clicks aimed past the modal.
pub fn scrim() -> Div {
    gpui::div()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .bg(gpui::rgba(0x0000_0099))
}

/// A modal container: title, body, and an optional footer.
pub fn panel(title: &str, width: f32) -> Div {
    column()
        .w(px(width))
        .rounded(px(layout::RADIUS_LG))
        .bg(rgb(active().surface))
        .border_1()
        .border_color(rgb(active().border))
        .overflow_hidden()
        .child(
            gpui::div()
                .w_full()
                .px(px(space::LG))
                .py(px(space::MD))
                .border_b_1()
                .border_color(rgb(active().border))
                .text_size(px(scaled(text::BASE)))
                .text_color(rgb(active().text))
                .child(title.to_string()),
        )
}

/// A button in a modal footer.
pub fn button(
    id: &'static str,
    label: &str,
    danger: bool,
    on_click: impl Fn(&mut gpui::App) + 'static,
) -> Stateful<Div> {
    gpui::div()
        .id(id)
        .px(px(space::MD))
        .py(px(space::XS))
        .rounded(px(layout::RADIUS))
        .cursor_pointer()
        .text_size(px(scaled(text::SM)))
        .text_color(rgb(if danger {
            active().on_accent
        } else {
            active().text
        }))
        .bg(rgb(if danger {
            active().danger
        } else {
            active().surface_sunken
        }))
        .hover(|style| style.bg(rgb(active().surface_hover)))
        .child(label.to_string())
        .on_click(move |_event, _window, cx| on_click(cx))
}

/// Who reacted with a given emoji.
pub fn reaction_users_view(
    glyph: &str,
    users: &[String],
    on_close: impl Fn(&mut gpui::App) + 'static,
) -> Div {
    let mut list = column()
        .id("reaction-users")
        .max_h(px(320.))
        .overflow_y_scroll();

    if users.is_empty() {
        list = list.child(
            gpui::div()
                .px(px(space::LG))
                .py(px(space::MD))
                .text_size(px(scaled(text::SM)))
                .text_color(rgb(active().text_subtle))
                .child(concord::t!("status-loading")),
        );
    }

    for user in users {
        list = list.child(
            crate::ui::chrome::row()
                .w_full()
                .px(px(space::LG))
                .py(px(space::XS))
                .text_size(px(scaled(text::SM)))
                .text_color(rgb(active().text))
                .child(user.clone()),
        );
    }

    panel(
        &concord::i18n::translate_text("label-reacted-with", &[("emoji", glyph)]),
        320.,
    )
    .child(list)
    .child(
        crate::ui::chrome::row()
            .w_full()
            .px(px(space::LG))
            .py(px(space::MD))
            .justify_end()
            .child(button(
                "reaction-users-close",
                &concord::t!("action-close"),
                false,
                on_close,
            )),
    )
}

/// One entry in the mention inbox, as rendered.
pub struct InboxRow {
    pub author: String,
    pub content: String,
}

/// Recent mentions across every guild.
pub fn inbox_view(
    rows: &[InboxRow],
    on_open: impl Fn(usize, &mut gpui::App) + Clone + 'static,
    on_dismiss: impl Fn(usize, &mut gpui::App) + Clone + 'static,
    on_close: impl Fn(&mut gpui::App) + 'static,
) -> Div {
    let mut list = column()
        .id("inbox-list")
        .max_h(px(420.))
        .overflow_y_scroll();

    if rows.is_empty() {
        list = list.child(
            gpui::div()
                .px(px(space::LG))
                .py(px(space::MD))
                .text_size(px(scaled(text::SM)))
                .text_color(rgb(active().text_subtle))
                .child(concord::t!("status-no-mentions")),
        );
    }

    for (index, entry) in rows.iter().enumerate() {
        let open = on_open.clone();
        let dismiss = on_dismiss.clone();

        list = list.child(
            crate::ui::chrome::row()
                .id(("inbox-row", index))
                .w_full()
                .px(px(space::LG))
                .py(px(space::SM))
                .gap(px(space::SM))
                .cursor_pointer()
                .hover(|style| style.bg(rgb(active().surface_hover)))
                .child(
                    column()
                        .flex_1()
                        .child(
                            gpui::div()
                                .text_size(px(scaled(text::SM)))
                                .text_color(rgb(active().text))
                                .child(entry.author.clone()),
                        )
                        .child(
                            gpui::div()
                                .text_size(px(scaled(text::XS)))
                                .text_color(rgb(active().text_muted))
                                .child(entry.content.clone()),
                        ),
                )
                .child(
                    gpui::div()
                        .id(("inbox-dismiss", index))
                        .px(px(space::XS))
                        .rounded(px(layout::RADIUS))
                        .cursor_pointer()
                        .text_size(px(scaled(text::XS)))
                        .text_color(rgb(active().text_subtle))
                        .hover(|style| style.text_color(rgb(active().text)))
                        .child("dismiss")
                        .on_click(move |_event, _window, cx| dismiss(index, cx)),
                )
                .on_click(move |_event, _window, cx| open(index, cx)),
        );
    }

    panel(&concord::t!("label-mentions"), 460.).child(list).child(
        crate::ui::chrome::row()
            .w_full()
            .px(px(space::LG))
            .py(px(space::MD))
            .justify_end()
            .child(button("inbox-close", &concord::t!("action-close"), false, on_close)),
    )
}

/// Audio input and output device selection.
pub fn audio_devices_view(
    inputs: &[(String, String)],
    outputs: &[(String, String)],
    selected_input: Option<&str>,
    selected_output: Option<&str>,
    error: Option<&str>,
    on_pick: impl Fn(bool, String, &mut gpui::App) + Clone + 'static,
    on_close: impl Fn(&mut gpui::App) + 'static,
) -> Div {
    let mut body = column().p(px(space::MD)).gap(px(space::SM));

    if let Some(error) = error {
        body = body.child(
            gpui::div()
                .text_size(px(scaled(text::XS)))
                .text_color(rgb(active().danger))
                .child(error.to_string()),
        );
    }

    for (is_input, label, devices, selected) in [
        (true, "Input", inputs, selected_input),
        (false, "Output", outputs, selected_output),
    ] {
        body = body.child(
            gpui::div()
                .text_size(px(scaled(text::XS)))
                .text_color(rgb(active().text_subtle))
                .child(label),
        );

        if devices.is_empty() {
            body = body.child(
                gpui::div()
                    .text_size(px(scaled(text::XS)))
                    .text_color(rgb(active().text_subtle))
                    .child(concord::t!("status-no-devices")),
            );
        }

        for (index, (id, name)) in devices.iter().enumerate() {
            let active_device = selected == Some(id.as_str());
            let pick = on_pick.clone();
            let id = id.clone();

            body = body.child(
                crate::ui::chrome::row()
                    .id((if is_input { "input" } else { "output" }, index))
                    .w_full()
                    .px(px(space::SM))
                    .py(px(space::XS))
                    .rounded(px(layout::RADIUS))
                    .cursor_pointer()
                    .text_size(px(scaled(text::SM)))
                    .text_color(rgb(if active_device {
                        active().text
                    } else {
                        active().text_muted
                    }))
                    .when(active_device, |d| d.bg(rgb(active().surface_active)))
                    .hover(|style| style.bg(rgb(active().surface_hover)))
                    .child(name.clone())
                    .on_click(move |_event, _window, cx| pick(is_input, id.clone(), cx)),
            );
        }
    }

    panel(&concord::t!("label-audio-devices"), 400.).child(body).child(
        crate::ui::chrome::row()
            .w_full()
            .px(px(space::LG))
            .py(px(space::MD))
            .justify_end()
            .child(button(
                "devices-close",
                &concord::t!("action-close"),
                false,
                on_close,
            )),
    )
}

#[cfg(test)]
mod connection_row_tests {
    use super::*;
    use concord::discord::{Connection, ConnectionVisibility};

    fn connection(visibility: ConnectionVisibility, show_activity: bool) -> Connection {
        Connection {
            id: "1".to_owned(),
            kind: "github".to_owned(),
            name: "someone".to_owned(),
            verified: true,
            show_activity,
            visibility,
        }
    }

    #[test]
    fn each_button_says_what_clicking_it_would_do() {
        let hidden = ConnectionRow::new(&connection(ConnectionVisibility::Hidden, false));
        assert_eq!(hidden.visibility_action, concord::t!("action-connection-show"));
        assert_eq!(hidden.activity_action, concord::t!("action-connection-activity-on"));

        let shown = ConnectionRow::new(&connection(ConnectionVisibility::Everyone, true));
        assert_eq!(shown.visibility_action, concord::t!("action-connection-hide"));
        assert_eq!(shown.activity_action, concord::t!("action-connection-activity-off"));
    }

    #[test]
    fn the_two_controls_are_independent() {
        let row = ConnectionRow::new(&connection(ConnectionVisibility::Everyone, false));
        assert_eq!(row.visibility_action, concord::t!("action-connection-hide"));
        assert_eq!(row.activity_action, concord::t!("action-connection-activity-on"));
    }
}

#[cfg(test)]
mod privacy_row_tests {
    use super::*;
    use concord::discord::{DmScanLevel, PrivacySetting, PrivacyState};

    #[test]
    fn a_three_state_setting_shows_its_name_rather_than_a_tick() {
        let state = PrivacyState {
            dm_scan_level: Some(DmScanLevel::NonFriends),
            ..PrivacyState::default()
        };
        let row = PrivacyRow::new(PrivacySetting::DmScanning, &state);

        assert_eq!(row.value.as_deref(), Some(DmScanLevel::NonFriends.label()));
        assert_eq!(row.enabled, Some(true));
    }

    #[test]
    fn a_plain_toggle_has_no_value_to_show() {
        let row = PrivacyRow::new(
            PrivacySetting::FriendsEveryone,
            &PrivacyState {
                friend_sources: Some(concord::discord::FriendSources {
                    everyone: true,
                    ..Default::default()
                }),
                ..PrivacyState::default()
            },
        );

        assert_eq!(row.value, None);
        assert_eq!(row.enabled, Some(true));
    }

    #[test]
    fn a_setting_that_never_arrived_is_unknown_rather_than_off() {
        let state = PrivacyState::default();
        for setting in PrivacySetting::ALL {
            assert_eq!(PrivacyRow::new(setting, &state).enabled, None);
        }
    }
}
