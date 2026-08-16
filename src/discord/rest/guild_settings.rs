//! A guild's own settings.
//!
//! Only what the client can show. Discord's modify-guild endpoint takes around
//! thirty fields, most of them for community features, discovery and
//! monetisation; sending one the client cannot display would mean silently
//! owning a setting nobody can see or change.

use serde_json::{Map, Value, json};

use crate::Result;
use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker},
};

use super::DiscordRest;

/// Discord's caps on a guild name.
pub const MIN_GUILD_NAME_CHARS: usize = 2;
pub const MAX_GUILD_NAME_CHARS: usize = 100;

/// Which messages notify by default.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DefaultNotifications {
    AllMessages,
    OnlyMentions,
}

impl DefaultNotifications {
    pub const ALL: [Self; 2] = [Self::AllMessages, Self::OnlyMentions];

    pub const fn code(self) -> u8 {
        match self {
            Self::AllMessages => 0,
            Self::OnlyMentions => 1,
        }
    }

    pub const fn from_code(code: u64) -> Self {
        match code {
            1 => Self::OnlyMentions,
            _ => Self::AllMessages,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::AllMessages => "All messages",
            Self::OnlyMentions => "Only mentions",
        }
    }
}

/// How much of what gets scanned for explicit content.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExplicitContentFilter {
    Disabled,
    MembersWithoutRoles,
    AllMembers,
}

impl ExplicitContentFilter {
    pub const ALL: [Self; 3] = [Self::Disabled, Self::MembersWithoutRoles, Self::AllMembers];

    pub const fn code(self) -> u8 {
        match self {
            Self::Disabled => 0,
            Self::MembersWithoutRoles => 1,
            Self::AllMembers => 2,
        }
    }

    pub const fn from_code(code: u64) -> Self {
        match code {
            1 => Self::MembersWithoutRoles,
            2 => Self::AllMembers,
            _ => Self::Disabled,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Disabled => "Do not scan",
            Self::MembersWithoutRoles => "Scan members without roles",
            Self::AllMembers => "Scan everyone",
        }
    }
}

/// What to change about a guild.
///
/// `None` means leave alone, as with channels and roles: sending the whole
/// guild back would overwrite the community, discovery and monetisation
/// settings this client never shows.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GuildEdit {
    pub name: Option<String>,
    pub verification_level: Option<crate::discord::GuildVerificationLevel>,
    pub default_notifications: Option<DefaultNotifications>,
    pub explicit_content_filter: Option<ExplicitContentFilter>,
    /// `Some(None)` clears the AFK channel.
    pub afk_channel_id: Option<Option<Id<ChannelMarker>>>,
    /// Seconds of silence before someone is moved to the AFK channel.
    pub afk_timeout_seconds: Option<u32>,
    /// `Some(None)` clears the system channel, silencing join messages.
    pub system_channel_id: Option<Option<Id<ChannelMarker>>>,
}

/// The AFK timeouts Discord accepts. Anything else is rejected.
pub const AFK_TIMEOUTS: [u32; 5] = [60, 300, 900, 1800, 3600];

impl GuildEdit {
    pub fn is_empty(&self) -> bool {
        self.name.is_none()
            && self.verification_level.is_none()
            && self.default_notifications.is_none()
            && self.explicit_content_filter.is_none()
            && self.afk_channel_id.is_none()
            && self.afk_timeout_seconds.is_none()
            && self.system_channel_id.is_none()
    }

    fn to_body(&self) -> Value {
        let mut fields = Map::new();
        if let Some(name) = &self.name {
            fields.insert(
                "name".to_owned(),
                Value::from(name.chars().take(MAX_GUILD_NAME_CHARS).collect::<String>()),
            );
        }
        if let Some(level) = self.verification_level {
            // An Unknown level came from a newer Discord than this build
            // knows; sending it back is the only honest thing, since anything
            // else would silently downgrade a security setting.
            fields.insert(
                "verification_level".to_owned(),
                Value::from(verification_code(level)),
            );
        }
        if let Some(notifications) = self.default_notifications {
            fields.insert(
                "default_message_notifications".to_owned(),
                Value::from(notifications.code()),
            );
        }
        if let Some(filter) = self.explicit_content_filter {
            fields.insert(
                "explicit_content_filter".to_owned(),
                Value::from(filter.code()),
            );
        }
        if let Some(channel) = &self.afk_channel_id {
            fields.insert("afk_channel_id".to_owned(), channel_value(*channel));
        }
        if let Some(timeout) = self.afk_timeout_seconds {
            // Discord rejects anything not in its own list, so the nearest
            // accepted value is sent rather than the request being refused.
            fields.insert(
                "afk_timeout".to_owned(),
                Value::from(nearest_afk_timeout(timeout)),
            );
        }
        if let Some(channel) = &self.system_channel_id {
            fields.insert("system_channel_id".to_owned(), channel_value(*channel));
        }
        Value::Object(fields)
    }
}

/// Discord's number for a verification level.
///
/// The core's `GuildVerificationLevel` parses these and keeps unknown ones, so
/// this is its inverse rather than a second copy of the mapping.
pub fn verification_code(level: crate::discord::GuildVerificationLevel) -> u64 {
    use crate::discord::GuildVerificationLevel as Level;
    match level {
        Level::None => 0,
        Level::Low => 1,
        Level::Medium => 2,
        Level::High => 3,
        Level::VeryHigh => 4,
        Level::Unknown(value) => value,
    }
}

