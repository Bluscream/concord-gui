use gpui::{prelude::*, px, rgb, Context, IntoElement};

use crate::theme::{active, layout, scaled, space, text};
use crate::ui::chrome::{avatar, avatar_with_url, column, presence_dot, row, section_label, sidebar_row};
use crate::ui::workspace::{ContextSubject, Pane, Workspace};

impl Workspace {
    pub(super) fn member_pane_impl(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut pane = column()
            .w(px(layout::MEMBERS))
            .h_full()
            .bg(rgb(active().surface_sunken))
            .border_l_1()
            .border_color(rgb(active().border))
            .pt(px(space::SM))
            .overflow_hidden();

        if self.model.members.is_empty() {
            return pane.child(
                gpui::div()
                    .px(px(space::MD))
                    .pt(px(space::MD))
                    .text_size(px(scaled(text::XS)))
                    .text_color(rgb(active().text_subtle))
                    .child(concord::t!("status-no-member-data")),
            );
        }

        for (pane_index, member) in self.model.members.iter().enumerate() {
            if member.is_group {
                pane = pane.child(section_label(member.name.clone()));
                continue;
            }

            let mut entry = sidebar_row(false)
                .when(self.options.display.show_avatars, |d| {
                    let avatar = self.still_avatar(member.avatar.as_deref());
                    d.child(avatar_with_url(
                        layout::AVATAR_SM,
                        &member.name,
                        avatar.as_deref(),
                        self.options.display.circular_avatars,
                    ))
                })
                .child(presence_dot(member.presence))
                .child(
                    column()
                        .flex_1()
                        .overflow_hidden()
                        .child(
                            gpui::div()
                                .text_color(rgb(member.color.unwrap_or(active().text_muted)))
                                .child(member.name.clone()),
                        )
                        .children(member.activity.clone().map(|activity| {
                            gpui::div()
                                .text_size(px(scaled(text::XS)))
                                .text_color(rgb(active().text_subtle))
                                .child(activity)
                        })),
                );

            if member.is_bot {
                entry = entry.child(
                    gpui::div()
                        .px(px(4.))
                        .rounded(px(3.))
                        .bg(rgb(active().accent))
                        .text_size(px(scaled(text::XS)))
                        .text_color(rgb(active().on_accent))
                        .child("BOT"),
                );
            }

            if self.focus_pane == Pane::Members
                && !member.is_group
                && !self.passes_filter(&member.name)
            {
                continue;
            }

            let entry = match member.user_id {
                Some(user_id) => entry
                    .id(("member", pane_index))
                    .cursor_pointer()
                    .on_mouse_down(
                        gpui::MouseButton::Right,
                        cx.listener(move |this, event: &gpui::MouseDownEvent, _window, cx| {
                            this.open_context_menu(ContextSubject::Member(user_id), event.position);
                            cx.notify();
                        }),
                    )
                    .on_click(cx.listener(move |this, _event, _window, cx| {
                        this.open_profile(user_id);
                        cx.notify();
                    }))
                    .into_any_element(),
                None => entry.into_any_element(),
            };

            pane = pane.child(entry);
        }

        pane
    }
}
