//! Projects `concord`'s state store onto the GUI's view model.
//!
//! This is the only place that knows about core types. Views render the
//! projected model and never touch `DiscordState`, which keeps rendering cheap
//! and makes the projection independently testable.
//!
//! The core publishes a `SnapshotRevision` on every state change; the session
//! bridge reprojects on each revision rather than diffing, because
//! `DiscordState` is already an immutable snapshot behind `Arc`s.

use concord::discord::{DiscordState, Id, PresenceStatus, marker};

use crate::theme::Presence;
use crate::ui::workspace::{ChannelEntry, ChannelKind, GuildEntry, MemberEntry, WorkspaceModel};

/// Identifies what the user currently has open.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Selection {
    /// The DM list rather than a guild.
    #[default]
    DirectMessages,
    Guild(Id<marker::GuildMarker>),
}

/// Everything the GUI tracks that is *not* derived from the core snapshot.
#[derive(Default)]
pub struct Navigation {
    pub selection: Selection,
    pub channel: Option<Id<marker::ChannelMarker>>,
}

fn presence_of(status: Option<PresenceStatus>) -> Presence {
    match status {
        Some(PresenceStatus::Online) => Presence::Online,
        Some(PresenceStatus::Idle) => Presence::Idle,
        Some(PresenceStatus::DoNotDisturb) => Presence::Dnd,
        _ => Presence::Offline,
    }
}

fn channel_kind(channel: &concord::discord::ChannelState) -> ChannelKind {
    if channel.is_category() {
        ChannelKind::Category
    } else if channel.is_voice() {
        ChannelKind::Voice
    } else if channel.is_forum() {
        ChannelKind::Forum
    } else {
        ChannelKind::Text
    }
}

/// Build the view model for the current navigation state.
pub fn project(state: &DiscordState, nav: &Navigation, connected: bool) -> WorkspaceModel {
    let guilds = project_guilds(state);
    let channels = project_channels(state, nav);
    let members = project_members(state, nav);

    let selected_guild = match nav.selection {
        Selection::DirectMessages => 0,
        Selection::Guild(id) => guilds
            .iter()
            .position(|g| g.id == Some(id))
            .unwrap_or_default(),
    };

    let selected_channel = nav
        .channel
        .and_then(|id| channels.iter().position(|c| c.id == Some(id)))
        .unwrap_or(usize::MAX);

    let status_line = if connected {
        format!("{} guilds", guilds.len().saturating_sub(1))
    } else {
        "connecting…".to_string()
    };

    WorkspaceModel {
        guilds,
        channels,
        members,
        selected_guild,
        selected_channel,
        connected,
        status_line,
    }
}

fn project_guilds(state: &DiscordState) -> Vec<GuildEntry> {
    // Index 0 is always the DM pseudo-guild, mirroring how the sidebar treats
    // direct messages as a peer of servers.
    let mut entries = vec![GuildEntry {
        id: None,
        name: "Direct Messages".to_string(),
        unread: false,
    }];

    let mut guilds = state.guilds();
    guilds.sort_by(|a, b| a.name.cmp(&b.name));

    entries.extend(guilds.into_iter().map(|guild| GuildEntry {
        id: Some(guild.id),
        name: guild.name.clone(),
        unread: false,
    }));

    entries
}

fn project_channels(state: &DiscordState, nav: &Navigation) -> Vec<ChannelEntry> {
    let guild_id = match nav.selection {
        Selection::DirectMessages => None,
        Selection::Guild(id) => Some(id),
    };

    let mut channels = state.channels_for_guild(guild_id);

    // Categories first at each position, then by Discord's own ordering.
    channels.sort_by_key(|c| (c.position.unwrap_or(i32::MAX), c.name.clone()));

    channels
        .into_iter()
        .filter(|c| !c.is_thread())
        .map(|channel| ChannelEntry {
            id: Some(channel.id),
            name: if channel.is_category() {
                channel.name.to_uppercase()
            } else {
                channel.name.clone()
            },
            kind: channel_kind(channel),
            unread: false,
            mentions: 0,
        })
        .collect()
}

fn project_members(state: &DiscordState, nav: &Navigation) -> Vec<MemberEntry> {
    let Selection::Guild(guild_id) = nav.selection else {
        return Vec::new();
    };

    let Some(guild) = state.guild(guild_id) else {
        return Vec::new();
    };

    let _ = guild;

    // Member list population is gated on GUILD_MEMBER_LIST_UPDATE, which the
    // core requests lazily per channel. Until the GUI issues that request the
    // honest projection is empty rather than a partial list.
    Vec::new()
}

/// Presence lookup for a single user, used by DM rows and member entries.
pub fn user_presence(state: &DiscordState, user_id: Id<marker::UserMarker>) -> Presence {
    presence_of(state.user_presence(user_id))
}
