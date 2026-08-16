//! AutoMod rules.
//!
//! Listing, toggling and deleting. Creating a rule with full trigger metadata -
//! keyword lists, regex patterns, mention caps - is a form of its own and is
//! not built yet; what is here is what a moderator does day to day, which is
//! look at the rules and turn one off.

use serde::Deserialize;
use serde_json::json;

use crate::Result;
use crate::discord::ids::{Id, marker::GuildMarker};

use super::DiscordRest;

/// What makes a rule fire.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AutoModTrigger {
    /// A user-defined keyword list.
    Keyword,
    /// Generic spam, as Discord judges it.
    Spam,
    /// One of Discord's own wordsets.
    KeywordPreset,
    /// More unique mentions than allowed.
    MentionSpam,
    /// Applied to profiles rather than messages.
    UserProfile,
    /// A trigger this build does not know. Discord adds them, and showing an
    /// unexplained rule beats hiding one that is really in force.
    Unknown(u64),
}

impl AutoModTrigger {
    pub const fn from_code(code: u64) -> Self {
        match code {
            1 => Self::Keyword,
            3 => Self::Spam,
            4 => Self::KeywordPreset,
            5 => Self::MentionSpam,
            6 => Self::UserProfile,
            other => Self::Unknown(other),
        }
    }

    pub fn label(self) -> String {
        match self {
            Self::Keyword => "Keywords".to_owned(),
            Self::Spam => "Spam".to_owned(),
            Self::KeywordPreset => "Discord's wordlists".to_owned(),
            Self::MentionSpam => "Mention spam".to_owned(),
            Self::UserProfile => "Profile content".to_owned(),
            Self::Unknown(code) => format!("Unrecognised trigger {code}"),
        }
    }
}

/// What a rule does when it fires.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AutoModAction {
    BlockMessage,
    SendAlert,
    TimeoutUser,
    QuarantineUser,
    Unknown(u64),
}

impl AutoModAction {
    pub const fn from_code(code: u64) -> Self {
        match code {
            1 => Self::BlockMessage,
            2 => Self::SendAlert,
            3 => Self::TimeoutUser,
            4 => Self::QuarantineUser,
            other => Self::Unknown(other),
        }
    }

    pub fn label(self) -> String {
        match self {
            Self::BlockMessage => "block the message".to_owned(),
            Self::SendAlert => "send an alert".to_owned(),
            Self::TimeoutUser => "time the member out".to_owned(),
            Self::QuarantineUser => "quarantine the member".to_owned(),
            Self::Unknown(code) => format!("do something (action {code})"),
        }
    }
}

/// One AutoMod rule, as the panel shows it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AutoModRule {
    pub id: u64,
    pub name: String,
    pub enabled: bool,
    pub trigger: AutoModTrigger,
    pub actions: Vec<AutoModAction>,
}

impl AutoModRule {
    /// A one-line summary: what fires it, and what it then does.
    pub fn summary(&self) -> String {
        let actions: Vec<String> = self.actions.iter().map(|action| action.label()).collect();
        let actions = if actions.is_empty() {
            "do nothing".to_owned()
        } else {
            actions.join(", ")
        };
        format!("{} - {actions}", self.trigger.label())
    }
}

#[derive(Deserialize)]
struct RuleBody {
    id: Option<String>,
    name: Option<String>,
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    trigger_type: u64,
    #[serde(default)]
    actions: Vec<ActionBody>,
}

#[derive(Deserialize)]
struct ActionBody {
    #[serde(default)]
    #[serde(rename = "type")]
    kind: u64,
}

