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

/// One row in the server-management panel.
///
/// Deliberately pre-rendered to strings by the caller: the three tabs show
/// very different things - an invite, an emoji, a log entry - and a view that
/// understood all three would grow a branch per kind for no benefit.
pub struct ServerRow {
    pub primary: String,
    pub secondary: Option<String>,
    /// What the row's button does, when it has one. The audit log has none:
    /// history is not something to be edited from here.
    pub action: Option<String>,
    /// A second, non-destructive button. Only emoji have one, for renaming.
    pub secondary_action: Option<String>,
    /// A third. Only events have one: cancel, edit and interest are three
    /// different things and none of them is the other two.
    pub tertiary_action: Option<String>,
}

/// A guild's invites, emoji or audit log.
/// What the panel is showing, as one value.
///
/// The five travel together because they are all "the state of the open tab";
/// passing them separately made the signature long enough that clippy objected
/// and a caller could transpose two without the compiler noticing.
pub struct ServerPanel<'a> {
    pub tabs: &'a [(String, bool)],
    pub rows: &'a [ServerRow],
    pub empty_label: &'a str,
    pub loading: bool,
    pub error: Option<&'a str>,
    /// A tab-wide action, when the tab has one. Only emoji do, for adding.
    pub add_label: Option<&'a str>,
}

/// What the panel's buttons do, grouped for the same reason `AccountActions`
/// is: passed separately the signature was long enough that clippy objected,
/// and a caller could transpose two same-shaped closures without the compiler
/// noticing - which is exactly how the third one landed in the wrong slot.
pub struct ServerPanelActions {
    pub reload: Box<dyn Fn(&mut gpui::App)>,
    pub add: Box<dyn Fn(&mut gpui::App)>,
    pub close: Box<dyn Fn(&mut gpui::App)>,
}

