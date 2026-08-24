//! Editing the shape of a server: roles, channels and emoji.
//!
//! These do live in [`crate::discord::DiscordState`], unlike the lists in
//! [`super::server`], so they are written straight into it and the next
//! projection picks them up.
//!
//! The backend used to refuse all of this, on the grounds that "the fixture's
//! channel tree is canned, so a create or delete would be undone by the next
//! reproject". That was not true: the fake holds one state for the life of
//! the session and hands out clones of it, exactly as `delete_thread` has
//! always relied on. Nothing was undoing anything - the writes were simply
//! never made.

use std::sync::Arc;

use super::{blank_channel, channel_id, emoji_id, role_id};
use crate::discord::{
    ChannelState, DiscordState, GuildEmojiInfo, Id, NewChannelKind, RoleState, marker,
};

/// The kind name the core matches on, for one of the kinds a user can create.
///
/// The core keys off names rather than Discord's numeric wire values, so this
/// is the one place the two vocabularies meet.
fn kind_name(kind: NewChannelKind) -> &'static str {
    match kind {
        NewChannelKind::Text => "text",
        NewChannelKind::Voice => "voice",
        NewChannelKind::Category => "category",
        NewChannelKind::Announcement => "announcement",
        NewChannelKind::Forum => "forum",
        NewChannelKind::Stage => "stage",
    }
}

/// Add a channel to a server.
pub fn create_channel(
    state: &mut DiscordState,
    guild: Id<marker::GuildMarker>,
    name: &str,
    kind: NewChannelKind,
    parent: Option<Id<marker::ChannelMarker>>,
    id: u64,
) {
    let navigation = Arc::make_mut(&mut state.navigation);

    // Below everything already there, which is where Discord puts a new one.
    let position = navigation
        .channels
        .values()
        .filter(|channel| channel.guild_id == Some(guild))
        .filter_map(|channel| channel.position)
        .max()
        .unwrap_or(0)
        + 1;

    let mut channel: ChannelState = blank_channel(id, kind_name(kind), name);
    channel.guild_id = Some(guild);
    channel.parent_id = parent;
    channel.position = Some(position);
    navigation.channels.insert(channel.id, channel);
}

/// Apply an edit to a channel.
pub fn modify_channel(
    state: &mut DiscordState,
    channel: Id<marker::ChannelMarker>,
    edit: &crate::discord::ChannelEdit,
) {
    let navigation = Arc::make_mut(&mut state.navigation);
    let Some(existing) = navigation.channels.get_mut(&channel) else {
        return;
    };

    if let Some(name) = &edit.name {
        existing.name = name.clone();
    }
    // Doubly optional: the outer says whether to touch it, the inner is the
    // value, and clearing a topic is a legitimate edit.
    if let Some(topic) = &edit.topic {
        existing.topic = topic.clone();
    }
    if let Some(nsfw) = edit.nsfw {
        existing.nsfw = Some(nsfw);
    }
    if let Some(slowmode) = edit.slowmode_seconds {
        existing.rate_limit_per_user = Some(u64::from(slowmode));
    }
    if let Some(limit) = edit.user_limit {
        existing.user_limit = Some(u64::from(limit));
    }
    if let Some(parent) = &edit.parent_id {
        existing.parent_id = *parent;
    }
}

/// Remove a channel, and anything nested under it.
pub fn delete_channel(state: &mut DiscordState, channel: Id<marker::ChannelMarker>) {
    let navigation = Arc::make_mut(&mut state.navigation);

    // A category takes its children with it, or they are left pointing at a
    // parent that is gone and vanish from the sidebar without being deleted.
    let orphans: Vec<_> = navigation
        .channels
        .values()
        .filter(|entry| entry.parent_id == Some(channel))
        .map(|entry| entry.id)
        .collect();

    navigation.channels.remove(&channel);
    for orphan in &orphans {
        navigation.channels.remove(orphan);
    }

    let cache = Arc::make_mut(&mut state.message_cache);
    cache.timelines.remove(&channel);
    for orphan in orphans {
        cache.timelines.remove(&orphan);
    }
}

