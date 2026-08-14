//! Synthetic state for offline development and testing.
//!
//! Gated behind the `fixtures` feature so it is never compiled into a release
//! build. Front-ends can use it to exercise every rendering path - guilds,
//! categories, text and voice channels, DMs, group DMs, message grouping,
//! replies, attachments, reactions, unread counts, mentions, member lists,
//! presence and typing - without a Discord account or network access.
//!
//! This populates the caches directly rather than replaying gateway payloads.
//! That keeps the fixture readable, at the cost of not exercising the ingest
//! path; ingest is covered by the crate's own tests.
//!
//! All ids are small, fixed numbers so they are recognisable in logs. Note
//! that snowflake ids encode a timestamp in their high bits, so small ids
//! render as timestamps near the Discord epoch (2015). Message ids are
//! therefore built from real times via [`snowflake_at`].

use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::discord::{
    ChannelRecipientState, ChannelState, DiscordState, GuildBoostTier, GuildMemberListEntry,
    GuildMemberState, GuildState, Id, MessageState, PresenceStatus, ReactionEmoji, ReactionInfo,
    ReplyInfo, RoleState, marker,
};

const DISCORD_EPOCH_MS: u64 = 1_420_070_400_000;

/// The token that selects fixture mode instead of a real session.
pub const FIXTURE_TOKEN: &str = "test";

/// Build a snowflake for `seconds_ago`, so fixture messages carry plausible
/// timestamps rather than 2015 dates.
fn snowflake_at(seconds_ago: u64) -> u64 {
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(DISCORD_EPOCH_MS);
    let target = now_ms.saturating_sub(seconds_ago * 1000);
    (target.saturating_sub(DISCORD_EPOCH_MS)) << 22
}

fn guild_id(raw: u64) -> Id<marker::GuildMarker> {
    Id::new(raw)
}
fn channel_id(raw: u64) -> Id<marker::ChannelMarker> {
    Id::new(raw)
}
fn user_id(raw: u64) -> Id<marker::UserMarker> {
    Id::new(raw)
}
fn role_id(raw: u64) -> Id<marker::RoleMarker> {
    Id::new(raw)
}
fn message_id(raw: u64) -> Id<marker::MessageMarker> {
    Id::new(raw)
}

fn guild(id: u64, name: &str, members: u64, online: u32) -> GuildState {
    GuildState {
        id: guild_id(id),
        name: name.to_string(),
        member_count: Some(members),
        online_count: Some(online),
        owner_id: Some(user_id(1001)),
        boost_tier: GuildBoostTier::default(),
        boost_count: 0,
        verification_level: None,
        mfa_level: None,
        features: None,
        onboarding: None,
    }
}

/// Base channel. Written out in full rather than via `Default`, because `Id`
/// has no `Default` by design - a zero snowflake is not a valid id.
fn blank_channel(id: u64, kind: &str, name: &str) -> ChannelState {
    ChannelState {
        id: channel_id(id),
        guild_id: None,
        parent_id: None,
        owner_id: None,
        position: None,
        last_message_id: None,
        name: name.to_string(),
        kind: kind.to_string(),
        message_count: None,
        member_count: None,
        total_message_sent: None,
        thread_metadata: None,
        flags: None,
        rate_limit_per_user: None,
        available_tags: Vec::new(),
        applied_tags: Vec::new(),
        current_user_joined_thread: false,
        current_user_thread_notification_flags: None,
        current_user_thread_muted: false,
        current_user_thread_mute_end_time: None,
        recipients: Vec::new(),
        permission_overwrites: Vec::new(),
        is_message_request: None,
        is_spam: None,
    }
}

fn channel(
    id: u64,
    guild: Option<u64>,
    parent: Option<u64>,
    name: &str,
    kind: &str,
    position: i32,
) -> ChannelState {
    let mut channel = blank_channel(id, kind, name);
    channel.guild_id = guild.map(guild_id);
    channel.parent_id = parent.map(channel_id);
    channel.position = Some(position);
    channel
}

