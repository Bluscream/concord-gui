//! Creating, editing, reordering and deleting channels.
//!
//! The largest hole in server administration: everything else in a server's
//! settings assumes the channels already exist.
//!
//! Only the fields the client actually offers are modelled. Discord's create
//! and modify endpoints take around twenty each, most of them for channel
//! kinds this client does not create (store channels, media channels) or for
//! settings with no interface yet. Sending a field this client cannot show
//! would mean silently owning a setting the user cannot see or change.

use serde_json::{Map, Value, json};

use crate::Result;
use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, RoleMarker, UserMarker},
};

use super::DiscordRest;

/// Discord's caps on the fields this client edits.
pub const MAX_CHANNEL_NAME_CHARS: usize = 100;
pub const MAX_CHANNEL_TOPIC_CHARS: usize = 4096;
/// Six hours, Discord's ceiling on slowmode.
pub const MAX_SLOWMODE_SECONDS: u32 = 21_600;
/// Discord's cap on a voice channel's occupancy. Zero means no limit.
pub const MAX_VOICE_USER_LIMIT: u32 = 99;

/// The channel kinds this client can create.
///
/// Deliberately not every kind Discord has: store and directory channels are
/// legacy or first-party, and creating one would produce something the client
/// cannot then display.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NewChannelKind {
    Text,
    Voice,
    Category,
    Announcement,
    Forum,
    Stage,
}

impl NewChannelKind {
    pub const ALL: [Self; 6] = [
        Self::Text,
        Self::Voice,
        Self::Category,
        Self::Announcement,
        Self::Forum,
        Self::Stage,
    ];

    /// Discord's numeric channel type.
    pub const fn code(self) -> u8 {
        match self {
            Self::Text => 0,
            Self::Voice => 2,
            Self::Category => 4,
            Self::Announcement => 5,
            Self::Stage => 13,
            Self::Forum => 15,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Text => "Text",
            Self::Voice => "Voice",
            Self::Category => "Category",
            Self::Announcement => "Announcement",
            Self::Stage => "Stage",
            Self::Forum => "Forum",
        }
    }

    /// Whether a topic means anything for this kind.
    pub const fn has_topic(self) -> bool {
        matches!(
            self,
            Self::Text | Self::Announcement | Self::Forum | Self::Stage
        )
    }

    /// Whether bitrate and user limit mean anything for this kind.
    pub const fn is_voice(self) -> bool {
        matches!(self, Self::Voice | Self::Stage)
    }
}

/// What to change about a channel.
///
/// Every field is optional and `None` means "leave alone", which is why this
/// is not simply the channel struct: sending the whole thing back would
/// overwrite settings this client never showed.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ChannelEdit {
    pub name: Option<String>,
    /// `Some(None)` clears the topic; `None` leaves it.
    pub topic: Option<Option<String>>,
    pub nsfw: Option<bool>,
    /// Seconds between messages. Zero turns slowmode off.
    pub slowmode_seconds: Option<u32>,
    pub bitrate: Option<u32>,
    /// Zero means no limit, which is Discord's own spelling.
    pub user_limit: Option<u32>,
    /// `Some(None)` moves the channel out of its category.
    pub parent_id: Option<Option<Id<ChannelMarker>>>,
}

impl ChannelEdit {
    /// Whether this would change anything.
    ///
    /// An empty edit is not sent: it would spend a request and write an audit
    /// log entry saying nothing happened.
    pub fn is_empty(&self) -> bool {
        self.name.is_none()
            && self.topic.is_none()
            && self.nsfw.is_none()
            && self.slowmode_seconds.is_none()
            && self.bitrate.is_none()
            && self.user_limit.is_none()
            && self.parent_id.is_none()
    }

    fn to_body(&self) -> Value {
        let mut fields = Map::new();
        if let Some(name) = &self.name {
            fields.insert(
                "name".to_owned(),
                Value::from(truncate(name, MAX_CHANNEL_NAME_CHARS)),
            );
        }
        if let Some(topic) = &self.topic {
            fields.insert(
                "topic".to_owned(),
                match topic {
                    Some(text) => Value::from(truncate(text, MAX_CHANNEL_TOPIC_CHARS)),
                    None => Value::Null,
                },
            );
        }
        if let Some(nsfw) = self.nsfw {
            fields.insert("nsfw".to_owned(), Value::from(nsfw));
        }
        if let Some(slowmode) = self.slowmode_seconds {
            fields.insert(
                "rate_limit_per_user".to_owned(),
                Value::from(slowmode.min(MAX_SLOWMODE_SECONDS)),
            );
        }
        if let Some(bitrate) = self.bitrate {
            fields.insert("bitrate".to_owned(), Value::from(bitrate));
        }
        if let Some(limit) = self.user_limit {
            fields.insert(
                "user_limit".to_owned(),
                Value::from(limit.min(MAX_VOICE_USER_LIMIT)),
            );
        }
        if let Some(parent) = &self.parent_id {
            fields.insert(
                "parent_id".to_owned(),
                match parent {
                    Some(id) => Value::from(id.get().to_string()),
                    None => Value::Null,
                },
            );
        }
        Value::Object(fields)
    }
}

