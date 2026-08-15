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

use crate::model::markdown::{self, Mentions, Parsed};
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

/// A poll, flattened for rendering.
pub struct PollRow {
    pub question: String,
    pub answers: Vec<PollAnswerRow>,
    pub multiselect: bool,
    /// True once voting has closed and results are final.
    pub finalized: bool,
    pub total_votes: u64,
    /// Whether this user has voted, which decides whether counts are shown.
    pub voted: bool,
}

pub struct PollAnswerRow {
    pub answer_id: u8,
    pub text: String,
    pub votes: u64,
    pub mine: bool,
    /// Share of the total, 0.0 to 1.0, for the result bar.
    pub share: f32,
}

fn project_poll(poll: &concord::discord::PollInfo) -> PollRow {
    let total = poll.total_votes.unwrap_or_else(|| {
        poll.answers
            .iter()
            .map(|answer| answer.vote_count.unwrap_or(0))
            .sum()
    });

    let voted = poll.answers.iter().any(|answer| answer.me_voted);

    PollRow {
        question: poll.question.clone(),
        multiselect: poll.allow_multiselect,
        finalized: poll.results_finalized.unwrap_or(false),
        total_votes: total,
        voted,
        answers: poll
            .answers
            .iter()
            .map(|answer| {
                let votes = answer.vote_count.unwrap_or(0);
                PollAnswerRow {
                    answer_id: answer.answer_id,
                    text: answer.text.clone(),
                    votes,
                    mine: answer.me_voted,
                    // Guard against a zero total: an empty poll would divide
                    // by zero and render a NaN-width bar.
                    share: if total == 0 {
                        0.0
                    } else {
                        votes as f32 / total as f32
                    },
                }
            })
            .collect(),
    }
}

/// One attachment, flattened for rendering.
pub struct AttachmentRow {
    pub filename: String,
    pub size_bytes: u64,
    pub is_image: bool,
    /// CDN source. Empty for demo attachments, which were never uploaded.
    pub url: String,
    /// Whether an external player could open this.
    pub is_playable: bool,
}

/// A single rendered message row.
pub struct MessageRow {
    pub id: Id<marker::MessageMarker>,
    pub author: String,
    pub author_id: Id<marker::UserMarker>,
    pub author_is_bot: bool,
    pub author_avatar: Option<String>,
    /// Role colour as packed RGB, when the author has a coloured role.
    pub author_color: Option<u32>,
    /// Raw source, kept for edit prefill.
    pub content: String,
    /// Parsed body with mentions resolved, ready to render.
    pub body: Parsed,
    pub timestamp: DateTime<Local>,
    /// True when this row continues the previous author's block: the header
    /// is suppressed and only the body indents.
    pub continues: bool,
    pub edited: bool,
    pub pinned: bool,
    /// Author, content and id of the message this one replies to. The id lets
    /// the reply context be clicked through to its target.
    pub reply_to: Option<(String, String, Option<Id<marker::MessageMarker>>)>,
    pub attachments: Vec<AttachmentRow>,
    /// `(emoji, count, me_reacted)`.
    pub reactions: Vec<(String, u64, bool)>,
    pub embed_count: usize,
    /// Poll attached to this message, if any.
    pub poll: Option<PollRow>,
    /// Set once the user clicks a hidden spoiler in this message.
    pub spoiler_revealed: bool,
    /// Links found in the body, in order, so they can be opened.
    pub links: Vec<String>,
    /// Whether the authenticated user wrote this message, which gates the
    /// edit and delete actions.
    pub own: bool,
}

impl MessageRow {
    /// Time of day for the gutter column.
    pub fn short_time(&self, hour24: bool) -> String {
        let format = if hour24 { "%H:%M" } else { "%l:%M %p" };
        self.timestamp.format(format).to_string().trim().to_string()
    }

    /// Full timestamp for the group header.
    pub fn long_time(&self, hour24: bool) -> String {
        let format = if hour24 {
            "%Y-%m-%d %H:%M"
        } else {
            "%Y-%m-%d %l:%M %p"
        };
        self.timestamp.format(format).to_string().trim().to_string()
    }
}

/// Project the message cache for a channel into renderable rows, oldest first.
pub fn project_messages(
    state: &DiscordState,
    channel_id: Id<marker::ChannelMarker>,
    current_user: Option<Id<marker::UserMarker>>,
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
            author_id: message.author_id,
            author_is_bot: message.author_is_bot,
            author_avatar: message.author_avatar_url.clone(),
            author_color: author_color(state, message),
            content: message.content.clone().unwrap_or_default(),
            body: markdown::parse_with(
                message.content.as_deref().unwrap_or_default(),
                &GuildMentions {
                    state,
                    guild_id: message.guild_id,
                },
            ),
            timestamp: created.with_timezone(&Local),
            continues,
            edited: message.edited_timestamp.is_some(),
            pinned: message.pinned,
            reply_to: message.reply.as_ref().map(|reply| {
                (
                    reply.author.clone(),
                    reply.content.clone().unwrap_or_default(),
                    // The referenced id lives on the reference, not the reply
                    // preview; without it the context cannot be jumped to.
                    message
                        .reference
                        .as_ref()
                        .and_then(|reference| reference.message_id),
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
                    url: attachment.url.clone(),
                    is_playable: attachment.content_type.as_deref().is_some_and(|kind| {
                        kind.starts_with("video/")
                            || kind.starts_with("audio/")
                            || kind.starts_with("image/")
                    }),
                })
                .collect(),
            reactions: message
                .reactions
                .iter()
                .map(|reaction| (reaction_glyph(&reaction.emoji), reaction.count, reaction.me))
                .collect(),
            embed_count: message.embeds.len(),
            poll: message.poll.as_ref().map(project_poll),
            spoiler_revealed: false,
            links: Vec::new(),
            own: current_user == Some(message.author_id),
        });

        // Links are read back off the parsed body, so what is openable is
        // exactly what was rendered as a link.
        if let Some(row) = rows.last_mut() {
            row.links = row
                .body
                .runs
                .iter()
                .filter(|(_, style)| style.kind == markdown::Kind::Url)
                .map(|(range, _)| row.body.text[range.clone()].to_string())
                .collect();
        }

        previous = Some((message.author_id, created));
    }

    rows
}

/// Renderable glyph for a reaction. Custom emoji have no text form, so the
/// name is shown in colons; inline custom-emoji images are not rendered.
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

/// Resolves mention targets against the state store.
///
/// Users are looked up in the message's own guild first; falling back to a
/// bare snowflake is deliberate, since inventing a name for an uncached user
/// would be worse than showing the id.
struct GuildMentions<'a> {
    state: &'a DiscordState,
    guild_id: Option<Id<marker::GuildMarker>>,
}

impl Mentions for GuildMentions<'_> {
    fn user(&self, id: u64) -> Option<String> {
        let guild_id = self.guild_id?;
        self.state
            .member_for_guild(guild_id, Id::new(id))
            .map(|member| member.display_name.clone())
    }

    fn channel(&self, id: u64) -> Option<String> {
        self.state
            .channel(Id::new(id))
            .map(|channel| channel.name.clone())
    }

    fn role(&self, id: u64) -> Option<String> {
        let guild_id = self.guild_id?;
        self.state
            .role_for_guild(guild_id, Id::new(id))
            .map(|role| role.name.clone())
    }

    fn emoji(&self, id: u64) -> Option<String> {
        self.state
            .custom_emojis()
            .find(|emoji| emoji.id.get() == id)
            .map(|emoji| emoji.name.clone())
    }
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