fn dm(id: u64, recipients: &[(u64, &str)]) -> ChannelState {
    // Discord channel kinds: 1 = DM, 3 = group DM.
    // The core matches on kind *names*, not Discord's numeric wire values.
    let kind = if recipients.len() > 1 {
        "group-dm"
    } else {
        "dm"
    };
    let name = recipients
        .iter()
        .map(|(_, name)| *name)
        .collect::<Vec<_>>()
        .join(", ");

    let mut channel = blank_channel(id, kind, &name);
    channel.recipients = recipients
        .iter()
        .map(|(id, name)| ChannelRecipientState {
            user_id: user_id(*id),
            display_name: name.to_string(),
            username: Some(name.to_lowercase()),
            is_bot: false,
            avatar_url: None,
            status: PresenceStatus::Online,
        })
        .collect();
    channel
}

fn member(id: u64, display: &str, bot: bool, roles: &[u64]) -> GuildMemberState {
    GuildMemberState {
        user_id: user_id(id),
        display_name: display.to_string(),
        username: Some(display.to_lowercase()),
        nickname: None,
        is_bot: bot,
        // Left unset: the fixture must not reach the network, so avatars fall
        // back to the deterministic initial. Real sessions supply CDN URLs.
        avatar_url: None,
        role_ids: roles.iter().map(|r| role_id(*r)).collect(),
        role_ids_known: true,
        joined_at: None,
        flags: None,
        pending: None,
        communication_disabled_until: None,
        status: PresenceStatus::Online,
    }
}

/// Build a message.
///
/// `guild` must be set for guild-channel messages: both mention resolution and
/// author role colours are guild-scoped, and omitting it silently disables
/// both. DMs correctly pass `None`.
fn message(
    id: u64,
    channel: u64,
    guild: Option<u64>,
    author: u64,
    name: &str,
    body: &str,
    age: u64,
) -> MessageState {
    // MessageState implements Default (with a placeholder id), so only the
    // fields that matter to rendering are overridden here.
    let mut message = MessageState::default();
    message.id = message_id(snowflake_at(age).max(id));
    message.channel_id = channel_id(channel);
    message.guild_id = guild.map(guild_id);
    message.author_id = user_id(author);
    message.author = name.to_string();
    message.content = Some(body.to_string());
    message
}

