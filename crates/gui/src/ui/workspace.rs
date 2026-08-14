//! The main three-pane workspace: guild rail, channel sidebar, content area.
//!
//! Currently renders from a placeholder model. The next step replaces that
//! model with projections of `concord::discord::DiscordSnapshot`, which the
//! session already publishes on every state revision - no new plumbing needed,
//! only mapping.

use concord::discord::{AppCommand, AppEvent, Id, marker, next_message_nonce};
use gpui::{Context, FocusHandle, KeyDownEvent, Window, WindowHandle, prelude::*, px, rgb};
use tokio::sync::mpsc;

use crate::model::message::{self, MessageRow};
use crate::model::projection::{self, Navigation, Selection};
use crate::session::{SessionHandle, Update};

use crate::theme::{DARK, Presence, layout, space, text};
use crate::ui::chrome::{
    avatar, column, header, hint, panel_sunken, presence_dot, row, section_label, sidebar_row,
};
use crate::ui::composer::{Composer, composer_view};
use crate::ui::messages::message_list;

/// Placeholder view-model. Mirrors the shape of the snapshot projections so
/// swapping in real data does not change the render code.
pub struct WorkspaceModel {
    pub guilds: Vec<GuildEntry>,
    pub channels: Vec<ChannelEntry>,
    pub members: Vec<MemberEntry>,
    pub selected_guild: usize,
    pub selected_channel: usize,
    pub connected: bool,
    pub status_line: String,
}

pub struct GuildEntry {
    /// `None` for the Direct Messages pseudo-guild.
    pub id: Option<Id<marker::GuildMarker>>,
    pub name: String,
    pub unread: bool,
}

pub struct ChannelEntry {
    pub id: Option<Id<marker::ChannelMarker>>,
    pub name: String,
    pub kind: ChannelKind,
    pub unread: bool,
    pub mentions: u32,
}

#[derive(PartialEq, Eq, Clone, Copy)]
pub enum ChannelKind {
    Text,
    Voice,
    Forum,
    Category,
}

impl ChannelKind {
    fn glyph(self) -> &'static str {
        match self {
            ChannelKind::Text => "#",
            ChannelKind::Voice => "♪",
            ChannelKind::Forum => "▤",
            ChannelKind::Category => "",
        }
    }
}

pub struct MemberEntry {
    pub name: String,
    pub presence: Presence,
    /// Group headers ("ONLINE - 42") render as section labels, not rows.
    pub is_group: bool,
    pub is_bot: bool,
    pub color: Option<u32>,
}

impl WorkspaceModel {
    /// Placeholder content so the layout can be evaluated before the snapshot
    /// projection lands. Explicitly not real data.
    pub fn placeholder() -> Self {
        Self {
            guilds: vec![
                GuildEntry {
                    id: None,
                    name: "Direct Messages".into(),
                    unread: false,
                },
                GuildEntry {
                    id: None,
                    name: "RostFaden".into(),
                    unread: true,
                },
            ],
            channels: vec![
                ChannelEntry {
                    id: None,
                    name: "TEXT CHANNELS".into(),
                    kind: ChannelKind::Category,
                    unread: false,
                    mentions: 0,
                },
                ChannelEntry {
                    id: None,
                    name: "general".into(),
                    kind: ChannelKind::Text,
                    unread: true,
                    mentions: 2,
                },
                ChannelEntry {
                    id: None,
                    name: "development".into(),
                    kind: ChannelKind::Text,
                    unread: false,
                    mentions: 0,
                },
                ChannelEntry {
                    id: None,
                    name: "VOICE".into(),
                    kind: ChannelKind::Category,
                    unread: false,
                    mentions: 0,
                },
                ChannelEntry {
                    id: None,
                    name: "General".into(),
                    kind: ChannelKind::Voice,
                    unread: false,
                    mentions: 0,
                },
            ],
            members: vec![],
            selected_guild: 1,
            selected_channel: 1,
            connected: false,
            status_line: "not connected - no session".into(),
        }
    }
}

