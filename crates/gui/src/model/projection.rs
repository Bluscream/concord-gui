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
use crate::ui::profile::ProfileView;
use crate::ui::switcher::Candidate;
use crate::ui::workspace::{
    ChannelEntry, ChannelKind, GuildEntry, GuildFolderEntry, MemberEntry, VoiceMember,
    WorkspaceModel,
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
    } else if channel.is_stage() {
        // Checked before is_voice, which now matches a stage too.
        ChannelKind::Stage
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
        folder: None,
    }];

    // Folder membership, so guilds can be grouped the way the user arranged
    // them on another client. A folder with no id is Discord's representation
    // of a single ungrouped guild, not a real folder.
    let mut folder_of = std::collections::HashMap::new();
    for folder in state.guild_folders() {
        let Some(id) = folder.id else {
            continue;
        };
        let entry = GuildFolderEntry {
            id,
            name: folder.name.clone(),
            color: folder.color,
        };
        for guild_id in &folder.guild_ids {
            folder_of.insert(*guild_id, entry.clone());
        }
    }

    let mut guilds = state.guilds();
    // Grouped first, so a folder's guilds are adjacent; alphabetical within.
    guilds.sort_by(|a, b| {
        let folder = |id| folder_of.get(id).map(|folder: &GuildFolderEntry| folder.id);
        folder(&a.id)
            .cmp(&folder(&b.id))
            .then_with(|| a.name.cmp(&b.name))
    });

    entries.extend(guilds.into_iter().map(|guild| {
        let unread = state.guild_sidebar_unread(guild.id);
        GuildEntry {
            id: Some(guild.id),
            name: guild.name.clone(),
            unread: !matches!(unread, ChannelUnreadState::Seen),
            mentions: mention_count(unread),
            folder: folder_of.get(&guild.id).cloned(),
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
                last_message: channel.last_message_id,
                parent: channel.parent_id,
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
        .map(|(_, entry)| match entry {
            GuildMemberListEntry::Group { id, count } => MemberEntry {
                name: format!("{} - {}", id.to_uppercase(), count),
                activity: None,
                user_id: None,
                avatar: None,
                presence: Presence::Offline,
                is_group: true,
                is_bot: false,
                color: None,
            },
            GuildMemberListEntry::Member { user_id } => {
                let member = state.member_for_guild(guild_id, *user_id);
                MemberEntry {
                    // The first activity only: a sidebar row has space for one
                    // line, and the list is for scanning rather than reading.
                    activity: state
                        .user_activities_for_guild(Some(guild_id), *user_id)
                        .first()
                        .and_then(|activity| activity.display_line()),
                    name: member
                        .map(|m| m.display_name.clone())
                        .unwrap_or_else(|| "unknown".to_string()),
                    user_id: Some(*user_id),
                    avatar: member.and_then(|m| m.avatar_url.clone()),
                    presence: presence_of(state.user_presence_for_guild(Some(guild_id), *user_id)),
                    is_group: false,
                    is_bot: member.is_some_and(|m| m.is_bot),
                    color: state
                        .member_role_color(guild_id, *user_id)
                        .filter(|color| *color != 0),
                }
            }
        })
        .collect()
}

/// Project a cached profile for the panel.
///
/// Returns `None` when the fetch has not completed; the caller renders a
/// loading state rather than an empty profile.
pub fn project_profile(
    state: &DiscordState,
    user_id: Id<marker::UserMarker>,
    guild_id: Option<Id<marker::GuildMarker>>,
) -> Option<ProfileView> {
    let profile = state.user_profile(user_id, guild_id)?;

    // Roles are resolved to names and colours here so the view stays free of
    // core types.
    let roles = guild_id
        .map(|guild_id| {
            profile
                .role_ids
                .iter()
                .filter_map(|role_id| state.role_for_guild(guild_id, *role_id))
                .map(|role| (role.name.clone(), role.color.filter(|c| *c != 0)))
                .collect()
        })
        .unwrap_or_default();

    let mutual_guilds = profile
        .mutual_guilds
        .iter()
        .filter_map(|mutual| state.guild(mutual.guild_id))
        .map(|guild| guild.name.clone())
        .collect();

    // Custom status first, then whatever they are doing - the same order the
    // TUI's profile popup uses, so the two clients do not disagree.
    let mut activities: Vec<_> = state
        .user_activities_for_guild(guild_id, user_id)
        .iter()
        .collect();
    activities.sort_by_key(|activity| match activity.kind {
        concord::discord::ActivityKind::Custom => 0,
        concord::discord::ActivityKind::Streaming => 1,
        concord::discord::ActivityKind::Playing => 2,
        concord::discord::ActivityKind::Listening => 3,
        concord::discord::ActivityKind::Watching => 4,
        concord::discord::ActivityKind::Competing => 5,
        concord::discord::ActivityKind::Unknown => 6,
    });
    let activities = activities
        .into_iter()
        .filter_map(|activity| activity.display_line())
        .collect();

    Some(ProfileView {
        activities,
        display_name: profile
            .guild_nick
            .clone()
            .or_else(|| profile.global_name.clone())
            .unwrap_or_else(|| profile.username.clone()),
        handle: Some(profile.username.clone()),
        avatar: profile.avatar_url.clone(),
        // Guild-specific pronouns win over the global value, matching how
        // Discord itself scopes them.
        pronouns: profile
            .guild_pronouns
            .clone()
            .or_else(|| profile.pronouns.clone())
            .filter(|value| !value.is_empty()),
        bio: profile.bio.clone().filter(|value| !value.is_empty()),
        roles,
        mutual_guilds,
        loaded: true,
    })
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
            user_id: participant.user_id,
            name: participant.display_name,
            // Server mute/deafen and self mute/deafen are shown identically:
            // from the listener's side the audible result is the same.
            muted: participant.mute || participant.self_mute,
            deafened: participant.deaf || participant.self_deaf,
            streaming: participant.self_stream,
            // Separate from streaming: a camera and a shared screen are two
            // different things, and someone can be doing both at once.
            on_camera: participant.self_video,
            speaking: participant.speaking,
        })
        .collect()
}

/// Every channel the user can jump to, across every guild.
///
/// Built from the whole state rather than the open guild: the switcher's
/// value is reaching somewhere you are *not* currently looking.
pub fn switcher_candidates(state: &DiscordState) -> Vec<Candidate> {
    let mut out = Vec::new();

    // Direct messages first: they have no guild and are addressed by
    // participant name.
    for channel in state.channels_for_guild(None) {
        if channel.is_category() {
            continue;
        }
        out.push(Candidate {
            channel_id: channel.id,
            guild_id: None,
            name: channel.name.clone(),
            context: "Direct Messages".to_string(),
            kind: channel_kind(channel),
            unread: !matches!(
                state.channel_sidebar_unread(channel.id),
                ChannelUnreadState::Seen
            ),
        });
    }

    for guild in state.guilds() {
        for channel in state.channels_for_guild(Some(guild.id)) {
            // Categories are not destinations, and voice channels are joined
            // rather than opened, so neither belongs here.
            if channel.is_category() || channel.is_voice() {
                continue;
            }
            out.push(Candidate {
                channel_id: channel.id,
                guild_id: Some(guild.id),
                name: channel.name.clone(),
                context: guild.name.clone(),
                kind: channel_kind(channel),
                unread: !matches!(
                    state.channel_sidebar_unread(channel.id),
                    ChannelUnreadState::Seen
                ),
            });
        }
    }

    out
}
