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
use std::sync::OnceLock;
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
/// The instant every fixture timestamp is measured back from.
///
/// Pinned once rather than read per message. Reading the clock per call made
/// "seconds ago" mean something slightly different each time: a backlog page
/// built a few milliseconds after the message it is supposed to precede came
/// out fractionally newer, and the ordering the whole scrollback depends on
/// flipped at that boundary. It surfaced as a test that failed once in a full
/// run and passed every time it was run alone.
fn fixture_now_ms() -> u64 {
    static NOW_MS: OnceLock<u64> = OnceLock::new();
    *NOW_MS.get_or_init(|| {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_millis() as u64)
            .unwrap_or(DISCORD_EPOCH_MS)
    })
}

fn snowflake_at(seconds_ago: u64) -> u64 {
    let target = fixture_now_ms().saturating_sub(seconds_ago * 1000);
    (target.saturating_sub(DISCORD_EPOCH_MS)) << 22
}

pub mod account;
pub mod guild;
pub mod people;
pub mod server;
pub mod voice;

pub use people::{
    autocomplete_choices, member_name, relationship, set_member_timeout,
    set_thread_notification_level, update_self_profile, user_by_name,
};

/// Turn a small fixture number into an id that looks like Discord issued it.
///
/// The fixture is written in terms of readable numbers - guild 10, user 1001 -
/// because a file full of nineteen-digit snowflakes is unreadable. This is the
/// one place they become real ids, so every cross-reference in the fixture
/// keeps pointing at the same thing.
///
/// A number like 1001 is not a snowflake: decoded, it claims to have been
/// created in the first millisecond of Discord's existence, in the same
/// millisecond as every other small number. Anything reading a creation date
/// out of an id - account age, "created on", sorting by id - was getting that
/// answer for the whole fixture, and getting it silently.
///
/// `kind` separates the spaces, so a guild and a user built from the same
/// number are still different ids, as they would be on Discord.
fn fixture_snowflake(kind: u64, raw: u64) -> u64 {
    // Spread across the year before the fixture's "now", so ids sort into a
    // sensible creation order and none of them is in the future.
    let year_ms = 365 * 24 * 60 * 60 * 1000_u64;
    let created = fixture_now_ms()
        .saturating_sub(year_ms)
        .saturating_add(raw.wrapping_mul(97) % year_ms);

    // Worker and process from the kind, increment from the number: two ids
    // built in the same millisecond still differ, which is what stops the
    // fixture handing out the same id twice by accident.
    Id::<()>::from_parts(created, kind, kind >> 5, raw).get()
}

fn guild_id(raw: u64) -> Id<marker::GuildMarker> {
    Id::new(fixture_snowflake(1, raw))
}
fn channel_id(raw: u64) -> Id<marker::ChannelMarker> {
    Id::new(fixture_snowflake(2, raw))
}
fn user_id(raw: u64) -> Id<marker::UserMarker> {
    Id::new(fixture_snowflake(3, raw))
}
fn role_id(raw: u64) -> Id<marker::RoleMarker> {
    Id::new(fixture_snowflake(4, raw))
}
fn emoji_id(raw: u64) -> Id<marker::EmojiMarker> {
    Id::new(fixture_snowflake(5, raw))
}
fn attachment_id(raw: u64) -> Id<marker::AttachmentMarker> {
    Id::new(fixture_snowflake(6, raw))
}
fn message_id(raw: u64) -> Id<marker::MessageMarker> {
    // Already a snowflake: the caller derives it from the message's age, so
    // the timeline sorts by id the way Discord's does.
    Id::new(raw)
}