pub struct Workspace {
    pub model: WorkspaceModel,
    /// Command sink into the core. `None` until a session starts.
    pub handle: Option<SessionHandle>,
    /// Navigation is GUI-owned: the core has no concept of "what is on screen".
    pub nav: Navigation,
    /// Projected rows for the open channel.
    pub messages: Vec<MessageRow>,
    pub composer: Composer,
    focus: FocusHandle,
}

impl Workspace {
    pub fn new(model: WorkspaceModel, cx: &mut Context<Self>) -> Self {
        Self {
            model,
            handle: None,
            nav: Navigation::default(),
            messages: Vec::new(),
            composer: Composer::default(),
            focus: cx.focus_handle(),
        }
    }

    /// Send the composer's contents to the open channel.
    ///
    /// The nonce lets the core match the gateway echo back to this send, so
    /// the message does not briefly appear twice.
    fn send_message(&mut self) {
        let (Some(handle), Some(channel_id)) = (&self.handle, self.nav.channel) else {
            return;
        };

        let content = self.composer.take();
        if content.trim().is_empty() {
            return;
        }

        handle.send(AppCommand::SendMessage {
            channel_id,
            nonce: next_message_nonce(),
            content,
            reply_to: None,
            attachments: Vec::new(),
        });
    }

    /// Attach the command sink once the session thread is running.
    pub fn attach(&mut self, handle: SessionHandle) {
        self.handle = Some(handle);
    }

    /// Drain the bridge's update stream on the foreground executor, reprojecting
    /// the view model whenever the core's state store advances.
    pub fn pump(
        window: WindowHandle<Workspace>,
        mut updates: mpsc::UnboundedReceiver<Update>,
        cx: &mut gpui::App,
    ) {
        cx.spawn(async move |cx| {
            while let Some(update) = updates.recv().await {
                let applied = window.update(cx, |workspace, _window, cx| {
                    match update {
                        Update::State(state) => {
                            workspace.model = projection::project(&state, &workspace.nav, true);
                            workspace.messages = match workspace.nav.channel {
                                Some(channel_id) => message::project_messages(&state, channel_id),
                                None => Vec::new(),
                            };
                        }
                        Update::Event(event) => workspace.absorb(*event),
                        Update::Closed(reason) => {
                            workspace.model.connected = false;
                            workspace.model.status_line =
                                reason.unwrap_or_else(|| "session closed".to_string());
                        }
                    }
                    cx.notify();
                });

                if applied.is_err() {
                    // Window is gone; stop pumping.
                    break;
                }
            }
        })
        .detach();
    }

    /// Switch the open channel.
    ///
    /// Three commands are needed: the core tracks its own notion of the
    /// selected channel (for read-state and typing), history must be requested
    /// because the cache is lazily populated, and a gateway subscription is
    /// required before Discord will push updates for it.
    pub fn open_channel(&mut self, channel_id: Id<marker::ChannelMarker>) {
        self.nav.channel = Some(channel_id);
        self.messages.clear();

        let Some(handle) = &self.handle else {
            return;
        };

        handle.send(AppCommand::SetSelectedMessageChannel {
            channel_id: Some(channel_id),
        });
        handle.send(AppCommand::LoadMessageHistory {
            channel_id,
            before: None,
        });

        match self.nav.selection {
            Selection::Guild(guild_id) => {
                handle.send(AppCommand::SubscribeGuildChannel {
                    guild_id,
                    channel_id,
                });
                // Discord streams the member list in windowed ranges; the
                // first two cover what fits on screen without over-fetching.
                handle.send(AppCommand::UpdateMemberListSubscription {
                    guild_id,
                    channel_id,
                    ranges: vec![(0, 99), (100, 199)],
                });
            }
            Selection::DirectMessages => {
                handle.send(AppCommand::SubscribeDirectMessage { channel_id });
            }
        }
    }