/// Put channels in a given order.
pub fn reorder_channels(state: &mut DiscordState, positions: &[(Id<marker::ChannelMarker>, u32)]) {
    let navigation = Arc::make_mut(&mut state.navigation);
    for (channel, position) in positions {
        if let Some(existing) = navigation.channels.get_mut(channel) {
            existing.position = Some(*position as i32);
        }
    }
}

/// Set or clear a voice channel's status line.
pub fn set_voice_status(
    state: &mut DiscordState,
    channel: Id<marker::ChannelMarker>,
    status: Option<String>,
) {
    let navigation = Arc::make_mut(&mut state.navigation);
    if let Some(existing) = navigation.channels.get_mut(&channel) {
        // Discord shows a voice channel's status where a text channel shows
        // its topic, and the state keeps them in the same field.
        existing.topic = status;
    }
}

/// Add a role to a server.
pub fn create_role(state: &mut DiscordState, guild: Id<marker::GuildMarker>, name: &str, id: u64) {
    let details = Arc::make_mut(&mut state.guild_details);
    let roles = details.roles.entry(guild).or_default();

    let position = roles.values().map(|role| role.position).max().unwrap_or(0) + 1;
    let role = RoleState {
        id: role_id(id),
        name: name.to_string(),
        // No colour, which is what Discord gives a new role.
        color: None,
        position,
        hoist: false,
        permissions: 0,
    };
    roles.insert(role.id, role);
}

/// Apply an edit to a role.
pub fn modify_role(
    state: &mut DiscordState,
    guild: Id<marker::GuildMarker>,
    role: Id<marker::RoleMarker>,
    edit: &crate::discord::RoleEdit,
) {
    let details = Arc::make_mut(&mut state.guild_details);
    let Some(existing) = details.roles.get_mut(&guild).and_then(|r| r.get_mut(&role)) else {
        return;
    };

    if let Some(name) = &edit.name {
        existing.name = name.clone();
    }
    if let Some(color) = &edit.color {
        existing.color = *color;
    }
    if let Some(hoist) = edit.hoist {
        existing.hoist = hoist;
    }
    if let Some(permissions) = edit.permissions {
        existing.permissions = permissions;
    }
}

/// Remove a role, and take it off everyone who had it.
pub fn delete_role(
    state: &mut DiscordState,
    guild: Id<marker::GuildMarker>,
    role: Id<marker::RoleMarker>,
) {
    let details = Arc::make_mut(&mut state.guild_details);
    if let Some(roles) = details.roles.get_mut(&guild) {
        roles.remove(&role);
    }

    // Left on its holders, the member list would colour and group people by
    // a role that no longer exists.
    if let Some(members) = details.members.get_mut(&guild) {
        for member in members.values_mut() {
            member.role_ids.retain(|held| *held != role);
        }
    }
}

/// Put roles in a given order.
pub fn reorder_roles(
    state: &mut DiscordState,
    guild: Id<marker::GuildMarker>,
    positions: &[(Id<marker::RoleMarker>, u32)],
) {
    let details = Arc::make_mut(&mut state.guild_details);
    let Some(roles) = details.roles.get_mut(&guild) else {
        return;
    };
    for (role, position) in positions {
        if let Some(existing) = roles.get_mut(role) {
            existing.position = i64::from(*position);
        }
    }
}

