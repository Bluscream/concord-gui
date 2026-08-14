//! Projects `concord`'s state store onto the GUI's view model.
//!
//! This is the only place that knows about core types. Views render the
//! projected model and never touch `DiscordState`, which keeps rendering cheap
//! and makes the projection independently testable.
//!
//! The core publishes a `SnapshotRevision` on every state change; the session
//! bridge reprojects on each revision rather than diffing, because
//! `DiscordState` is already an immutable snapshot behind `Arc`s.

use concord::discord::{
    ChannelUnreadState, DiscordState, GuildMemberListEntry, Id, PresenceStatus, marker,
};

use crate::theme::Presence;
use crate::ui::workspace::{
    ChannelEntry, ChannelKind, GuildEntry, MemberEntry, VoiceMember, WorkspaceModel,
};

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

/// Badge count for an unread state. `Unread` shows a dot, not a number, so
/// only explicit mention/notify counts produce a badge.
fn mention_count(unread: ChannelUnreadState) -> u32 {
    match unread {
        ChannelUnreadState::Mentioned(count) | ChannelUnreadState::Notified(count) => count,
        _ => 0,
    }
}

/// Users currently typing in a channel, resolved to display names.
pub fn typing_names(
    state: &DiscordState,
    channel_id: Id<marker::ChannelMarker>,
    guild_id: Option<Id<marker::GuildMarker>>,
) -> Vec<String> {
    state
        .typing_users(channel_id)
        .into_iter()
        .map(|typer| {
            guild_id
                .and_then(|guild_id| state.member_for_guild(guild_id, typer.user_id))
                .map(|member| member.display_name.clone())
                .unwrap_or_else(|| "someone".to_string())
        })
        .collect()
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
    } else if channel.is_thread() {
        ChannelKind::Thread
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
    let dm_unread = state.direct_message_unread_count();
    let mut entries = vec![GuildEntry {
        id: None,
        name: "Direct Messages".to_string(),
        unread: dm_unread > 0,
        mentions: dm_unread as u32,
    }];

    let mut guilds = state.guilds();
    guilds.sort_by(|a, b| a.name.cmp(&b.name));

    entries.extend(guilds.into_iter().map(|guild| {
        let unread = state.guild_sidebar_unread(guild.id);
        GuildEntry {
            id: Some(guild.id),
            name: guild.name.clone(),
            unread: !matches!(unread, ChannelUnreadState::Seen),
            mentions: mention_count(unread),
        }
    }));

    entries
}

fn project_channels(state: &DiscordState, nav: &Navigation) -> Vec<ChannelEntry> {
    let guild_id = match nav.selection {
        Selection::DirectMessages => None,
        Selection::Guild(id) => Some(id),
    };

    let mut channels = state.channels_for_guild(guild_id);

    // Discord's own ordering, with threads pulled out and reinserted directly
    // beneath their parent so the tree reads correctly.
    channels.sort_by_key(|c| (c.position.unwrap_or(i32::MAX), c.name.clone()));

    let (threads, roots): (Vec<_>, Vec<_>) = channels.into_iter().partition(|c| c.is_thread());

    let mut ordered = Vec::with_capacity(roots.len() + threads.len());
    for channel in roots {
        let id = channel.id;
        ordered.push(channel);
        ordered.extend(
            threads
                .iter()
                .filter(|thread| thread.parent_id == Some(id))
                .copied(),
        );
    }

    ordered
        .into_iter()
        .map(|channel| {
            let unread = state.channel_sidebar_unread(channel.id);
            let voice = match (channel.is_voice(), guild_id) {
                (true, Some(guild_id)) => voice_participants(state, guild_id, channel.id),
                _ => Vec::new(),
            };

            ChannelEntry {
                id: Some(channel.id),
                name: if channel.is_category() {
                    channel.name.to_uppercase()
                } else {
                    channel.name.clone()
                },
                kind: channel_kind(channel),
                unread: !matches!(unread, ChannelUnreadState::Seen),
                mentions: mention_count(unread),
                voice,
                archived: channel.thread_archived().unwrap_or(false),
            }
        })
        .collect()
}

fn project_members(state: &DiscordState, nav: &Navigation) -> Vec<MemberEntry> {
    let Selection::Guild(guild_id) = nav.selection else {
        return Vec::new();
    };

    // Discord's member list is a positional structure interleaving group
    // headers ("Online", role names) with members, so it is walked in order
    // rather than sorted here - the server's ordering is the correct one.
    let entries = state.member_list_entries_for_guild(guild_id);

    if entries.is_empty() {
        return Vec::new();
    }

    entries
        .into_iter()
        .filter_map(|(_, entry)| match entry {
            GuildMemberListEntry::Group { id, count } => Some(MemberEntry {
                name: format!("{} - {}", id.to_uppercase(), count),
                avatar: None,
                presence: Presence::Offline,
                is_group: true,
                is_bot: false,
                color: None,
            }),
            GuildMemberListEntry::Member { user_id } => {
                let member = state.member_for_guild(guild_id, *user_id);
                Some(MemberEntry {
                    name: member
                        .map(|m| m.display_name.clone())
                        .unwrap_or_else(|| "unknown".to_string()),
                    avatar: member.and_then(|m| m.avatar_url.clone()),
                    presence: presence_of(state.user_presence_for_guild(Some(guild_id), *user_id)),
                    is_group: false,
                    is_bot: member.is_some_and(|m| m.is_bot),
                    color: state
                        .member_role_color(guild_id, *user_id)
                        .filter(|color| *color != 0),
                })
            }
        })
        .collect()
}

/// Who is sitting in a voice channel, for the sidebar's nested participant
/// rows. Empty for non-voice channels.
pub fn voice_participants(
    state: &DiscordState,
    guild_id: Id<marker::GuildMarker>,
    channel_id: Id<marker::ChannelMarker>,
) -> Vec<VoiceMember> {
    state
        .voice_participants_for_channel(guild_id, channel_id)
        .into_iter()
        .map(|participant| VoiceMember {
            name: participant.display_name,
            // Server mute/deafen and self mute/deafen are shown identically:
            // from the listener's side the audible result is the same.
            muted: participant.mute || participant.self_mute,
            deafened: participant.deaf || participant.self_deaf,
            streaming: participant.self_stream,
            speaking: participant.speaking,
        })
        .collect()
}

/// Presence lookup for a single user, used by DM rows and member entries.
pub fn user_presence(state: &DiscordState, user_id: Id<marker::UserMarker>) -> Presence {
    presence_of(state.user_presence(user_id))
}
