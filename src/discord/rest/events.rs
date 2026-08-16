//! Scheduled events and server templates.
//!
//! Together because both are things a server has rather than things a channel
//! does, and both are lists a moderator reads far more often than edits.

use serde::Deserialize;
use serde_json::{Map, Value, json};

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
    /// When it finishes. Only an event somewhere else has one; Discord ends a
    /// channel event when the channel empties.
    pub ends_at: Option<String>,
    pub status: EventStatus,
    pub location: EventLocation,
    /// How many people said they are coming. Absent unless Discord was asked
    /// for it, which is why this is optional rather than zero.
    pub interested: Option<u64>,
}

impl ScheduledEvent {
    /// The event as one editable line, in the same format the form parses.
    ///
    /// Seeded so a change is a correction rather than a retype - and so what
    /// is shown is exactly what will be sent back.
    pub fn to_line(&self) -> String {
        // A channel event has no place to type, and its end time is Discord's
        // to decide - showing either would invite typing something ignored.
        let (ends_at, place) = match &self.location {
            EventLocation::External(place) => {
                (self.ends_at.clone().unwrap_or_default(), place.clone())
            }
            EventLocation::Channel(_) | EventLocation::Unknown => (String::new(), String::new()),
        };
        format!(
            "{} | {} | {ends_at} | {place}",
            self.name,
            self.starts_at.clone().unwrap_or_default(),
        )
    }

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
    scheduled_end_time: Option<String>,
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
                    ends_at: event.scheduled_end_time,
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
            ends_at: Some("2026-09-01T22:00:00Z".to_owned()),
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
    fn an_event_round_trips_through_the_line_the_form_parses() {
        // What is shown must be exactly what is sent back, or editing one
        // field would silently rewrite another.
        let original = event(
            EventStatus::Scheduled,
            EventLocation::External("The pub".to_owned()),
        );
        let parsed = crate::discord::parse_new_event(&original.to_line()).expect("should parse");

        assert_eq!(parsed.name, original.name);
        assert_eq!(Some(parsed.starts_at.clone()), original.starts_at);
        assert_eq!(Some(parsed.ends_at.clone()), original.ends_at);
        assert_eq!(parsed.problem(), None);
    }

    #[test]
    fn a_channel_event_seeds_no_place_or_end_time() {
        // Discord decides when a channel event is over, and there is no place
        // to type - showing either would invite typing something ignored.
        let line = event(EventStatus::Scheduled, EventLocation::Channel(Id::new(7))).to_line();
        assert!(line.ends_with(" |  | "), "got {line:?}");
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

/// Discord's caps on a scheduled event.
pub const MAX_EVENT_NAME_CHARS: usize = 100;
pub const MAX_EVENT_DESCRIPTION_CHARS: usize = 1000;
pub const MAX_EVENT_LOCATION_CHARS: usize = 100;

/// A new scheduled event, as the form describes it.
///
/// In the core rather than once per client, for the same reason `AccountForm`
/// is: the parts that drift when written twice are which fields are required,
/// and that an external event needs an end time while a channel event must not
/// have one.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct NewEvent {
    pub name: String,
    pub description: String,
    /// ISO 8601. Typed rather than picked: a date picker is a widget neither
    /// client has, and Discord's own format is what its error messages quote.
    pub starts_at: String,
    pub ends_at: String,
    /// A channel in this server, or free text for somewhere else.
    pub location: NewEventLocation,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NewEventLocation {
    Channel(Id<ChannelMarker>),
    External(String),
}

impl Default for NewEventLocation {
    fn default() -> Self {
        Self::External(String::new())
    }
}

/// Why an event cannot be created yet.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NewEventProblem {
    NameMissing,
    NameTooLong,
    StartMissing,
    /// Discord requires an end time for an event that is not in a channel,
    /// because nothing else can tell it when the event is over.
    ExternalNeedsEnd,
    ExternalNeedsLocation,
    DescriptionTooLong,
}

impl NewEventProblem {
    pub fn message(self) -> String {
        match self {
            Self::NameMissing => "The event needs a name".to_owned(),
            Self::NameTooLong => format!("A name is at most {MAX_EVENT_NAME_CHARS} characters"),
            Self::StartMissing => "The event needs a start time".to_owned(),
            Self::ExternalNeedsEnd => {
                "An event somewhere else needs an end time - nothing else says when it is over"
                    .to_owned()
            }
            Self::ExternalNeedsLocation => "Say where the event is".to_owned(),
            Self::DescriptionTooLong => {
                format!("A description is at most {MAX_EVENT_DESCRIPTION_CHARS} characters")
            }
        }
    }
}

impl NewEvent {
    /// Why this cannot be created, or `None` when it can.
    pub fn problem(&self) -> Option<NewEventProblem> {
        if self.name.trim().is_empty() {
            return Some(NewEventProblem::NameMissing);
        }
        if self.name.chars().count() > MAX_EVENT_NAME_CHARS {
            return Some(NewEventProblem::NameTooLong);
        }
        if self.description.chars().count() > MAX_EVENT_DESCRIPTION_CHARS {
            return Some(NewEventProblem::DescriptionTooLong);
        }
        if self.starts_at.trim().is_empty() {
            return Some(NewEventProblem::StartMissing);
        }
        if let NewEventLocation::External(place) = &self.location {
            if place.trim().is_empty() {
                return Some(NewEventProblem::ExternalNeedsLocation);
            }
            // Discord rejects an external event without one, and the message it
            // returns does not say which field is missing.
            if self.ends_at.trim().is_empty() {
                return Some(NewEventProblem::ExternalNeedsEnd);
            }
        }
        None
    }