/// Who a permission overwrite applies to.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OverwriteTarget {
    Role(Id<RoleMarker>),
    Member(Id<UserMarker>),
}

impl OverwriteTarget {
    const fn code(self) -> u8 {
        match self {
            Self::Role(_) => 0,
            Self::Member(_) => 1,
        }
    }

    fn id(self) -> u64 {
        match self {
            Self::Role(id) => id.get(),
            Self::Member(id) => id.get(),
        }
    }
}

/// Truncate on character boundaries, not bytes.
fn truncate(text: &str, max_chars: usize) -> String {
    text.chars().take(max_chars).collect()
}

impl DiscordRest {
    /// Create a channel in a guild.
    pub async fn create_guild_channel(
        &self,
        guild_id: Id<GuildMarker>,
        name: &str,
        kind: NewChannelKind,
        parent_id: Option<Id<ChannelMarker>>,
    ) -> Result<()> {
        let mut body = json!({
            "name": truncate(name, MAX_CHANNEL_NAME_CHARS),
            "type": kind.code(),
        });
        // A category cannot live inside a category, so the parent is dropped
        // rather than sent and refused.
        if let (Some(parent_id), false, Value::Object(fields)) =
            (parent_id, kind == NewChannelKind::Category, &mut body)
        {
            fields.insert(
                "parent_id".to_owned(),
                Value::from(parent_id.get().to_string()),
            );
        }

        self.send_unit(
            self.raw_http
                .post(format!(
                    "https://discord.com/api/v9/guilds/{}/channels",
                    guild_id.get()
                ))
                .json(&body),
            "create channel",
        )
        .await
    }

    /// Change a channel's settings.
    pub async fn modify_channel(
        &self,
        channel_id: Id<ChannelMarker>,
        edit: &ChannelEdit,
    ) -> Result<()> {
        if edit.is_empty() {
            return Ok(());
        }

        self.send_unit(
            self.raw_http
                .patch(format!(
                    "https://discord.com/api/v9/channels/{}",
                    channel_id.get()
                ))
                .json(&edit.to_body()),
            "modify channel",
        )
        .await
    }

    /// Delete a channel. Deleting a category leaves its children behind.
    pub async fn delete_channel(&self, channel_id: Id<ChannelMarker>) -> Result<()> {
        self.send_unit(
            self.raw_http.delete(format!(
                "https://discord.com/api/v9/channels/{}",
                channel_id.get()
            )),
            "delete channel",
        )
        .await
    }

    /// Move channels within a guild.
    ///
    /// Sent as one request for all of them, which is what Discord's endpoint
    /// takes: moving one channel shifts the others, and one request per move
    /// would make the intermediate states visible to everyone.
    pub async fn reorder_channels(
        &self,
        guild_id: Id<GuildMarker>,
        positions: &[(Id<ChannelMarker>, u32)],
    ) -> Result<()> {
        let body: Vec<Value> = positions
            .iter()
            .map(|(channel_id, position)| {
                json!({ "id": channel_id.get().to_string(), "position": position })
            })
            .collect();

        self.send_unit(
            self.raw_http
                .patch(format!(
                    "https://discord.com/api/v9/guilds/{}/channels",
                    guild_id.get()
                ))
                .json(&body),
            "reorder channels",
        )
        .await
    }

    /// Set a permission overwrite on a channel.
    ///
    /// `allow` and `deny` are permission bitfields. A bit in neither is
    /// inherited, which is a third state the editor has to be able to express.
    pub async fn set_channel_overwrite(
        &self,
        channel_id: Id<ChannelMarker>,
        target: OverwriteTarget,
        allow: u64,
        deny: u64,
    ) -> Result<()> {
        self.send_unit(
            self.raw_http
                .put(format!(
                    "https://discord.com/api/v9/channels/{}/permissions/{}",
                    channel_id.get(),
                    target.id()
                ))
                .json(&json!({
                    // Discord takes the bitfields as strings, since they
                    // exceed what JSON numbers hold safely.
                    "allow": allow.to_string(),
                    "deny": deny.to_string(),
                    "type": target.code(),
                })),
            "set channel permissions",
        )
        .await
    }

