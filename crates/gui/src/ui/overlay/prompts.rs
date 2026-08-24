//! Prompt and dialog overlay views.

use gpui::{prelude::*, px, rgb, Div};

use concord::t;

use crate::theme::{active, layout, scaled, space, text};
use crate::ui::chrome::{column, row};
use crate::ui::overlay::{button, panel};

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

/// Single-line text prompt: a title, the text as typed, and save/cancel.
pub fn text_prompt_view(
    title: &str,
    placeholder: &str,
    current: &str,
    extra: Option<Div>,
    on_submit: impl Fn(&mut gpui::App) + 'static,
    on_cancel: impl Fn(&mut gpui::App) + 'static,
) -> Div {
    panel(title, 380.)
        .children(extra)
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
    on_toggle_dont_ask: impl Fn(&mut gpui::App) + 'static,
    on_continue: impl Fn(&mut gpui::App) + 'static,
    on_cancel: impl Fn(&mut gpui::App) + 'static,
) -> Div {
    panel(warning.title, 420.)
        .child(
            gpui::div()
                .px(px(space::LG))
                .py(px(space::MD))
                .text_size(px(scaled(text::SM)))
                .text_color(rgb(active().text_muted))
                .child(warning.body.to_string()),
        )
        .child(
            row()
                .id("risk-dont-ask")
                .w_full()
                .px(px(space::LG))
                .py(px(space::XS))
                .gap(px(space::SM))
                .items_center()
                .cursor_pointer()
                .on_click(move |_event, _window, cx| on_toggle_dont_ask(cx))
                .child(
                    gpui::div()
                        .text_size(px(scaled(text::SM)))
                        .text_color(rgb(active().text_muted))
                        .child(if warning.dont_ask { "[x]" } else { "[ ]" }),
                )
                .child(
                    gpui::div()
                        .text_size(px(scaled(text::SM)))
                        .text_color(rgb(active().text))
                        .child(warning.dont_ask_label.to_string()),
                ),
        )
        .child(
            row()
                .w_full()
                .px(px(space::LG))
                .py(px(space::MD))
                .gap(px(space::SM))
                .justify_end()
                .child(button(
                    "risk-cancel",
                    warning.cancel_label,
                    false,
                    on_cancel,
                ))
                .child(button(
                    "risk-continue",
                    warning.continue_label,
                    true,
                    on_continue,
                )),
        )
}

pub struct InviteRow {
    pub guild_name: String,
    pub channel_name: Option<String>,
    pub inviter: Option<String>,
    pub member_count: Option<u64>,
    pub online_count: Option<u64>,
    pub already_joined: bool,
    pub status: Option<String>,
}

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

pub struct StickerChoice {
    pub name: String,
    pub image: Option<std::sync::Arc<gpui::Image>>,
}

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

pub struct RoleChoice {
    pub name: String,
    pub color: Option<u32>,
    pub assigned: bool,
    pub disabled_reason: Option<&'static str>,
}

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
                        .child(if role.assigned { "[x]" } else { "[ ]" }),
                )
                .child(
                    gpui::div()
                        .flex_1()
                        .text_size(px(scaled(text::SM)))
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

pub struct BanRow {
    pub username: String,
    pub reason: Option<String>,
}

pub fn ban_list_view(
    bans: &[BanRow],
    status: Option<&str>,
    on_unban: impl Fn(usize, &mut gpui::App) + Clone + 'static,
    on_bulk_ban: impl Fn(&mut gpui::App) + 'static,
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
                .child(status.to_string()),
        );
    }

    for (index, ban) in bans.iter().enumerate() {
        let unban = on_unban.clone();
        list = list.child(
            row()
                .id(("ban-row", index))
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
                                .child(ban.username.clone()),
                        )
                        .children(ban.reason.as_ref().map(|reason| {
                            gpui::div()
                                .text_size(px(scaled(text::XS)))
                                .text_color(rgb(active().text_subtle))
                                .child(reason.clone())
                        })),
                )
                .child(
                    gpui::div()
                        .id(("unban", index))
                        .px(px(space::SM))
                        .py(px(space::XS))
                        .rounded(px(4.))
                        .cursor_pointer()
                        .text_size(px(scaled(text::XS)))
                        .text_color(rgb(active().danger))
                        .hover(|style| style.bg(rgb(active().surface_hover)))
                        .on_click(move |_event, _window, cx| unban(index, cx))
                        .child(t!("action-unban")),
                ),
        );
    }

    panel(&t!("label-bans"), 460.)
        .child(list)
        .child(
            row()
                .w_full()
                .px(px(space::LG))
                .py(px(space::MD))
                .gap(px(space::SM))
                .justify_end()
                .child(button(
                    "bulk-ban",
                    &t!("action-bulk-ban"),
                    true,
                    on_bulk_ban,
                ))
                .child(button("bans-close", &t!("action-close"), false, on_close)),
        )
}

pub struct ContextItem {
    pub label: String,
    pub disabled_reason: Option<String>,
    pub destructive: bool,
}

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PermissionState {
    Allow,
    Inherit,
    Deny,
}

impl PermissionState {
    pub fn marker(self) -> &'static str {
        match self {
            Self::Allow => "[+]",
            Self::Inherit => "[ ]",
            Self::Deny => "[-]",
        }
    }
}

pub struct PermissionRow {
    pub label: String,
    pub description: String,
    pub setting: PermissionState,
}

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
