use crate::discord::ids::{Id, marker::EmojiMarker};

use super::emoji::custom_emoji_image_url;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum PresenceStatus {
    Online,
    Idle,
    DoNotDisturb,
    Offline,
    Unknown,
}

impl PresenceStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Online => "Online",
            Self::Idle => "Idle",
            Self::DoNotDisturb => "Do Not Disturb",
            Self::Offline => "Offline",
            Self::Unknown => "Unknown",
        }
    }

    pub(crate) fn gateway_status(self) -> &'static str {
        match self {
            Self::Online => "online",
            Self::Idle => "idle",
            Self::DoNotDisturb => "dnd",
            Self::Offline => "invisible",
            Self::Unknown => "unknown",
        }
    }

    pub(crate) const fn user_selectable() -> [Self; 4] {
        [Self::Online, Self::Idle, Self::DoNotDisturb, Self::Offline]
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum ActivityKind {
    Playing,
    Streaming,
    Listening,
    Watching,
    Custom,
    Competing,
    Unknown,
}

impl ActivityKind {
    pub fn from_code(code: u64) -> Self {
        match code {
            0 => Self::Playing,
            1 => Self::Streaming,
            2 => Self::Listening,
            3 => Self::Watching,
            4 => Self::Custom,
            5 => Self::Competing,
            _ => Self::Unknown,
        }
    }

    pub(crate) const fn gateway_code(self) -> u8 {
        match self {
            Self::Playing => 0,
            Self::Streaming => 1,
            Self::Listening => 2,
            Self::Watching => 3,
            Self::Custom => 4,
            Self::Competing => 5,
            Self::Unknown => 0,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActivityEmoji {
    pub name: String,
    pub id: Option<Id<EmojiMarker>>,
    pub animated: bool,
}

impl ActivityEmoji {
    /// CDN URL for a custom emoji (one with an `id`). `None` for unicode emojis,
    /// which render as text.
    pub fn image_url(&self) -> Option<String> {
        let id = self.id?;
        Some(custom_emoji_image_url(id.get(), self.animated))
    }
}

/// Start/end of the activity in Unix **milliseconds**.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ActivityTimestamps {
    pub start: Option<i64>,
    pub end: Option<i64>,
}

/// Image slots of a rich presence card. Each `*_image` is an app-asset key, a
/// numeric asset id, or an `mp:` external ref.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActivityAssets {
    pub large_image: Option<String>,
    pub large_text: Option<String>,
    pub small_image: Option<String>,
    pub small_text: Option<String>,
}

/// Party grouping for an activity. `size` is `(current, max)` members.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActivityParty {
    pub id: Option<String>,
    pub size: Option<(u32, u32)>,
}

/// A clickable button. User-account gateway presence encodes these differently
/// from RPC's `{ label, url }` (see `activity_gateway_payload`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActivityButton {
    pub label: String,
    pub url: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActivityInfo {
    pub kind: ActivityKind,
    pub name: String,
    pub details: Option<String>,
    pub state: Option<String>,
    pub url: Option<String>,
    pub application_id: Option<String>,
    pub emoji: Option<ActivityEmoji>,
    pub timestamps: Option<ActivityTimestamps>,
    pub assets: Option<ActivityAssets>,
    pub party: Option<ActivityParty>,
    pub buttons: Vec<ActivityButton>,
}

impl ActivityInfo {
    /// A one-line description, as a client should show it.
    ///
    /// Here rather than in a front end so both word it the same way, and so a
    /// translator sees one set of strings instead of two.
    ///
    /// A custom status is its own text - "Custom Status" is Discord's internal
    /// name for the activity, never something to show. Everything else reads
    /// as a verb and a subject, with the extra fields appended when present:
    /// Spotify puts the track in `details` and the artist in `state`, and
    /// showing only the name would say "Listening to Spotify" for every song.
    pub fn display_line(&self) -> Option<String> {
        if self.kind == ActivityKind::Custom {
            let text = self.state.as_deref().unwrap_or_default().trim();
            return (!text.is_empty()).then(|| match &self.emoji {
                Some(emoji) if !emoji.name.trim().is_empty() => {
                    format!("{} {text}", emoji.name.trim())
                }
                _ => text.to_owned(),
            });
        }

        let name = self.name.trim();
        if name.is_empty() {
            return None;
        }

        let verb = match self.kind {
            ActivityKind::Playing => "Playing",
            ActivityKind::Streaming => "Streaming",
            ActivityKind::Listening => "Listening to",
            ActivityKind::Watching => "Watching",
            ActivityKind::Competing => "Competing in",
            // An unrecognised kind still has a name worth showing, but no
            // verb that would be honest.
            ActivityKind::Custom | ActivityKind::Unknown => "",
        };

        let mut line = if verb.is_empty() {
            name.to_owned()
        } else {
            format!("{verb} {name}")
        };

        let details = self.details.as_deref().unwrap_or_default().trim();
        let state = self.state.as_deref().unwrap_or_default().trim();
        match (details.is_empty(), state.is_empty()) {
            (false, false) => line.push_str(&format!(" - {details} by {state}")),
            (false, true) => line.push_str(&format!(" - {details}")),
            (true, false) => line.push_str(&format!(" - {state}")),
            (true, true) => {}
        }
        Some(line)
    }

    pub fn playing(name: impl Into<String>) -> Self {
        Self {
            kind: ActivityKind::Playing,
            name: name.into(),
            details: None,
            state: None,
            url: None,
            application_id: None,
            emoji: None,
            timestamps: None,
            assets: None,
            party: None,
            buttons: Vec::new(),
        }
    }
}

#[cfg(test)]
#[allow(dead_code)]
impl ActivityInfo {
    pub(crate) fn test(kind: ActivityKind, name: impl Into<String>) -> Self {
        Self {
            kind,
            name: name.into(),
            details: None,
            state: None,
            url: None,
            application_id: None,
            emoji: None,
            timestamps: None,
            assets: None,
            party: None,
            buttons: Vec::new(),
        }
    }
}

#[cfg(test)]
mod activity_display_tests {
    use super::*;

    fn custom(state: Option<&str>, emoji: Option<&str>) -> ActivityInfo {
        ActivityInfo {
            kind: ActivityKind::Custom,
            name: "Custom Status".to_owned(),
            state: state.map(str::to_owned),
            emoji: emoji.map(|name| ActivityEmoji {
                name: name.to_owned(),
                id: None,
                animated: false,
            }),
            ..ActivityInfo::playing("")
        }
    }

    #[test]
    fn a_custom_status_shows_its_own_text() {
        // "Custom Status" is Discord's internal name for the activity and must
        // never reach the screen.
        let activity = custom(Some("out for lunch"), None);
        assert_eq!(activity.display_line().as_deref(), Some("out for lunch"));
    }

    #[test]
    fn a_custom_status_keeps_its_emoji() {
        let activity = custom(Some("busy"), Some(":coffee:"));
        assert_eq!(activity.display_line().as_deref(), Some(":coffee: busy"));
    }

    #[test]
    fn an_empty_custom_status_shows_nothing() {
        // Someone with an emoji and no text, or nothing at all, should not
        // produce a blank row.
        assert_eq!(custom(None, None).display_line(), None);
        assert_eq!(custom(Some("   "), None).display_line(), None);
    }

    #[test]
    fn listening_names_the_track_rather_than_the_app() {
        // Spotify puts the track in details and the artist in state, so
        // showing only the name would say "Listening to Spotify" for
        // every song anyone ever played.
        let activity = ActivityInfo {
            kind: ActivityKind::Listening,
            name: "Spotify".to_owned(),
            details: Some("Windowlicker".to_owned()),
            state: Some("Aphex Twin".to_owned()),
            ..ActivityInfo::playing("")
        };

        assert_eq!(
            activity.display_line().as_deref(),
            Some("Listening to Spotify - Windowlicker by Aphex Twin")
        );
    }

    #[test]
    fn each_kind_reads_as_a_sentence() {
        for (kind, expected) in [
            (ActivityKind::Playing, "Playing Doom"),
            (ActivityKind::Streaming, "Streaming Doom"),
            (ActivityKind::Watching, "Watching Doom"),
            (ActivityKind::Competing, "Competing in Doom"),
        ] {
            let activity = ActivityInfo {
                kind,
                ..ActivityInfo::playing("Doom")
            };
            assert_eq!(activity.display_line().as_deref(), Some(expected));
        }
    }

    #[test]
    fn an_unknown_kind_shows_the_name_without_inventing_a_verb() {
        let activity = ActivityInfo {
            kind: ActivityKind::Unknown,
            ..ActivityInfo::playing("Something")
        };
        assert_eq!(activity.display_line().as_deref(), Some("Something"));
    }

    #[test]
    fn a_nameless_activity_shows_nothing() {
        let activity = ActivityInfo::playing("   ");
        assert_eq!(activity.display_line(), None);
    }
}