    /// The member pane only applies to guild channels.
    fn shows_members(&self) -> bool {
        matches!(self.nav.selection, Selection::Guild(_)) && self.nav.channel.is_some()
    }

    /// Switch the open guild, clearing the channel selection.
    pub fn open_guild(&mut self, guild_id: Option<Id<marker::GuildMarker>>) {
        self.nav.selection = match guild_id {
            Some(id) => Selection::Guild(id),
            None => Selection::DirectMessages,
        };
        self.nav.channel = None;
        self.messages.clear();

        if let (Some(handle), Some(guild_id)) = (&self.handle, guild_id) {
            handle.send(AppCommand::SetSelectedGuild {
                guild_id: Some(guild_id),
            });
        }
    }

    /// Fold a discrete event into the model.
    ///
    /// Most state arrives through reprojection; this handles only what is not
    /// represented in the state store, such as transient errors.
    fn absorb(&mut self, event: AppEvent) {
        match event {
            AppEvent::GatewayError { message } => {
                self.model.status_line = message;
            }
            AppEvent::Ready { user, .. } => {
                self.model.connected = true;
                self.model.status_line = format!("connected as {user}");
            }
            _ => {}
        }
    }

    fn guild_rail(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut rail = column()
            .w(px(layout::GUILD_RAIL))
            .h_full()
            .bg(rgb(DARK.bg))
            .items_center()
            .pt(px(space::MD))
            .gap(px(space::SM));

        for (index, guild) in self.model.guilds.iter().enumerate() {
            let selected = index == self.model.selected_guild;
            let guild_id = guild.id;
            rail = rail.child(
                gpui::div()
                    .id(("guild", index))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _event, _window, cx| {
                        this.open_guild(guild_id);
                        cx.notify();
                    }))
                    .relative()
                    .child(avatar(44., &guild.name))
                    .when(selected, |d| {
                        d.border_2().border_color(rgb(DARK.accent)).rounded_full()
                    })
                    .when(guild.unread && !selected, |d| {
                        d.border_1()
                            .border_color(rgb(DARK.text_muted))
                            .rounded_full()
                    }),
            );
        }

        rail
    }

    fn channel_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let guild_name = self
            .model
            .guilds
            .get(self.model.selected_guild)
            .map(|g| g.name.clone())
            .unwrap_or_default();

        let sidebar = panel_sunken(layout::SIDEBAR).child(
            row()
                .w_full()
                .h(px(layout::HEADER))
                .px(px(space::MD))
                .border_b_1()
                .border_color(rgb(DARK.border))
                .text_size(px(text::BASE))
                .text_color(rgb(DARK.text))
                .child(guild_name),
        );

        let mut list = column().w_full().pt(px(space::XS)).gap(px(1.));

        for (index, channel) in self.model.channels.iter().enumerate() {
            if channel.kind == ChannelKind::Category {
                list = list.child(section_label(channel.name.clone()));
                continue;
            }

            let selected = index == self.model.selected_channel;
            let mut entry = sidebar_row(selected)
                .child(
                    gpui::div()
                        .w(px(14.))
                        .text_color(rgb(DARK.text_subtle))
                        .child(channel.kind.glyph()),
                )
                .child(gpui::div().flex_1().child(channel.name.clone()));

            if channel.mentions > 0 {
                entry = entry.child(
                    gpui::div()
                        .px(px(6.))
                        .rounded_full()
                        .bg(rgb(DARK.danger))
                        .text_size(px(text::XS))
                        .text_color(rgb(DARK.on_accent))
                        .child(channel.mentions.to_string()),
                );
            }

            // Voice channels need a join, not a channel switch; only text-like
            // channels are click-to-open until voice controls land.
            let entry = match channel.id {
                Some(channel_id) if channel.kind != ChannelKind::Voice => entry
                    .id(("channel", index))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _event, _window, cx| {
                        this.open_channel(channel_id);
                        cx.notify();
                    }))
                    .into_any_element(),
                _ => entry.into_any_element(),
            };

            list = list.child(entry);
        }

        sidebar.child(list)
    }

    fn member_pane(&self) -> impl IntoElement {
        let mut pane = column()
            .w(px(layout::MEMBERS))
            .h_full()
            .bg(rgb(DARK.surface_sunken))
            .border_l_1()
            .border_color(rgb(DARK.border))
            .pt(px(space::SM))
            .overflow_hidden();

        if self.model.members.is_empty() {
            return pane.child(
                gpui::div()
                    .px(px(space::MD))
                    .pt(px(space::MD))
                    .text_size(px(text::XS))
                    .text_color(rgb(DARK.text_subtle))
                    .child("No member data"),
            );
        }

        for member in &self.model.members {
            if member.is_group {
                pane = pane.child(section_label(member.name.clone()));
                continue;
            }

            let mut entry = sidebar_row(false)
                .child(presence_dot(member.presence))
                .child(
                    gpui::div()
                        .flex_1()
                        .text_color(rgb(member.color.unwrap_or(DARK.text_muted)))
                        .child(member.name.clone()),
                );

            if member.is_bot {
                entry = entry.child(
                    gpui::div()
                        .px(px(4.))
                        .rounded(px(3.))
                        .bg(rgb(DARK.accent))
                        .text_size(px(text::XS))
                        .text_color(rgb(DARK.on_accent))
                        .child("BOT"),
                );
            }

            pane = pane.child(entry);
        }

        pane
    }

    fn content(&self, window: &Window) -> impl IntoElement {
        let channel_name = self
            .model
            .channels
            .get(self.model.selected_channel)
            .map(|c| c.name.clone())
            .unwrap_or_default();

        column()
            .flex_1()
            .h_full()
            .bg(rgb(DARK.surface))
            .child(
                header()
                    .child(
                        gpui::div()
                            .text_color(rgb(DARK.text_subtle))
                            .child(ChannelKind::Text.glyph()),
                    )
                    .child(
                        gpui::div()
                            .text_size(px(text::BASE))
                            .text_color(rgb(DARK.text))
                            .child(channel_name),
                    ),
            )
            .child(message_list(&self.messages))
            .child(self.composer_row(window))
    }

    fn composer_row(&self, window: &Window) -> impl IntoElement {
        let enabled = self.nav.channel.is_some() && self.handle.is_some();
        let placeholder = if !enabled {
            "Select a channel to start typing".to_string()
        } else {
            let name = self
                .model
                .channels
                .get(self.model.selected_channel)
                .map(|c| c.name.clone())
                .unwrap_or_default();
            format!("Message #{name}")
        };

        composer_view(
            &self.composer,
            self.focus.is_focused(window),
            enabled,
            &placeholder,
        )
    }

    fn status_bar(&self) -> impl IntoElement {
        row()
            .w_full()
            .h(px(24.))
            .px(px(space::MD))
            .gap(px(space::SM))
            .bg(rgb(DARK.surface_sunken))
            .border_t_1()
            .border_color(rgb(DARK.border))
            .text_size(px(text::XS))
            .text_color(rgb(DARK.text_subtle))
            .child(presence_dot(if self.model.connected {
                Presence::Online
            } else {
                Presence::Offline
            }))
            .child(self.model.status_line.clone())
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        column()
            .track_focus(&self.focus)
            .key_context("Workspace")
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, cx| {
                if this.composer.handle_key(event) {
                    this.send_message();
                }
                cx.notify();
            }))
            .size_full()
            .bg(rgb(DARK.bg))
            .text_size(px(text::BASE))
            .child(
                row()
                    .flex_1()
                    .w_full()
                    .overflow_hidden()
                    .child(self.guild_rail(cx))
                    .child(self.channel_sidebar(cx))
                    .child(self.content(window))
                    .when(self.shows_members(), |d| d.child(self.member_pane())),
            )
            .child(self.status_bar())
    }
}