impl DiscordRest {
    /// Every AutoMod rule in a guild.
    pub async fn automod_rules(&self, guild_id: Id<GuildMarker>) -> Result<Vec<AutoModRule>> {
        let rules: Vec<RuleBody> = self
            .send_json(
                self.raw_http.get(format!(
                    "https://discord.com/api/v9/guilds/{}/auto-moderation/rules",
                    guild_id.get()
                )),
                "automod rules",
            )
            .await?;

        Ok(rules
            .into_iter()
            .filter_map(|rule| {
                // A rule with no id cannot be toggled or deleted, so there is
                // nothing a row for it could do.
                let id = rule.id?.parse::<u64>().ok()?;
                Some(AutoModRule {
                    id,
                    name: rule.name.unwrap_or_default(),
                    enabled: rule.enabled,
                    trigger: AutoModTrigger::from_code(rule.trigger_type),
                    actions: rule
                        .actions
                        .into_iter()
                        .map(|action| AutoModAction::from_code(action.kind))
                        .collect(),
                })
            })
            .collect())
    }

    /// Turn a rule on or off.
    ///
    /// Toggling rather than deleting is what a moderator usually wants: a rule
    /// switched off can be switched back on, and its keyword list survives.
    pub async fn set_automod_rule_enabled(
        &self,
        guild_id: Id<GuildMarker>,
        rule_id: u64,
        enabled: bool,
    ) -> Result<()> {
        self.send_unit(
            self.raw_http
                .patch(format!(
                    "https://discord.com/api/v9/guilds/{}/auto-moderation/rules/{rule_id}",
                    guild_id.get()
                ))
                .json(&json!({ "enabled": enabled })),
            "toggle automod rule",
        )
        .await
    }

    pub async fn delete_automod_rule(&self, guild_id: Id<GuildMarker>, rule_id: u64) -> Result<()> {
        self.send_unit(
            self.raw_http.delete(format!(
                "https://discord.com/api/v9/guilds/{}/auto-moderation/rules/{rule_id}",
                guild_id.get()
            )),
            "delete automod rule",
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(trigger: AutoModTrigger, actions: Vec<AutoModAction>) -> AutoModRule {
        AutoModRule {
            id: 1,
            name: "no links".to_owned(),
            enabled: true,
            trigger,
            actions,
        }
    }

    #[test]
    fn a_rule_says_what_fires_it_and_what_it_does() {
        let summary = rule(
            AutoModTrigger::Keyword,
            vec![AutoModAction::BlockMessage, AutoModAction::TimeoutUser],
        )
        .summary();

        assert!(summary.contains("Keywords"));
        assert!(summary.contains("block the message"));
        assert!(summary.contains("time the member out"));
    }

    #[test]
    fn a_rule_with_no_actions_says_so_rather_than_reading_as_broken() {
        // Discord allows it, and an empty half-sentence would look like a
        // rendering fault rather than a rule that does nothing.
        let summary = rule(AutoModTrigger::Spam, Vec::new()).summary();
        assert!(summary.contains("do nothing"), "got {summary:?}");
    }

    #[test]
    fn unrecognised_triggers_and_actions_are_shown_rather_than_hidden() {
        // Discord adds these. A rule nobody can name is still in force, and
        // hiding it would mean a moderator cannot see what is filtering their
        // server.
        let summary = rule(AutoModTrigger::Unknown(9), vec![AutoModAction::Unknown(7)]).summary();

        assert!(summary.contains('9'));
        assert!(summary.contains('7'));
    }

    #[test]
    fn known_codes_map_to_the_right_trigger() {
        // A transposed code would describe a rule as doing something other
        // than what it does.
        assert_eq!(AutoModTrigger::from_code(1), AutoModTrigger::Keyword);
        assert_eq!(AutoModTrigger::from_code(3), AutoModTrigger::Spam);
        assert_eq!(AutoModTrigger::from_code(4), AutoModTrigger::KeywordPreset);
        assert_eq!(AutoModTrigger::from_code(5), AutoModTrigger::MentionSpam);
        // 2 is absent from Discord's own table.
        assert_eq!(AutoModTrigger::from_code(2), AutoModTrigger::Unknown(2));

        assert_eq!(AutoModAction::from_code(1), AutoModAction::BlockMessage);
        assert_eq!(AutoModAction::from_code(3), AutoModAction::TimeoutUser);
    }
}