fn guild(id: u64, name: &str, members: u64, online: u32) -> GuildState {
    GuildState {
        id: guild_id(id),
        name: name.to_string(),
        icon: None,
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
fn blank_channel_with_id(id: Id<marker::ChannelMarker>, kind: &str, name: &str) -> ChannelState {
    let mut channel = blank_channel(1, kind, name);
    channel.id = id;
    channel
}

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
        topic: None,
        nsfw: None,
        user_limit: None,
        available_tags: Vec::new(),
        applied_tags: Vec::new(),
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

/// One attachment, with the fields the message list actually reads.
fn attachment(
    id: u64,
    filename: &str,
    content_type: &str,
    size: u64,
    dimensions: Option<(u64, u64)>,
) -> crate::discord::AttachmentInfo {
    crate::discord::AttachmentInfo {
        id: attachment_id(id),
        filename: filename.to_string(),
        // Images are previewed by URL, and an empty one is skipped; the
        // scheme is ours because nothing here is really hosted anywhere.
        url: format!("concord-demo://attachment/{filename}"),
        proxy_url: String::new(),
        content_type: Some(content_type.to_string()),
        size,
        width: dimensions.map(|(w, _)| w),
        height: dimensions.map(|(_, h)| h),
        description: None,
        flags: 0,
    }
}

/// One message containing everything Discord's own client will render.
///
/// Deliberately wider than what this client parses: headings, lists, masked
/// links and subtext are all Discord syntax that [`crate::discord`]'s
/// markdown does not handle yet. They are here so the gap is visible when
/// this message is put beside the same text in the official client, rather
/// than being something nobody notices until a user asks about it.
///
/// Here as a constant so its length can be asserted: it has to stay under the
/// limit, or it is not a message anyone could send and stops being a fair
/// test of the thing it is testing.
fn kitchen_sink() -> String {
    // Built rather than written out: mentions carry their ids in the text, so
    // hard-coded ones stopped resolving the moment the fixture started
    // issuing real snowflakes - and a mention that resolves to nobody renders
    // as a raw `<@1001>`, which is the exact thing this message exists to
    // check. The braces the message itself contains are doubled for `format!`.
    format!(
        "# Heading 1\n## Heading 2\n### Heading 3\n-# Subtext under a heading\n**bold** *italic* _italic_ __underline__ ~~strike~~ ***bold italic*** __**underline bold**__ __*underline italic*__\n`inline code`, ||a spoiler||, and \\*escaped\\* \\_markers\\_\n> a single-line quote\n> a second quote line\n- bullet one\n- bullet two\n  - nested bullet\n    - twice nested\n1. ordered one\n2. ordered two\n   1. nested ordered\n```rust\nfn main() {{\n    println!(\"fenced, with a language\");\n}}\n```\n```\nfenced, no language\n```\n[masked link](https://github.com/bluscream/concord), bare https://github.com/bluscream/concord, and suppressed <https://example.invalid/no-embed>\nMentions: <@{user}> a user, <@&{role}> a role, <#{channel}> a channel, </settings:1> a command, @everyone, @here\nEmoji: ❤ 🦀 <:ferris:{emoji}> <a:crab_party:{animated}>\nNav: <id:customize> <id:guide>, a sound <sound:{sound}:{gid}>, a number <tel:+15551234567>\nRefs: https://discord.com/channels/{gid}/{channel}, https://discord.com/channels/{gid}/{channel}/{msg}, https://discord.com/channels/{gid}, a post https://discord.com/channels/{gid}/{channel}/threads/{thread}/{msg}, a game <@$1234567890>\nA file: https://cdn.discordapp.com/attachments/{channel}/{msg}/report.pdf\n¯\\_(ツ)_/¯ then * not italic * beside *italic *\nTimes: <t:1756000000:t> <t:1756000000:T> <t:1756000000:d> <t:1756000000:D> <t:1756000000:f> <t:1756000000:F> <t:1756000000:R>\nA long unbroken token to test wrapping: https://example.invalid/a/very/long/path/that/keeps/going/and/going/until/it/has/to/wrap/somewhere\n-# and a closing subtext line\n>>> a block quote that swallows\neverything after it on its own lines",
        user = user_id(1002),
        role = role_id(2),
        channel = channel_id(112),
        emoji = emoji_id(4001),
        animated = emoji_id(4002),
        sound = fixture_snowflake(7, 5001),
        gid = guild_id(10),
        thread = channel_id(130),
        msg = message_id(fixture_now_ms() - 90_000),
    )
}

/// Servers that exist only to make the rail longer than the window.
///
/// Named rather than numbered: a rail reading "Server 7, Server 8" tells you
/// nothing about whether the names are being drawn correctly, and one of
/// these is deliberately long enough to test that they are truncated.
/// Member and online counts for [`FILLER_GUILDS`], in the same order.
///
/// Drawn to match the shape a real account shows: a long tail of small
/// servers, a couple in the tens of thousands, one very large. Online counts
/// run a few percent of members, which is what an idle server looks like.
const FILLER_MEMBERS: [(u64, u32); 14] = [
    (312, 24),
    (1_204, 88),
    (86, 9),
    (447, 31),
    (2_918, 173),
    (58_402, 3_104),
    (742, 51),
    (139, 12),
    (5_610, 288),
    (61, 4),
    (18_337, 951),
    (203, 17),
    (414_308, 22_770),
    (1_066, 74),
];

const FILLER_GUILDS: [&str; 14] = [
    "Compiler Explorer",
    "Embedded Rust",
    "Game Dev",
    "GPUI Builders",
    "Home Lab",
    "Linux Gaming",
    "Mechanical Keyboards",
    "Nix Users",
    "Open Source Fridays",
    "Rust Gamedev Working Group With A Very Long Name",
    "Self Hosted",
    "Type Theory",
    "Wayland",
    "Zig Learners",
];

/// A fully-populated state for offline UI work.
pub fn demo_state() -> DiscordState {
    let mut state = DiscordState::default();

    let navigation = Arc::make_mut(&mut state.navigation);

    // ---- guilds ------------------------------------------------------------
    let mut rostfaden = guild(10, "RostFaden", 953, 61);
    rostfaden.icon = Some("https://cdn.discordapp.com/icons/747967102895390741/4ed9b9516ae3bb878c8e15f4ca089141.webp?size=1024".to_string());
    navigation.guilds.insert(guild_id(10), rostfaden);
    navigation
        .guilds
        .insert(guild_id(20), guild(20, "Rust Community", 4210, 812));

    // Enough servers that the rail runs past the bottom of the window and
    // has to scroll. Two named servers exercised the list; they never
    // exercised what happens when it does not fit, which is where the last
    // few become unreachable.
    for (offset, name) in FILLER_GUILDS.iter().enumerate() {
        let id = 40 + offset as u64;
        navigation.guilds.insert(
            guild_id(id),
            // Sizes spread the way real ones do rather than climbing evenly:
            // a surveyed account's guilds ran median 953, p90 55k, max 414k,
            // so most are small, a few are enormous, and none of them sit on
            // a neat arithmetic progression.
            guild(id, name, FILLER_MEMBERS[offset].0, FILLER_MEMBERS[offset].1),
        );
    }

    // ---- guild 10 channels -------------------------------------------------
    //
    // The names deliberately do not all follow one convention. A survey of a
    // real account's 7020 channels put pure lowercase-kebab at 29% - the
    // fixture used to be at 100%, which is the sort of tidiness no actual
    // server has and the first thing that makes a demo look staged.
    //
    // The mix here follows the survey: underscores are commoner than hyphens
    // (44% against 32%), about one in seven carries an emoji, a few use
    // separators, and categories are a different convention again - 59% have
    // an uppercase letter and 44% a space, while barely 2% are kebab.
    let channels = [
        channel(100, Some(10), None, "Information", "category", 0),
        channel(101, Some(10), Some(100), "announcements", "text", 1),
        channel(102, Some(10), Some(100), "rules", "text", 2),
        channel(110, Some(10), None, "\u{1f4bb} DEVELOPMENT", "category", 3),
        channel(111, Some(10), Some(110), "general", "text", 4),
        channel(112, Some(10), Some(110), "gui-rewrite", "text", 5),
        channel(113, Some(10), Some(110), "ci_logs", "text", 6),
        channel(
            115,
            Some(10),
            Some(110),
            "\u{1f41b}\u{fe0f}bug-reports",
            "text",
            7,
        ),
        // A forum, so the post-list view has something to render offline.
        channel(114, Some(10), Some(110), "help-forum", "forum", 8),
        channel(
            120,
            Some(10),
            None,
            "Voice \u{2502} Hangouts",
            "category",
            9,
        ),
        channel(121, Some(10), Some(120), "Standup", "voice", 10),
        channel(122, Some(10), Some(120), "Pairing", "voice", 11),
        // guild 20
        channel(200, Some(20), None, "COMMUNITY", "category", 0),
        channel(201, Some(20), Some(200), "help", "text", 1),
        channel(202, Some(20), Some(200), "showcase", "text", 2),
        channel(203, Some(20), Some(200), "off_topic", "text", 3),
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

    // Colours are Discord's own default role palette, in the order a survey
    // of 4290 real roles found them used: #3498db first, then #f1c40f,
    // #e74c3c, #9b59b6, #2ecc71. Server owners overwhelmingly pick from the
    // swatches Discord offers rather than typing a hex code, so a fixture
    // using its own tasteful blues is a tell.
    //
    // The uncoloured role is deliberate too: 41% of real roles have no colour
    // at all, and a list where every entry is tinted does not look like one.
    let roles = [
        RoleState {
            id: role_id(1),
            name: "Maintainer".into(),
            color: Some(0x3498db),
            position: 4,
            hoist: true,
            permissions: 0,
        },
        RoleState {
            id: role_id(2),
            name: "Contributor".into(),
            color: Some(0x2ecc71),
            position: 3,
            hoist: true,
            permissions: 0,
        },
        RoleState {
            id: role_id(3),
            name: "Bot".into(),
            color: Some(0xf1c40f),
            position: 2,
            hoist: false,
            permissions: 0,
        },
        RoleState {
            id: role_id(4),
            name: "Member".into(),
            color: None,
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

    // Activities, so both clients show something under a name and in a
    // profile: a game, a track with artist, and a custom status - the three
    // shapes that format differently.
    for (user, activity) in [
        (
            1002u64,
            crate::discord::ActivityInfo {
                kind: crate::discord::ActivityKind::Playing,
                name: "Factorio".to_string(),
                ..crate::discord::ActivityInfo::playing("")
            },
        ),
        (
            1003,
            crate::discord::ActivityInfo {
                kind: crate::discord::ActivityKind::Listening,
                name: "Spotify".to_string(),
                details: Some("Windowlicker".to_string()),
                state: Some("Aphex Twin".to_string()),
                ..crate::discord::ActivityInfo::playing("")
            },
        ),
        (
            1004,
            crate::discord::ActivityInfo {
                kind: crate::discord::ActivityKind::Custom,
                name: "Custom Status".to_string(),
                state: Some("out for lunch".to_string()),
                ..crate::discord::ActivityInfo::playing("")
            },
        ),
    ] {
        presence
            .guild_user_activities
            .insert((guild_id(10), user_id(user)), vec![activity.clone()]);
        presence
            .user_activities
            .insert(user_id(user), vec![activity]);
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
            41,
            111,
            Some(10),
            1002,
            "ferris",
            "check out our guild icon: https://cdn.discordapp.com/icons/747967102895390741/4ed9b9516ae3bb878c8e15f4ca089141.webp?size=1024",
            7080,
        ),
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
                id: emoji_id(9001),
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
        // Built for the same reason as the kitchen-sink message: a mention
        // carries its id in the text, and a hard-coded one resolves to nobody.
        &format!(
            "ping <@{}> about <#{}> when you get a chance",
            user_id(1002),
            channel_id(111)
        ),
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

    // ---- media -------------------------------------------------------------
    //
    // A picture, something animated, a video and a link preview, so the demo
    // covers every shape the message list can draw rather than only text.
    // The URLs use a scheme of our own: nothing is on a CDN, and demo mode
    // answers the preview request for them itself.
    let mut shot = message(
        60,
        111,
        Some(10),
        1001,
        "blu",
        "here is the new member list",
        520,
    );
    shot.attachments.push(attachment(
        9100,
        "member-list.png",
        "image/png",
        184_320,
        Some((320, 180)),
    ));
    general.push(shot);

    let mut moving = message(61, 111, Some(10), 1002, "ferris", "it scrolls now", 480);
    moving.attachments.push(attachment(
        9101,
        "marquee.gif",
        "image/gif",
        61_440,
        Some((160, 120)),
    ));
    general.push(moving);

    let mut clip = message(
        62,
        111,
        Some(10),
        1003,
        "turing",
        "screen recording of the voice panel",
        440,
    );
    clip.attachments.push(attachment(
        9102,
        "voice-panel.mp4",
        "video/mp4",
        2_355_200,
        None,
    ));
    general.push(clip);

    let mut linked = message(
        63,
        111,
        Some(10),
        1005,
        "ci-bot",
        "build finished: https://github.com/bluscream/concord/actions/runs/482",
        400,
    );
    linked.embeds.push(crate::discord::EmbedInfo {
        color: Some(0x57_9F_6E),
        provider_name: Some("RostFaden CI".to_string()),
        author_name: Some("pipeline #482".to_string()),
        title: Some("Build passed on gui".to_string()),
        description: Some("2329 tests, no warnings. Artefacts kept for seven days.".to_string()),
        fields: vec![
            crate::discord::EmbedFieldInfo {
                name: "Duration".to_string(),
                value: "3m 12s".to_string(),
                inline: false,
            },
            crate::discord::EmbedFieldInfo {
                name: "Commit".to_string(),
                value: "48c857c5".to_string(),
                inline: false,
            },
        ],
        footer_text: Some("finished a moment ago".to_string()),
        url: Some("https://github.com/bluscream/concord/actions/runs/482".to_string()),
        // A real, resolvable PNG: the embed picture is fetched over the
        // network in demo mode, so a dead link shows an empty card and
        // proves nothing about the rendering.
        image_url: Some(
            "https://github.githubassets.com/images/modules/logos_page/GitHub-Mark.png".to_string(),
        ),
        ..Default::default()
    });
    general.push(linked);

    let mut gifv = message(64, 111, Some(10), 1002, "ferris", "mood", 360);
    gifv.embeds.push(crate::discord::EmbedInfo {
        provider_name: Some("Tenor".to_string()),
        title: Some("shipping it".to_string()),
        url: Some("https://tenor.com/view/shipping-it-gif-1234567".to_string()),
        // A gifv embed carries the animation separately, because Discord
        // reports only a video URL for this kind.
        gifv_image_url: Some(
            "https://media.tenor.com/x8v1oNUOmg4AAAAM/rickroll-roll.gif".to_string(),
        ),
        ..Default::default()
    });
    general.push(gifv);

    // A message with one of everything, so a change to the renderer can be
    // checked against every case at once rather than by hunting for an
    // example of each. Kept under the 2000-character limit on purpose: it has
    // to be a message that could actually be sent.
    let mut kitchen_sink = message(70, 111, Some(10), 1003, "turing", &kitchen_sink(), 300);
    kitchen_sink.mention_everyone = true;
    kitchen_sink.attachments.push(attachment(
        9200,
        "screenshot.png",
        "image/png",
        184_320,
        Some((640, 400)),
    ));
    kitchen_sink.attachments.push(attachment(
        9201,
        "recording.mp4",
        "video/mp4",
        4_194_304,
        None,
    ));
    kitchen_sink
        .attachments
        .push(attachment(9202, "notes.txt", "text/plain", 2_048, None));
    kitchen_sink.attachments.push(attachment(
        9203,
        "archive.zip",
        "application/zip",
        10_485_760,
        None,
    ));
    kitchen_sink.embeds.push(crate::discord::EmbedInfo {
        color: Some(0x24_29_2F),
        provider_name: Some("GitHub".to_string()),
        author_name: Some("bluscream".to_string()),
        title: Some("bluscream/concord".to_string()),
        description: Some("A Discord client in Rust.".to_string()),
        url: Some("https://github.com/bluscream/concord".to_string()),
        fields: vec![crate::discord::EmbedFieldInfo {
            name: "Language".to_string(),
            value: "Rust".to_string(),
            inline: false,
        }],
        footer_text: Some("github.com".to_string()),
        ..Default::default()
    });
    general.push(kitchen_sink);

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
                &format!("<@{}> can you take a look at this one?", user_id(1001)),
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
            mutual_friends: Vec::new(),
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
    archived: bool,
) -> (
    Vec<crate::discord::ChannelInfo>,
    Vec<crate::discord::MessageInfo>,
) {
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
            topic: None,
            nsfw: None,
            user_limit: None,
            available_tags: Vec::new(),
            applied_tags: Vec::new(),
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
            id: attachment_id(9_000 + index as u64 + 1),
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

    // A fresh snowflake, so it cannot collide with a fixture channel and
    // inherit its timeline.
    let id = Id::new(snowflake_at(0));

    let navigation = Arc::make_mut(&mut state.navigation);
    // Built directly rather than through `channel`, which takes the small
    // seeds the fixture is written in and runs them through the snowflake
    // encoder. These ids are already real, and passing them in would encode
    // them a second time - which is how the post ended up parented to a
    // channel that does not exist.
    let mut post = blank_channel_with_id(id, "thread", title);
    post.guild_id = guild_id;
    post.parent_id = Some(parent);
    post.position = Some(0);
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

    const W: u32 = 640;
    const H: u32 = 400;

    // Drawn as a rough screenshot rather than a gradient. A gradient is a
    // fair test of the layout and a poor test of everything else: you cannot
    // tell a correctly-scaled gradient from a stretched one, or a cropped one
    // from a whole one, which is exactly what an image viewer has to get
    // right. Straight edges and a repeating pattern make all three obvious.
    let hue = (seed % 6) as u8;
    let mut pixels = Vec::with_capacity((W * H * 3) as usize);

    for y in 0..H {
        for x in 0..W {
            // A title bar, a sidebar, and rows of "text" in the body.
            let title_bar = y < 28;
            let sidebar = x < 140 && !title_bar;
            let gutter = (140..160).contains(&x);
            let text_row = !title_bar && !sidebar && !gutter && (y % 24) < 10 && x < W - 40;

            let (r, g, b) = if title_bar {
                (46, 48, 54)
            } else if sidebar {
                // Rows in the sidebar, so a vertical crop is visible too.
                if (y % 20) < 12 && x > 12 && x < 128 {
                    (70, 74, 84)
                } else {
                    (34, 36, 42)
                }
            } else if text_row {
                let shade = 90 + ((x / 7 + y / 24) % 3) as u8 * 30;
                match hue {
                    0 => (shade, 120, 200),
                    1 => (120, shade, 150),
                    2 => (200, 140, shade),
                    3 => (shade, 190, 190),
                    4 => (190, shade, 120),
                    _ => (150, 150, shade),
                }
            } else {
                (24, 25, 30)
            };

            // A one-pixel border, so an edge that has been cropped away is
            // immediately visible rather than merely suspected.
            let edge = x == 0 || y == 0 || x == W - 1 || y == H - 1;
            if edge {
                pixels.extend_from_slice(&[220, 220, 220]);
            } else {
                pixels.extend_from_slice(&[r, g, b]);
            }
        }
    }

    let mut out = Vec::new();
    let encoded =
        PngEncoder::new(&mut out).write_image(&pixels, W, H, image::ExtendedColorType::Rgb8);

    // An encoder failure would mean a bug here, not bad input; an empty
    // result is preferable to a panic in a demo path.
    if encoded.is_err() { Vec::new() } else { out }
}

/// A short animated GIF, so the demo has something that actually moves.
///
/// A still image would exercise the same code path as a PNG and prove
/// nothing about the animated one - which is the path that decides whether
/// the "animate images" setting does anything.
pub fn demo_preview_gif(seed: u64) -> Vec<u8> {
    use image::{Delay, Frame, RgbaImage, codecs::gif::GifEncoder};
    use std::time::Duration;

    const W: u32 = 160;
    const H: u32 = 120;
    const FRAMES: u32 = 12;

    let mut out = Vec::new();
    {
        let mut encoder = GifEncoder::new(&mut out);
        if encoder
            .set_repeat(image::codecs::gif::Repeat::Infinite)
            .is_err()
        {
            return Vec::new();
        }

        for index in 0..FRAMES {
            let mut frame = RgbaImage::new(W, H);
            // A band sweeping across, which reads as motion at any size.
            let sweep = index * W / FRAMES;
            for (x, y, pixel) in frame.enumerate_pixels_mut() {
                let near = x.abs_diff(sweep).min(W - x.abs_diff(sweep));
                let glow = 255u32.saturating_sub(near * 6) as u8;
                *pixel = image::Rgba([
                    glow,
                    (y * 255 / H) as u8,
                    ((seed as u32 + index * 20) % 256) as u8,
                    255,
                ]);
            }
            if encoder
                .encode_frame(Frame::from_parts(
                    frame,
                    0,
                    0,
                    Delay::from_saturating_duration(Duration::from_millis(80)),
                ))
                .is_err()
            {
                return Vec::new();
            }
        }
    }
    out
}

/// Build an embed for a link, the way Discord's unfurler would.
///
/// Discord does this server-side: the client sends a message with a link in
/// it and the embed arrives afterwards in a MESSAGE_UPDATE. Offline there is
/// no server to do it, so the fixture stands in - otherwise pasting a link
/// in demo mode shows nothing at all and the embed rendering can only be
/// seen on messages that were canned in advance.
///
/// Recognises the shapes worth showing rather than pretending to fetch: a
/// direct image, a GitHub repository, and a Tenor GIF.
pub fn unfurl(url: &str) -> Option<crate::discord::EmbedInfo> {
    let trimmed = url.trim_end_matches(['.', ',', ')', '>']);
    if !trimmed.starts_with("http://") && !trimmed.starts_with("https://") {
        return None;
    }

    let lower = trimmed.to_ascii_lowercase();
    let host = lower
        .split_once("://")
        .map(|(_, rest)| rest.split('/').next().unwrap_or(""))
        .unwrap_or("");

    // A link straight to an image is its own preview.
    if [".png", ".jpg", ".jpeg", ".gif", ".webp"]
        .iter()
        .any(|ext| lower.ends_with(ext))
    {
        return Some(crate::discord::EmbedInfo {
            provider_name: Some(host.to_string()),
            image_url: Some(trimmed.to_string()),
            ..Default::default()
        });
    }

    if host.ends_with("tenor.com") {
        return Some(crate::discord::EmbedInfo {
            provider_name: Some("Tenor".to_string()),
            title: Some(tenor_slug(trimmed)),
            url: Some(trimmed.to_string()),
            ..Default::default()
        });
    }

    if host.ends_with("github.com") {
        let path: Vec<&str> = trimmed
            .split_once("github.com/")
            .map(|(_, rest)| rest.split('/').collect())
            .unwrap_or_default();
        if path.len() >= 2 {
            return Some(crate::discord::EmbedInfo {
                color: Some(0x24_29_2F),
                provider_name: Some("GitHub".to_string()),
                author_name: Some(path[0].to_string()),
                title: Some(format!("{}/{}", path[0], path[1])),
                description: Some(
                    "Offline: the fixture recognises the link but cannot read the page."
                        .to_string(),
                ),
                url: Some(trimmed.to_string()),
                ..Default::default()
            });
        }
    }

    None
}

/// The words out of a Tenor share link, which is where its title lives.
fn tenor_slug(url: &str) -> String {
    let slug = url.rsplit('/').next().unwrap_or("");
    let words: Vec<&str> = slug
        .split('-')
        .filter(|part| *part != "gif" && !part.chars().all(|c| c.is_ascii_digit()))
        .collect();
    if words.is_empty() {
        "Tenor".to_string()
    } else {
        words.join(" ")
    }
}

/// Attach an unfurled embed to the last message in a channel.
pub fn unfurl_last_message(state: &mut DiscordState, channel_id: Id<marker::ChannelMarker>) {
    let cache = Arc::make_mut(&mut state.message_cache);
    let Some(timeline) = cache.timelines.get_mut(&channel_id) else {
        return;
    };
    let Some(message) = timeline.messages.back_mut() else {
        return;
    };
    let Some(content) = message.content.clone() else {
        return;
    };

    // Only the first link, as Discord does for an ordinary message.
    for word in content.split_whitespace() {
        if let Some(embed) = unfurl(word) {
            message.embeds.push(embed);
            return;
        }
    }
}

/// The id the fixture gave one of its channels.
///
/// Public because a test cannot write `Id::new(111)` any more: the fixture
/// issues real snowflakes, so the only way to name a channel from outside is
/// to ask for it by the same small number the fixture used.
pub fn demo_channel_id(seed: u64) -> Id<marker::ChannelMarker> {
    channel_id(seed)
}

/// The id the fixture gave one of its users.
pub fn demo_user(seed: u64) -> Id<marker::UserMarker> {
    user_id(seed)
}

/// The id the fixture gave one of its servers.
pub fn demo_guild(seed: u64) -> Id<marker::GuildMarker> {
    guild_id(seed)
}

/// The id the fixture gave one of its roles.
pub fn demo_role_id(seed: u64) -> Id<marker::RoleMarker> {
    role_id(seed)
}

/// The server the fixture's administration panels belong to.
pub fn demo_guild_id() -> Id<marker::GuildMarker> {
    guild_id(10)
}

/// The bot in the fixture, which answers slash commands.
pub fn demo_bot_id() -> Id<marker::UserMarker> {
    user_id(1005)
}

/// Linked accounts, for the connections panel.
pub fn demo_connections() -> Vec<crate::discord::Connection> {
    use crate::discord::{Connection, ConnectionVisibility};
    vec![
        Connection {
            id: "gh-1".to_string(),
            kind: "github".to_string(),
            name: "bluscream".to_string(),
            verified: true,
            show_activity: true,
            visibility: ConnectionVisibility::Everyone,
        },
        Connection {
            id: "sp-1".to_string(),
            kind: "spotify".to_string(),
            name: "blu".to_string(),
            verified: true,
            show_activity: true,
            visibility: ConnectionVisibility::Everyone,
        },
        // Unverified and hidden, so the panel shows both states rather than
        // a list where every row looks the same.
        Connection {
            id: "st-1".to_string(),
            kind: "steam".to_string(),
            name: "rostfaden".to_string(),
            verified: false,
            show_activity: false,
            visibility: ConnectionVisibility::Hidden,
        },
    ]
}

/// Signed-in devices, for the sessions panel.
pub fn demo_auth_sessions() -> Vec<crate::discord::AuthSession> {
    use crate::discord::AuthSession;
    vec![
        AuthSession {
            id_hash: "current".to_string(),
            os: "Linux".to_string(),
            platform: "concord".to_string(),
            location: Some("Berlin, DE".to_string()),
            last_used: Some("just now".to_string()),
            current: true,
        },
        AuthSession {
            id_hash: "phone".to_string(),
            os: "Android".to_string(),
            platform: "Discord Android".to_string(),
            location: Some("Berlin, DE".to_string()),
            last_used: Some("yesterday".to_string()),
            current: false,
        },
        // No location, which Discord does for a session it cannot place.
        AuthSession {
            id_hash: "unknown".to_string(),
            os: "Windows".to_string(),
            platform: "Discord Desktop".to_string(),
            location: None,
            last_used: Some("last week".to_string()),
            current: false,
        },
    ]
}

/// Authorised applications, for the apps panel.
pub fn demo_authorised_apps() -> Vec<crate::discord::AuthorisedApp> {
    use crate::discord::AuthorisedApp;
    vec![
        AuthorisedApp {
            id: "app-1".to_string(),
            name: "RostFaden CI".to_string(),
            scopes: vec!["identify".to_string(), "guilds".to_string()],
        },
        // Discord allows an app with no scopes, and the panel has a line for
        // it that would otherwise never be exercised.
        AuthorisedApp {
            id: "app-2".to_string(),
            name: "Old Bot".to_string(),
            scopes: Vec::new(),
        },
    ]
}

/// Two-factor backup codes, one already spent.
pub fn demo_backup_codes() -> Vec<crate::discord::BackupCode> {
    (0..8)
        .map(|index| crate::discord::BackupCode {
            code: format!("{:04}-{:04}", 1000 + index * 7, 4321 + index * 13),
            consumed: index == 2,
        })
        .collect()
}

/// Mute or unmute a guild.
pub fn set_guild_muted(state: &mut DiscordState, guild_id: Id<marker::GuildMarker>, muted: bool) {
    let notifications = Arc::make_mut(&mut state.notifications);
    notifications.set_fixture_guild_muted(guild_id, muted);
}

/// Mute or unmute one channel.
pub fn set_channel_muted(
    state: &mut DiscordState,
    channel_id: Id<marker::ChannelMarker>,
    muted: bool,
) {
    // Channel overrides hang off the guild's settings, so a channel with no
    // guild - a DM - has nowhere to store one.
    let Some(guild_id) = state
        .channel(channel_id)
        .and_then(|channel| channel.guild_id)
    else {
        return;
    };
    let notifications = Arc::make_mut(&mut state.notifications);
    notifications.set_fixture_channel_muted(guild_id, channel_id, muted);
}

/// Archive or restore a thread.
pub fn set_thread_archived(
    state: &mut DiscordState,
    channel_id: Id<marker::ChannelMarker>,
    archived: bool,
) {
    let navigation = Arc::make_mut(&mut state.navigation);
    if let Some(channel) = navigation.channels.get_mut(&channel_id)
        && let Some(metadata) = channel.thread_metadata.as_mut()
    {
        metadata.archived = archived;
    }
}

/// Remove a member from a guild, as a kick or ban does.
pub fn remove_member(
    state: &mut DiscordState,
    guild_id: Id<marker::GuildMarker>,
    user_id: Id<marker::UserMarker>,
) {
    {
        let guild_details = Arc::make_mut(&mut state.guild_details);
        if let Some(members) = guild_details.members.get_mut(&guild_id) {
            members.remove(&user_id);
        }
    }
    // The member list is a separate projection kept as a whole snapshot, so
    // it is rebuilt without the removed member rather than edited in place.
    let remaining: Vec<_> = state
        .member_list_entries_for_guild(guild_id)
        .into_iter()
        .filter(|(_, entry)| !matches!(entry, crate::discord::GuildMemberListEntry::Member { user_id: id } if *id == user_id))
        .map(|(index, entry)| (index, entry.clone()))
        .collect();

    let guild_details = Arc::make_mut(&mut state.guild_details);
    guild_details.set_fixture_member_list(guild_id, remaining);
}

/// Replace a member's roles.
pub fn set_member_roles(
    state: &mut DiscordState,
    guild_id: Id<marker::GuildMarker>,
    user_id: Id<marker::UserMarker>,
    role_ids: &[Id<marker::RoleMarker>],
) {
    let guild_details = Arc::make_mut(&mut state.guild_details);
    if let Some(member) = guild_details
        .members
        .get_mut(&guild_id)
        .and_then(|members| members.get_mut(&user_id))
    {
        member.role_ids = role_ids.to_vec();
        member.role_ids_known = true;
    }
}

/// Set the demo user's presence.
pub fn set_current_presence(state: &mut DiscordState, status: PresenceStatus) {
    let presence = Arc::make_mut(&mut state.presence);
    presence
        .guild_user_presences
        .insert((guild_id(10), demo_user_id()), status);
    presence.user_presences.insert(demo_user_id(), status);
}

/// Drop the embeds from a message, as "remove embeds" does.
pub fn remove_embeds(
    state: &mut DiscordState,
    channel_id: Id<marker::ChannelMarker>,
    target: Id<marker::MessageMarker>,
) {
    let cache = Arc::make_mut(&mut state.message_cache);
    if let Some(timeline) = cache.timelines.get_mut(&channel_id)
        && let Some(message) = timeline.messages.iter_mut().find(|m| m.id == target)
    {
        message.embeds.clear();
    }
}

/// Remove a guild, as leaving does.
pub fn leave_guild(state: &mut DiscordState, guild_id: Id<marker::GuildMarker>) {
    let navigation = Arc::make_mut(&mut state.navigation);
    navigation.guilds.remove(&guild_id);
    // Its channels go with it, or the sidebar keeps offering channels in a
    // guild that is no longer listed.
    navigation
        .channels
        .retain(|_, channel| channel.guild_id != Some(guild_id));
}

/// The ids of every custom emoji this fixture invents.
///
/// Listed rather than derived, so adding an emoji to the fixture without
/// adding it here shows up as a blank gap rather than as a picture that
/// silently comes from somewhere else.
const FIXTURE_EMOJI: [u64; 6] = [4001, 4002, 4003, 4004, 4005, 9001];

/// Whether an emoji CDN address names an emoji this fixture made up.
///
/// Demo mode answers a preview request for one of these, because there is
/// nothing real behind the address to cover up - the ids do not exist on
/// Discord's CDN and never will. Every other address is left alone.
pub fn is_fixture_emoji_url(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://cdn.discordapp.com/emojis/") else {
        return false;
    };
    // The id ends at the extension; the query string that marks an animated
    // emoji comes after that and is not part of it.
    let rest = rest.split(['?', '#']).next().unwrap_or(rest);
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    let Ok(id) = digits.parse::<u64>() else {
        return false;
    };
    FIXTURE_EMOJI.iter().any(|raw| emoji_id(*raw).get() == id)
}

/// A small square standing in for a custom emoji.
///
/// Its own generator rather than [`demo_preview_png`] scaled down: that draws
/// a 640x400 mock screenshot, and squeezing one into 22 pixels gives a grey
/// smudge that reads as a failed load. A flat disc on a transparent ground is
/// what an emoji looks like at that size, and the colour comes from the id so
/// two different emoji are visibly two different emoji.
pub fn demo_emoji_png(id: u64) -> Vec<u8> {
    use image::{ImageEncoder, codecs::png::PngEncoder};

    const SIDE: u32 = 64;
    let radius = (SIDE / 2) as f32 - 1.0;
    let centre = (SIDE as f32 - 1.0) / 2.0;

    // Spread around the wheel by the id, at a fixed saturation, so every
    // emoji is legible against both themes rather than occasionally black.
    let hue = (id % 360) as f32;
    let (r, g, b) = hue_to_rgb(hue);

    let mut pixels = Vec::with_capacity((SIDE * SIDE * 4) as usize);
    for y in 0..SIDE {
        for x in 0..SIDE {
            let dx = x as f32 - centre;
            let dy = y as f32 - centre;
            let distance = (dx * dx + dy * dy).sqrt();
            // One pixel of feather, so the disc does not have a staircase
            // edge at the size it is actually drawn.
            let alpha = ((radius - distance).clamp(0.0, 1.0) * 255.0) as u8;
            // A lighter upper-left quadrant reads as a highlight, which is
            // enough to stop it looking like a coloured dot.
            let lift = if dx + dy < -radius * 0.4 { 40 } else { 0 };
            pixels.push(r.saturating_add(lift));
            pixels.push(g.saturating_add(lift));
            pixels.push(b.saturating_add(lift));
            pixels.push(alpha);
        }
    }

    let mut out = Vec::new();
    if PngEncoder::new(&mut out)
        .write_image(&pixels, SIDE, SIDE, image::ExtendedColorType::Rgba8)
        .is_err()
    {
        return Vec::new();
    }
    out
}

/// An animated stand-in for an animated custom emoji.
///
/// The same disc as [`demo_emoji_png`], pulsing. Animation is the whole
/// difference between an animated emoji and a still one, so a still image
/// here would make the two indistinguishable and prove nothing.
pub fn demo_emoji_gif(id: u64) -> Vec<u8> {
    use image::{Delay, Frame, RgbaImage, codecs::gif::GifEncoder};
    use std::time::Duration;

    const SIDE: u32 = 64;
    const FRAMES: u32 = 8;

    let hue = (id % 360) as f32;
    let (r, g, b) = hue_to_rgb(hue);
    let centre = (SIDE as f32 - 1.0) / 2.0;

    let mut out = Vec::new();
    {
        let mut encoder = GifEncoder::new(&mut out);
        if encoder
            .set_repeat(image::codecs::gif::Repeat::Infinite)
            .is_err()
        {
            return Vec::new();
        }
        for index in 0..FRAMES {
            // Breathes between two thirds and full size and back, so the loop
            // has no seam where it restarts.
            let phase = (index as f32 / FRAMES as f32) * std::f32::consts::TAU;
            let radius = (SIDE / 2) as f32 * (0.82 + 0.18 * phase.sin());

            let mut image = RgbaImage::new(SIDE, SIDE);
            for (x, y, pixel) in image.enumerate_pixels_mut() {
                let dx = x as f32 - centre;
                let dy = y as f32 - centre;
                let distance = (dx * dx + dy * dy).sqrt();
                let alpha = ((radius - distance).clamp(0.0, 1.0) * 255.0) as u8;
                *pixel = image::Rgba([r, g, b, alpha]);
            }

            if encoder
                .encode_frame(Frame::from_parts(
                    image,
                    0,
                    0,
                    Delay::from_saturating_duration(Duration::from_millis(90)),
                ))
                .is_err()
            {
                return Vec::new();
            }
        }
    }
    out
}

/// A fully saturated colour for a hue in degrees.
fn hue_to_rgb(hue: f32) -> (u8, u8, u8) {
    let sector = hue / 60.0;
    let rising = ((sector % 2.0) - 1.0).abs();
    let x = ((1.0 - rising) * 200.0) as u8;
    match sector as u32 {
        0 => (220, x, 40),
        1 => (x, 220, 40),
        2 => (40, 220, x),
        3 => (40, x, 220),
        4 => (x, 40, 220),
        _ => (220, 40, x),
    }
}

#[cfg(test)]
mod emoji_url_tests {
    use super::{FIXTURE_EMOJI, emoji_id, is_fixture_emoji_url};
    use crate::discord::custom_emoji_image_url;

    #[test]
    fn recognises_every_emoji_the_fixture_hands_out() {
        for raw in FIXTURE_EMOJI {
            for animated in [false, true] {
                let url = custom_emoji_image_url(emoji_id(raw).get(), animated);
                assert!(
                    is_fixture_emoji_url(&url),
                    "{url} is the fixture's own emoji {raw} and was not recognised"
                );
            }
        }
    }

    #[test]
    fn leaves_real_addresses_alone() {
        for url in [
            // A real Discord emoji: answering this would cover up the actual
            // picture with a generated one.
            "https://cdn.discordapp.com/emojis/123456789012345678.png",
            "https://cdn.discordapp.com/attachments/1/2/photo.png",
            "https://example.com/emojis/4001.png",
            "not a url at all",
        ] {
            assert!(!is_fixture_emoji_url(url), "{url} should be left alone");
        }
    }
}

#[cfg(test)]
mod emoji_image_tests {
    use super::{demo_emoji_gif, demo_emoji_png};

    #[test]
    fn the_still_emoji_is_a_decodable_png() {
        let bytes = demo_emoji_png(4001);
        assert!(!bytes.is_empty(), "encoding produced nothing");
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n", "not a PNG");
        let decoded = image::load_from_memory(&bytes).expect("should decode");
        assert_eq!((decoded.width(), decoded.height()), (64, 64));
    }

    #[test]
    fn the_animated_emoji_is_a_decodable_gif() {
        let bytes = demo_emoji_gif(4002);
        assert!(!bytes.is_empty(), "encoding produced nothing");
        assert_eq!(&bytes[..3], b"GIF", "not a GIF");
        image::load_from_memory(&bytes).expect("should decode");
    }

    /// Two emoji must not come out looking like the same emoji.
    #[test]
    fn different_ids_give_different_pictures() {
        assert_ne!(
            demo_emoji_png(4001),
            demo_emoji_png(4002),
            "every emoji would look identical"
        );
    }
}
