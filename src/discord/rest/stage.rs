//! Stage instances, and asking to speak on one.
//!
//! A stage channel is a voice channel with an audience. The *instance* is the
//! live session in it - its topic, and the fact that it is running at all. A
//! stage channel with no instance is a room nobody has opened yet.

use serde::Deserialize;
use serde_json::json;

use crate::Result;
use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, UserMarker},
};

use super::DiscordRest;

/// Discord's caps on a stage topic.
pub const MIN_STAGE_TOPIC_CHARS: usize = 1;
pub const MAX_STAGE_TOPIC_CHARS: usize = 120;

/// Who gets told a stage has started.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StagePrivacy {
    /// Members of this server only. The only level Discord still accepts;
    /// public stages were removed.
    GuildOnly,
}

impl StagePrivacy {
    pub const fn code(self) -> u8 {
        match self {
            Self::GuildOnly => 2,
        }
    }
}

/// A live stage.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StageInstance {
    pub id: u64,
    pub channel_id: Id<ChannelMarker>,
    pub topic: String,
}

/// Whether Discord would accept this as a stage topic.
///
/// Checked here so a rejected start costs no round trip, and so the reason is
/// specific rather than Discord's generic complaint.
pub fn stage_topic_problem(topic: &str) -> Option<&'static str> {
    let count = topic.trim().chars().count();
    if count < MIN_STAGE_TOPIC_CHARS {
        return Some("needs a topic");
    }
    if count > MAX_STAGE_TOPIC_CHARS {
        return Some("topic is too long");
    }
    None
}

#[derive(Deserialize)]
struct StageInstanceBody {
    id: Option<String>,
    channel_id: Option<String>,
    topic: Option<String>,
}

impl DiscordRest {
    /// The live stage in this channel, if one is running.
    ///
    /// `None` rather than an error when there is none: a stage channel nobody
    /// has opened is an ordinary state, not a failure.
    pub async fn stage_instance(
        &self,
        channel_id: Id<ChannelMarker>,
    ) -> Result<Option<StageInstance>> {
        // Discord answers 404 for a stage nobody has started, which is not an
        // error worth showing anyone - so a failure here reads as "none".
        let body: Option<StageInstanceBody> = self
            .send_json::<StageInstanceBody>(
                self.raw_http.get(format!(
                    "https://discord.com/api/v9/stage-instances/{}",
                    channel_id.get()
                )),
                "stage instance",
            )
            .await
            .ok();

        Ok(body.and_then(|body| {
            Some(StageInstance {
                id: body.id?.parse().ok()?,
                channel_id: body
                    .channel_id
                    .and_then(|id| id.parse::<u64>().ok())
                    .map_or(channel_id, Id::new),
                topic: body.topic.unwrap_or_default(),
            })
        }))
    }

    /// Open a stage with a topic.
    pub async fn start_stage_instance(
        &self,
        channel_id: Id<ChannelMarker>,
        topic: &str,
        privacy: StagePrivacy,
    ) -> Result<()> {
        self.send_unit(
            self.raw_http
                .post("https://discord.com/api/v9/stage-instances")
                .json(&json!({
                    "channel_id": channel_id.get().to_string(),
                    "topic": topic.chars().take(MAX_STAGE_TOPIC_CHARS).collect::<String>(),
                    "privacy_level": privacy.code(),
                })),
            "start stage",
        )
        .await
    }

    pub async fn modify_stage_topic(
        &self,
        channel_id: Id<ChannelMarker>,
        topic: &str,
    ) -> Result<()> {
        self.send_unit(
            self.raw_http
                .patch(format!(
                    "https://discord.com/api/v9/stage-instances/{}",
                    channel_id.get()
                ))
                .json(&json!({
                    "topic": topic.chars().take(MAX_STAGE_TOPIC_CHARS).collect::<String>(),
                })),
            "stage topic",
        )
        .await
    }

    /// Close the stage. The channel stays; the session ends.
    pub async fn end_stage_instance(&self, channel_id: Id<ChannelMarker>) -> Result<()> {
        self.send_unit(
            self.raw_http.delete(format!(
                "https://discord.com/api/v9/stage-instances/{}",
                channel_id.get()
            )),
            "end stage",
        )
        .await
    }

