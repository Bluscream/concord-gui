//! Scheduled events and server templates.
//!
//! Together because both are things a server has rather than things a channel
//! does, and both are lists a moderator reads far more often than edits.

use serde::Deserialize;
use serde_json::json;

use crate::Result;
use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker},
};

use super::DiscordRest;

/// Where a scheduled event happens.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EventLocation {
    /// A stage or voice channel in this server.
    Channel(Id<ChannelMarker>),
    /// Somewhere else, described in free text.
    External(String),
    /// Discord gave neither, which happens for an event mid-edit.
    Unknown,
}

/// How far along a scheduled event is.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventStatus {
    Scheduled,
    Active,
    Completed,
    Cancelled,
    /// A status this build does not know. Shown with its number rather than
    /// hidden: an event nobody can name is still on the calendar.
    Unknown(u64),
}

impl EventStatus {
    pub const fn from_code(code: u64) -> Self {
        match code {
            1 => Self::Scheduled,
            2 => Self::Active,
            3 => Self::Completed,
            4 => Self::Cancelled,
            other => Self::Unknown(other),
        }
    }

    pub fn label(self) -> String {
        match self {
            Self::Scheduled => "scheduled".to_owned(),
            Self::Active => "happening now".to_owned(),
            Self::Completed => "finished".to_owned(),
            Self::Cancelled => "cancelled".to_owned(),
            Self::Unknown(code) => format!("status {code}"),
        }
    }

    /// Whether this event can still be cancelled.
    ///
    /// Discord rejects a cancel on one already finished, so the row says so
    /// rather than offering a button that fails.
    pub const fn is_cancellable(self) -> bool {
        matches!(self, Self::Scheduled | Self::Active)
    }
}

/// One scheduled event.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScheduledEvent {
    pub id: u64,
    pub name: String,
    pub description: Option<String>,
    /// ISO 8601, as Discord gives it. Not parsed: the client shows it, and a
    /// parse that failed would lose the only time information there is.
    pub starts_at: Option<String>,
    pub status: EventStatus,
    pub location: EventLocation,
    /// How many people said they are coming. Absent unless Discord was asked
    /// for it, which is why this is optional rather than zero.
    pub interested: Option<u64>,
}

impl ScheduledEvent {
    /// The line under the name.
    pub fn summary(&self) -> String {
        let mut parts = vec![self.status.label()];
        if let Some(starts_at) = &self.starts_at {
            parts.push(starts_at.clone());
        }
        match &self.location {
            EventLocation::Channel(_) => parts.push("in a channel here".to_owned()),
            EventLocation::External(place) if !place.is_empty() => parts.push(place.clone()),
            // Said rather than left blank: an event with nowhere to be reads
            // as a rendering fault rather than as one Discord has no place for.
            EventLocation::External(_) | EventLocation::Unknown => {
                parts.push("no location given".to_owned());
            }
        }
        if let Some(interested) = self.interested {
            parts.push(format!("{interested} interested"));
        }
        parts.join(" - ")
    }
}

/// One server template.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GuildTemplate {
    pub code: String,
    pub name: String,
    pub description: Option<String>,
    pub usage_count: u64,
    /// Whether the template is behind the server as it stands now.
    ///
    /// Discord marks this itself; a template that has drifted produces servers
    /// that do not match the one it was taken from, which is not obvious from
    /// the template alone.
    pub is_dirty: bool,
}

impl GuildTemplate {
    pub fn summary(&self) -> String {
        let mut parts = vec![format!("used {} times", self.usage_count)];
        if self.is_dirty {
            parts.push("out of date - sync to update".to_owned());
        }
        if let Some(description) = &self.description
            && !description.is_empty()
        {
            parts.push(description.clone());
        }
        parts.join(" - ")
    }

    /// The link people actually share.
    pub fn url(&self) -> String {
        format!("https://discord.new/{}", self.code)
    }
}

#[derive(Deserialize)]
struct EventBody {
    id: Option<String>,
    name: Option<String>,
    description: Option<String>,
    scheduled_start_time: Option<String>,
    #[serde(default)]
    status: u64,
    channel_id: Option<String>,
    entity_metadata: Option<EntityMetadataBody>,
    user_count: Option<u64>,
}

