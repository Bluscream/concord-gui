use crate::ui::workspace::ChannelKind;
use concord::discord::PresenceStatus;

use concord::t;
use gpui::{ClipboardItem, Context, Window, prelude::*, px, rgb};

use crate::model::projection::Selection;
use crate::theme::{Presence, active, layout, scaled, space, text};
use crate::ui::chrome::{column, header, icon_button, presence_dot, row};
use crate::ui::composer::{Composer, character_counter, composer_view};
use crate::ui::forum;
use crate::ui::login::login_view;
use crate::ui::messages::{RenderOptions, message_list};
use crate::ui::slash::slash_view;
use crate::ui::workspace::*;

impl Workspace {
    pub(crate) fn self_status_item(
        label: &'static str,
        target_status: PresenceStatus,
        current_status: PresenceStatus,
        on_click: impl Fn(&mut gpui::App) + 'static,
    ) -> gpui::Stateful<gpui::Div> {
        let is_selected = target_status == current_status;
        row()
            .id(label)
            .w_full()
            .px(px(space::SM))
            .py(px(space::XS))
            .rounded(px(layout::RADIUS))
            .cursor_pointer()
            .hover(|s| s.bg(rgb(active().surface_hover)))
            .on_click(move |_, _, cx| on_click(cx))
            .child(
                row()
                    .items_center()
                    .gap(px(space::SM))
                    .child(presence_dot(match target_status {
                        PresenceStatus::Online => Presence::Online,
                        PresenceStatus::Idle => Presence::Idle,
                        PresenceStatus::DoNotDisturb => Presence::Dnd,
                        _ => Presence::Offline,
                    }))
                    .child(
                        gpui::div()
                            .text_size(px(scaled(text::SM)))
                            .text_color(rgb(if is_selected {
                                active().text
                            } else {
                                active().text_muted
                            }))
                            .child(label),
                    ),
            )
    }