/// What a verification level actually requires, which the number does not say.
pub fn verification_label(level: crate::discord::GuildVerificationLevel) -> String {
    use crate::discord::GuildVerificationLevel as Level;
    match level {
        Level::None => "None".to_owned(),
        Level::Low => "Low - verified email".to_owned(),
        Level::Medium => "Medium - registered over 5 minutes".to_owned(),
        Level::High => "High - a member for over 10 minutes".to_owned(),
        Level::VeryHigh => "Highest - verified phone".to_owned(),
        // Kept rather than shown as "None": a newer level is stricter, and
        // displaying it as the weakest one would be actively misleading.
        Level::Unknown(value) => format!("Unrecognised level {value}"),
    }
}

fn channel_value(channel: Option<Id<ChannelMarker>>) -> Value {
    match channel {
        Some(id) => Value::from(id.get().to_string()),
        None => Value::Null,
    }
}

/// The accepted timeout closest to what was asked for.
pub fn nearest_afk_timeout(seconds: u32) -> u32 {
    AFK_TIMEOUTS
        .into_iter()
        .min_by_key(|candidate| candidate.abs_diff(seconds))
        .unwrap_or(300)
}

/// Whether Discord will accept this as a guild name.
pub fn is_valid_guild_name(name: &str) -> bool {
    (MIN_GUILD_NAME_CHARS..=MAX_GUILD_NAME_CHARS).contains(&name.trim().chars().count())
}

impl DiscordRest {
    pub async fn modify_guild(&self, guild_id: Id<GuildMarker>, edit: &GuildEdit) -> Result<()> {
        if edit.is_empty() {
            return Ok(());
        }

        self.send_unit(
            self.raw_http
                .patch(format!(
                    "https://discord.com/api/v9/guilds/{}",
                    guild_id.get()
                ))
                .json(&edit.to_body()),
            "modify guild",
        )
        .await
    }

    /// Set a guild's icon from an image.
    ///
    /// Separate from the rest of the edit because it needs the image read and
    /// encoded, and because failing to read a file should not abandon a name
    /// change made in the same form.
    pub async fn set_guild_icon(
        &self,
        guild_id: Id<GuildMarker>,
        image: &crate::discord::ProfileAvatarUpload,
    ) -> Result<()> {
        let data = crate::discord::upload::read_profile_avatar_image(image)
            .await
            .map_err(crate::AppError::DiscordRequest)?;
        let uri = format!(
            "data:{};base64,{}",
            data.content_type,
            base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &data.bytes)
        );

        self.send_unit(
            self.raw_http
                .patch(format!(
                    "https://discord.com/api/v9/guilds/{}",
                    guild_id.get()
                ))
                .json(&json!({ "icon": uri })),
            "set guild icon",
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_edit_is_not_sent() {
        assert!(GuildEdit::default().is_empty());
        assert!(
            !GuildEdit {
                name: Some("guild".to_owned()),
                ..GuildEdit::default()
            }
            .is_empty()
        );
    }

    #[test]
    fn an_afk_timeout_is_snapped_to_one_discord_accepts() {
        // Discord rejects anything outside its own list, so a slider or a
        // typed number would otherwise produce a request that simply fails.
        assert_eq!(nearest_afk_timeout(0), 60);
        assert_eq!(nearest_afk_timeout(200), 300);
        assert_eq!(nearest_afk_timeout(100_000), 3600);

        for accepted in AFK_TIMEOUTS {
            assert_eq!(nearest_afk_timeout(accepted), accepted);
        }
    }

    #[test]
    fn clearing_a_channel_is_distinct_from_leaving_it() {
        let cleared = GuildEdit {
            afk_channel_id: Some(None),
            ..GuildEdit::default()
        };
        assert_eq!(cleared.to_body()["afk_channel_id"], Value::Null);

        let untouched = GuildEdit {
            name: Some("guild".to_owned()),
            ..GuildEdit::default()
        };
        assert!(untouched.to_body().get("afk_channel_id").is_none());
    }

    #[test]
    fn every_level_round_trips_through_its_code() {
        // A transposed code would silently set a different verification level,
        // which is a security setting rather than a cosmetic one.
        use crate::discord::GuildVerificationLevel as Level;
        for level in [
            Level::None,
            Level::Low,
            Level::Medium,
            Level::High,
            Level::VeryHigh,
        ] {
            assert_eq!(Level::from_value(verification_code(level)), level);
        }
        // A level from a newer Discord survives the round trip rather than
        // being flattened to None, which would downgrade the setting.
        assert_eq!(verification_code(Level::Unknown(9)), 9);
        assert_eq!(Level::from_value(9), Level::Unknown(9));
        assert!(verification_label(Level::Unknown(9)).contains('9'));

        for notifications in DefaultNotifications::ALL {
            assert_eq!(
                DefaultNotifications::from_code(u64::from(notifications.code())),
                notifications
            );
        }
        for filter in ExplicitContentFilter::ALL {
            assert_eq!(
                ExplicitContentFilter::from_code(u64::from(filter.code())),
                filter
            );
        }
    }

    #[test]
    fn an_unknown_filter_code_falls_back_rather_than_panicking() {
        assert_eq!(
            ExplicitContentFilter::from_code(99),
            ExplicitContentFilter::Disabled
        );
    }

    #[test]
    fn guild_names_discord_would_reject_are_refused_here() {
        assert!(is_valid_guild_name("ab"));
        assert!(!is_valid_guild_name("a"));
        assert!(!is_valid_guild_name("   "));
        assert!(!is_valid_guild_name(&"a".repeat(101)));
    }
}
