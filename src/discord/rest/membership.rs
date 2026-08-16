//! Pruning, the welcome screen, and the guild widget.
//!
//! Three things a server owner reaches for that share nothing technically but
//! sit together in Discord's own settings, under how members arrive and leave.

use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::Result;
use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, RoleMarker},
};

use super::DiscordRest;

/// The inactivity windows Discord accepts for a prune. Anything else is
/// rejected, so a typed number is snapped to one of these.
pub const PRUNE_DAYS: [u16; 5] = [1, 7, 14, 30, 90];

/// The accepted window closest to what was asked for.
pub fn nearest_prune_days(days: u16) -> u16 {
    PRUNE_DAYS
        .into_iter()
        .min_by_key(|candidate| candidate.abs_diff(days))
        .unwrap_or(7)
}

/// Discord's cap on the welcome-screen description.
pub const MAX_WELCOME_DESCRIPTION_CHARS: usize = 140;
/// Discord's cap on the number of channels the welcome screen may feature.
pub const MAX_WELCOME_CHANNELS: usize = 5;

/// One channel featured on the welcome screen.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WelcomeChannel {
    pub channel_id: Id<ChannelMarker>,
    pub description: String,
    pub emoji_name: Option<String>,
}

/// The welcome screen as it stands.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WelcomeScreen {
    pub enabled: bool,
    pub description: Option<String>,
    pub channels: Vec<WelcomeChannel>,
}

/// What to change about it. `None` means leave alone, as elsewhere: this
/// endpoint replaces what it is given.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WelcomeScreenEdit {
    pub enabled: Option<bool>,
    /// `Some(None)` clears the description.
    pub description: Option<Option<String>>,
    pub channels: Option<Vec<WelcomeChannel>>,
}

impl WelcomeScreenEdit {
    pub fn is_empty(&self) -> bool {
        self.enabled.is_none() && self.description.is_none() && self.channels.is_none()
    }

    fn to_body(&self) -> Value {
        let mut fields = Map::new();
        if let Some(enabled) = self.enabled {
            fields.insert("enabled".to_owned(), Value::Bool(enabled));
        }
        if let Some(description) = &self.description {
            fields.insert(
                "description".to_owned(),
                match description {
                    Some(text) => Value::from(
                        text.chars()
                            .take(MAX_WELCOME_DESCRIPTION_CHARS)
                            .collect::<String>(),
                    ),
                    None => Value::Null,
                },
            );
        }
        if let Some(channels) = &self.channels {
            fields.insert(
                "welcome_channels".to_owned(),
                Value::Array(
                    channels
                        .iter()
                        // Discord rejects more than five, so the extras are
                        // dropped here rather than the whole edit failing.
                        .take(MAX_WELCOME_CHANNELS)
                        .map(|channel| {
                            json!({
                                "channel_id": channel.channel_id.get().to_string(),
                                "description": channel.description,
                                "emoji_name": channel.emoji_name,
                            })
                        })
                        .collect(),
                ),
            );
        }
        Value::Object(fields)
    }
}

/// The guild widget - the embeddable panel and its invite.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GuildWidget {
    pub enabled: bool,
    /// Which channel the widget's invite points at. `None` means it issues no
    /// invite, which is a meaningful state rather than a missing value.
    pub channel_id: Option<Id<ChannelMarker>>,
}

#[derive(Deserialize)]
struct PruneCountBody {
    pruned: Option<u64>,
}

#[derive(Deserialize)]
struct WelcomeScreenBody {
    #[serde(default)]
    enabled: bool,
    description: Option<String>,
    #[serde(default)]
    welcome_channels: Vec<WelcomeChannelBody>,
}

#[derive(Deserialize)]
struct WelcomeChannelBody {
    channel_id: Option<String>,
    description: Option<String>,
    emoji_name: Option<String>,
}

#[derive(Deserialize)]
struct WidgetBody {
    #[serde(default)]
    enabled: bool,
    channel_id: Option<String>,
}

impl DiscordRest {
    /// How many members a prune would remove.
    ///
    /// Always asked before pruning: the count is the only thing that makes an
    /// irreversible action reviewable beforehand.
    pub async fn prune_count(
        &self,
        guild_id: Id<GuildMarker>,
        days: u16,
        include_roles: &[Id<RoleMarker>],
    ) -> Result<u64> {
        let mut url = format!(
            "https://discord.com/api/v9/guilds/{}/prune?days={}",
            guild_id.get(),
            nearest_prune_days(days)
        );
        if !include_roles.is_empty() {
            let roles: Vec<String> = include_roles
                .iter()
                .map(|id| id.get().to_string())
                .collect();
            url.push_str(&format!("&include_roles={}", roles.join(",")));
        }

        let body: PruneCountBody = self
            .send_json(self.raw_http.get(url), "prune count")
            .await?;
        Ok(body.pruned.unwrap_or(0))
    }

    /// Remove inactive members.
    ///
    /// `include_roles` is required to reach anyone with a role at all: Discord
    /// exempts every member who has one unless their role is named here, which
    /// is why a prune so often reports zero.
    pub async fn prune_guild(
        &self,
        guild_id: Id<GuildMarker>,
        days: u16,
        include_roles: &[Id<RoleMarker>],
    ) -> Result<u64> {
        let roles: Vec<String> = include_roles
            .iter()
            .map(|id| id.get().to_string())
            .collect();
        let body: PruneCountBody = self
            .send_json(
                self.raw_http
                    .post(format!(
                        "https://discord.com/api/v9/guilds/{}/prune",
                        guild_id.get()
                    ))
                    .json(&json!({
                        "days": nearest_prune_days(days),
                        "compute_prune_count": true,
                        "include_roles": roles,
                    })),
                "prune",
            )
            .await?;
        Ok(body.pruned.unwrap_or(0))
    }