    pub(crate) fn content(&self, window: &Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        if let Some(view) = &self.forum {
            return forum::forum_view(
                view,
                {
                    let entity = cx.entity();
                    move |index, cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.open_forum_post(index);
                            cx.notify();
                        });
                    }
                },
                {
                    let entity = cx.entity();
                    move |cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.toggle_forum_archived();
                            cx.notify();
                        });
                    }
                },
                {
                    let entity = cx.entity();
                    move |cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            let offset =
                                workspace.forum.as_ref().map(|f| f.next_offset).unwrap_or(0);
                            if let Some(forum) = &mut workspace.forum {
                                forum.loading = true;
                            }
                            workspace.request_forum_posts(offset);
                            cx.notify();
                        });
                    }
                },
                {
                    let entity = cx.entity();
                    move |cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.prompt = Some((Prompt::ForumPostTitle, Composer::default()));
                            cx.notify();
                        });
                    }
                },
            )
            .into_any_element();
        }

        self.chat_content(window, cx).into_any_element()
    }

    /// The tab strip, shown only once more than one channel is open.
    pub(crate) fn tab_strip(&self, cx: &mut Context<Self>) -> Option<gpui::Stateful<gpui::Div>> {
        if self.tabs.len() < 2 {
            return None;
        }

        let mut strip = row()
            .id("tab-strip")
            .w_full()
            .h(px(28.))
            .gap(px(space::XS))
            .px(px(space::SM))
            .bg(rgb(active().surface_sunken))
            .border_b_1()
            .border_color(rgb(active().border))
            .overflow_x_scroll();

        for (index, tab) in self.tabs.iter().enumerate() {
            let selected = index == self.active_tab;
            let name = if tab.name.is_empty() {
                self.channel_name(tab.channel_id)
            } else {
                tab.name.clone()
            };

            strip = strip.child(
                row()
                    .id(("tab", index))
                    .px(px(space::SM))
                    .gap(px(space::XS))
                    .items_center()
                    .rounded(px(layout::RADIUS))
                    .cursor_pointer()
                    .text_size(px(scaled(text::XS)))
                    .when(selected, |tab| tab.bg(rgb(active().surface_active)))
                    .text_color(rgb(if selected {
                        active().text
                    } else {
                        active().text_muted
                    }))
                    .hover(|style| style.bg(rgb(active().surface_hover)))
                    .child(format!("# {name}"))
                    .child(
                        gpui::div()
                            .id(("tab-close", index))
                            .px(px(space::XS))
                            .text_color(rgb(active().text_subtle))
                            .hover(|style| style.text_color(rgb(active().danger)))
                            .child("\u{2715}")
                            .on_click(cx.listener(move |this, _event, _window, cx| {
                                cx.stop_propagation();
                                this.close_tab(index);
                                cx.notify();
                            })),
                    )
                    .on_click(cx.listener(move |this, _event, _window, cx| {
                        this.activate_tab(index);
                        cx.notify();
                    })),
            );
        }

        Some(strip)
    }

    /// The ordinary message view.
    pub(crate) fn chat_content(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let selected_channel_opt = self
            .nav
            .channel
            .and_then(|id| self.last_state.as_ref().and_then(|s| s.channel(id)));

        let model_channel = self
            .nav
            .channel
            .and_then(|id| self.model.channels.iter().find(|c| c.id == Some(id)));

        let channel_name = selected_channel_opt
            .map(|c| c.name.clone())
            .or_else(|| model_channel.map(|c| c.name.clone()))
            .unwrap_or_default();

        let channel_glyph = selected_channel_opt
            .map(|c| {
                if c.is_thread() {
                    ChannelKind::Thread.glyph()
                } else {
                    ChannelKind::Text.glyph()
                }
            })
            .or_else(|| {
                model_channel.map(|c| {
                    if c.kind == ChannelKind::Thread {
                        ChannelKind::Thread.glyph()
                    } else {
                        c.kind.glyph()
                    }
                })
            })
            .unwrap_or_else(|| ChannelKind::Text.glyph());

        column()
            .flex_1()
            // A flex item will not shrink below its content unless told it
            // may, so one long unbroken URL in a message was widening this
            // column and squeezing the member list beside it.
            .min_w(px(0.))
            .overflow_hidden()
            .h_full()
            .bg(rgb(active().surface))
            .children(self.tab_strip(cx))
            .child(
                header()
                    .child(
                        gpui::div()
                            .text_color(rgb(active().text_subtle))
                            .child(channel_glyph),
                    )
                    .child(
                        gpui::div()
                            .flex_1()
                            .text_size(px(scaled(text::BASE)))
                            .text_color(rgb(active().text))
                            .child(channel_name.clone()),
                    )
                    .when(self.in_thread(), |header| {
                        header
                            .child(
                                icon_button("thread-follow", "\u{2606}", "Follow thread", false)
                                    .on_click(cx.listener(|this, _event, _window, cx| {
                                        this.set_thread_followed(true);
                                        cx.notify();
                                    })),
                            )
                            .child(
                                icon_button(
                                    "thread-mute",
                                    "\u{2573}",
                                    "Mute thread notifications",
                                    false,
                                )
                                .on_click(cx.listener(
                                    |this, _event, _window, cx| {
                                        this.set_thread_muted(true);
                                        cx.notify();
                                    },
                                )),
                            )
                            .child(
                                icon_button(
                                    "thread-notify-all",
                                    "\u{25C9}",
                                    t!("action-notify-all"),
                                    false,
                                )
                                .on_click(cx.listener(
                                    |this, _event, _window, cx| {
                                        this.set_thread_notification_level(2);
                                        cx.notify();
                                    },
                                )),
                            )
                            .child(
                                icon_button(
                                    "thread-notify-mentions",
                                    "@",
                                    t!("action-notify-mentions"),
                                    false,
                                )
                                .on_click(cx.listener(
                                    |this, _event, _window, cx| {
                                        this.set_thread_notification_level(4);
                                        cx.notify();
                                    },
                                )),
                            )
                            .child(
                                icon_button(
                                    "thread-notify-none",
                                    "\u{25CB}",
                                    t!("action-notify-none"),
                                    false,
                                )
                                .on_click(cx.listener(
                                    |this, _event, _window, cx| {
                                        this.set_thread_notification_level(8);
                                        cx.notify();
                                    },
                                )),
                            )
                            .child(
                                icon_button("thread-pin", "\u{25B2}", "Pin thread", false)
                                    .on_click(cx.listener(|this, _event, _window, cx| {
                                        this.set_thread_pinned(true);
                                        cx.notify();
                                    })),
                            )
                            .child(
                                icon_button("thread-rename", "\u{270F}", "Rename thread", false)
                                    .on_click(cx.listener(|this, _event, _window, cx| {
                                        this.prompt =
                                            Some((Prompt::ThreadName, Composer::default()));
                                        cx.notify();
                                    })),
                            )
                            .child(
                                icon_button("thread-lock", "\u{25A3}", "Lock thread", false)
                                    .on_click(cx.listener(|this, _event, _window, cx| {
                                        this.set_thread_locked(true);
                                        cx.notify();
                                    })),
                            )
                            .child(
                                icon_button("thread-delete", "\u{2715}", "Delete thread", true)
                                    .on_click(cx.listener(|this, _event, _window, cx| {
                                        this.delete_thread();
                                        cx.notify();
                                    })),
                            )
                            .child(
                                icon_button("thread-archive", "\u{25A6}", "Archive thread", false)
                                    .on_click(cx.listener(|this, _event, _window, cx| {
                                        this.set_thread_archived(true);
                                        cx.notify();
                                    })),
                            )
                    })
                    .when(self.can_manage_channels() && !self.in_thread(), |header| {
                        header
                            .child(
                                icon_button(
                                    "channel-new",
                                    "\u{FF0B}",
                                    t!("action-new-channel"),
                                    false,
                                )
                                .on_click(cx.listener(
                                    |this, _event, _window, cx| {
                                        this.prompt =
                                            Some((Prompt::NewChannel, Composer::default()));
                                        cx.notify();
                                    },
                                )),
                            )
                            .child(
                                icon_button(
                                    "channel-rename",
                                    "\u{270E}",
                                    t!("action-rename-channel"),
                                    false,
                                )
                                .on_click(cx.listener(
                                    |this, _event, _window, cx| {
                                        if let Some(channel_id) = this.nav.channel {
                                            let mut text = Composer::default();
                                            text.set_text(&this.channel_name(channel_id));
                                            this.prompt =
                                                Some((Prompt::ChannelName(channel_id), text));
                                        }
                                        cx.notify();
                                    },
                                )),
                            )
                            .child(
                                icon_button(
                                    "channel-topic",
                                    "\u{2261}",
                                    t!("action-channel-topic"),
                                    false,
                                )
                                .on_click(cx.listener(
                                    |this, _event, _window, cx| {
                                        if let Some(channel_id) = this.nav.channel {
                                            let mut text = Composer::default();
                                            text.set_text(&this.channel_topic(channel_id));
                                            this.prompt =
                                                Some((Prompt::ChannelTopic(channel_id), text));
                                        }
                                        cx.notify();
                                    },
                                )),
                            )
                            .child(
                                icon_button(
                                    "channel-slowmode",
                                    "\u{29D6}",
                                    t!("action-channel-slowmode"),
                                    false,
                                )
                                .on_click(cx.listener(
                                    |this, _event, _window, cx| {
                                        if let Some(channel_id) = this.nav.channel {
                                            this.prompt = Some((
                                                Prompt::ChannelSlowmode(channel_id),
                                                Composer::default(),
                                            ));
                                        }
                                        cx.notify();
                                    },
                                )),
                            )
                            .child(
                                icon_button(
                                    "channel-permissions",
                                    "\u{26BF}",
                                    t!("action-channel-permissions"),
                                    false,
                                )
                                .on_click(cx.listener(
                                    |this, _event, _window, cx| {
                                        if let Some(channel_id) = this.nav.channel {
                                            this.open_channel_overwrite(channel_id);
                                        }
                                        cx.notify();
                                    },
                                )),
                            )
                            .child(
                                icon_button(
                                    "channel-delete",
                                    "\u{2716}",
                                    t!("action-delete-channel"),
                                    false,
                                )
                                .on_click(cx.listener(
                                    |this, _event, _window, cx| {
                                        if let Some(channel_id) = this.nav.channel {
                                            this.deleting_channel = Some(channel_id);
                                        }
                                        cx.notify();
                                    },
                                )),
                            )
                    })
                    .when(
                        self.last_state.as_ref().is_some_and(|state| {
                            matches!(self.nav.selection, Selection::Guild(guild_id)
                                if state.can_create_invites(guild_id))
                        }),
                        |header| {
                            header.child(
                                icon_button(
                                    "channel-invite",
                                    "\u{2709}",
                                    t!("action-create-invite"),
                                    false,
                                )
                                .on_click(cx.listener(
                                    |this, _event, _window, cx| {
                                        this.create_invite_here();
                                        cx.notify();
                                    },
                                )),
                            )
                        },
                    )
                    .child(
                        icon_button("channel-pins", "\u{27A6}", t!("action-pins"), false).on_click(
                            cx.listener(|this, _event, _window, cx| {
                                this.open_pins();
                                cx.notify();
                            }),
                        ),
                    )
                    .child(
                        icon_button(
                            "channel-mute",
                            if self.channel_muted {
                                "\u{2298}"
                            } else {
                                "\u{25CE}"
                            },
                            if self.channel_muted {
                                t!("action-unmute-channel")
                            } else {
                                t!("action-mute-channel")
                            },
                            self.channel_muted,
                        )
                        .on_click(cx.listener(
                            |this, _event, _window, cx| {
                                this.toggle_channel_muted();
                                cx.notify();
                            },
                        )),
                    )
                    .when(self.can_call(), |header| {
                        let call_channel = self.nav.channel;
                        let call_name = channel_name.clone();
                        header.child(
                            icon_button("dm-call", "\u{260E}", "Start voice call", false).on_click(
                                cx.listener(move |this, _event, _window, cx| {
                                    if let Some(channel_id) = call_channel {
                                        this.join_voice(channel_id, call_name.clone());
                                    }
                                    cx.notify();
                                }),
                            ),
                        )
                    }),
            )
            .child(message_list(
                &self.messages,
                self.selected_message,
                &self.message_scroll,
                RenderOptions {
                    show_avatars: self.options.display.show_avatars,
                    circular_avatars: self.options.display.circular_avatars,
                    hour24: self.options.display.hour_format_24,
                    show_emoji: self.options.display.show_custom_emoji,
                    show_images: self.options.display.show_images
                        && !self.options.display.disable_image_preview,
                    animate: self.options.display.animate_images,
                    previews: &self.attachment_previews,
                },
                self.has_newer_messages(),
                {
                    let entity = cx.entity();
                    move |index, action, cx: &mut gpui::App| {
                        entity.update(cx, |workspace, cx| {
                            workspace.handle_message_action(index, action);
                            cx.notify();
                        });
                    }
                },
            ))
            .child(self.composer_row(window, cx))
    }

    pub(crate) fn composer_row(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let refusal = self
            .nav
            .channel
            .zip(self.last_state.as_ref())
            .and_then(|(channel_id, state)| state.send_block_reason(channel_id));
        let enabled = self.nav.channel.is_some() && self.handle.is_some() && refusal.is_none();
        let placeholder = if let Some(reason) = &refusal {
            format!("Cannot send here: {reason}")
        } else if !enabled {
            "Select a channel to start typing".to_string()
        } else {
            let name = self
                .model
                .channels
                .get(self.model.selected_channel)
                .map(|c| c.name.clone())
                .unwrap_or_default();
            format!("Message #{name}  ·  ctrl-o to attach")
        };

        column()
            .w_full()
            .child(
                row()
                    .w_full()
                    .px(px(space::MD))
                    .py(px(space::XS))
                    .gap(px(space::SM))
                    .items_center()
                    .children(self.pending_stickers.iter().enumerate().map(|(slot, _)| {
                        gpui::div()
                            .id(("staged-sticker", slot))
                            .px(px(space::SM))
                            .py(px(space::XS))
                            .rounded(px(layout::RADIUS))
                            .bg(rgb(active().surface_hover))
                            .cursor_pointer()
                            .text_size(px(scaled(text::XS)))
                            .text_color(rgb(active().text))
                            .hover(|style| style.bg(rgb(active().surface_active)))
                            .child("sticker x")
                            .on_click(cx.listener(move |this, _event, _window, cx| {
                                if slot < this.pending_stickers.len() {
                                    this.pending_stickers.remove(slot);
                                }
                                cx.notify();
                            }))
                    })),
            )
            .children((!self.attachments.is_empty()).then(|| {
                let mut tray = row()
                    .w_full()
                    .gap(px(space::SM))
                    .px(px(space::MD))
                    .py(px(space::XS));
                for (index, upload) in self.attachments.iter().enumerate() {
                    tray = tray.child(
                        row()
                            .id(("staged", index))
                            .gap(px(space::XS))
                            .px(px(space::SM))
                            .py(px(space::XS))
                            .rounded(px(layout::RADIUS))
                            .bg(rgb(active().surface_hover))
                            .text_size(px(scaled(text::XS)))
                            .text_color(rgb(active().text))
                            .cursor_pointer()
                            .hover(|style| style.bg(rgb(active().surface_active)))
                            .child(upload.filename.clone())
                            .child(gpui::div().text_color(rgb(active().danger)).child("x"))
                            .on_click(cx.listener(move |this, _event, _window, cx| {
                                this.remove_attachment(index);
                                cx.notify();
                            })),
                    );
                }
                tray
            }))
            .children(self.attachment_error.as_ref().map(|error| {
                gpui::div()
                    .px(px(space::MD))
                    .text_size(px(scaled(text::XS)))
                    .text_color(rgb(active().danger))
                    .child(error.clone())
            }))
            .children(
                self.config_warnings
                    .iter()
                    .enumerate()
                    .map(|(slot, warning)| {
                        gpui::div()
                            .id(("config-warning", slot))
                            .px(px(space::MD))
                            .text_size(px(scaled(text::XS)))
                            .text_color(rgb(active().danger))
                            .cursor_pointer()
                            .child(format!("config: {warning}"))
                            .on_click(cx.listener(|this, _event, _window, cx| {
                                this.config_warnings.clear();
                                cx.notify();
                            }))
                    }),
            )
            .children(self.settings_note.as_ref().map(|note| {
                gpui::div()
                    .px(px(space::MD))
                    .text_size(px(scaled(text::XS)))
                    .text_color(rgb(active().text_subtle))
                    .child(note.clone())
            }))
            .children(self.slash.as_ref().map(slash_view))
            .children((!self.command_choices.is_empty()).then(|| {
                let mut list = column()
                    .w_full()
                    .rounded(px(layout::RADIUS))
                    .bg(rgb(active().surface))
                    .border_1()
                    .border_color(rgb(active().border));

                for (slot, choice) in self.command_choices.iter().enumerate().take(10) {
                    let value = choice.clone();
                    list = list.child(
                        gpui::div()
                            .id(("choice", slot))
                            .w_full()
                            .px(px(space::MD))
                            .py(px(space::XS))
                            .cursor_pointer()
                            .text_size(px(scaled(text::SM)))
                            .text_color(rgb(active().text_muted))
                            .hover(|style| style.bg(rgb(active().surface_hover)))
                            .child(choice.clone())
                            .on_click(cx.listener(move |this, _event, _window, cx| {
                                this.accept_command_choice(&value);
                                cx.notify();
                            })),
                    );
                }

                list
            }))
            .child(composer_view(
                &self.composer,
                self.focus.is_focused(window),
                enabled,
                &placeholder,
                self.composer_leading(cx),
                self.composer_trailing(enabled, cx),
            ))
    }

    /// The attach button, at the left of the composer.
    fn composer_leading(&self, cx: &mut Context<Self>) -> Vec<gpui::AnyElement> {
        vec![
            icon_button(
                "composer-attach",
                "\u{FF0B}",
                t!("action-attach"),
                self.attach_menu,
            )
            .on_click(cx.listener(|this, _event, _window, cx| {
                this.attach_menu = !this.attach_menu;
                this.composer_emoji = false;
                cx.notify();
            }))
            .into_any_element(),
        ]
    }

    /// The counter and the buttons at the right of the composer.
    ///
    /// Send is here as well as on the Enter key: a manual button is the only
    /// way to send from a touchscreen, and it is where anyone who has not
    /// learnt the keyboard will look for it.
    fn composer_trailing(&self, enabled: bool, cx: &mut Context<Self>) -> Vec<gpui::AnyElement> {
        let mut out = Vec::new();

        // The account's limit, not a constant: Nitro doubles it, and a
        // counter warning at 1500 for a 4000-character allowance would be on
        // screen for most of a message it has no business warning about.
        let limit = self
            .nav
            .channel
            .zip(self.last_state.as_ref())
            .map_or(2_000, |(channel_id, state)| {
                state.message_send_limits(channel_id).max_content_chars
            });
        if let Some(counter) = character_counter(self.composer.text().chars().count(), limit) {
            out.push(counter.into_any_element());
        }

        out.push(
            icon_button("composer-gif", "\u{25B7}", t!("action-gif"), false)
                .on_click(cx.listener(|this, _event, _window, cx| {
                    this.open_sticker_picker();
                    cx.notify();
                }))
                .into_any_element(),
        );
        out.push(
            icon_button("composer-sticker", "\u{229A}", t!("action-sticker"), false)
                .on_click(cx.listener(|this, _event, _window, cx| {
                    this.open_sticker_picker();
                    cx.notify();
                }))
                .into_any_element(),
        );
        out.push(
            icon_button(
                "composer-emoji",
                "\u{263A}",
                t!("action-emoji"),
                self.composer_emoji,
            )
            .on_click(cx.listener(|this, _event, _window, cx| {
                this.composer_emoji = !this.composer_emoji;
                this.attach_menu = false;
                cx.notify();
            }))
            .into_any_element(),
        );

        // Only once there is something to send, so the button is never a
        // control that does nothing when pressed.
        let ready = enabled && !self.composer.is_empty();
        if ready {
            out.push(
                icon_button("composer-send", "\u{27A4}", t!("action-send"), true)
                    .on_click(cx.listener(|this, _event, _window, cx| {
                        this.send_message();
                        cx.notify();
                    }))
                    .into_any_element(),
            );
        }

        out
    }

    pub(crate) fn accept_command_choice(&mut self, value: &str) {
        let content = self.composer.text().to_string();
        let head = content
            .rsplit_once(char::is_whitespace)
            .map(|(head, _)| head)
            .unwrap_or(&content);
        self.composer.set_text(&format!("{head} {value}"));
        self.command_choices.clear();
    }

    pub(crate) fn status_bar(&self, _cx: &mut Context<Self>) -> impl IntoElement {
        row()
            .w_full()
            .h(px(24.))
            .px(px(space::MD))
            .gap(px(space::SM))
            .bg(rgb(active().surface_sunken))
            .border_t_1()
            .border_color(rgb(active().border))
            .text_size(px(scaled(text::XS)))
            .text_color(rgb(active().text_subtle))
            .child(presence_dot(if self.model.connected {
                Presence::Online
            } else {
                Presence::Offline
            }))
            .child(gpui::div().flex_1().child(self.model.status_line.clone()))
            .children(
                self.downloads
                    .iter()
                    .enumerate()
                    .map(|(slot, (_, filename, progress))| {
                        gpui::div()
                            .id(("download", slot))
                            .px(px(space::SM))
                            .text_color(rgb(active().accent))
                            .child(match progress {
                                Some(fraction) => format!("{filename} {:.0}%", fraction * 100.0),
                                None => format!("{filename}..."),
                            })
                    }),
            )
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.window_focused = window.is_window_active();

        if let Some(text) = self.pending_copy.take() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }

        column()
            .track_focus(&self.focus)
            .key_context("Workspace")
            .on_key_down(cx.listener(Self::on_key))
            .size_full()
            .bg(rgb(active().bg))
            .text_size(px(scaled(text::BASE)))
            .when(matches!(self.screen, Screen::Login(_)), |d| {
                let Screen::Login(login) = &self.screen else {
                    return d;
                };
                d.child(login_view(login, cx))
            })
            .when(matches!(self.screen, Screen::Ready), |d| {
                d.child(
                    row()
                        .flex_1()
                        .w_full()
                        .overflow_hidden()
                        .when(self.ui_state.guild_pane_visible, |d| {
                            d.child(self.guild_rail(cx))
                        })
                        .when(self.ui_state.channel_pane_visible, |d| {
                            d.child(self.channel_sidebar(cx))
                        })
                        .child(self.content(window, cx))
                        .when(self.profile.is_some(), |d| d.child(self.profile_pane(cx)))
                        .when(self.profile.is_none() && self.search.is_some(), |d| {
                            d.child(self.search_pane(cx))
                        })
                        .when(
                            self.profile.is_none() && self.search.is_none() && self.shows_members(),
                            |d| d.child(self.member_pane(cx)),
                        ),
                )
                .child(self.status_bar(cx))
                .children(self.overlays(cx))
            })
    }
}
