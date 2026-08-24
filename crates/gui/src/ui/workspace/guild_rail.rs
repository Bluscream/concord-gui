use gpui::{prelude::*, px, rgb, Context, IntoElement};

use crate::theme::{active, scaled, space, text, layout};
use crate::ui::chrome::{avatar_with_url, column};
use crate::ui::composer::Composer;
use crate::ui::workspace::{ContextSubject, Prompt, Workspace};

impl Workspace {
    pub(super) fn guild_rail_impl(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut rail = column()
            .w(px(layout::GUILD_RAIL))
            .h_full()
            .bg(rgb(active().bg))
            .items_center()
            .pt(px(space::MD))
            .gap(px(space::SM));

        let mut open_folder: Option<u64> = None;

        for (index, guild) in self.model.guilds.iter().enumerate() {
            let selected = index == self.model.selected_guild;
            let guild_id = guild.id;

            if let Some(folder) = &guild.folder
                && open_folder != Some(folder.id)
            {
                open_folder = Some(folder.id);
                let folder_id = folder.id;
                let label = folder.name.clone().unwrap_or_else(|| "Folder".to_string());
                let color = folder.color.unwrap_or(active().text_subtle);

                rail = rail.child(
                    gpui::div()
                        .id(("folder", folder_id as usize))
                        .w(px(44.))
                        .py(px(space::XS))
                        .flex()
                        .justify_center()
                        .cursor_pointer()
                        .text_size(px(scaled(text::XS)))
                        .text_color(rgb(color))
                        .hover(|style| style.bg(rgb(active().surface_hover)))
                        .child(label.chars().take(6).collect::<String>())
                        .on_click(cx.listener(move |this, _event, _window, cx| {
                            this.renaming_folder = Some((folder_id, Composer::default()));
                            cx.notify();
                        })),
                );
            } else if guild.folder.is_none() {
                open_folder = None;
            }
            rail = rail.child(
                gpui::div()
                    .id(("guild", index))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _event, _window, cx| {
                        this.open_guild(guild_id);
                        cx.notify();
                    }))
                    .when_some(guild_id, |entry, guild_id| {
                        entry.on_mouse_down(
                            gpui::MouseButton::Right,
                            cx.listener(move |this, event: &gpui::MouseDownEvent, _window, cx| {
                                this.open_context_menu(
                                    ContextSubject::Guild(guild_id),
                                    event.position,
                                );
                                cx.notify();
                            }),
                        )
                    })
                    .relative()
                    .child(avatar_with_url(44., &guild.name, guild.icon.as_deref(), true))
                    .when(selected, |d| {
                        d.border_2()
                            .border_color(rgb(active().accent))
                            .rounded_full()
                    })
                    .when(guild.unread && !selected, |d| {
                        d.border_1()
                            .border_color(rgb(active().text_muted))
                            .rounded_full()
                    })
                    .when(guild.mentions > 0 || guild.unread, |d| {
                        d.child(
                            gpui::div()
                                .absolute()
                                .bottom(px(-4.))
                                .right(px(-4.))
                                .min_w(px(18.))
                                .h(px(18.))
                                .px(px(4.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded_full()
                                .bg(rgb(if guild.mentions > 0 {
                                    active().danger
                                } else {
                                    active().surface
                                }))
                                .border_2()
                                .border_color(rgb(active().bg))
                                .text_size(px(scaled(text::XS)))
                                .text_color(rgb(if guild.mentions > 0 {
                                    active().on_accent
                                } else {
                                    active().text
                                }))
                                .child(if guild.mentions > 99 {
                                    "99+".to_string()
                                } else if guild.mentions > 0 {
                                    guild.mentions.to_string()
                                } else {
                                    "•".to_string()
                                }),
                        )
                    }),
            );
        }

        rail.child(
            gpui::div()
                .id("guild-join")
                .w(px(44.))
                .h(px(44.))
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .bg(rgb(active().surface))
                .cursor_pointer()
                .text_size(px(scaled(text::LG)))
                .text_color(rgb(active().success))
                .hover(|style| style.bg(rgb(active().surface_hover)))
                .child("+")
                .on_click(cx.listener(|this, _event, _window, cx| {
                    this.prompt = Some((Prompt::InviteCode, Composer::default()));
                    cx.notify();
                })),
        )
    }
}
