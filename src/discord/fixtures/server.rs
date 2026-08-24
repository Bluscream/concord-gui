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

// ---------------------------------------------------------------------------
// mutable state

/// The lists a server administrator edits, as this fake session holds them.
///
/// Separate from [`crate::discord::DiscordState`] because none of it lives
/// there: stickers, events, templates and the rest come back from REST calls
/// and are held by whichever panel asked. Without somewhere to keep them, a
/// rename or a delete had nowhere to be written and the panel redrew from the
/// canned list as though nothing had happened.
///
/// Held for the life of the session, so an edit sticks until the client is
/// restarted - which is what a real server would do.
pub struct ServerAdmin {
    pub stickers: Vec<GuildSticker>,
    pub events: Vec<ScheduledEvent>,
    pub templates: Vec<GuildTemplate>,
    pub welcome: WelcomeScreen,
    pub widget: GuildWidget,
    pub discovery: DiscoveryMetadata,
    pub automod: Vec<crate::discord::AutoModRule>,
    pub invites: Vec<crate::discord::GuildInviteInfo>,
    pub sounds: Vec<crate::discord::SoundboardSound>,
    pub emojis: Vec<crate::discord::GuildEmojiInfo>,
    pub bans: Vec<crate::discord::GuildBanInfo>,
    pub connections: Vec<crate::discord::Connection>,
    pub sessions: Vec<crate::discord::AuthSession>,
    pub apps: Vec<crate::discord::AuthorisedApp>,
    pub backup_codes: Vec<crate::discord::BackupCode>,
    pub totp_enabled: bool,
    /// Where the next generated id comes from.
    ///
    /// A counter rather than a hash of the name: two stickers may share a
    /// name, and an id collision would make the second edit act on the first.
    next_id: u64,
}

impl Default for ServerAdmin {
    fn default() -> Self {
        Self::new()
    }
}

impl ServerAdmin {
    pub fn new() -> Self {
        Self {
            stickers: stickers(),
            events: scheduled_events(),
            templates: templates(),
            welcome: welcome_screen(),
            widget: widget(),
            discovery: discovery_metadata(),
            automod: automod_rules(),
            invites: invites(),
            sounds: sounds(),
            emojis: super::guild::emojis(),
            bans: super::people::bans(),
            connections: super::demo_connections(),
            sessions: super::demo_auth_sessions(),
            apps: super::demo_authorised_apps(),
            backup_codes: super::demo_backup_codes(),
            totp_enabled: false,
            next_id: 50_000,
        }
    }

    /// An id nothing else is using.
    pub fn fresh_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }
}

/// Automod rules, one of each shape the list renders.
pub fn automod_rules() -> Vec<crate::discord::AutoModRule> {
    use crate::discord::{AutoModAction, AutoModRule, AutoModTrigger};
    vec![
        AutoModRule {
            id: 1,
            name: "No invite links".to_string(),
            enabled: true,
            trigger: AutoModTrigger::Keyword,
            actions: vec![AutoModAction::BlockMessage],
        },
        // Disabled, so the enable/disable toggle has both states to show.
        AutoModRule {
            id: 2,
            name: "Mention spam".to_string(),
            enabled: false,
            trigger: AutoModTrigger::MentionSpam,
            actions: vec![AutoModAction::BlockMessage],
        },
    ]
}

/// Invites, covering unlimited, limited and temporary.
pub fn invites() -> Vec<crate::discord::GuildInviteInfo> {
    use crate::discord::GuildInviteInfo;
    vec![
        GuildInviteInfo {
            code: "aBc-123".to_string(),
            channel_id: Some(channel_id(111)),
            channel_name: Some("general".to_string()),
            inviter: Some("ferris".to_string()),
            uses: 3,
            max_uses: None,
            max_age_seconds: None,
            temporary: false,
        },
        GuildInviteInfo {
            code: "xyz-789".to_string(),
            channel_id: Some(channel_id(112)),
            channel_name: Some("gui-rewrite".to_string()),
            inviter: Some("blu".to_string()),
            uses: 9,
            max_uses: Some(10),
            max_age_seconds: Some(86_400),
            temporary: true,
        },
    ]
}

/// Soundboard sounds, including one the server may not play.
pub fn sounds() -> Vec<crate::discord::SoundboardSound> {
    use crate::discord::SoundboardSound;
    vec![
        SoundboardSound {
            sound_id: 1,
            name: "airhorn".to_string(),
            volume: 1.0,
            emoji_id: None,
            emoji_name: Some("\u{266A}".to_string()),
            guild_id: Some(guild_id(10)),
            available: true,
        },
        SoundboardSound {
            sound_id: 2,
            name: "quack".to_string(),
            volume: 0.5,
            emoji_id: None,
            emoji_name: None,
            guild_id: Some(guild_id(10)),
            // Shown and refused, so the reason is visible.
            available: false,
        },
    ]
}

/// Turn a compose-form event into the shape the list holds.
///
/// The two differ because one describes what to create and the other what
/// exists: the created event has an id, a status and an interest count that
/// the form has no opinion about.
pub fn event_from_new(id: u64, new: &crate::discord::NewEvent) -> ScheduledEvent {
    use crate::discord::NewEventLocation;
    ScheduledEvent {
        id,
        name: new.name.clone(),
        description: (!new.description.is_empty()).then(|| new.description.clone()),
        starts_at: (!new.starts_at.is_empty()).then(|| new.starts_at.clone()),
        ends_at: (!new.ends_at.is_empty()).then(|| new.ends_at.clone()),
        status: EventStatus::Scheduled,
        location: match &new.location {
            NewEventLocation::Channel(id) => EventLocation::Channel(*id),
            NewEventLocation::External(place) => EventLocation::External(place.clone()),
        },
        interested: Some(0),
    }
}

/// The roles the picked onboarding answers grant.
///
/// Onboarding is the one place a client hands out roles on your behalf, so
/// the fixture applies them rather than accepting the answers and doing
/// nothing - which would leave the whole flow with no visible result.
pub fn roles_for_onboarding(
    picked: &[u64],
) -> Vec<crate::discord::Id<crate::discord::marker::RoleMarker>> {
    onboarding()
        .questions
        .iter()
        .flat_map(|question| question.options.iter())
        .filter(|option| picked.contains(&option.id))
        .flat_map(|option| option.role_ids.iter().copied())
        .collect()
}