#[derive(Deserialize)]
struct EntityMetadataBody {
    location: Option<String>,
}

#[derive(Deserialize)]
struct TemplateBody {
    code: Option<String>,
    name: Option<String>,
    description: Option<String>,
    #[serde(default)]
    usage_count: u64,
    #[serde(default)]
    is_dirty: Option<bool>,
}

impl DiscordRest {
    /// Every scheduled event in a guild.
    pub async fn scheduled_events(&self, guild_id: Id<GuildMarker>) -> Result<Vec<ScheduledEvent>> {
        let events: Vec<EventBody> = self
            .send_json(
                self.raw_http.get(format!(
                    "https://discord.com/api/v9/guilds/{}/scheduled-events?with_user_count=true",
                    guild_id.get()
                )),
                "scheduled events",
            )
            .await?;

        Ok(events
            .into_iter()
            .filter_map(|event| {
                // Without an id there is nothing to open or cancel.
                let id = event.id?.parse::<u64>().ok()?;
                let location = match (
                    event.channel_id.and_then(|id| id.parse::<u64>().ok()),
                    event.entity_metadata.and_then(|meta| meta.location),
                ) {
                    (Some(channel_id), _) => EventLocation::Channel(Id::new(channel_id)),
                    (None, Some(place)) => EventLocation::External(place),
                    (None, None) => EventLocation::Unknown,
                };
                Some(ScheduledEvent {
                    id,
                    name: event.name.unwrap_or_default(),
                    description: event.description.filter(|text| !text.is_empty()),
                    starts_at: event.scheduled_start_time,
                    status: EventStatus::from_code(event.status),
                    location,
                    interested: event.user_count,
                })
            })
            .collect())
    }

    /// Cancel an event.
    ///
    /// A status change rather than a delete: Discord keeps a cancelled event
    /// visible so people who said they were coming can see it is off.
    pub async fn cancel_scheduled_event(
        &self,
        guild_id: Id<GuildMarker>,
        event_id: u64,
    ) -> Result<()> {
        self.send_unit(
            self.raw_http
                .patch(format!(
                    "https://discord.com/api/v9/guilds/{}/scheduled-events/{event_id}",
                    guild_id.get()
                ))
                .json(&json!({ "status": 4 })),
            "cancel event",
        )
        .await
    }

    pub async fn delete_scheduled_event(
        &self,
        guild_id: Id<GuildMarker>,
        event_id: u64,
    ) -> Result<()> {
        self.send_unit(
            self.raw_http.delete(format!(
                "https://discord.com/api/v9/guilds/{}/scheduled-events/{event_id}",
                guild_id.get()
            )),
            "delete event",
        )
        .await
    }

    /// Say you are or are not coming.
    pub async fn set_event_interest(
        &self,
        guild_id: Id<GuildMarker>,
        event_id: u64,
        interested: bool,
    ) -> Result<()> {
        let url = format!(
            "https://discord.com/api/v9/guilds/{}/scheduled-events/{event_id}/users/@me",
            guild_id.get()
        );
        let request = if interested {
            self.raw_http.put(url)
        } else {
            self.raw_http.delete(url)
        };
        self.send_unit(request, "event interest").await
    }

    pub async fn guild_templates(&self, guild_id: Id<GuildMarker>) -> Result<Vec<GuildTemplate>> {
        let templates: Vec<TemplateBody> = self
            .send_json(
                self.raw_http.get(format!(
                    "https://discord.com/api/v9/guilds/{}/templates",
                    guild_id.get()
                )),
                "templates",
            )
            .await?;

        Ok(templates
            .into_iter()
            .filter_map(|template| {
                // The code is the template: without it there is no link to
                // share and nothing to sync or delete.
                Some(GuildTemplate {
                    code: template.code?,
                    name: template.name.unwrap_or_default(),
                    description: template.description.filter(|text| !text.is_empty()),
                    usage_count: template.usage_count,
                    // Absent means Discord did not say, which is not the same
                    // as up to date - but claiming "out of date" on silence
                    // would nag about every template forever.
                    is_dirty: template.is_dirty.unwrap_or(false),
                })
            })
            .collect())
    }