    fn to_body(&self) -> Value {
        let mut fields = Map::new();
        fields.insert(
            "name".to_owned(),
            Value::from(
                self.name
                    .chars()
                    .take(MAX_EVENT_NAME_CHARS)
                    .collect::<String>(),
            ),
        );
        fields.insert(
            "scheduled_start_time".to_owned(),
            Value::from(self.starts_at.trim()),
        );
        // Everyone, which is the only privacy level Discord accepts here.
        fields.insert("privacy_level".to_owned(), Value::from(2));
        if !self.description.trim().is_empty() {
            fields.insert(
                "description".to_owned(),
                Value::from(
                    self.description
                        .chars()
                        .take(MAX_EVENT_DESCRIPTION_CHARS)
                        .collect::<String>(),
                ),
            );
        }
        match &self.location {
            NewEventLocation::Channel(channel_id) => {
                // Entity type 2 is a voice channel. A channel event takes no
                // end time: Discord decides it is over when the channel empties.
                fields.insert("entity_type".to_owned(), Value::from(2));
                fields.insert(
                    "channel_id".to_owned(),
                    Value::from(channel_id.get().to_string()),
                );
            }
            NewEventLocation::External(place) => {
                fields.insert("entity_type".to_owned(), Value::from(3));
                fields.insert(
                    "scheduled_end_time".to_owned(),
                    Value::from(self.ends_at.trim()),
                );
                fields.insert(
                    "entity_metadata".to_owned(),
                    json!({
                        "location": place.chars().take(MAX_EVENT_LOCATION_CHARS).collect::<String>()
                    }),
                );
            }
        }
        Value::Object(fields)
    }
}

impl DiscordRest {
    /// Change an event that already exists.
    ///
    /// The same body as creating one: Discord's patch takes every field, and
    /// sending a partial one leaves the rest as they were - which is not what
    /// a form showing all of them appears to promise.
    pub async fn modify_scheduled_event(
        &self,
        guild_id: Id<GuildMarker>,
        event_id: u64,
        event: &NewEvent,
    ) -> Result<()> {
        self.send_unit(
            self.raw_http
                .patch(format!(
                    "https://discord.com/api/v9/guilds/{}/scheduled-events/{event_id}",
                    guild_id.get()
                ))
                .json(&event.to_body()),
            "modify event",
        )
        .await
    }

