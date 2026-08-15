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

use std::collections::BTreeMap;
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
    MessageState {
        id: message_id(snowflake_at(age).max(id)),
        channel_id: channel_id(channel),
        guild_id: guild.map(guild_id),
        author_id: user_id(author),
        author: name.to_string(),
        content: Some(body.to_string()),
        ..MessageState::default()
    }
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
        // A forum, so the post-list view has something to render offline.
        channel(114, Some(10), Some(110), "help-forum", "forum", 7),
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

    let roles = [
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

    let members = [
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
        crate::discord::VoiceScope::Guild(guild_id(10)),
        channel_id(121),
        &[
            (user_id(1001), "blu", true, false, false),
            (user_id(1002), "ferris", false, true, false),
            (user_id(1005), "ci-bot", false, false, true),
        ],
        false,
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
    // Discord sends both the preview and a reference carrying the target id;
    // the preview alone cannot be jumped to.
    reply.reference = Some(crate::discord::MessageReferenceInfo {
        guild_id: Some(guild_id(10)),
        channel_id: Some(channel_id(111)),
        message_id: Some(message_id(snowflake_at(6000).max(5))),
    });
    reply.reply = Some(ReplyInfo {
        author_id: Some(user_id(1003)),
        author: "turing".into(),
        content: Some("what's the plan for today?".into()),
        sticker_names: Vec::new(),
        stickers: Vec::new(),
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

    // A poll, so the vote bars and the reveal-after-voting rule are visible
    // offline.
    let mut polled = message(10, 111, Some(10), 1001, "blu", "", 2400);
    polled.poll = Some(crate::discord::PollInfo {
        question: "Which toolkit for the rewrite?".to_string(),
        answers: vec![
            crate::discord::PollAnswerInfo {
                answer_id: 1,
                text: "GPUI".to_string(),
                vote_count: Some(7),
                me_voted: false,
            },
            crate::discord::PollAnswerInfo {
                answer_id: 2,
                text: "Iced".to_string(),
                vote_count: Some(3),
                me_voted: false,
            },
            crate::discord::PollAnswerInfo {
                answer_id: 3,
                text: "Stay in the terminal".to_string(),
                vote_count: Some(5),
                me_voted: false,
            },
        ],
        allow_multiselect: false,
        results_finalized: Some(false),
        total_votes: Some(15),
    });
    general.push(polled);

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
            message(
                23,
                112,
                Some(10),
                1003,
                "turing",
                "<@1001> can you take a look at this one?",
                450,
            ),
            message(
                22,
                112,
                Some(10),
                1003,
                "turing",
                "the fix was ||a missing guild_id|| all along",
                300,
            ),
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
    //
    // It is taken from the newest cached message rather than invented: an
    // id past the end of the timeline can never be acked, so the badge would
    // be permanently stuck unread with no way for a client to clear it.
    {
        let newest: Vec<_> = [(112u64, 21u64), (113, 40), (300, 31), (111, 8)]
            .into_iter()
            .map(|(channel, fallback)| {
                let id = channel_id(channel);
                // A channel with no cached timeline keeps a synthetic id: it
                // still reads as unread, which is what an unvisited channel
                // looks like before its history is fetched.
                let latest = state
                    .messages_for_channel(id)
                    .last()
                    .map(|message| message.id)
                    .unwrap_or_else(|| message_id(snowflake_at(60).max(fallback)));
                (id, latest)
            })
            .collect();

        let navigation = Arc::make_mut(&mut state.navigation);
        for (channel, latest) in newest {
            if let Some(channel) = navigation.channels.get_mut(&channel) {
                channel.last_message_id = Some(latest);
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

// ---------------------------------------------------------------------------
// Demo-mode mutation
//
// A front-end running offline has no server to answer its commands, so it
// answers them itself. These helpers let it mutate the synthetic state, which
// the caches' visibility otherwise keeps inside this module.
// ---------------------------------------------------------------------------

/// Append a message to a channel, as though it had just arrived.
///
/// Returns the id it was given, so a caller can reference it afterwards.
pub fn append_message(
    state: &mut DiscordState,
    channel_id: Id<marker::ChannelMarker>,
    guild_id: Option<Id<marker::GuildMarker>>,
    author_id: Id<marker::UserMarker>,
    author: &str,
    content: &str,
) -> Id<marker::MessageMarker> {
    let message = MessageState {
        id: message_id(snowflake_at(0)),
        channel_id,
        guild_id,
        author_id,
        author: author.to_string(),
        content: Some(content.to_string()),
        ..MessageState::default()
    };

    let id = message.id;

    let cache = Arc::make_mut(&mut state.message_cache);
    let timeline = cache.timelines.entry(channel_id).or_default();
    timeline.messages.push_back(message);

    // Keep last_message_id consistent, or the channel reads as having no
    // messages and its unread state collapses to Seen.
    let navigation = Arc::make_mut(&mut state.navigation);
    if let Some(channel) = navigation.channels.get_mut(&channel_id) {
        channel.last_message_id = Some(id);
    }

    id
}

/// The authenticated user in demo mode.
pub fn demo_user_id() -> Id<marker::UserMarker> {
    user_id(1001)
}

/// Add a reaction to a message, or remove it if the user already reacted.
pub fn toggle_reaction(
    state: &mut DiscordState,
    channel_id: Id<marker::ChannelMarker>,
    target: Id<marker::MessageMarker>,
    emoji: &str,
) {
    let cache = Arc::make_mut(&mut state.message_cache);
    let Some(timeline) = cache.timelines.get_mut(&channel_id) else {
        return;
    };
    let Some(message) = timeline.messages.iter_mut().find(|m| m.id == target) else {
        return;
    };

    let existing = message.reactions.iter().position(
        |reaction| matches!(&reaction.emoji, ReactionEmoji::Unicode(text) if text == emoji),
    );

    match existing {
        Some(index) if message.reactions[index].me => {
            if message.reactions[index].count <= 1 {
                message.reactions.remove(index);
            } else {
                message.reactions[index].count -= 1;
                message.reactions[index].me = false;
            }
        }
        Some(index) => {
            message.reactions[index].count += 1;
            message.reactions[index].me = true;
        }
        None => message.reactions.push(ReactionInfo {
            emoji: ReactionEmoji::Unicode(emoji.to_string()),
            count: 1,
            me: true,
        }),
    }
}

/// Edit a message's body in place.
pub fn edit_message(
    state: &mut DiscordState,
    channel_id: Id<marker::ChannelMarker>,
    target: Id<marker::MessageMarker>,
    content: &str,
) {
    let cache = Arc::make_mut(&mut state.message_cache);
    if let Some(timeline) = cache.timelines.get_mut(&channel_id)
        && let Some(message) = timeline.messages.iter_mut().find(|m| m.id == target)
    {
        message.content = Some(content.to_string());
        message.edited_timestamp = Some("now".to_string());
    }
}

/// Delete a message.
pub fn delete_message(
    state: &mut DiscordState,
    channel_id: Id<marker::ChannelMarker>,
    target: Id<marker::MessageMarker>,
) {
    let cache = Arc::make_mut(&mut state.message_cache);
    if let Some(timeline) = cache.timelines.get_mut(&channel_id) {
        timeline.messages.retain(|message| message.id != target);
    }
}

/// Populate a user's profile, so the profile panel resolves offline.
pub fn add_profile(
    state: &mut DiscordState,
    user: Id<marker::UserMarker>,
    guild: Option<Id<marker::GuildMarker>>,
) {
    let profiles = Arc::make_mut(&mut state.profiles);

    let (username, bio, pronouns) = match user.get() {
        1001 => ("blu", "Maintaining this client.", Some("they/them")),
        1002 => ("ferris", "Rust mascot. Mostly here for the crabs.", None),
        1003 => ("turing", "Thinking about machines.", None),
        1005 => ("ci-bot", "Automated build reporter.", None),
        _ => ("unknown", "", None),
    };

    profiles.user_profiles.insert(
        super::profile::state::UserProfileCacheKey::new(user, guild),
        crate::discord::UserProfileInfo {
            user_id: user,
            username: username.to_string(),
            global_name: Some(username.to_string()),
            guild_nick: None,
            role_ids: guild.map(|_| vec![role_id(2)]).unwrap_or_default(),
            role_ids_present: guild.is_some(),
            avatar_url: None,
            bio: Some(bio.to_string()),
            // A distinct per-guild bio, so demo mode shows that the two are
            // separate rather than the same text twice.
            guild_bio: guild.map(|_| format!("{bio} (in this server)")),
            pronouns: pronouns.map(str::to_string),
            guild_pronouns: None,
            mutual_guilds: vec![crate::discord::MutualGuildInfo {
                guild_id: guild_id(10),
                nick: None,
            }],
            mutual_friends_count: 3,
            friend_status: crate::discord::FriendStatus::Friend,
            note: None,
        },
    );
}

/// Synthetic capture sources for the screenshare picker.
pub fn capture_targets() -> Vec<crate::discord::StreamCaptureTarget> {
    vec![
        crate::discord::StreamCaptureTarget {
            kind: crate::discord::StreamCaptureTargetKind::Display,
            id: 1,
            title: "Screen 1 (2560x1440)".to_string(),
        },
        crate::discord::StreamCaptureTarget {
            kind: crate::discord::StreamCaptureTargetKind::Window,
            id: 2,
            title: "concord-gui".to_string(),
        },
        crate::discord::StreamCaptureTarget {
            kind: crate::discord::StreamCaptureTargetKind::Window,
            id: 3,
            title: "Terminal".to_string(),
        },
    ]
}

/// Channels the demo search should look through.
pub fn demo_channel_ids() -> Vec<Id<marker::ChannelMarker>> {
    [111u64, 112, 113, 300, 301, 302]
        .into_iter()
        .map(channel_id)
        .collect()
}

/// Convert a stored message into the wire-shaped `MessageInfo` that search
/// results and forum pages carry.
pub fn message_info(message: &MessageState) -> crate::discord::MessageInfo {
    crate::discord::MessageInfo {
        guild_id: message.guild_id,
        channel_id: message.channel_id,
        message_id: message.id,
        author_id: message.author_id,
        author: message.author.clone(),
        author_is_bot: message.author_is_bot,
        content: message.content.clone(),
        pinned: message.pinned,
        ..crate::discord::MessageInfo::default()
    }
}

/// Synthetic forum posts for the demo forum channel.
///
/// Active and archived are distinct sets, matching Discord: archived posts are
/// not a filtered view of the active page.
pub fn forum_posts(
    forum: Id<marker::ChannelMarker>,
    archive_state: crate::discord::ForumPostArchiveState,
) -> (
    Vec<crate::discord::ChannelInfo>,
    Vec<crate::discord::MessageInfo>,
) {
    let archived = matches!(
        archive_state,
        crate::discord::ForumPostArchiveState::Archived
    );

    let posts: &[(u64, &str, &str, &str, u64)] = if archived {
        &[(
            940,
            "How do I build on musl?",
            "ferris",
            "Resolved: the DMA-BUF ioctl needed a cfg guard.",
            12,
        )]
    } else {
        &[
            (
                900,
                "Voice keeps dropping on reconnect",
                "turing",
                "Happens after a suspend/resume cycle. Logs attached.",
                8,
            ),
            (
                901,
                "Feature request: message pinning UI",
                "lovelace",
                "The command exists in the core but there is no control for it.",
                3,
            ),
            (
                902,
                "Wayland fractional scaling looks blurry",
                "ferris",
                "Only on 125%. 150% and 200% are fine.",
                21,
            ),
        ]
    };

    let mut threads = Vec::new();
    let mut first_messages = Vec::new();

    for (id, title, author, body, replies) in posts {
        let thread_id = channel_id(*id);

        let mut thread = blank_channel(*id, "thread", title);
        thread.guild_id = Some(guild_id(10));
        thread.parent_id = Some(forum);
        thread.message_count = Some(*replies);
        threads.push(crate::discord::ChannelInfo {
            guild_id: thread.guild_id,
            channel_id: thread_id,
            parent_id: thread.parent_id,
            owner_id: None,
            position: None,
            last_message_id: None,
            name: title.to_string(),
            kind: "thread".to_string(),
            message_count: Some(*replies),
            member_count: None,
            total_message_sent: None,
            thread_metadata: Some(crate::discord::ThreadMetadataInfo {
                archived,
                auto_archive_duration: Some(1440),
                archive_timestamp: None,
                locked: false,
                invitable: None,
                create_timestamp: None,
            }),
            flags: None,
            rate_limit_per_user: None,
            available_tags: Vec::new(),
            applied_tags: Vec::new(),
            current_user_joined_thread: Some(false),
            current_user_thread_notification_flags: None,
            current_user_thread_muted: Some(false),
            current_user_thread_mute_end_time: None,
            recipients: None,
            permission_overwrites: Vec::new(),
            is_message_request: None,
            is_spam: None,
        });

        first_messages.push(crate::discord::MessageInfo {
            guild_id: Some(guild_id(10)),
            channel_id: thread_id,
            message_id: message_id(snowflake_at(3600)),
            author: author.to_string(),
            content: Some(body.to_string()),
            ..crate::discord::MessageInfo::default()
        });
    }

    (threads, first_messages)
}

/// Seat the demo user in a voice channel, or move them if already seated.
pub fn join_voice(
    state: &mut DiscordState,
    scope: crate::discord::VoiceScope,
    channel: Id<marker::ChannelMarker>,
    muted: bool,
    deafened: bool,
) {
    let voice = Arc::make_mut(&mut state.voice);
    voice.set_fixture_participants(
        scope,
        channel,
        &[(demo_user_id(), "blu", false, muted, false)],
        deafened,
    );
}

/// Remove the demo user from voice.
pub fn leave_voice(state: &mut DiscordState, scope: crate::discord::VoiceScope) {
    let voice = Arc::make_mut(&mut state.voice);
    voice.remove_fixture_participant(scope, demo_user_id());
}

/// Attach files to the most recent message in a channel.
///
/// Demo attachments carry no URL: nothing is uploaded, so pointing at a CDN
/// path that does not exist would produce broken previews.
pub fn attach_to_last_message(
    state: &mut DiscordState,
    channel_id: Id<marker::ChannelMarker>,
    files: &[(String, u64)],
) {
    let cache = Arc::make_mut(&mut state.message_cache);
    let Some(timeline) = cache.timelines.get_mut(&channel_id) else {
        return;
    };
    let Some(message) = timeline.messages.back_mut() else {
        return;
    };

    for (index, (filename, size)) in files.iter().enumerate() {
        // Image attachments get a URL, because a preview is requested by URL
        // and an empty one is skipped. It is not reachable over the network -
        // demo mode answers the request itself.
        let url = if guess_content_type(filename)
            .as_deref()
            .is_some_and(|kind| kind.starts_with("image/"))
        {
            format!("concord-demo://attachment/{filename}")
        } else {
            String::new()
        };

        message.attachments.push(crate::discord::AttachmentInfo {
            id: Id::new(9_000 + index as u64 + 1),
            filename: filename.clone(),
            url,
            proxy_url: String::new(),
            content_type: guess_content_type(filename),
            size: *size,
            width: None,
            height: None,
            description: None,
            flags: 0,
        });
    }
}

fn guess_content_type(filename: &str) -> Option<String> {
    let extension = filename.rsplit('.').next()?.to_lowercase();
    let kind = match extension.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "txt" | "log" => "text/plain",
        _ => return None,
    };
    Some(kind.to_string())
}

/// Prepend a page of older messages, as history paging would.
///
/// Returns false once the synthetic backlog is exhausted, so the caller can
/// report that there is nothing further rather than paging forever.
pub fn prepend_history(
    state: &mut DiscordState,
    channel_id: Id<marker::ChannelMarker>,
    page: usize,
) -> bool {
    // Three pages of backlog is enough to exercise scrolling without
    // pretending the fixture is bottomless.
    if page >= 3 {
        return false;
    }

    let authors = [(1002u64, "ferris"), (1003, "turing"), (1004, "lovelace")];
    let guild = state.channel(channel_id).and_then(|c| c.guild_id);

    let mut older = Vec::new();
    for index in 0..10 {
        let (author, name) = authors[(page * 10 + index) % authors.len()];
        // Ages increase with the page so ordering stays consistent.
        let age = 7200 + (page as u64 * 10 + index as u64) * 300;

        older.push(MessageState {
            id: message_id(snowflake_at(age)),
            channel_id,
            guild_id: guild,
            author_id: user_id(author),
            author: name.to_string(),
            content: Some(format!(
                "Earlier message {} from the backlog",
                page * 10 + index
            )),
            ..MessageState::default()
        });
    }

    // Oldest first, inserted ahead of what is already loaded.
    older.sort_by_key(|message| message.id);

    let cache = Arc::make_mut(&mut state.message_cache);
    let timeline = cache.timelines.entry(channel_id).or_default();
    for message in older.into_iter().rev() {
        timeline.messages.push_front(message);
    }

    true
}

/// Show a fixture user as typing in a channel.
pub fn set_typing(
    state: &mut DiscordState,
    channel_id: Id<marker::ChannelMarker>,
    user: Id<marker::UserMarker>,
) {
    let presence = Arc::make_mut(&mut state.presence);
    presence.set_fixture_typing(channel_id, &[user]);
}

/// Stop showing a user as typing.
pub fn clear_typing(
    state: &mut DiscordState,
    channel_id: Id<marker::ChannelMarker>,
    user: Id<marker::UserMarker>,
) {
    let presence = Arc::make_mut(&mut state.presence);
    presence.clear_fixture_typing(channel_id, user);
}

/// Who replies to the demo user, and what they say.
///
/// Canned rather than generated: a demo should be predictable enough to
/// screenshot, and inventing text risks it reading as real conversation.
pub fn demo_responder(
    channel_id: Id<marker::ChannelMarker>,
) -> (Id<marker::UserMarker>, &'static str, &'static str) {
    if channel_id == channel_id_of(300) {
        (user_id(2001), "ferris", "got it, thanks")
    } else {
        (user_id(1002), "ferris", "sounds good to me")
    }
}

fn channel_id_of(raw: u64) -> Id<marker::ChannelMarker> {
    channel_id(raw)
}

/// Pin or unpin a message.
pub fn set_pinned(
    state: &mut DiscordState,
    channel_id: Id<marker::ChannelMarker>,
    target: Id<marker::MessageMarker>,
    pinned: bool,
) {
    let cache = Arc::make_mut(&mut state.message_cache);
    if let Some(timeline) = cache.timelines.get_mut(&channel_id)
        && let Some(message) = timeline.messages.iter_mut().find(|m| m.id == target)
    {
        message.pinned = pinned;
    }
}

/// Record a vote on a poll.
///
/// `answer_ids` is the user's full selection, not a delta, so previous votes
/// are withdrawn by their absence - the same shape the API uses.
pub fn vote_poll(
    state: &mut DiscordState,
    channel_id: Id<marker::ChannelMarker>,
    target: Id<marker::MessageMarker>,
    answer_ids: &[u8],
) {
    let cache = Arc::make_mut(&mut state.message_cache);
    let Some(timeline) = cache.timelines.get_mut(&channel_id) else {
        return;
    };
    let Some(message) = timeline.messages.iter_mut().find(|m| m.id == target) else {
        return;
    };
    let Some(poll) = &mut message.poll else {
        return;
    };

    for answer in &mut poll.answers {
        let now_voted = answer_ids.contains(&answer.answer_id);
        let count = answer.vote_count.unwrap_or(0);

        if now_voted && !answer.me_voted {
            answer.vote_count = Some(count + 1);
        } else if !now_voted && answer.me_voted {
            answer.vote_count = Some(count.saturating_sub(1));
        }
        answer.me_voted = now_voted;
    }

    poll.total_votes = Some(
        poll.answers
            .iter()
            .map(|answer| answer.vote_count.unwrap_or(0))
            .sum(),
    );
}

/// Attach a poll to a message.
pub fn set_poll(
    state: &mut DiscordState,
    channel_id: Id<marker::ChannelMarker>,
    target: Id<marker::MessageMarker>,
    poll: crate::discord::PollInfo,
) {
    let cache = Arc::make_mut(&mut state.message_cache);
    if let Some(timeline) = cache.timelines.get_mut(&channel_id)
        && let Some(message) = timeline.messages.iter_mut().find(|m| m.id == target)
    {
        message.poll = Some(poll);
    }
}

/// Rename a thread.
pub fn rename_thread(state: &mut DiscordState, channel_id: Id<marker::ChannelMarker>, name: &str) {
    let navigation = Arc::make_mut(&mut state.navigation);
    if let Some(channel) = navigation.channels.get_mut(&channel_id) {
        channel.name = name.to_string();
    }
}

/// Remove a thread, and its messages with it.
pub fn delete_thread(state: &mut DiscordState, channel_id: Id<marker::ChannelMarker>) {
    let navigation = Arc::make_mut(&mut state.navigation);
    navigation.channels.remove(&channel_id);

    // The timeline goes too, or a later reopen of a recycled id would show
    // messages belonging to a thread that no longer exists.
    let cache = Arc::make_mut(&mut state.message_cache);
    cache.timelines.remove(&channel_id);
}

/// Lock or unlock a thread.
pub fn set_thread_locked(
    state: &mut DiscordState,
    channel_id: Id<marker::ChannelMarker>,
    locked: bool,
) {
    let navigation = Arc::make_mut(&mut state.navigation);
    if let Some(channel) = navigation.channels.get_mut(&channel_id)
        && let Some(metadata) = channel.thread_metadata.as_mut()
    {
        metadata.locked = locked;
    }
}

/// Mute or unmute a thread for the demo user.
pub fn set_thread_muted(
    state: &mut DiscordState,
    channel_id: Id<marker::ChannelMarker>,
    muted: bool,
) {
    let navigation = Arc::make_mut(&mut state.navigation);
    if let Some(channel) = navigation.channels.get_mut(&channel_id) {
        channel.current_user_thread_muted = muted;
    }
}

/// Pin or unpin a thread in its forum parent.
///
/// Only the `PINNED` bit is touched; the rest of the bitfield carries meaning
/// Discord set and this client does not interpret.
pub fn set_thread_pinned(
    state: &mut DiscordState,
    channel_id: Id<marker::ChannelMarker>,
    pinned: bool,
) {
    const PINNED: u64 = 1 << 1;

    let navigation = Arc::make_mut(&mut state.navigation);
    if let Some(channel) = navigation.channels.get_mut(&channel_id) {
        let flags = channel.flags.unwrap_or(0);
        channel.flags = Some(if pinned {
            flags | PINNED
        } else {
            flags & !PINNED
        });
    }
}

/// Create a forum post: a thread under the forum, plus its opening message.
pub fn create_forum_post(
    state: &mut DiscordState,
    parent: Id<marker::ChannelMarker>,
    title: &str,
    content: &str,
) -> Id<marker::ChannelMarker> {
    let guild_id = {
        let navigation = &state.navigation;
        navigation
            .channels
            .get(&parent)
            .and_then(|channel| channel.guild_id)
    };

    // The post's channel id is a fresh snowflake so it cannot collide with a
    // fixture channel and inherit its timeline.
    let id = channel_id(snowflake_at(0));

    let navigation = Arc::make_mut(&mut state.navigation);
    // The kind is the core's own string name, not a numeric wire value.
    let mut post = channel(
        id.get(),
        guild_id.map(|guild| guild.get()),
        Some(parent.get()),
        title,
        "thread",
        0,
    );
    post.thread_metadata = Some(crate::discord::ThreadMetadataInfo {
        archived: false,
        auto_archive_duration: Some(1440),
        archive_timestamp: None,
        locked: false,
        invitable: None,
        create_timestamp: None,
    });
    navigation.channels.insert(id, post);

    append_message(state, id, guild_id, demo_user_id(), "blu", content);

    id
}

/// Mark a channel read up to a message.
pub fn mark_read(
    state: &mut DiscordState,
    channel_id: Id<marker::ChannelMarker>,
    message_id: Id<marker::MessageMarker>,
) {
    let notifications = Arc::make_mut(&mut state.notifications);
    notifications.set_fixture_acked(channel_id, message_id);
}

/// Answer a member search.
///
/// The fixture's members are all already present, so this exists to make the
/// command observable rather than to add anyone: a search that silently did
/// nothing would look identical to one that was dropped.
pub fn search_members(
    state: &mut DiscordState,
    guild_id: Id<marker::GuildMarker>,
    query: &str,
) -> Vec<Id<marker::UserMarker>> {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return Vec::new();
    }

    state
        .members_for_guild(guild_id)
        .into_iter()
        .filter(|member| {
            member.display_name.to_lowercase().contains(&needle)
                || member
                    .username
                    .as_deref()
                    .is_some_and(|name| name.to_lowercase().contains(&needle))
        })
        .map(|member| member.user_id)
        .collect()
}

/// A small generated image, used to answer preview requests in demo mode.
///
/// Generated rather than embedded so the fixture carries no binary blob: a
/// gradient is enough to show that decoding, sizing and layout all work.
pub fn demo_preview_png(seed: u64) -> Vec<u8> {
    use image::{ImageEncoder, codecs::png::PngEncoder};

    const W: u32 = 320;
    const H: u32 = 180;

    let mut pixels = Vec::with_capacity((W * H * 3) as usize);
    for y in 0..H {
        for x in 0..W {
            // The seed shifts the hue so two attachments are distinguishable.
            pixels.push((x * 255 / W) as u8);
            pixels.push((y * 255 / H) as u8);
            pixels.push((seed % 256) as u8);
        }
    }

    let mut out = Vec::new();
    let encoded =
        PngEncoder::new(&mut out).write_image(&pixels, W, H, image::ExtendedColorType::Rgb8);

    // An encoder failure would mean a bug here, not bad input; an empty
    // result is preferable to a panic in a demo path.
    if encoded.is_err() { Vec::new() } else { out }
}
