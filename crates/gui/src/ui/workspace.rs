//! The main three-pane workspace: guild rail, channel sidebar, content area.
//!
//! Currently renders from a placeholder model. The next step replaces that
//! model with projections of `concord::discord::DiscordSnapshot`, which the
//! session already publishes on every state revision - no new plumbing needed,
//! only mapping.

use gpui::{Context, Window, prelude::*, px, rgb};

use crate::theme::{DARK, Presence, layout, space, text};
use crate::ui::chrome::{
    avatar, column, header, hint, panel_sunken, presence_dot, row, section_label, sidebar_row,
};

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
    pub name: String,
    pub unread: bool,
}

pub struct ChannelEntry {
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
}

impl WorkspaceModel {
    /// Placeholder content so the layout can be evaluated before the snapshot
    /// projection lands. Explicitly not real data.
    pub fn placeholder() -> Self {
        Self {
            guilds: vec![
                GuildEntry {
                    name: "Direct Messages".into(),
                    unread: false,
                },
                GuildEntry {
                    name: "RostFaden".into(),
                    unread: true,
                },
            ],
            channels: vec![
                ChannelEntry {
                    name: "TEXT CHANNELS".into(),
                    kind: ChannelKind::Category,
                    unread: false,
                    mentions: 0,
                },
                ChannelEntry {
                    name: "general".into(),
                    kind: ChannelKind::Text,
                    unread: true,
                    mentions: 2,
                },
                ChannelEntry {
                    name: "development".into(),
                    kind: ChannelKind::Text,
                    unread: false,
                    mentions: 0,
                },
                ChannelEntry {
                    name: "VOICE".into(),
                    kind: ChannelKind::Category,
                    unread: false,
                    mentions: 0,
                },
                ChannelEntry {
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
}

impl Workspace {
    pub fn new(model: WorkspaceModel) -> Self {
        Self { model }
    }

    fn guild_rail(&self) -> impl IntoElement {
        let mut rail = column()
            .w(px(layout::GUILD_RAIL))
            .h_full()
            .bg(rgb(DARK.bg))
            .items_center()
            .pt(px(space::MD))
            .gap(px(space::SM));

        for (index, guild) in self.model.guilds.iter().enumerate() {
            let selected = index == self.model.selected_guild;
            rail = rail.child(
                gpui::div()
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

    fn channel_sidebar(&self) -> impl IntoElement {
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

            list = list.child(entry);
        }

        sidebar.child(list)
    }

    fn content(&self) -> impl IntoElement {
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
            .child(
                // Message area. Empty until the snapshot projection lands -
                // showing fabricated messages here would misrepresent progress.
                column()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .gap(px(space::SM))
                    .child(hint("No session"))
                    .child(hint(self.model.status_line.clone())),
            )
            .child(self.composer())
    }

    fn composer(&self) -> impl IntoElement {
        gpui::div()
            .w_full()
            .px(px(space::LG))
            .pb(px(space::LG))
            .child(
                row()
                    .w_full()
                    .h(px(42.))
                    .px(px(space::MD))
                    .gap(px(space::SM))
                    .rounded(px(layout::RADIUS_LG))
                    .bg(rgb(DARK.surface_hover))
                    .text_size(px(text::BASE))
                    .text_color(rgb(DARK.text_subtle))
                    .child("Message input - not yet wired"),
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
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        column()
            .size_full()
            .bg(rgb(DARK.bg))
            .text_size(px(text::BASE))
            .child(
                row()
                    .flex_1()
                    .w_full()
                    .overflow_hidden()
                    .child(self.guild_rail())
                    .child(self.channel_sidebar())
                    .child(self.content()),
            )
            .child(self.status_bar())
    }
}