    /// Raise or lower your hand.
    ///
    /// Discord models this as a voice-state field rather than an endpoint of
    /// its own: setting a request timestamp is the request, and clearing it
    /// withdraws it.
    pub async fn request_to_speak(
        &self,
        guild_id: Id<GuildMarker>,
        channel_id: Id<ChannelMarker>,
        requesting: bool,
    ) -> Result<()> {
        self.send_unit(
            self.raw_http
                .patch(format!(
                    "https://discord.com/api/v9/guilds/{}/voice-states/@me",
                    guild_id.get()
                ))
                .json(&json!({
                    "channel_id": channel_id.get().to_string(),
                    // Null withdraws. Discord accepts any timestamp for the
                    // request itself; it orders the queue by arrival.
                    "request_to_speak_timestamp": requesting.then_some("1970-01-01T00:00:00Z"),
                })),
            "request to speak",
        )
        .await
    }

    /// Invite someone in the audience to speak, or move them back down.
    ///
    /// `suppress` is Discord's word for "in the audience": a suppressed member
    /// is one whose microphone does nothing.
    pub async fn set_stage_speaker(
        &self,
        guild_id: Id<GuildMarker>,
        channel_id: Id<ChannelMarker>,
        user_id: Id<UserMarker>,
        speaking: bool,
    ) -> Result<()> {
        self.send_unit(
            self.raw_http
                .patch(format!(
                    "https://discord.com/api/v9/guilds/{}/voice-states/{}",
                    guild_id.get(),
                    user_id.get()
                ))
                .json(&json!({
                    "channel_id": channel_id.get().to_string(),
                    "suppress": !speaking,
                })),
            "stage speaker",
        )
        .await
    }
}

/// What submitting a stage topic should send.
///
/// Three endpoints, and picking the wrong one always fails: Discord's start
/// rejects a running stage, its patch rejects one that is not running, and an
/// emptied topic means end rather than either. The rule lives here so both
/// clients decide it the same way.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StageAction {
    Start,
    ChangeTopic,
    End,
}

pub fn stage_action_for(topic: &str, already_running: bool) -> StageAction {
    if topic.trim().is_empty() {
        return StageAction::End;
    }
    if already_running {
        StageAction::ChangeTopic
    } else {
        StageAction::Start
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stage_needs_a_topic_and_discord_caps_its_length() {
        // Refused locally so a rejected start costs no round trip, and so the
        // reason names the field rather than being Discord's generic error.
        assert_eq!(stage_topic_problem("Book club"), None);
        assert!(stage_topic_problem("").is_some());
        assert!(stage_topic_problem("   ").is_some());
        assert!(stage_topic_problem(&"a".repeat(MAX_STAGE_TOPIC_CHARS + 1)).is_some());
    }

    #[test]
    fn topic_length_is_counted_in_characters_not_bytes() {
        // A topic of multi-byte characters would otherwise be refused while
        // being well within Discord's limit.
        assert_eq!(
            stage_topic_problem(&"é".repeat(MAX_STAGE_TOPIC_CHARS)),
            None
        );
        assert!(stage_topic_problem(&"é".repeat(MAX_STAGE_TOPIC_CHARS + 1)).is_some());
    }

    #[test]
    fn the_endpoint_matches_whether_a_stage_is_already_running() {
        // Picking the wrong one always fails: start rejects a running stage,
        // patch rejects one that is not, and the two are indistinguishable
        // from the form alone.
        assert_eq!(stage_action_for("Book club", false), StageAction::Start);
        assert_eq!(
            stage_action_for("Book club", true),
            StageAction::ChangeTopic
        );
    }

    #[test]
    fn an_emptied_topic_ends_the_stage_whether_or_not_it_was_running() {
        // Ending is the only way to close a stage from a form with no button
        // of its own, and "  " is emptying it rather than a topic of spaces.
        assert_eq!(stage_action_for("", true), StageAction::End);
        assert_eq!(stage_action_for("   ", true), StageAction::End);
        assert_eq!(stage_action_for("", false), StageAction::End);
    }

    #[test]
    fn the_only_privacy_level_is_the_one_discord_still_accepts() {
        // Public stages were removed; sending 1 is rejected outright.
        assert_eq!(StagePrivacy::GuildOnly.code(), 2);
    }
}