    pub async fn create_guild_template(&self, guild_id: Id<GuildMarker>, name: &str) -> Result<()> {
        self.send_unit(
            self.raw_http
                .post(format!(
                    "https://discord.com/api/v9/guilds/{}/templates",
                    guild_id.get()
                ))
                .json(&json!({ "name": name })),
            "create template",
        )
        .await
    }

    /// Bring a template up to date with the server as it stands.
    pub async fn sync_guild_template(&self, guild_id: Id<GuildMarker>, code: &str) -> Result<()> {
        self.send_unit(
            self.raw_http.put(format!(
                "https://discord.com/api/v9/guilds/{}/templates/{code}",
                guild_id.get()
            )),
            "sync template",
        )
        .await
    }

    pub async fn delete_guild_template(&self, guild_id: Id<GuildMarker>, code: &str) -> Result<()> {
        self.send_unit(
            self.raw_http.delete(format!(
                "https://discord.com/api/v9/guilds/{}/templates/{code}",
                guild_id.get()
            )),
            "delete template",
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(status: EventStatus, location: EventLocation) -> ScheduledEvent {
        ScheduledEvent {
            id: 1,
            name: "Games night".to_owned(),
            description: None,
            starts_at: Some("2026-09-01T19:00:00Z".to_owned()),
            status,
            location,
            interested: Some(4),
        }
    }

    #[test]
    fn only_an_event_that_has_not_finished_can_be_cancelled() {
        // Discord rejects a cancel on a finished event, so offering one would
        // be a button that always fails.
        assert!(EventStatus::Scheduled.is_cancellable());
        assert!(EventStatus::Active.is_cancellable());
        assert!(!EventStatus::Completed.is_cancellable());
        assert!(!EventStatus::Cancelled.is_cancellable());
        // An unrecognised status is not assumed to be cancellable: guessing
        // wrong here means offering an action Discord will refuse.
        assert!(!EventStatus::Unknown(9).is_cancellable());
    }

    #[test]
    fn known_status_codes_map_to_the_right_status() {
        // A transposed code would describe a cancelled event as happening now.
        assert_eq!(EventStatus::from_code(1), EventStatus::Scheduled);
        assert_eq!(EventStatus::from_code(2), EventStatus::Active);
        assert_eq!(EventStatus::from_code(3), EventStatus::Completed);
        assert_eq!(EventStatus::from_code(4), EventStatus::Cancelled);
        assert_eq!(EventStatus::from_code(0), EventStatus::Unknown(0));
    }

    #[test]
    fn an_unrecognised_status_is_shown_rather_than_hidden() {
        // An event nobody can name is still on the calendar.
        assert!(EventStatus::Unknown(9).label().contains('9'));
    }

    #[test]
    fn an_event_with_nowhere_to_be_says_so() {
        // A blank tail reads as a rendering fault rather than as an event
        // Discord has no place for.
        let summary = event(EventStatus::Scheduled, EventLocation::Unknown).summary();
        assert!(summary.contains("no location"), "got {summary:?}");

        let summary = event(
            EventStatus::Scheduled,
            EventLocation::External(String::new()),
        )
        .summary();
        assert!(summary.contains("no location"), "got {summary:?}");
    }

    #[test]
    fn an_external_location_is_shown_as_given() {
        let summary = event(
            EventStatus::Scheduled,
            EventLocation::External("The pub".to_owned()),
        )
        .summary();
        assert!(summary.contains("The pub"));
    }

    #[test]
    fn a_template_behind_its_server_says_so() {
        // A drifted template produces servers that do not match the one it was
        // taken from, which the template alone does not reveal.
        let stale = GuildTemplate {
            code: "abc".to_owned(),
            name: "Base".to_owned(),
            description: None,
            usage_count: 3,
            is_dirty: true,
        };
        assert!(stale.summary().contains("out of date"));

        let fresh = GuildTemplate {
            is_dirty: false,
            ..stale.clone()
        };
        assert!(!fresh.summary().contains("out of date"));
    }

    #[test]
    fn a_template_url_is_the_link_people_share() {
        let template = GuildTemplate {
            code: "abc".to_owned(),
            name: "Base".to_owned(),
            description: None,
            usage_count: 0,
            is_dirty: false,
        };
        assert_eq!(template.url(), "https://discord.new/abc");
    }
}
