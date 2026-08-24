//! Moderating people, and the account's relationships with them.
//!
//! The fixture has one signed-in account, so a friend request can never be
//! accepted by anybody. What it can do is put the relationship into the state
//! the request leaves it in - outgoing, blocked, gone - which is what the
//! profile pane and the friends list read, and is the whole visible result of
//! pressing those buttons.

use std::sync::Arc;

use super::{demo_guild_id, guild_id, user_id};
use crate::discord::{
    ApplicationCommandAutocompleteInvocation, ApplicationCommandChoiceInfo, DiscordState,
    FriendStatus, GuildBanInfo, Id, RelationshipInfo, UserProfileUpdate, marker,
};

/// Silence a member for a while, or lift the silence.
pub fn set_member_timeout(
    state: &mut DiscordState,
    guild: Id<marker::GuildMarker>,
    user: Id<marker::UserMarker>,
    minutes: Option<u32>,
) {
    let details = Arc::make_mut(&mut state.guild_details);
    let Some(member) = details
        .members
        .get_mut(&guild)
        .and_then(|m| m.get_mut(&user))
    else {
        return;
    };
    // Discord stores a timeout as the moment it ends, so lifting one is
    // clearing the field rather than writing a zero.
    member.communication_disabled_until =
        minutes.map(|minutes| chrono::Utc::now() + chrono::Duration::minutes(i64::from(minutes)));
}

/// A member's display name, for a list that will outlive their membership.
///
/// Read before removing them: a ban list showing a bare id would be useless,
/// and once the member is gone there is nowhere left to look the name up.
pub fn member_name(
    state: &DiscordState,
    guild: Id<marker::GuildMarker>,
    user: Id<marker::UserMarker>,
) -> String {
    state
        .guild_details
        .members
        .get(&guild)
        .and_then(|members| members.get(&user))
        .map(|member| member.display_name.clone())
        .unwrap_or_else(|| user.get().to_string())
}

/// Look somebody up by what was typed into the add-friend box.
///
/// Matched against display name and username, because either is what a person
/// would type, and case-insensitively for the same reason.
pub fn user_by_name(state: &DiscordState, typed: &str) -> Option<Id<marker::UserMarker>> {
    let needle = typed.trim().trim_start_matches('@').to_ascii_lowercase();
    if needle.is_empty() {
        return None;
    }
    state
        .guild_details
        .members
        .get(&demo_guild_id())?
        .values()
        .find(|member| {
            member.display_name.to_ascii_lowercase() == needle
                || member
                    .username
                    .as_deref()
                    .is_some_and(|name| name.to_ascii_lowercase() == needle)
        })
        .map(|member| member.user_id)
}

/// A relationship record for somebody already in the fixture.
pub fn relationship(
    state: &DiscordState,
    user: Id<marker::UserMarker>,
    status: FriendStatus,
) -> RelationshipInfo {
    let member = state
        .guild_details
        .members
        .get(&demo_guild_id())
        .and_then(|members| members.get(&user));

    RelationshipInfo {
        user_id: user,
        status,
        nickname: None,
        display_name: member.map(|member| member.display_name.clone()),
        username: member.and_then(|member| member.username.clone()),
    }
}

/// Ban records, for a server that has some.
pub fn bans() -> Vec<GuildBanInfo> {
    vec![
        GuildBanInfo {
            user_id: user_id(2001),
            username: "spammer".to_string(),
            reason: Some("Advertising".to_string()),
        },
        // No reason, which Discord allows and the row has to cope with.
        GuildBanInfo {
            user_id: user_id(2002),
            username: "throwaway".to_string(),
            reason: None,
        },
    ]
}

/// Apply an edit to the signed-in account's own profile.
///
/// Only the parts the state carries: an avatar upload has no file behind it
/// offline, and a bio lives on the profile record rather than the member.
pub fn update_self_profile(state: &mut DiscordState, update: &UserProfileUpdate) {
    let details = Arc::make_mut(&mut state.guild_details);

    if let Some(name) = &update.global.display_name {
        // The account is a member of every server it is in, and the display
        // name is per-member, so all of them move together.
        for members in details.members.values_mut() {
            if let Some(member) = members.get_mut(&update.user_id) {
                member.display_name = name.clone();
            }
        }
    }

    if let Some(guild) = &update.guild
        && let Some(nickname) = &guild.nickname
        && let Some(member) = details
            .members
            .get_mut(&guild.guild_id)
            .and_then(|members| members.get_mut(&update.user_id))
    {
        member.nickname = Some(nickname.clone());
    }
}

/// Autocomplete suggestions for a half-typed slash command argument.
///
/// Filtered on what has been typed so the list narrows as you type, which is
/// the behaviour worth demonstrating - a fixed list would look the same at
/// every keystroke and prove nothing.
pub fn autocomplete_choices(
    invocation: &ApplicationCommandAutocompleteInvocation,
) -> Vec<ApplicationCommandChoiceInfo> {
    const CANDIDATES: [&str; 5] = [
        "general",
        "gui-rewrite",
        "ci-logs",
        "announcements",
        "rules",
    ];

    let typed = invocation
        .content
        .rsplit(' ')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();

    CANDIDATES
        .iter()
        .filter(|candidate| typed.is_empty() || candidate.contains(&typed))
        .map(|candidate| ApplicationCommandChoiceInfo {
            name: (*candidate).to_string(),
            value: serde_json::Value::String((*candidate).to_string()),
        })
        .collect()
}

/// Set a thread's notification level.
///
/// Stored on the channel's flags, which is where Discord keeps it and where
/// the thread header reads it back from.
pub fn set_thread_notification_level(
    state: &mut DiscordState,
    channel: Id<marker::ChannelMarker>,
    flags: u64,
) {
    let navigation = Arc::make_mut(&mut state.navigation);
    if let Some(existing) = navigation.channels.get_mut(&channel) {
        existing.flags = Some(flags);
    }
}

/// The server the ban list belongs to.
pub fn ban_guild() -> Id<marker::GuildMarker> {
    guild_id(10)
}