    pub async fn welcome_screen(&self, guild_id: Id<GuildMarker>) -> Result<WelcomeScreen> {
        let body: WelcomeScreenBody = self
            .send_json(
                self.raw_http.get(format!(
                    "https://discord.com/api/v9/guilds/{}/welcome-screen",
                    guild_id.get()
                )),
                "welcome screen",
            )
            .await?;

        Ok(WelcomeScreen {
            enabled: body.enabled,
            description: body.description.filter(|text| !text.is_empty()),
            channels: body
                .welcome_channels
                .into_iter()
                .filter_map(|channel| {
                    // Without a channel there is nothing for the row to point
                    // at, so it could neither be shown nor edited.
                    let channel_id = channel.channel_id?.parse::<u64>().ok()?;
                    Some(WelcomeChannel {
                        channel_id: Id::new(channel_id),
                        description: channel.description.unwrap_or_default(),
                        emoji_name: channel.emoji_name.filter(|name| !name.is_empty()),
                    })
                })
                .collect(),
        })
    }

    pub async fn modify_welcome_screen(
        &self,
        guild_id: Id<GuildMarker>,
        edit: &WelcomeScreenEdit,
    ) -> Result<()> {
        if edit.is_empty() {
            return Ok(());
        }

        self.send_unit(
            self.raw_http
                .patch(format!(
                    "https://discord.com/api/v9/guilds/{}/welcome-screen",
                    guild_id.get()
                ))
                .json(&edit.to_body()),
            "welcome screen",
        )
        .await
    }

    pub async fn guild_widget(&self, guild_id: Id<GuildMarker>) -> Result<GuildWidget> {
        let body: WidgetBody = self
            .send_json(
                self.raw_http.get(format!(
                    "https://discord.com/api/v9/guilds/{}/widget",
                    guild_id.get()
                )),
                "guild widget",
            )
            .await?;

        Ok(GuildWidget {
            enabled: body.enabled,
            channel_id: body
                .channel_id
                .and_then(|id| id.parse::<u64>().ok())
                .map(Id::new),
        })
    }

    pub async fn modify_guild_widget(
        &self,
        guild_id: Id<GuildMarker>,
        widget: &GuildWidget,
    ) -> Result<()> {
        self.send_unit(
            self.raw_http
                .patch(format!(
                    "https://discord.com/api/v9/guilds/{}/widget",
                    guild_id.get()
                ))
                .json(&json!({
                    "enabled": widget.enabled,
                    // Null rather than omitted: omitting leaves the old
                    // channel in place, which is not what "no invite" means.
                    "channel_id": widget.channel_id.map(|id| id.get().to_string()),
                })),
            "guild widget",
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prune_days_are_snapped_to_something_discord_accepts() {
        // Discord rejects anything outside its own list, so a typed number
        // would otherwise produce a request that simply fails.
        assert_eq!(nearest_prune_days(0), 1);
        assert_eq!(nearest_prune_days(10), 7);
        assert_eq!(nearest_prune_days(1000), 90);

        for accepted in PRUNE_DAYS {
            assert_eq!(nearest_prune_days(accepted), accepted);
        }
    }

    #[test]
    fn an_empty_welcome_edit_is_not_sent() {
        assert!(WelcomeScreenEdit::default().is_empty());
        assert!(
            !WelcomeScreenEdit {
                enabled: Some(true),
                ..WelcomeScreenEdit::default()
            }
            .is_empty()
        );
    }

    #[test]
    fn clearing_a_description_is_distinct_from_leaving_it() {
        let cleared = WelcomeScreenEdit {
            description: Some(None),
            ..WelcomeScreenEdit::default()
        };
        assert_eq!(cleared.to_body()["description"], Value::Null);

        let untouched = WelcomeScreenEdit {
            enabled: Some(true),
            ..WelcomeScreenEdit::default()
        };
        assert!(untouched.to_body().get("description").is_none());
    }

    #[test]
    fn a_description_is_truncated_on_character_boundaries() {
        let edit = WelcomeScreenEdit {
            description: Some(Some("é".repeat(400))),
            ..WelcomeScreenEdit::default()
        };

        let description = edit.to_body()["description"].as_str().unwrap().to_owned();
        assert_eq!(description.chars().count(), MAX_WELCOME_DESCRIPTION_CHARS);
    }

    #[test]
    fn more_channels_than_discord_allows_are_dropped_rather_than_failing_the_edit() {
        // Discord rejects the whole request over the limit, which would lose a
        // description change made in the same form.
        let edit = WelcomeScreenEdit {
            channels: Some(
                (1..=9)
                    .map(|index| WelcomeChannel {
                        channel_id: Id::new(index),
                        description: String::new(),
                        emoji_name: None,
                    })
                    .collect(),
            ),
            ..WelcomeScreenEdit::default()
        };

        let channels = edit.to_body()["welcome_channels"]
            .as_array()
            .expect("channels should be an array")
            .len();
        assert_eq!(channels, MAX_WELCOME_CHANNELS);
    }

    #[test]
    fn a_widget_with_no_channel_sends_null_rather_than_omitting_it() {
        // Omitting would leave the old channel in place, which is not what
        // "issues no invite" means.
        let widget = GuildWidget {
            enabled: true,
            channel_id: None,
        };
        let body = json!({
            "enabled": widget.enabled,
            "channel_id": widget.channel_id.map(|id| id.get().to_string()),
        });

        assert_eq!(body["channel_id"], Value::Null);
    }
}