    pub async fn create_scheduled_event(
        &self,
        guild_id: Id<GuildMarker>,
        event: &NewEvent,
    ) -> Result<()> {
        self.send_unit(
            self.raw_http
                .post(format!(
                    "https://discord.com/api/v9/guilds/{}/scheduled-events",
                    guild_id.get()
                ))
                .json(&event.to_body()),
            "create event",
        )
        .await
    }
}

/// Read one line of `name | start | end | where` into an event.
///
/// A single line rather than five prompts in sequence: the panel has one text
/// field, and a wizard of five would be worse than one line with a stated
/// format. `end` may be empty for an event in a channel, which Discord ends
/// when the channel empties.
pub fn parse_new_event(text: &str) -> Option<NewEvent> {
    let mut parts = text.split('|').map(str::trim);
    let name = parts.next()?.to_owned();
    let starts_at = parts.next().unwrap_or_default().to_owned();
    let ends_at = parts.next().unwrap_or_default().to_owned();
    let place = parts.next().unwrap_or_default().to_owned();
    // Anything after the fourth separator is part of the location, since a
    // place name may itself contain one.
    let rest: Vec<&str> = parts.collect();
    let place = if rest.is_empty() {
        place
    } else {
        format!("{place} | {}", rest.join(" | "))
    };

    Some(NewEvent {
        name,
        description: String::new(),
        starts_at,
        ends_at,
        location: NewEventLocation::External(place),
    })
}

#[cfg(test)]
mod new_event_tests {
    use super::*;

    fn external() -> NewEvent {
        NewEvent {
            name: "Games night".to_owned(),
            description: String::new(),
            starts_at: "2026-09-01T19:00:00Z".to_owned(),
            ends_at: "2026-09-01T22:00:00Z".to_owned(),
            location: NewEventLocation::External("The pub".to_owned()),
        }
    }

    fn in_channel() -> NewEvent {
        NewEvent {
            ends_at: String::new(),
            location: NewEventLocation::Channel(Id::new(7)),
            ..external()
        }
    }

    #[test]
    fn a_complete_external_event_is_accepted() {
        assert_eq!(external().problem(), None);
    }

    #[test]
    fn an_event_somewhere_else_needs_an_end_time_and_a_place() {
        // Discord rejects both cases, and the message it returns does not say
        // which field is missing - so the form says it instead.
        let no_end = NewEvent {
            ends_at: String::new(),
            ..external()
        };
        assert_eq!(no_end.problem(), Some(NewEventProblem::ExternalNeedsEnd));

        let no_place = NewEvent {
            location: NewEventLocation::External("  ".to_owned()),
            ..external()
        };
        assert_eq!(
            no_place.problem(),
            Some(NewEventProblem::ExternalNeedsLocation)
        );
    }

    #[test]
    fn an_event_in_a_channel_needs_no_end_time() {
        // Discord decides a channel event is over when the channel empties, so
        // requiring one here would block a perfectly valid event.
        assert_eq!(in_channel().problem(), None);
        assert!(in_channel().to_body().get("scheduled_end_time").is_none());
    }

    #[test]
    fn a_channel_event_and_an_external_one_send_different_entity_types() {
        // Transposing these makes Discord reject the request with a message
        // about a field the form never showed.
        assert_eq!(in_channel().to_body()["entity_type"], Value::from(2));
        assert_eq!(external().to_body()["entity_type"], Value::from(3));
        assert!(external().to_body().get("channel_id").is_none());
    }

    #[test]
    fn a_nameless_or_startless_event_is_refused_before_the_round_trip() {
        let nameless = NewEvent {
            name: "  ".to_owned(),
            ..external()
        };
        assert_eq!(nameless.problem(), Some(NewEventProblem::NameMissing));

        let startless = NewEvent {
            starts_at: String::new(),
            ..external()
        };
        assert_eq!(startless.problem(), Some(NewEventProblem::StartMissing));
    }

    #[test]
    fn lengths_are_counted_in_characters_not_bytes() {
        // A name of multi-byte characters would otherwise be refused while
        // being well within Discord's limit.
        let ok = NewEvent {
            name: "é".repeat(MAX_EVENT_NAME_CHARS),
            ..external()
        };
        assert_eq!(ok.problem(), None);

        let too_long = NewEvent {
            name: "é".repeat(MAX_EVENT_NAME_CHARS + 1),
            ..external()
        };
        assert_eq!(too_long.problem(), Some(NewEventProblem::NameTooLong));
    }

    #[test]
    fn an_empty_description_is_left_out_rather_than_sent_blank() {
        assert!(external().to_body().get("description").is_none());

        let described = NewEvent {
            description: "Bring dice".to_owned(),
            ..external()
        };
        assert_eq!(
            described.to_body()["description"],
            Value::from("Bring dice")
        );
    }
}