pub fn server_management_view(
    panel_state: ServerPanel<'_>,
    on_tab: impl Fn(usize, &mut gpui::App) + Clone + 'static,
    on_row_action: impl Fn(usize, &mut gpui::App) + Clone + 'static,
    on_row_secondary: impl Fn(usize, &mut gpui::App) + Clone + 'static,
    on_row_tertiary: impl Fn(usize, &mut gpui::App) + Clone + 'static,
    actions: ServerPanelActions,
) -> Div {
    let on_reload = actions.reload;
    let on_add = actions.add;
    let on_close = actions.close;
    let mut tab_row = row()
        .w_full()
        .px(px(space::LG))
        .py(px(space::SM))
        .gap(px(space::XS));

    for (index, (label, selected)) in panel_state.tabs.iter().enumerate() {
        let pick = on_tab.clone();
        tab_row = tab_row.child(
            gpui::div()
                .id(("server-tab", index))
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

    let mut list = column()
        .id("server-rows")
        .max_h(px(360.))
        .overflow_y_scroll();

    // Loading, failed and empty are three different things and each says so.
    // A blank list that might mean any of them is the worst of the three.
    let notice = if panel_state.loading {
        Some(t!("status-loading"))
    } else if let Some(error) = panel_state.error {
        Some(error.to_owned())
    } else if panel_state.rows.is_empty() {
        Some(panel_state.empty_label.to_owned())
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
        let act = on_row_action.clone();
        let second = on_row_secondary.clone();
        let third = on_row_tertiary.clone();
        list = list.child(
            row()
                .id(("server-row", index))
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
                        .children(entry.secondary.as_ref().map(|detail| {
                            gpui::div()
                                .text_size(px(scaled(text::XS)))
                                .text_color(rgb(active().text_subtle))
                                .child(detail.clone())
                        })),
                )
                // Before the destructive one, so the safe button is not where
                // the eye lands last on its way to clicking.
                .children(entry.tertiary_action.as_ref().map(|label| {
                    gpui::div()
                        .id(("server-row-third", index))
                        .px(px(space::SM))
                        .py(px(space::XS))
                        .rounded(px(4.))
                        .cursor_pointer()
                        .text_size(px(scaled(text::XS)))
                        .text_color(rgb(active().text_muted))
                        .hover(|style| style.bg(rgb(active().surface_hover)))
                        .on_click(move |_event, _window, cx| third(index, cx))
                        .child(label.clone())
                }))
                .children(entry.secondary_action.as_ref().map(|label| {
                    gpui::div()
                        .id(("server-row-second", index))
                        .px(px(space::SM))
                        .py(px(space::XS))
                        .rounded(px(4.))
                        .cursor_pointer()
                        .text_size(px(scaled(text::XS)))
                        .text_color(rgb(active().text_muted))
                        .hover(|style| style.bg(rgb(active().surface_hover)))
                        .on_click(move |_event, _window, cx| second(index, cx))
                        .child(label.clone())
                }))
                .children(entry.action.as_ref().map(|label| {
                    gpui::div()
                        .id(("server-row-action", index))
                        .px(px(space::SM))
                        .py(px(space::XS))
                        .rounded(px(4.))
                        .cursor_pointer()
                        .text_size(px(scaled(text::XS)))
                        .text_color(rgb(active().danger))
                        .hover(|style| style.bg(rgb(active().surface_hover)))
                        .on_click(move |_event, _window, cx| act(index, cx))
                        .child(label.clone())
                })),
        );
    }

    panel(&t!("label-server-management"), 520.)
        .child(tab_row)
        .child(list)
        .child(
            row()
                .w_full()
                .px(px(space::LG))
                .py(px(space::MD))
                .gap(px(space::SM))
                .justify_end()
                .children(
                    panel_state
                        .add_label
                        .map(|label| button("server-add", label, false, on_add)),
                )
                .child(button(
                    "server-reload",
                    &t!("action-reload"),
                    false,
                    on_reload,
                ))
                .child(button("server-close", &t!("action-close"), true, on_close)),
        )
}

/// One image, full size.
///
/// The scrim is the close target as well as the backdrop: clicking beside a
/// full-screen image to dismiss it is what every other viewer does, and
/// hunting for a small button is not.
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
                // Only when there is more than one, so a single image does not
                // carry a meaningless "1 / 1".
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

/// One sound in the picker.
pub struct SoundRow {
    pub label: String,
    pub name: String,
    /// Unavailable sounds are shown greyed with the reason rather than hidden,
    /// so a guild that lost its boosts does not look like it lost its sounds.
    pub available: bool,
}

/// The soundboard.
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

/// One linked account, as the panel shows it.
pub struct ConnectionRow {
    pub primary: String,
    pub secondary: String,
    /// What clicking the visibility button would do, phrased as the outcome.
    pub visibility_action: String,
    pub activity_action: String,
}

impl ConnectionRow {
    pub fn new(connection: &concord::discord::Connection) -> Self {
        use concord::discord::ConnectionVisibility;
        Self {
            primary: format!("{} - {}", connection.kind, connection.name),
            secondary: connection.summary(),
            // Phrased as what clicking does, not as the state it is already
            // in: a button labelled with the current state reads as inert.
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

/// Linked accounts.
///
/// Separate from the server-management panel rather than a tab in it: these
/// belong to the account, not to a guild, and a row here needs three controls
/// where a server row has two.
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

    // Loading, failed and "none linked" are three different things, and a
    // blank list that might mean any of them is the worst of the three.
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
                // Last, so the destructive control is not between the two
                // safe ones where a stray click lands on it.
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
                // Linking is an OAuth flow through a browser, which would mean
                // handling someone else's credentials. This client does not.
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

/// One row of the privacy panel.
pub struct PrivacyRow {
    pub label: String,
    pub detail: String,
    /// Whether the setting is on. `None` when it never arrived, which is not
    /// the same as off - showing a default would describe the account as more
    /// exposed than it may be.
    pub enabled: Option<bool>,
    /// For settings that are not a plain on/off. Only DM scanning has three
    /// states, and its name is what the row must show rather than a tick.
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

/// Privacy and safety.
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
                        // A three-state setting shows its name; only a real
                        // on/off gets a tick, or "Scan every direct message"
                        // would be flattened to "On".
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

/// One row of the sessions-and-apps panel.
pub struct AccessRow {
    pub primary: String,
    pub secondary: String,
    /// What the row's button does. Sessions select for logout; apps revoke.
    pub action: String,
    pub destructive: bool,
    /// Whether a session row is currently selected for logout.
    pub selected: bool,
}

/// What the panel is showing, as one value.
///
/// Grouped for the same reason `ServerPanel` is: passed separately the
/// signature was long enough that clippy objected, and a caller could
/// transpose two booleans without the compiler noticing.
pub struct AccessPanel<'a> {
    pub rows: &'a [AccessRow],
    pub loading: bool,
    pub error: Option<&'a str>,
    /// The masked password, when the field is showing.
    pub password: Option<&'a str>,
    pub logout_enabled: bool,
}

/// Sessions and authorised applications.
///
/// One panel for both: they answer the same question - what else has access to
/// this account - and after a scare, hunting through two panels is the last
/// thing wanted.
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

    // Loading, failed and "nothing else has access" are three different
    // things, and a blank list that might mean any of them is the worst.
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

    // The password field appears only once something is selected: an always
    // present password box on a settings panel invites typing one for no
    // reason.
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
                        // Bullets. The real value never reaches this function.
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

/// What the account panel is showing.
pub struct AccountPanel<'a> {
    /// Label, drawn value, hint, and whether it is the focused field.
    pub fields: &'a [(String, String, String, bool)],
    /// Why it cannot be saved, when it cannot.
    pub problem: Option<&'a str>,
    /// The enrolment URI while two-factor is being set up. Shown on purpose:
    /// enrolment cannot happen unless it reaches the authenticator app.
    pub enrolment_uri: Option<&'a str>,
    pub enrolment_code: &'a str,
    /// Backup codes and whether each is used.
    pub backup_codes: &'a [(String, bool)],
}

/// What the account panel's buttons do.
///
/// Grouped for the same reason `ServerPanel` and `AccessPanel` are: passed
/// separately the signature was long enough that clippy objected.
/// A button's handler.
type AccountAction = Box<dyn Fn(&mut gpui::App)>;
/// The backup-codes handler, which also carries whether to regenerate.
type BackupCodesAction = Box<dyn Fn(bool, &mut gpui::App)>;

pub struct AccountActions {
    pub save: AccountAction,
    pub enrol: AccountAction,
    pub submit_enrolment: AccountAction,
    pub disable: AccountAction,
    pub backup_codes: BackupCodesAction,
    pub close: AccountAction,
}

/// Credentials and two-factor.
pub fn account_view(
    panel_state: AccountPanel<'_>,
    on_focus: impl Fn(usize, &mut gpui::App) + Clone + 'static,
    on_key: impl Fn(&str, &mut gpui::App) + Clone + 'static,
    on_code: impl Fn(&str, &mut gpui::App) + Clone + 'static,
    actions: AccountActions,
) -> Div {
    let mut list = column()
        .id("account-fields")
        .max_h(px(340.))
        .overflow_y_scroll();

    for (index, (label, value, hint, focused)) in panel_state.fields.iter().enumerate() {
        let focus = on_focus.clone();
        let typed = on_key.clone();
        list = list.child(
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
                        // Already bullets for a credential; the real value
                        // never reaches this function.
                        .child(value.clone())
                        .when(*focused, |field| {
                            field.on_key_down(move |event: &gpui::KeyDownEvent, _window, cx| {
                                typed(&event.keystroke.key, cx);
                            })
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

    let mut panel = panel(&t!("label-account"), 560.).child(list);

    if let Some(problem) = panel_state.problem {
        panel = panel.child(
            gpui::div()
                .px(px(space::LG))
                .py(px(space::XS))
                .text_size(px(scaled(text::XS)))
                .text_color(rgb(active().text_subtle))
                .child(problem.to_owned()),
        );
    }

    if let Some(uri) = panel_state.enrolment_uri {
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
                        .child(t!("hint-totp-enrolment")),
                )
                // The whole URI, so it can be copied into an authenticator app
                // on another machine. Truncating it would make it useless.
                .child(
                    gpui::div()
                        .text_size(px(scaled(text::XS)))
                        .text_color(rgb(active().text))
                        .child(uri.to_owned()),
                )
                .child(
                    gpui::div()
                        .id("totp-code")
                        .w_full()
                        .px(px(space::SM))
                        .py(px(space::XS))
                        .rounded(px(layout::RADIUS))
                        .bg(rgb(active().surface_sunken))
                        .text_size(px(scaled(text::SM)))
                        .text_color(rgb(active().text))
                        .child(panel_state.enrolment_code.to_owned())
                        .on_key_down({
                            let typed = on_code.clone();
                            move |event: &gpui::KeyDownEvent, _window, cx| {
                                typed(&event.keystroke.key, cx);
                            }
                        }),
                ),
        );
    }

    if !panel_state.backup_codes.is_empty() {
        let mut codes = row().w_full().flex_wrap().gap(px(space::SM));
        for (code, consumed) in panel_state.backup_codes {
            codes = codes.child(
                gpui::div()
                    .text_size(px(scaled(text::SM)))
                    // A used code is dimmed rather than hidden, so the count
                    // on screen matches the count Discord issued.
                    .text_color(rgb(if *consumed {
                        active().text_subtle
                    } else {
                        active().text
                    }))
                    .child(code.clone()),
            );
        }
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
                        .child(t!("hint-backup-codes")),
                )
                .child(codes),
        );
    }

    panel.child(
        row()
            .w_full()
            .px(px(space::LG))
            .py(px(space::MD))
            .gap(px(space::SM))
            .justify_end()
            // Fetching the codes, then regenerating - regenerating is last
            // because it invalidates the ones just fetched.
            .child(button(
                "account-codes",
                &t!("action-backup-codes"),
                false,
                {
                    let codes = actions.backup_codes;
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
            // Only when no enrolment is in progress, or this would offer to
            // disable using a code meant for the enrolment being set up.
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

/// One entry in a context menu.
pub struct ContextItem {
    pub label: String,
    /// Why it cannot be used, when it cannot. Shown greyed with the reason
    /// rather than hidden, so the menu teaches what a permission is for.
    pub disabled_reason: Option<String>,
    /// Destructive entries are coloured, because a menu is a fast path and a
    /// fast path to deleting something should look like one.
    pub destructive: bool,
}

/// A context menu at the pointer.
///
/// Positioned rather than centred: a context menu that appears in the middle
/// of the screen has lost the context it is named for.
pub fn context_menu_view(
    items: &[ContextItem],
    at: gpui::Point<gpui::Pixels>,
    on_pick: impl Fn(usize, &mut gpui::App) + Clone + 'static,
) -> Div {
    let mut menu = column()
        .absolute()
        .left(at.x)
        .top(at.y)
        .min_w(px(180.))
        .py(px(space::XS))
        .rounded(px(layout::RADIUS))
        .bg(rgb(active().surface))
        .border_1()
        .border_color(rgb(active().border));

    for (index, item) in items.iter().enumerate() {
        let pick = on_pick.clone();
        let enabled = item.disabled_reason.is_none();

        menu = menu.child(
            column()
                .id(("context-item", index))
                .w_full()
                .px(px(space::SM))
                .py(px(space::XS))
                .when(enabled, |row| {
                    row.cursor_pointer()
                        .hover(|style| style.bg(rgb(active().surface_hover)))
                        .on_click(move |_event, _window, cx| pick(index, cx))
                })
                .child(
                    gpui::div()
                        .text_size(px(scaled(text::SM)))
                        .text_color(rgb(if !enabled {
                            active().text_subtle
                        } else if item.destructive {
                            active().danger
                        } else {
                            active().text
                        }))
                        .child(item.label.clone()),
                )
                .children(item.disabled_reason.as_ref().map(|reason| {
                    gpui::div()
                        .text_size(px(scaled(text::XS)))
                        .text_color(rgb(active().text_subtle))
                        .child(reason.clone())
                })),
        );
    }
    menu
}

/// How one permission stands.
///
/// Three states rather than two, because a channel overwrite has an inherit
/// that a role does not. Showing two where there are three would turn inherit
/// into deny without saying so.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PermissionState {
    Allow,
    Inherit,
    Deny,
}

impl PermissionState {
    /// The marker carries the state, so the grid reads without colour.
    pub fn marker(self) -> &'static str {
        match self {
            Self::Allow => "[+]",
            Self::Inherit => "[ ]",
            Self::Deny => "[-]",
        }
    }
}

/// One switch in the permission grid.
pub struct PermissionRow {
    pub label: String,
    pub description: String,
    pub setting: PermissionState,
}

/// What a role may do.
pub fn permission_grid_view(
    title: &str,
    rows: &[PermissionRow],
    dirty: bool,
    on_toggle: impl Fn(usize, &mut gpui::App) + Clone + 'static,
    on_save: impl Fn(&mut gpui::App) + 'static,
    on_cancel: impl Fn(&mut gpui::App) + 'static,
) -> Div {
    let mut list = column()
        .id("permission-list")
        .max_h(px(420.))
        .overflow_y_scroll();

    for (index, row) in rows.iter().enumerate() {
        let toggle = on_toggle.clone();
        list = list.child(
            column()
                .id(("permission", index))
                .w_full()
                .px(px(space::LG))
                .py(px(space::XS))
                .cursor_pointer()
                .hover(|style| style.bg(rgb(active().surface_hover)))
                .on_click(move |_event, _window, cx| toggle(index, cx))
                .child(
                    gpui::div()
                        .text_size(px(scaled(text::SM)))
                        .text_color(rgb(match row.setting {
                            PermissionState::Allow => active().success,
                            PermissionState::Deny => active().danger,
                            PermissionState::Inherit => active().text_subtle,
                        }))
                        .child(format!("{} {}", row.setting.marker(), row.label)),
                )
                .child(
                    gpui::div()
                        .text_size(px(scaled(text::XS)))
                        .text_color(rgb(active().text_subtle))
                        .child(row.description.clone()),
                ),
        );
    }

    // Said in the title because cancelling discards, and a grid this long is
    // easy to walk away from by accident.
    let heading = if dirty {
        format!("{title} - {}", t!("status-unsaved"))
    } else {
        title.to_owned()
    };

    panel(&heading, 460.).child(list).child(
        row()
            .w_full()
            .px(px(space::LG))
            .py(px(space::MD))
            .gap(px(space::SM))
            .justify_end()
            .child(button(
                "permissions-cancel",
                &t!("action-cancel"),
                false,
                on_cancel,
            ))
            .child(button(
                "permissions-save",
                &t!("action-save"),
                true,
                on_save,
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
        // Not what the connection currently is. A button reading "Hidden" on a
        // hidden connection looks like a label rather than a control, and the
        // two readings are opposites - so getting this backwards would make
        // every click do the reverse of what the button appeared to offer.
        let hidden = ConnectionRow::new(&connection(ConnectionVisibility::Hidden, false));
        assert_eq!(hidden.visibility_action, t!("action-connection-show"));
        assert_eq!(hidden.activity_action, t!("action-connection-activity-on"));

        let shown = ConnectionRow::new(&connection(ConnectionVisibility::Everyone, true));
        assert_eq!(shown.visibility_action, t!("action-connection-hide"));
        assert_eq!(shown.activity_action, t!("action-connection-activity-off"));
    }

    #[test]
    fn the_two_controls_are_independent() {
        // They share one request, so a row that derived one from the other
        // would be the visible half of sending a stale value for the other.
        let row = ConnectionRow::new(&connection(ConnectionVisibility::Everyone, false));
        assert_eq!(row.visibility_action, t!("action-connection-hide"));
        assert_eq!(row.activity_action, t!("action-connection-activity-on"));
    }
}

#[cfg(test)]
mod privacy_row_tests {
    use super::*;
    use concord::discord::{DmScanLevel, PrivacySetting, PrivacyState};

    #[test]
    fn a_three_state_setting_shows_its_name_rather_than_a_tick() {
        // "Scan every direct message" and "Scan messages from non-friends" are
        // both "on"; a tick would make two different settings look identical.
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