    /// Remove an overwrite, returning that target to inherited permissions.
    pub async fn delete_channel_overwrite(
        &self,
        channel_id: Id<ChannelMarker>,
        target: OverwriteTarget,
    ) -> Result<()> {
        self.send_unit(
            self.raw_http.delete(format!(
                "https://discord.com/api/v9/channels/{}/permissions/{}",
                channel_id.get(),
                target.id()
            )),
            "remove channel permissions",
        )
        .await
    }

    /// Set the short status line on a voice channel.
    pub async fn set_voice_channel_status(
        &self,
        channel_id: Id<ChannelMarker>,
        status: Option<&str>,
    ) -> Result<()> {
        self.send_unit(
            self.raw_http
                .put(format!(
                    "https://discord.com/api/v9/channels/{}/voice-status",
                    channel_id.get()
                ))
                .json(&json!({ "status": status })),
            "set voice channel status",
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_edit_is_not_sent() {
        // It would spend a request and write an audit log entry saying that
        // nothing happened.
        assert!(ChannelEdit::default().is_empty());
        assert!(
            !ChannelEdit {
                name: Some("general".to_owned()),
                ..ChannelEdit::default()
            }
            .is_empty()
        );
    }

    #[test]
    fn clearing_a_topic_is_distinct_from_leaving_it() {
        // Some(None) sends null and clears it; None omits the field entirely,
        // which is what stops an edit overwriting settings never shown.
        let cleared = ChannelEdit {
            topic: Some(None),
            ..ChannelEdit::default()
        };
        assert_eq!(cleared.to_body()["topic"], Value::Null);

        let untouched = ChannelEdit {
            name: Some("general".to_owned()),
            ..ChannelEdit::default()
        };
        assert!(untouched.to_body().get("topic").is_none());
    }

    #[test]
    fn limits_are_clamped_rather_than_rejected_by_discord() {
        let edit = ChannelEdit {
            slowmode_seconds: Some(u32::MAX),
            user_limit: Some(500),
            ..ChannelEdit::default()
        };
        let body = edit.to_body();

        assert_eq!(
            body["rate_limit_per_user"],
            Value::from(MAX_SLOWMODE_SECONDS)
        );
        assert_eq!(body["user_limit"], Value::from(MAX_VOICE_USER_LIMIT));
    }

    #[test]
    fn a_name_is_truncated_on_character_boundaries() {
        // Byte truncation would split a multi-byte character and produce a
        // name Discord rejects as invalid UTF-8.
        let edit = ChannelEdit {
            name: Some("é".repeat(200)),
            ..ChannelEdit::default()
        };

        let name = edit.to_body()["name"].as_str().unwrap().to_owned();
        assert_eq!(name.chars().count(), MAX_CHANNEL_NAME_CHARS);
    }

    #[test]
    fn moving_a_channel_out_of_a_category_is_distinct_from_leaving_it() {
        let out = ChannelEdit {
            parent_id: Some(None),
            ..ChannelEdit::default()
        };
        assert_eq!(out.to_body()["parent_id"], Value::Null);

        let into = ChannelEdit {
            parent_id: Some(Some(Id::new(42))),
            ..ChannelEdit::default()
        };
        assert_eq!(into.to_body()["parent_id"], Value::from("42"));
    }

    #[test]
    fn every_creatable_kind_has_a_distinct_code() {
        let mut codes: Vec<u8> = NewChannelKind::ALL.iter().map(|kind| kind.code()).collect();
        codes.sort_unstable();
        codes.dedup();

        assert_eq!(codes.len(), NewChannelKind::ALL.len());
    }

    #[test]
    fn only_the_kinds_that_have_one_offer_a_topic_or_bitrate() {
        assert!(NewChannelKind::Text.has_topic());
        assert!(!NewChannelKind::Voice.has_topic());
        assert!(!NewChannelKind::Category.has_topic());

        assert!(NewChannelKind::Voice.is_voice());
        assert!(NewChannelKind::Stage.is_voice());
        assert!(!NewChannelKind::Text.is_voice());
    }
}
