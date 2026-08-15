//! Modal panels drawn above the workspace.
//!
//! Every panel here has workspace state and key handling but had no render
//! path, so the features behind them - the quick switcher, the emoji picker,
//! screenshare, confirmations, reaction users, the mention inbox - existed in
//! the command layer and were invisible on screen.
//!
//! They share one layer because only one can be open at a time, and because a
//! modal needs a scrim: without one, clicks fall through to the workspace
//! underneath and act on whatever happens to be there.

use gpui::{Div, Stateful, prelude::*, px, rgb};

use concord::t;

use crate::theme::{active, layout, scaled, space, text};
use crate::ui::chrome::{column, row};

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
        // Layered above the workspace but below nothing else: only one modal
        // is ever open, so a single level is enough.
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

/// Confirmation dialog.
pub fn confirm_view(
    prompt: &str,
    on_confirm: impl Fn(&mut gpui::App) + 'static,
    on_cancel: impl Fn(&mut gpui::App) + 'static,
) -> Div {
    panel(&t!("label-confirm"), 380.)
        .child(
            gpui::div()
                .px(px(space::LG))
                .py(px(space::MD))
                .text_size(px(scaled(text::SM)))
                .text_color(rgb(active().text_muted))
                .child(prompt.to_string()),
        )
        .child(
            row()
                .w_full()
                .px(px(space::LG))
                .py(px(space::MD))
                .gap(px(space::SM))
                .justify_end()
                .child(button(
                    "confirm-cancel",
                    &t!("action-cancel"),
                    false,
                    on_cancel,
                ))
                .child(button(
                    "confirm-ok",
                    &t!("action-confirm"),
                    true,
                    on_confirm,
                )),
        )
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
                // Distinct from "nobody reacted": the reaction exists, so the
                // list is still arriving.
                .child(t!("status-loading")),
        );
    }

    for user in users {
        list = list.child(
            row()
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
        row()
            .w_full()
            .px(px(space::LG))
            .py(px(space::MD))
            .justify_end()
            .child(button(
                "reaction-users-close",
                &t!("action-close"),
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
                .child(t!("status-no-mentions")),
        );
    }

    for (index, entry) in rows.iter().enumerate() {
        let open = on_open.clone();
        let dismiss = on_dismiss.clone();

        list = list.child(
            row()
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

    panel(&t!("label-mentions"), 460.).child(list).child(
        row()
            .w_full()
            .px(px(space::LG))
            .py(px(space::MD))
            .justify_end()
            .child(button("inbox-close", &t!("action-close"), false, on_close)),
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
                    // The device list is fetched, so an empty one before the
                    // reply arrives is not the same as having no devices.
                    .child(t!("status-no-devices")),
            );
        }

        for (index, (id, name)) in devices.iter().enumerate() {
            let active_device = selected == Some(id.as_str());
            let pick = on_pick.clone();
            let id = id.clone();

            body = body.child(
                row()
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

    panel(&t!("label-audio-devices"), 400.).child(body).child(
        row()
            .w_full()
            .px(px(space::LG))
            .py(px(space::MD))
            .justify_end()
            .child(button(
                "devices-close",
                &t!("action-close"),
                false,
                on_close,
            )),
    )
}

/// Single-line text prompt: a title, the text as typed, and save/cancel.
pub fn text_prompt_view(
    title: &str,
    placeholder: &str,
    current: &str,
    on_submit: impl Fn(&mut gpui::App) + 'static,
    on_cancel: impl Fn(&mut gpui::App) + 'static,
) -> Div {
    panel(title, 380.)
        .child(
            gpui::div()
                .px(px(space::LG))
                .py(px(space::MD))
                .text_size(px(scaled(text::SM)))
                .text_color(rgb(if current.is_empty() {
                    active().text_subtle
                } else {
                    active().text
                }))
                .child(if current.is_empty() {
                    placeholder.to_string()
                } else {
                    current.to_string()
                }),
        )
        .child(
            row()
                .w_full()
                .px(px(space::LG))
                .py(px(space::MD))
                .gap(px(space::SM))
                .justify_end()
                .child(button(
                    "prompt-cancel",
                    &t!("action-cancel"),
                    false,
                    on_cancel,
                ))
                .child(button("prompt-save", &t!("action-save"), false, on_submit)),
        )
}

/// What an invite points at, as shown before joining.
pub struct InviteRow {
    pub guild_name: String,
    pub channel_name: Option<String>,
    pub inviter: Option<String>,
    pub member_count: Option<u64>,
    pub online_count: Option<u64>,
    pub already_joined: bool,
    /// Set while the lookup is still running, or when it failed.
    pub status: Option<String>,
}

/// Invite preview, with the join confirmation.
pub fn invite_view(
    invite: &InviteRow,
    on_accept: impl Fn(&mut gpui::App) + 'static,
    on_cancel: impl Fn(&mut gpui::App) + 'static,
) -> Div {
    let mut body = column()
        .px(px(space::LG))
        .py(px(space::MD))
        .gap(px(space::XS));

    body = body.child(
        gpui::div()
            .text_size(px(scaled(text::BASE)))
            .text_color(rgb(active().text))
            .child(invite.guild_name.clone()),
    );

    // Counts are what tell someone whether this is the server they meant, so
    // they are shown when Discord provided them and omitted when it did not -
    // never guessed at.
    if let (Some(members), Some(online)) = (invite.member_count, invite.online_count) {
        body = body.child(
            gpui::div()
                .text_size(px(scaled(text::XS)))
                .text_color(rgb(active().text_muted))
                .child(format!("{members} members, {online} online")),
        );
    }

    for (label, value) in [
        ("Channel", invite.channel_name.clone()),
        ("Invited by", invite.inviter.clone()),
    ] {
        if let Some(value) = value {
            body = body.child(
                gpui::div()
                    .text_size(px(scaled(text::XS)))
                    .text_color(rgb(active().text_subtle))
                    .child(format!("{label}: {value}")),
            );
        }
    }

    if let Some(status) = &invite.status {
        body = body.child(
            gpui::div()
                .text_size(px(scaled(text::XS)))
                .text_color(rgb(active().danger))
                .child(status.clone()),
        );
    }

    let joinable = invite.status.is_none() && !invite.already_joined;
    if invite.already_joined {
        body = body.child(
            gpui::div()
                .text_size(px(scaled(text::XS)))
                .text_color(rgb(active().text_muted))
                .child(t!("status-already-joined")),
        );
    }

    panel(&t!("label-join-server"), 400.).child(body).child(
        row()
            .w_full()
            .px(px(space::LG))
            .py(px(space::MD))
            .gap(px(space::SM))
            .justify_end()
            .child(button(
                "invite-cancel",
                &t!("action-cancel"),
                false,
                on_cancel,
            ))
            .children(joinable.then(|| button("invite-join", &t!("action-join"), true, on_accept))),
    )
}

/// One sticker offered by the picker.
pub struct StickerChoice {
    pub name: String,
    /// `None` for formats that cannot be shown as an image, which are listed
    /// by name rather than hidden.
    pub image: Option<std::sync::Arc<gpui::Image>>,
}

/// Guild stickers, to send with the next message.
pub fn sticker_picker_view(
    stickers: &[StickerChoice],
    on_pick: impl Fn(usize, &mut gpui::App) + Clone + 'static,
    on_close: impl Fn(&mut gpui::App) + 'static,
) -> Div {
    let mut body = row()
        .id("sticker-grid")
        .p(px(space::MD))
        .gap(px(space::SM))
        .flex_wrap()
        .max_h(px(360.))
        .overflow_y_scroll();

    if stickers.is_empty() {
        body = body.child(
            gpui::div()
                .text_size(px(scaled(text::SM)))
                .text_color(rgb(active().text_subtle))
                // Only the guild's own stickers are sendable without Nitro,
                // so a guild with none has nothing to offer here.
                .child(t!("status-no-stickers")),
        );
    }

    for (index, sticker) in stickers.iter().enumerate() {
        let pick = on_pick.clone();

        body = body.child(
            gpui::div()
                .id(("sticker", index))
                .w(px(88.))
                .p(px(space::XS))
                .rounded(px(layout::RADIUS))
                .cursor_pointer()
                .hover(|style| style.bg(rgb(active().surface_hover)))
                .child(match &sticker.image {
                    Some(image) => gpui::img(image.clone())
                        .w(px(80.))
                        .h(px(80.))
                        .into_any_element(),
                    None => gpui::div()
                        .text_size(px(scaled(text::XS)))
                        .text_color(rgb(active().text_muted))
                        .child(sticker.name.clone())
                        .into_any_element(),
                })
                .on_click(move |_event, _window, cx| pick(index, cx)),
        );
    }

    panel(&t!("label-stickers"), 420.).child(body).child(
        row()
            .w_full()
            .px(px(space::LG))
            .py(px(space::MD))
            .justify_end()
            .child(button(
                "sticker-close",
                &t!("action-close"),
                false,
                on_close,
            )),
    )
}

/// One role offered by the picker.
pub struct RoleChoice {
    pub name: String,
    pub color: Option<u32>,
    pub assigned: bool,
    /// Why this role cannot be changed, when it cannot.
    pub disabled_reason: Option<&'static str>,
}

/// Roles for a member, toggled here and sent as a set on save.
pub fn role_picker_view(
    roles: &[RoleChoice],
    on_toggle: impl Fn(usize, &mut gpui::App) + Clone + 'static,
    on_save: impl Fn(&mut gpui::App) + 'static,
    on_cancel: impl Fn(&mut gpui::App) + 'static,
) -> Div {
    let mut list = column().id("role-list").max_h(px(360.)).overflow_y_scroll();

    if roles.is_empty() {
        list = list.child(
            gpui::div()
                .px(px(space::LG))
                .py(px(space::MD))
                .text_size(px(scaled(text::SM)))
                .text_color(rgb(active().text_subtle))
                .child(t!("status-no-assignable-roles")),
        );
    }

    for (index, role) in roles.iter().enumerate() {
        let toggle = on_toggle.clone();
        let refused = role.disabled_reason;

        list = list.child(
            row()
                .id(("role", index))
                .w_full()
                .px(px(space::LG))
                .py(px(space::XS))
                .gap(px(space::SM))
                .items_center()
                .when(refused.is_none(), |entry| {
                    entry
                        .cursor_pointer()
                        .hover(|style| style.bg(rgb(active().surface_hover)))
                        .on_click(move |_event, _window, cx| toggle(index, cx))
                })
                .child(
                    gpui::div()
                        .w(px(20.))
                        .text_size(px(scaled(text::SM)))
                        .text_color(rgb(active().text_muted))
                        // The marker carries the state, so scanning the column
                        // shows what the member has without reading each line.
                        .child(if role.assigned { "[x]" } else { "[ ]" }),
                )
                .child(
                    gpui::div()
                        .flex_1()
                        .text_size(px(scaled(text::SM)))
                        // Role colour, which is how they are told apart at a
                        // glance everywhere else in Discord.
                        .text_color(rgb(role.color.filter(|color| *color != 0).unwrap_or(
                            if refused.is_some() {
                                active().text_subtle
                            } else {
                                active().text
                            },
                        )))
                        .child(role.name.clone()),
                )
                .children(refused.map(|reason| {
                    gpui::div()
                        .text_size(px(scaled(text::XS)))
                        .text_color(rgb(active().text_subtle))
                        .child(reason)
                })),
        );
    }

    panel(&t!("label-roles"), 420.).child(list).child(
        row()
            .w_full()
            .px(px(space::LG))
            .py(px(space::MD))
            .gap(px(space::SM))
            .justify_end()
            .child(button(
                "roles-cancel",
                &t!("action-cancel"),
                false,
                on_cancel,
            ))
            .child(button("roles-save", &t!("action-save"), true, on_save)),
    )
}

/// One row in the ban list.
pub struct BanRow {
    pub username: String,
    pub reason: Option<String>,
}

/// A guild's bans, with the option to lift one.
pub fn ban_list_view(
    bans: &[BanRow],
    status: Option<&str>,
    on_unban: impl Fn(usize, &mut gpui::App) + Clone + 'static,
    on_close: impl Fn(&mut gpui::App) + 'static,
) -> Div {
    let mut list = column().id("ban-list").max_h(px(360.)).overflow_y_scroll();

    if let Some(status) = status {
        list = list.child(
            gpui::div()
                .px(px(space::LG))
                .py(px(space::MD))
                .text_size(px(scaled(text::SM)))
                .text_color(rgb(active().text_subtle))
                .child(status.to_owned()),
        );
    }

    for (index, ban) in bans.iter().enumerate() {
        let unban = on_unban.clone();

        list = list.child(
            row()
                .w_full()
                .px(px(space::LG))
                .py(px(space::SM))
                .gap(px(space::SM))
                .items_center()
                .child(
                    column()
                        .flex_1()
                        .child(
                            gpui::div()
                                .text_size(px(scaled(text::SM)))
                                .text_color(rgb(active().text))
                                .child(ban.username.clone()),
                        )
                        .children(ban.reason.clone().map(|reason| {
                            gpui::div()
                                .text_size(px(scaled(text::XS)))
                                .text_color(rgb(active().text_subtle))
                                .child(reason)
                        })),
                )
                .child(
                    gpui::div()
                        .id(("unban", index))
                        .px(px(space::SM))
                        .py(px(space::XS))
                        .rounded(px(layout::RADIUS))
                        .cursor_pointer()
                        .text_size(px(scaled(text::XS)))
                        .text_color(rgb(active().accent))
                        .hover(|style| style.bg(rgb(active().surface_hover)))
                        .child("unban")
                        .on_click(move |_event, _window, cx| unban(index, cx)),
                ),
        );
    }

    panel(&t!("label-bans"), 460.).child(list).child(
        row()
            .w_full()
            .px(px(space::LG))
            .py(px(space::MD))
            .justify_end()
            .child(button("bans-close", &t!("action-close"), false, on_close)),
    )
}

/// A risk warning: what the risk is, and a way to proceed anyway.
///
/// Never a refusal. The account is the user's and so is the decision; this
/// exists to make it an informed one. "Don't ask again" is offered because a
/// warning that cannot be dismissed becomes noise, and noise gets clicked
/// through without reading.
///
/// The six strings travel together because they are all translated and all
/// belong to the same warning; passing them as one value keeps the signature
/// readable and the call site from depending on their order.
pub struct RiskWarning<'a> {
    pub title: &'a str,
    pub body: &'a str,
    pub dont_ask_label: &'a str,
    pub dont_ask: bool,
    pub continue_label: &'a str,
    pub cancel_label: &'a str,
}

pub fn risk_warning_view(
    warning: RiskWarning<'_>,
    on_toggle: impl Fn(&mut gpui::App) + 'static,
    on_continue: impl Fn(&mut gpui::App) + 'static,
    on_cancel: impl Fn(&mut gpui::App) + 'static,
) -> Div {
    panel(warning.title, 460.)
        .child(
            gpui::div()
                .px(px(space::LG))
                .py(px(space::MD))
                .text_size(px(scaled(text::SM)))
                .text_color(rgb(active().text_muted))
                .child(warning.body.to_owned()),
        )
        .child(
            row()
                .id("warning-dont-ask")
                .w_full()
                .px(px(space::LG))
                .pb(px(space::SM))
                .gap(px(space::SM))
                .items_center()
                .cursor_pointer()
                .child(
                    gpui::div()
                        .w(px(14.))
                        .h(px(14.))
                        .rounded(px(3.))
                        .border_1()
                        .border_color(rgb(active().border))
                        .flex()
                        .items_center()
                        .justify_center()
                        .when(warning.dont_ask, |box_| {
                            box_.bg(rgb(active().accent)).child(
                                gpui::div()
                                    .text_size(px(scaled(text::XS)))
                                    .text_color(rgb(active().on_accent))
                                    // A tick from the Basic Multilingual Plane:
                                    // the emoji one renders as an empty box.
                                    .child("\u{2713}"),
                            )
                        }),
                )
                .child(
                    gpui::div()
                        .text_size(px(scaled(text::XS)))
                        .text_color(rgb(active().text_subtle))
                        .child(warning.dont_ask_label.to_owned()),
                )
                .on_click(move |_event, _window, cx| on_toggle(cx)),
        )
        .child(
            row()
                .w_full()
                .px(px(space::LG))
                .py(px(space::MD))
                .gap(px(space::SM))
                .justify_end()
                .child(button(
                    "warning-cancel",
                    warning.cancel_label,
                    false,
                    on_cancel,
                ))
                .child(button(
                    "warning-continue",
                    warning.continue_label,
                    true,
                    on_continue,
                )),
        )
}

/// One field of the activity editor.
pub struct ActivityField {
    pub label: String,
    pub placeholder: String,
    pub value: String,
    /// Which field typing goes into, since only one caret can be live.
    pub focused: bool,
}

/// A rich activity: what kind it is, and the three lines Discord shows.
///
/// Beyond a custom status, which is all the GUI could set before. The kinds
/// offered are the ones a user can honestly claim - Streaming is excluded
/// because it needs a verified stream URL to render as anything, and Custom
/// has its own simpler prompt.
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
                        // The border carries the focus, because a caret alone
                        // is easy to lose in a form of near-identical rows.
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
                // Clearing is its own button rather than "save an empty name":
                // stopping a broadcast should not require guessing that.
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
