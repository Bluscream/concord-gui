//! Server administration, as a fake server would answer it.
//!
//! Every panel behind the server-settings menu asks Discord for a list and
//! shows a spinner until one arrives. With nothing answering, each of those
//! panels sits on "loading" for ever, which reads as broken rather than as
//! offline - so the whole of server administration was invisible in demo
//! mode even though the client can drive all of it.
//!
//! In its own file because it is a pile of canned data with no logic worth
//! reading, and it would bury the parts of [`super`] that do have some.
//!
//! The data is chosen to cover the states each panel branches on rather than
//! to look plausible: an unavailable sticker, a cancelled event, a template
//! with unsynced changes, a server with no vanity code. A list where every row
//! is the same exercises one path and hides the rest.

use super::{channel_id, guild_id, role_id};
use crate::discord::{
    DiscoverableGuild, DiscoveryCategory, DiscoveryMetadata, EventLocation, EventStatus,
    GuildSticker, GuildTemplate, GuildWidget, Onboarding, OnboardingOption, OnboardingQuestion,
    ScheduledEvent, StageInstance, StickerFormat, WelcomeChannel, WelcomeScreen,
};

/// Stickers, covering both a usable one and one the server cannot send.
pub fn stickers() -> Vec<GuildSticker> {
    vec![
        GuildSticker {
            id: 8001,
            name: "ferris-wave".to_string(),
            description: Some("Ferris waving".to_string()),
            tags: "wave".to_string(),
            format: StickerFormat::Png,
            available: true,
        },
        GuildSticker {
            id: 8002,
            name: "crab-rave".to_string(),
            description: Some("It never stops".to_string()),
            tags: "dance".to_string(),
            format: StickerFormat::Gif,
            available: true,
        },
        // A server that loses its boosts keeps its stickers but cannot send
        // them, which is not visible from the name alone.
        GuildSticker {
            id: 8003,
            name: "old-mascot".to_string(),
            description: None,
            tags: "retired".to_string(),
            format: StickerFormat::Lottie,
            available: false,
        },
    ]
}

/// The categories a server can list itself under.
pub fn discovery_categories() -> Vec<DiscoveryCategory> {
    [
        (1u32, "Science & Tech", true),
        (2, "Education", false),
        (3, "Gaming", false),
    ]
    .into_iter()
    .map(|(id, name, is_primary)| DiscoveryCategory {
        id,
        name: name.to_string(),
        is_primary,
    })
    .collect()
}

/// How this server describes itself to discovery.
pub fn discovery_metadata() -> DiscoveryMetadata {
    DiscoveryMetadata {
        primary_category_id: Some(1),
        keywords: vec!["rust".to_string(), "discord".to_string()],
        emoji_discoverability_enabled: true,
        about: Some("A client written in Rust, and the people writing it.".to_string()),
        category_ids: vec![1, 2],
    }
}

/// Servers that can be browsed and joined.
pub fn discoverable_guilds() -> Vec<DiscoverableGuild> {
    vec![
        DiscoverableGuild {
            id: guild_id(30),
            name: "Rust Programming Language".to_string(),
            description: Some("The official community server.".to_string()),
            approximate_member_count: Some(92_400),
            approximate_presence_count: Some(8_120),
            vanity_url_code: Some("rust-lang".to_string()),
        },
        DiscoverableGuild {
            id: guild_id(31),
            name: "GPUI Builders".to_string(),
            description: Some("Toolkit talk.".to_string()),
            approximate_member_count: Some(1_840),
            approximate_presence_count: Some(210),
            // No vanity code, so this one cannot be joined from the list -
            // the row says so, and that path needs something to say it about.
            vanity_url_code: None,
        },
    ]
}