/// A fully-populated state for offline UI work.
pub fn demo_state() -> DiscordState {
    let mut state = DiscordState::default();

    let navigation = Arc::make_mut(&mut state.navigation);

    // ---- guilds ------------------------------------------------------------
    navigation
        .guilds
        .insert(guild_id(10), guild(10, "RostFaden", 128, 34));
    navigation
        .guilds
        .insert(guild_id(20), guild(20, "Rust Community", 4210, 812));

    // ---- guild 10 channels -------------------------------------------------
    let channels = [
        channel(100, Some(10), None, "information", "category", 0),
        channel(101, Some(10), Some(100), "announcements", "text", 1),
        channel(102, Some(10), Some(100), "rules", "text", 2),
        channel(110, Some(10), None, "development", "category", 3),
        channel(111, Some(10), Some(110), "general", "text", 4),
        channel(112, Some(10), Some(110), "gui-rewrite", "text", 5),
        channel(113, Some(10), Some(110), "ci-logs", "text", 6),
        channel(120, Some(10), None, "voice", "category", 7),
        channel(121, Some(10), Some(120), "Standup", "voice", 8),
        channel(122, Some(10), Some(120), "Pairing", "voice", 9),
        // guild 20
        channel(200, Some(20), None, "community", "category", 0),
        channel(201, Some(20), Some(200), "help", "text", 1),
        channel(202, Some(20), Some(200), "showcase", "text", 2),
    ];
    for channel in channels {
        navigation.channels.insert(channel.id, channel);
    }

    // A thread under #gui-rewrite, plus an archived one, so the sidebar's
    // nesting and dimming both have something to render.
    let mut thread = channel(130, Some(10), Some(112), "avatar-loading", "thread", 0);
    thread.thread_metadata = Some(crate::discord::ThreadMetadataInfo {
        archived: false,
        auto_archive_duration: Some(1440),
        archive_timestamp: None,
        locked: false,
        invitable: None,
        create_timestamp: None,
    });
    navigation.channels.insert(thread.id, thread);

    let mut archived = channel(131, Some(10), Some(112), "old-discussion", "thread", 1);
    archived.thread_metadata = Some(crate::discord::ThreadMetadataInfo {
        archived: true,
        auto_archive_duration: Some(1440),
        archive_timestamp: None,
        locked: false,
        invitable: None,
        create_timestamp: None,
    });
    navigation.channels.insert(archived.id, archived);

    // ---- direct messages ---------------------------------------------------
    for channel in [
        dm(300, &[(2001, "ferris")]),
        dm(301, &[(2002, "hoare")]),
        dm(
            302,
            &[(2001, "ferris"), (2003, "turing"), (2004, "lovelace")],
        ),
    ] {
        navigation.channels.insert(channel.id, channel);
    }

    // ---- roles and members -------------------------------------------------
    let guild_details = Arc::make_mut(&mut state.guild_details);

    let roles = vec![
        RoleState {
            id: role_id(1),
            name: "Maintainer".into(),
            color: Some(0x5b8def),
            position: 3,
            hoist: true,
            permissions: 0,
        },
        RoleState {
            id: role_id(2),
            name: "Contributor".into(),
            color: Some(0x3fb950),
            position: 2,
            hoist: true,
            permissions: 0,
        },
        RoleState {
            id: role_id(3),
            name: "Bot".into(),
            color: Some(0xd29922),
            position: 1,
            hoist: false,
            permissions: 0,
        },
    ];
    guild_details.roles.insert(
        guild_id(10),
        roles.iter().map(|role| (role.id, role.clone())).collect(),
    );

    let members = vec![
        member(1001, "blu", false, &[1]),
        member(1002, "ferris", false, &[2]),
        member(1003, "turing", false, &[2]),
        member(1004, "lovelace", false, &[]),
        member(1005, "ci-bot", true, &[3]),
    ];
    let member_map: BTreeMap<_, _> = members
        .iter()
        .map(|member| (member.user_id, member.clone()))
        .collect();
    guild_details.members.insert(guild_id(10), member_map);

    // Member list: Discord interleaves group headers with members.
    let member_list = vec![
        (
            0u32,
            GuildMemberListEntry::Group {
                id: "maintainer".into(),
                count: 1,
            },
        ),
        (
            1,
            GuildMemberListEntry::Member {
                user_id: user_id(1001),
            },
        ),
        (
            2,
            GuildMemberListEntry::Group {
                id: "contributor".into(),
                count: 2,
            },
        ),
        (
            3,
            GuildMemberListEntry::Member {
                user_id: user_id(1002),
            },
        ),
        (
            4,
            GuildMemberListEntry::Member {
                user_id: user_id(1003),
            },
        ),
        (
            5,
            GuildMemberListEntry::Group {
                id: "online".into(),
                count: 2,
            },
        ),
        (
            6,
            GuildMemberListEntry::Member {
                user_id: user_id(1004),
            },
        ),
        (
            7,
            GuildMemberListEntry::Member {
                user_id: user_id(1005),
            },
        ),
    ];
    guild_details.set_fixture_member_list(guild_id(10), member_list);

    // ---- presence ----------------------------------------------------------
    let presence = Arc::make_mut(&mut state.presence);
    for (user, status) in [
        (1001, PresenceStatus::Online),
        (1002, PresenceStatus::Online),
        (1003, PresenceStatus::Idle),
        (1004, PresenceStatus::DoNotDisturb),
        (1005, PresenceStatus::Online),
        (2001, PresenceStatus::Online),
        (2002, PresenceStatus::Offline),
    ] {
        presence
            .guild_user_presences
            .insert((guild_id(10), user_id(user)), status);
        presence.user_presences.insert(user_id(user), status);
    }

    // Someone typing in #gui-rewrite, to exercise the indicator.
    presence.set_fixture_typing(channel_id(112), &[user_id(1002)]);

    // Occupants in a voice channel, covering speaking/muted/streaming rows.
    let voice = Arc::make_mut(&mut state.voice);
    voice.set_fixture_participants(
        guild_id(10),
        channel_id(121),
        &[
            (user_id(1001), "blu", true, false, false),
            (user_id(1002), "ferris", false, true, false),
            (user_id(1005), "ci-bot", false, false, true),
        ],
    );

    // ---- messages ----------------------------------------------------------
    let message_cache = Arc::make_mut(&mut state.message_cache);

    let mut general = vec![
        message(1, 111, Some(10), 1001, "blu", "morning all", 7200),
        message(2, 111, Some(10), 1002, "ferris", "morning", 7100),
        message(
            3,
            111,
            Some(10),
            1002,
            "ferris",
            "grouped with the line above",
            7095,
        ),
        message(4, 111, Some(10), 1002, "ferris", "and this one too", 7090),
        message(
            5,
            111,
            Some(10),
            1003,
            "turing",
            "what's the plan for today?",
            6000,
        ),
    ];

    // A reply, which must break grouping.
    let mut reply = message(
        6,
        111,
        Some(10),
        1001,
        "blu",
        "finishing the member list",
        5900,
    );
    reply.reply = Some(ReplyInfo {
        author_id: Some(user_id(1003)),
        author: "turing".into(),
        content: Some("what's the plan for today?".into()),
        sticker_names: Vec::new(),
        mentions: Vec::new(),
    });
    general.push(reply);

    // Reactions.
    let mut reacted = message(
        7,
        111,
        Some(10),
        1004,
        "lovelace",
        "nice work on the composer",
        3000,
    );
    reacted.reactions = vec![
        ReactionInfo {
            emoji: ReactionEmoji::Unicode("👍".into()),
            count: 3,
            me: true,
        },
        ReactionInfo {
            emoji: ReactionEmoji::Custom {
                id: Id::new(9001),
                name: Some("ferris".into()),
                animated: false,
            },
            count: 1,
            me: false,
        },
    ];
    general.push(reacted);

    // Exercises mention resolution end to end.
    general.push(message(
        9,
        111,
        Some(10),
        1003,
        "turing",
        "ping <@1002> about <#111> when you get a chance",
        1200,
    ));

    let mut edited = message(
        8,
        111,
        Some(10),
        1005,
        "ci-bot",
        "build #482 passed in 3m12s",
        600,
    );
    edited.edited_timestamp = Some("2026-08-14T12:00:00Z".into());
    general.push(edited);

    message_cache.set_fixture_messages(channel_id(111), general);

    message_cache.set_fixture_messages(
        channel_id(112),
        vec![
            message(
                20,
                112,
                Some(10),
                1001,
                "blu",
                "pushed the projection layer",
                1800,
            ),
            message(21, 112, Some(10), 1002, "ferris", "reviewing now", 900),
        ],
    );

    message_cache.set_fixture_messages(
        channel_id(300),
        vec![
            message(30, 300, None, 2001, "ferris", "hey, got a minute?", 4000),
            message(31, 300, None, 1001, "blu", "sure, what's up", 3900),
        ],
    );

    // ---- unread / mentions -------------------------------------------------
    //
    // `channel_unread` short-circuits to Seen unless the channel has a
    // `last_message_id`, so every channel carrying unread state needs one.
    {
        let navigation = Arc::make_mut(&mut state.navigation);
        for (channel, latest) in [(112u64, 21u64), (113, 40), (300, 31), (111, 8)] {
            if let Some(channel) = navigation.channels.get_mut(&channel_id(channel)) {
                channel.last_message_id = Some(message_id(snowflake_at(60).max(latest)));
            }
        }
    }

    let notifications = Arc::make_mut(&mut state.notifications);
    // Three distinct unread states, so a front-end can verify it renders each
    // differently:
    //   #gui-rewrite - mentions  -> numeric badge
    //   #ci-logs     - plain unread (no counts) -> bold name, no badge
    //   DM 300       - notify-level unread -> numeric badge
    notifications.set_fixture_unread(channel_id(112), 2, 2);
    notifications.set_fixture_unread(channel_id(113), 0, 0);
    notifications.set_fixture_unread(channel_id(300), 0, 1);

    state
}

/// Whether a token selects fixture mode.
pub fn is_fixture_token(token: &str) -> bool {
    token.trim().eq_ignore_ascii_case(FIXTURE_TOKEN)
}

/// A VecDeque helper used by the cache setters.
pub(in crate::discord) fn into_deque<T>(items: Vec<T>) -> VecDeque<T> {
    items.into()
}
