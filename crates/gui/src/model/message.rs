//! Message projection: `MessageState` -> renderable rows.
//!
//! Two things happen here that the core deliberately leaves to a front-end:
//!
//! * **Timestamps.** `MessageState` carries no timestamp field; Discord
//!   snowflakes encode creation time in their high bits, so it is derived.
//! * **Grouping.** Consecutive messages from the same author within a short
//!   window render as one block with a single header, which is what makes a
//!   chat log readable rather than a wall of repeated names.

use chrono::{DateTime, Local, TimeZone, Utc};
use concord::discord::{DiscordState, Id, MessageState, ReactionEmoji, marker};

/// Discord epoch (2015-01-01T00:00:00Z) in milliseconds.
const DISCORD_EPOCH_MS: u64 = 1_420_070_400_000;

/// Messages closer together than this by the same author are grouped.
const GROUP_WINDOW_SECS: i64 = 7 * 60;

/// Derive creation time from a snowflake.
pub fn snowflake_time(id: u64) -> DateTime<Utc> {
    let millis = (id >> 22) + DISCORD_EPOCH_MS;
    Utc.timestamp_millis_opt(millis as i64)
        .single()
        .unwrap_or_else(Utc::now)
}

/// One attachment, flattened for rendering.
pub struct AttachmentRow {
    pub filename: String,
    pub size_bytes: u64,
    pub is_image: bool,
}

/// A single rendered message row.
pub struct MessageRow {
    pub id: Id<marker::MessageMarker>,
    pub author: String,
    pub author_is_bot: bool,
    /// Role colour as packed RGB, when the author has a coloured role.
    pub author_color: Option<u32>,
    pub content: String,
    pub timestamp: DateTime<Local>,
    /// True when this row continues the previous author's block: the header
    /// is suppressed and only the body indents.
    pub continues: bool,
    pub edited: bool,
    pub pinned: bool,
    /// Author and content of the message this one replies to.
    pub reply_to: Option<(String, String)>,
    pub attachments: Vec<AttachmentRow>,
    /// `(emoji, count, me_reacted)`.
    pub reactions: Vec<(String, u64, bool)>,
    pub embed_count: usize,
}

impl MessageRow {
    /// `HH:MM` for the group header / hover column.
    pub fn short_time(&self) -> String {
        self.timestamp.format("%H:%M").to_string()
    }

    /// Full timestamp for the group header.
    pub fn long_time(&self) -> String {
        self.timestamp.format("%Y-%m-%d %H:%M").to_string()
    }
}

/// Project the message cache for a channel into renderable rows, oldest first.
pub fn project_messages(
    state: &DiscordState,
    channel_id: Id<marker::ChannelMarker>,
) -> Vec<MessageRow> {
    let messages = state.messages_for_channel(channel_id);

    let mut rows: Vec<MessageRow> = Vec::with_capacity(messages.len());
    let mut previous: Option<(Id<marker::UserMarker>, DateTime<Utc>)> = None;

    for message in messages {
        let created = snowflake_time(message.id.get());

        let continues = match previous {
            Some((author_id, previous_time)) => {
                author_id == message.author_id
                    && (created - previous_time).num_seconds() < GROUP_WINDOW_SECS
                    && message.reply.is_none()
            }
            None => false,
        };

        rows.push(MessageRow {
            id: message.id,
            author: display_author(message),
            author_is_bot: message.author_is_bot,
            author_color: author_color(state, message),
            content: message.content.clone().unwrap_or_default(),
            timestamp: created.with_timezone(&Local),
            continues,
            edited: message.edited_timestamp.is_some(),
            pinned: message.pinned,
            reply_to: message.reply.as_ref().map(|reply| {
                (
                    reply.author.clone(),
                    reply.content.clone().unwrap_or_default(),
                )
            }),
            attachments: message
                .attachments
                .iter()
                .map(|attachment| AttachmentRow {
                    filename: attachment.filename.clone(),
                    size_bytes: attachment.size,
                    is_image: attachment
                        .content_type
                        .as_deref()
                        .is_some_and(|kind| kind.starts_with("image/")),
                })
                .collect(),
            reactions: message
                .reactions
                .iter()
                .map(|reaction| (reaction_glyph(&reaction.emoji), reaction.count, reaction.me))
                .collect(),
            embed_count: message.embeds.len(),
        });

        previous = Some((message.author_id, created));
    }

    rows
}

/// Renderable glyph for a reaction. Custom emoji have no text form, so the
/// name is shown in colons until image rendering lands.
fn reaction_glyph(emoji: &ReactionEmoji) -> String {
    match emoji {
        ReactionEmoji::Unicode(text) => text.clone(),
        ReactionEmoji::Custom { name, .. } => {
            format!(":{}:", name.clone().unwrap_or_else(|| "emoji".to_string()))
        }
    }
}

fn display_author(message: &MessageState) -> String {
    if message.author.is_empty() {
        "unknown".to_string()
    } else {
        message.author.clone()
    }
}

fn author_color(state: &DiscordState, message: &MessageState) -> Option<u32> {
    let guild_id = message.guild_id?;
    state
        .message_author_role_color(guild_id, message.channel_id, message.id, message.author_id)
        .filter(|color| *color != 0)
}

/// Human-readable byte size for attachment rows.
pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[0])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}