/// Custom emoji, as the emoji panel lists them.
///
/// Held here rather than in the state because the state has no place for a
/// server's emoji set - it only ever sees them inside reactions.
pub fn emojis() -> Vec<GuildEmojiInfo> {
    vec![
        GuildEmojiInfo {
            id: emoji_id(4001),
            name: "ferris".to_string(),
            animated: false,
            role_restricted: false,
        },
        // Animated, which about one in five real custom emoji is.
        GuildEmojiInfo {
            id: emoji_id(4002),
            name: "crab_party".to_string(),
            animated: true,
            role_restricted: false,
        },
        // Role-restricted, which almost none are - 0.4% across 4775 real
        // emoji. Kept because the flag needs somewhere to show, but it is one
        // of several rather than half the set.
        GuildEmojiInfo {
            id: emoji_id(4003),
            name: "shipit".to_string(),
            animated: false,
            role_restricted: true,
        },
        GuildEmojiInfo {
            id: emoji_id(4004),
            name: "rustaceanthink".to_string(),
            animated: false,
            role_restricted: false,
        },
        GuildEmojiInfo {
            id: emoji_id(4005),
            name: "blobwave".to_string(),
            animated: true,
            role_restricted: false,
        },
    ]
}

/// The channel id a newly created channel should take.
///
/// Well clear of the canned tree, so a new channel cannot collide with one
/// the fixture already placed.
pub fn fresh_channel_id(seed: u64) -> Id<marker::ChannelMarker> {
    channel_id(90_000 + seed)
}

/// Apply an edit to a server's own settings.
pub fn modify_guild(
    state: &mut DiscordState,
    guild: Id<marker::GuildMarker>,
    edit: &crate::discord::GuildEdit,
) {
    let navigation = Arc::make_mut(&mut state.navigation);
    let Some(existing) = navigation.guilds.get_mut(&guild) else {
        return;
    };
    // The name is the only part of a guild edit the state carries; the rest
    // are server-side policies with nothing on screen to change.
    if let Some(name) = &edit.name {
        existing.name = name.clone();
    }
}

/// Swap a server's icon for a different generated one.
///
/// Nothing is uploaded offline, but the rail redrawing with a new picture is
/// the visible half of setting an icon, and an action that appears to do
/// nothing is worse than one that clearly stands in.
pub fn cycle_guild_icon(state: &mut DiscordState, guild: Id<marker::GuildMarker>) {
    let navigation = Arc::make_mut(&mut state.navigation);
    let Some(existing) = navigation.guilds.get_mut(&guild) else {
        return;
    };
    existing.icon = match existing.icon.as_deref() {
        Some(url) if url.contains("Octocat") => None,
        _ => Some(
            "https://github.githubassets.com/images/modules/logos_page/Octocat.png".to_string(),
        ),
    };
}

/// Set a permission overwrite on a channel.
///
/// Replaces any existing one for the same target, which is what Discord's
/// endpoint does - two overwrites for one role would make the effective
/// permission depend on which was read first.
pub fn set_overwrite(
    state: &mut DiscordState,
    channel: Id<marker::ChannelMarker>,
    target: crate::discord::OverwriteTarget,
    allow: u64,
    deny: u64,
) {
    use crate::discord::{OverwriteTarget, PermissionOverwriteInfo, PermissionOverwriteKind};

    let navigation = Arc::make_mut(&mut state.navigation);
    let Some(existing) = navigation.channels.get_mut(&channel) else {
        return;
    };

    let (id, kind) = match target {
        OverwriteTarget::Role(role) => (role.get(), PermissionOverwriteKind::Role),
        OverwriteTarget::Member(user) => (user.get(), PermissionOverwriteKind::Member),
    };

    existing
        .permission_overwrites
        .retain(|overwrite| overwrite.id != id);
    existing
        .permission_overwrites
        .push(PermissionOverwriteInfo {
            id,
            kind,
            allow,
            deny,
        });
}

/// Remove a permission overwrite from a channel.
pub fn delete_overwrite(
    state: &mut DiscordState,
    channel: Id<marker::ChannelMarker>,
    target: crate::discord::OverwriteTarget,
) {
    use crate::discord::OverwriteTarget;

    let navigation = Arc::make_mut(&mut state.navigation);
    let Some(existing) = navigation.channels.get_mut(&channel) else {
        return;
    };
    let id = match target {
        OverwriteTarget::Role(role) => role.get(),
        OverwriteTarget::Member(user) => user.get(),
    };
    existing
        .permission_overwrites
        .retain(|overwrite| overwrite.id != id);
}
