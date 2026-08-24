use gpui::{prelude::*, px, rgb, Context, IntoElement};

use concord::discord::marker;
use concord::discord::Id;
use concord::t;

use crate::theme::{active, layout, scaled, space, text};
use crate::ui::chrome::{
    avatar, avatar_with_url, column, icon_button, panel_sunken, presence_dot, row, section_label, sidebar_row,
    voice_participant_row, VoiceRow,
};
use crate::ui::stream::share_button;
use crate::ui::workspace::{ChannelKind, ContextSubject, Pane, Presence, Selection, Workspace};

impl Workspace {
    pub(super) fn channel_sidebar_impl(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let guild_name = self
            .model
            .guilds
            .get(self.model.selected_guild)
            .map(|g| g.name.clone())
            .unwrap_or_default();

        let header_row = row()
            .w_full()
            .h(px(layout::HEADER))
            .px(px(space::MD))
            .border_b_1()
            .border_color(rgb(active().border))
            .text_size(px(scaled(text::BASE)))
            .text_color(rgb(active().text))
            .gap(px(space::SM))
            .child(gpui::div().flex_1().child(guild_name))
            .when(
                matches!(self.nav.selection, Selection::Guild(_)),
                |header| {
                    let muted = self.guild_muted;
                    header
                        .child(
                            icon_button(
                                "guild-mute",
                                if muted { "\u{2298}" } else { "\u{25CE}" },
                                if muted {
                                    t!("action-unmute-server")
                                } else {
                                    t!("action-mute-server")
                                },
                                muted,
                            )
                            .on_click(cx.listener(|this, _event, _window, cx| {
                                this.toggle_guild_muted();
                                cx.notify();
                            })),
                        )
                        .when(
                            self.last_state.as_ref().is_some_and(|state| {
                                matches!(self.nav.selection, Selection::Guild(guild_id)
                                    if state.can_ban_members(guild_id))
                            }),
                            |header| {
                                header.child(
                                    icon_button("guild-bans", "\u{2717}", t!("action-view-bans"), false)
                                        .on_click(cx.listener(|this, _event, _window, cx| {
                                            this.open_ban_list();
                                            cx.notify();
                                        })),
                                )
                            },
                        )
                        .when(
                            self.last_state.as_ref().is_some_and(|state| {
                                matches!(self.nav.selection, Selection::Guild(guild_id)
                                    if state.can_manage_invites(guild_id)
                                        || state.can_manage_emoji(guild_id)
                                        || state.can_view_audit_log(guild_id))
                            }),
                            |header| {
                                header.child(
                                    icon_button("guild-manage", "\u{2699}", t!("label-server-management"), false)
                                        .on_click(cx.listener(|this, _event, _window, cx| {
                                            this.open_server_management(this.first_server_tab());
                                            cx.notify();
                                        })),
                                )
                            },
                        )
                        .when(
                            self.last_state.as_ref().is_some_and(|state| {
                                matches!(self.nav.selection, Selection::Guild(guild_id)
                                    if state.is_departed_guild(guild_id))
                            }),
                            |header| {
                                header.child(
                                    icon_button("guild-forget", "\u{2716}", t!("action-forget-guild"), false)
                                        .on_click(cx.listener(|this, _event, _window, cx| {
                                            this.forget_guild();
                                            cx.notify();
                                        })),
                                )
                            },
                        )
                        .child(
                            icon_button("guild-leave", "\u{2192}", t!("action-leave-server"), false)
                                .on_click(cx.listener(|this, _event, _window, cx| {
                                    this.leave_guild();
                                    cx.notify();
                                })),
                        )
                },
            );

        let mut list = column()
            .id("channel-list")
            .flex_1()
            .w_full()
            .pt(px(space::XS))
            .gap(px(1.))
            .overflow_y_scroll();

        let mut hidden_parent: Option<Id<marker::ChannelMarker>> = None;

        if let Some(filter) = &self.pane_filter
            && self.focus_pane == Pane::Channels
        {
            list = list.child(
                gpui::div()
                    .w_full()
                    .px(px(space::MD))
                    .py(px(space::XS))
                    .text_size(px(scaled(text::XS)))
                    .text_color(rgb(active().accent))
                    .child(if filter.text().is_empty() {
                        "filter…".to_string()
                    } else {
                        format!("filter: {}", filter.text())
                    }),
            );
        }

        for (index, channel) in self.model.channels.iter().enumerate() {
            if channel.kind == ChannelKind::Category {
                let collapsed = channel.id.is_some_and(|id| self.category_collapsed(id));
                let category_id = channel.id;

                list = list.child(
                    row()
                        .id(("category", index))
                        .w_full()
                        .cursor_pointer()
                        .child(section_label(format!(
                            "{} {}",
                            if collapsed { "\u{25b8}" } else { "\u{25be}" },
                            channel.name
                        )))
                        .on_click(cx.listener(move |this, _event, _window, cx| {
                            if let Some(id) = category_id {
                                this.toggle_category(id);
                            }
                            cx.notify();
                        })),
                );

                hidden_parent = collapsed.then_some(channel.id).flatten();
                continue;
            }

            if hidden_parent.is_some() && channel.parent == hidden_parent {
                continue;
            }

            if self.focus_pane == Pane::Channels && !self.passes_filter(&channel.name) {
                continue;
            }

            let selected = channel.id.is_some() && channel.id == self.nav.channel;
            let is_thread = channel.kind == ChannelKind::Thread;

            let mut entry = sidebar_row(selected)
                .when(is_thread, |d| d.pl(px(space::LG)))
                .when(channel.archived, |d| d.opacity(0.55))
                .child(
                    gpui::div()
                        .w(px(14.))
                        .text_color(rgb(active().text_subtle))
                        .child(channel.kind.glyph()),
                )
                .child(
                    gpui::div()
                        .flex_1()
                        .when(channel.unread && !selected, |d| {
                            d.text_color(rgb(active().text))
                        })
                        .child(channel.name.clone()),
                );

            if channel.mentions > 0 {
                entry = entry.child(
                    gpui::div()
                        .px(px(6.))
                        .rounded_full()
                        .bg(rgb(active().danger))
                        .text_size(px(scaled(text::XS)))
                        .text_color(rgb(active().on_accent))
                        .child(channel.mentions.to_string()),
                );
            }

            let entry = match channel.id {
                Some(channel_id) if channel.kind.joins_voice() => {
                    let name = channel.name.clone();
                    entry
                        .id(("channel", index))
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _event, _window, cx| {
                            this.join_voice(channel_id, name.clone());
                            cx.notify();
                        }))
                        .into_any_element()
                }
                Some(channel_id) if channel.kind == ChannelKind::Forum => {
                    let name = channel.name.clone();
                    entry
                        .id(("channel", index))
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _event, _window, cx| {
                            this.open_forum(channel_id, name.clone());
                            cx.notify();
                        }))
                        .into_any_element()
                }
                Some(channel_id) => entry
                    .id(("channel", index))
                    .cursor_pointer()
                    .on_mouse_down(
                        gpui::MouseButton::Right,
                        cx.listener(move |this, event: &gpui::MouseDownEvent, _window, cx| {
                            this.open_context_menu(
                                ContextSubject::Channel(channel_id),
                                event.position,
                            );
                            cx.notify();
                        }),
                    )
                    .on_click(
                        cx.listener(move |this, event: &gpui::ClickEvent, _window, cx| {
                            this.forum = None;
                            let modifiers = event.modifiers();
                            if modifiers.control || modifiers.platform {
                                this.open_channel_in_new_tab(channel_id);
                            } else {
                                this.open_channel(channel_id);
                            }
                            cx.notify();
                        }),
                    )
                    .into_any_element(),
                None => entry.into_any_element(),
            };

            list = list.child(entry);

            for participant in &channel.voice {
                let participant_id = participant.user_id;
                let participant_name = participant.name.clone();
                list = list.child(voice_participant_row(
                    VoiceRow {
                        name: &participant.name,
                        muted: participant.muted,
                        deafened: participant.deafened,
                        streaming: participant.streaming,
                        on_camera: participant.on_camera,
                        speaking: participant.speaking,
                        id_seed: participant_id.get(),
                    },
                    {
                        let entity = cx.entity();
                        move |cx: &mut gpui::App| {
                            let name = participant_name.clone();
                            entity.update(cx, |workspace, cx| {
                                workspace.watch_stream(participant_id, name);
                                cx.notify();
                            });
                        }
                    },
                    {
                        let entity = cx.entity();
                        move |cx: &mut gpui::App| {
                            entity.update(cx, |workspace, cx| {
                                let muted = workspace.locally_muted.insert(participant_id);
                                if !muted {
                                    workspace.locally_muted.remove(&participant_id);
                                }
                                let volume =
                                    workspace.options.voice.voice_output_volume.value() as u16;
                                let hidden = workspace.video_hidden.contains(&participant_id);
                                workspace.set_participant_playback(
                                    participant_id,
                                    volume,
                                    muted,
                                    hidden,
                                );
                                cx.notify();
                            });
                        }
                    },
                ));
            }
        }

        let mut sidebar = panel_sunken(layout::SIDEBAR).child(header_row).child(list);

        if let Some((_, name)) = &self.voice_channel {
            sidebar = sidebar.child(self.voice_connected_card_impl(name, cx));
        }

        sidebar = sidebar.child(self.user_profile_bar_impl(cx));

        sidebar
    }

    pub(super) fn voice_connected_card_impl(&self, name: &str, cx: &mut Context<Self>) -> gpui::Div {
        let mute = self.self_mute;
        let deaf = self.self_deaf;

        column()
            .w_full()
            .p(px(space::SM))
            .gap(px(space::XS))
            .bg(rgb(active().surface))
            .border_t_1()
            .border_color(rgb(active().border))
            .child(
                row()
                    .w_full()
                    .items_center()
                    .gap(px(space::SM))
                    .child(
                        gpui::div()
                            .text_size(px(14.))
                            .text_color(rgb(active().success))
                            .child("~"),
                    )
                    .child(
                        column()
                            .flex_1()
                            .child(
                                gpui::div()
                                    .text_size(px(scaled(text::XS)))
                                    .text_color(rgb(active().success))
                                    .child(t!("label-voice-connected")),
                            )
                            .child(
                                gpui::div()
                                    .text_size(px(scaled(text::XS)))
                                    .text_color(rgb(active().text_subtle))
                                    .child(name.to_string()),
                            ),
                    )
                    .child(
                        gpui::div()
                            .id("voice-card-leave")
                            .px(px(6.))
                            .py(px(2.))
                            .rounded(px(layout::RADIUS))
                            .cursor_pointer()
                            .hover(|s| s.bg(rgb(active().surface_hover)))
                            .text_size(px(14.))
                            .text_color(rgb(active().danger))
                            .child("\u{2715}")
                            .on_click(cx.listener(|this, _event, _window, cx| {
                                this.leave_voice();
                                cx.notify();
                            })),
                    ),
            )
            .child(
                row()
                    .w_full()
                    .gap(px(space::XS))
                    .justify_around()
                    .child(
                        icon_button(
                            "card-mute",
                            if mute { "\u{2298}" } else { "\u{25CB}" },
                            if mute {
                                t!("action-unmute")
                            } else {
                                t!("action-mute")
                            },
                            mute,
                        )
                        .flex_1()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.toggle_voice_flag(false);
                            cx.notify();
                        })),
                    )
                    .child(
                        icon_button(
                            "card-deafen",
                            if deaf { "\u{2297}" } else { "\u{25D1}" },
                            if deaf {
                                t!("action-undeafen")
                            } else {
                                t!("action-deafen")
                            },
                            deaf,
                        )
                        .flex_1()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.toggle_voice_flag(true);
                            cx.notify();
                        })),
                    )
                    .child(
                        gpui::div()
                            .id("card-screen")
                            .flex_1()
                            .h(px(28.))
                            .items_center()
                            .justify_center()
                            .rounded(px(layout::RADIUS))
                            .bg(rgb(active().surface_sunken))
                            .text_size(px(12.))
                            .text_color(rgb(active().text))
                            .cursor_pointer()
                            .hover(|s| s.bg(rgb(active().surface_hover)))
                            .child(share_button(self.broadcasting, self.can_broadcast()))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.toggle_stream();
                                cx.notify();
                            })),
                    )
                    .child(
                        icon_button(
                            "card-soundboard",
                            "\u{266B}",
                            t!("action-soundboard"),
                            self.soundboard.is_some(),
                        )
                        .flex_1()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.open_soundboard();
                            cx.notify();
                        })),
                    )
                    .child(
                        icon_button(
                            "card-devices",
                            "\u{2699}",
                            t!("action-audio-devices"),
                            false,
                        )
                        .flex_1()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.open_audio_devices();
                            cx.notify();
                        })),
                    )
                    .child(
                        icon_button(
                            "card-mic-permission",
                            "\u{25C9}",
                            t!("action-microphone"),
                            !self.allow_microphone_transmit,
                        )
                        .flex_1()
                        .on_click(cx.listener(|this, _, _, cx| {
                            let allowed = !this.allow_microphone_transmit;
                            this.set_microphone_allowed(allowed);
                            cx.notify();
                        })),
                    ),
            )
    }

    pub(super) fn user_profile_bar_impl(&self, cx: &mut Context<Self>) -> gpui::Div {
        let user_name = self
            .last_state
            .as_ref()
            .and_then(|s| s.current_user())
            .unwrap_or("blu")
            .to_string();

        let mute = self.self_mute;
        let deaf = self.self_deaf;

        row()
            .w_full()
            .h(px(52.))
            .px(px(space::SM))
            .items_center()
            .bg(rgb(active().surface))
            .border_t_1()
            .border_color(rgb(active().border))
            .child(
                row()
                    .id("user-bar-profile")
                    .flex_1()
                    .items_center()
                    .gap(px(space::SM))
                    .px(px(4.))
                    .py(px(4.))
                    .rounded(px(layout::RADIUS))
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(active().surface_hover)))
                    .on_click(cx.listener(|this, _event, _window, cx| {
                        this.self_profile_popout = !this.self_profile_popout;
                        cx.notify();
                    }))
                    .child(
                        gpui::div()
                            .id("bar-avatar")
                            .relative()
                            .child(avatar(32., &user_name))
                            .child(presence_dot(Presence::Online)),
                    )
                    .child(
                        column()
                            .overflow_hidden()
                            .child(
                                gpui::div()
                                    .text_size(px(scaled(text::SM)))
                                    .text_color(rgb(active().text))
                                    .child(user_name),
                            )
                            .child(
                                gpui::div()
                                    .text_size(px(scaled(text::XS)))
                                    .text_color(rgb(active().text_subtle))
                                    .child("Online"),
                            ),
                    ),
            )
            .child(
                row()
                    .gap(px(2.))
                    .child(
                        gpui::div()
                            .id("bar-mute")
                            .w(px(28.))
                            .h(px(28.))
                            .items_center()
                            .justify_center()
                            .rounded(px(layout::RADIUS))
                            .bg(rgb(if mute {
                                active().danger
                            } else {
                                active().surface
                            }))
                            .text_size(px(14.))
                            .text_color(rgb(if mute {
                                active().on_accent
                            } else {
                                active().text_muted
                            }))
                            .cursor_pointer()
                            .hover(|s| {
                                s.bg(rgb(active().surface_hover))
                                    .text_color(rgb(active().text))
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.toggle_voice_flag(false);
                                cx.notify();
                            }))
                            .child(if mute { "\u{2298}" } else { "\u{25CB}" })
                            .tooltip(move |_window, cx| {
                                cx.new(|_| {
                                    crate::ui::chrome::Tooltip::new(if mute {
                                        "Unmute Microphone"
                                    } else {
                                        "Mute Microphone"
                                    })
                                })
                                .into()
                            }),
                    )
                    .child(
                        gpui::div()
                            .id("bar-deafen")
                            .w(px(28.))
                            .h(px(28.))
                            .items_center()
                            .justify_center()
                            .rounded(px(layout::RADIUS))
                            .bg(rgb(if deaf {
                                active().danger
                            } else {
                                active().surface
                            }))
                            .text_size(px(14.))
                            .text_color(rgb(if deaf {
                                active().on_accent
                            } else {
                                active().text_muted
                            }))
                            .cursor_pointer()
                            .hover(|s| {
                                s.bg(rgb(active().surface_hover))
                                    .text_color(rgb(active().text))
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.toggle_voice_flag(true);
                                cx.notify();
                            }))
                            .child(if deaf { "\u{2297}" } else { "\u{25D1}" })
                            .tooltip(move |_window, cx| {
                                cx.new(|_| {
                                    crate::ui::chrome::Tooltip::new(if deaf {
                                        "Undeafen Audio"
                                    } else {
                                        "Deafen Audio"
                                    })
                                })
                                .into()
                            }),
                    )
                    .child(
                        gpui::div()
                            .id("bar-settings")
                            .w(px(28.))
                            .h(px(28.))
                            .items_center()
                            .justify_center()
                            .rounded(px(layout::RADIUS))
                            .bg(rgb(active().surface))
                            .text_size(px(14.))
                            .text_color(rgb(active().text_muted))
                            .cursor_pointer()
                            .hover(|s| {
                                s.bg(rgb(active().surface_hover))
                                    .text_color(rgb(active().text))
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.open_settings_window(cx);
                            }))
                            .child("⚙")
                            .tooltip(|_window, cx| {
                                cx.new(|_| crate::ui::chrome::Tooltip::new("User Settings"))
                                    .into()
                            }),
                    ),
            )
    }
}