/// The questions a new member is asked on joining.
pub fn onboarding() -> Onboarding {
    Onboarding {
        enabled: true,
        questions: vec![
            OnboardingQuestion {
                id: 9001,
                title: "What brings you here?".to_string(),
                single_select: true,
                required: true,
                options: vec![
                    OnboardingOption {
                        id: 9101,
                        title: "Writing Rust".to_string(),
                        description: Some("Gets you the Contributor role".to_string()),
                        role_ids: vec![role_id(1002)],
                        channel_ids: vec![channel_id(111)],
                    },
                    OnboardingOption {
                        id: 9102,
                        title: "Just looking".to_string(),
                        description: None,
                        role_ids: Vec::new(),
                        channel_ids: vec![channel_id(101)],
                    },
                ],
            },
            OnboardingQuestion {
                id: 9002,
                title: "Which channels interest you?".to_string(),
                single_select: false,
                required: false,
                options: vec![
                    OnboardingOption {
                        id: 9201,
                        title: "The rewrite".to_string(),
                        description: Some("Daily churn".to_string()),
                        role_ids: Vec::new(),
                        channel_ids: vec![channel_id(112)],
                    },
                    OnboardingOption {
                        id: 9202,
                        title: "Build logs".to_string(),
                        description: None,
                        role_ids: Vec::new(),
                        channel_ids: vec![channel_id(113)],
                    },
                ],
            },
        ],
    }
}

/// The panel shown to someone who has just joined.
pub fn welcome_screen() -> WelcomeScreen {
    WelcomeScreen {
        enabled: true,
        description: Some("A Discord client in Rust. Read the rules, then say hello.".to_string()),
        channels: vec![
            WelcomeChannel {
                channel_id: channel_id(102),
                description: "Start here".to_string(),
                emoji_name: Some("book".to_string()),
            },
            WelcomeChannel {
                channel_id: channel_id(111),
                description: "Say hello".to_string(),
                emoji_name: Some("wave".to_string()),
            },
        ],
    }
}

/// The embeddable widget's settings.
pub fn widget() -> GuildWidget {
    GuildWidget {
        enabled: true,
        channel_id: Some(channel_id(111)),
    }
}

/// Scheduled events, one in each state the list renders differently.
pub fn scheduled_events() -> Vec<ScheduledEvent> {
    vec![
        ScheduledEvent {
            id: 7001,
            name: "Weekly standup".to_string(),
            description: Some("What everyone is on.".to_string()),
            starts_at: Some("2026-08-25T09:00:00Z".to_string()),
            ends_at: Some("2026-08-25T09:30:00Z".to_string()),
            status: EventStatus::Scheduled,
            location: EventLocation::Channel(channel_id(121)),
            interested: Some(6),
        },
        ScheduledEvent {
            id: 7002,
            name: "Pairing on the GUI".to_string(),
            description: None,
            starts_at: Some("2026-08-24T14:00:00Z".to_string()),
            ends_at: None,
            status: EventStatus::Active,
            location: EventLocation::Channel(channel_id(122)),
            interested: Some(3),
        },
        ScheduledEvent {
            id: 7003,
            name: "Release party".to_string(),
            description: Some("Somewhere that is not here.".to_string()),
            starts_at: Some("2026-09-01T18:00:00Z".to_string()),
            ends_at: None,
            status: EventStatus::Cancelled,
            location: EventLocation::External("A pub in Berlin".to_string()),
            interested: Some(11),
        },
    ]
}

/// Server templates, including one with changes not yet synced.
pub fn templates() -> Vec<GuildTemplate> {
    vec![
        GuildTemplate {
            code: "rostfaden-base".to_string(),
            name: "RostFaden base".to_string(),
            description: Some("Channels and roles, no messages.".to_string()),
            usage_count: 14,
            is_dirty: false,
        },
        // Dirty, because the "sync" action has nothing to act on otherwise.
        GuildTemplate {
            code: "rostfaden-wip".to_string(),
            name: "RostFaden (work in progress)".to_string(),
            description: None,
            usage_count: 2,
            is_dirty: true,
        },
    ]
}

/// The live stage in a channel, if that channel is running one.
pub fn stage_instance(
    channel: crate::discord::Id<crate::discord::marker::ChannelMarker>,
) -> Option<StageInstance> {
    // Only the stage channel has one; asking about an ordinary voice room
    // should answer "none" rather than inventing one.
    (channel == channel_id(121)).then(|| StageInstance {
        id: 6001,
        channel_id: channel,
        topic: "Standup, out loud".to_string(),
    })
}

/// How many members a prune would remove.
///
/// Derived from the window rather than fixed, so the number moves when the
/// day count does - a count that never changes looks like it was not read.
pub fn prune_count(days: u16) -> u64 {
    u64::from(days).saturating_mul(3) / 2
}
