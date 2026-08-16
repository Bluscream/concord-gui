//! Privacy and safety settings.
//!
//! Written through the legacy `/users/@me/settings` endpoint rather than the
//! settings-proto in `user_settings.rs`. That is deliberate: these same fields
//! are *read* from the legacy `user_settings` payload in READY, so writing them
//! anywhere else would mean the client edits one representation and displays
//! another, and a rejected write would leave the panel showing the new value.

use serde_json::{Map, Value};

use crate::Result;

use super::DiscordRest;

/// How much of your direct messages Discord scans for explicit images.
///
/// Distinct from the guild-level filter in `guild_settings`, which has the same
/// shape and different meanings - its middle option is about members without
/// roles, which has no sense in a DM.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DmScanLevel {
    None,
    NonFriends,
    Everyone,
}

impl DmScanLevel {
    pub const ALL: [Self; 3] = [Self::None, Self::NonFriends, Self::Everyone];

    pub const fn code(self) -> u8 {
        match self {
            Self::None => 0,
            Self::NonFriends => 1,
            Self::Everyone => 2,
        }
    }

    pub const fn from_code(code: u64) -> Self {
        match code {
            1 => Self::NonFriends,
            2 => Self::Everyone,
            // Anything unrecognised reads as "do not scan" only because that is
            // what code 0 is; it is the value Discord itself defaults to.
            _ => Self::None,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::None => "Do not scan",
            Self::NonFriends => "Scan messages from non-friends",
            Self::Everyone => "Scan every direct message",
        }
    }

    /// The next option, wrapping.
    ///
    /// Wrapping so every option is reachable from every other: a cycle that
    /// stopped at the end would make the first option unreachable without
    /// closing and reopening the panel.
    pub const fn next(self) -> Self {
        match self {
            Self::None => Self::NonFriends,
            Self::NonFriends => Self::Everyone,
            Self::Everyone => Self::None,
        }
    }
}

/// Who may send you a friend request.
///
/// Discord stores three flags, but they are not independent: `all` covers the
/// other two. Kept as three so the panel can show what Discord's own does, with
/// `everyone` taking precedence when it is set.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FriendSources {
    pub everyone: bool,
    pub mutual_friends: bool,
    pub mutual_guilds: bool,
}

impl FriendSources {
    /// Whether anyone at all can send a request.
    ///
    /// All three off is a valid state and means nobody can, which the panel
    /// should say rather than leaving three empty checkboxes to be read as a
    /// failed load.
    pub const fn nobody(self) -> bool {
        !self.everyone && !self.mutual_friends && !self.mutual_guilds
    }

    fn to_body(self) -> Value {
        let mut flags = Map::new();
        // Only the flags that are set are sent, which is Discord's own
        // convention here: it treats an absent flag as false.
        if self.everyone {
            flags.insert("all".to_owned(), Value::Bool(true));
        }
        if self.mutual_friends {
            flags.insert("mutual_friends".to_owned(), Value::Bool(true));
        }
        if self.mutual_guilds {
            flags.insert("mutual_guilds".to_owned(), Value::Bool(true));
        }
        Value::Object(flags)
    }

    pub fn from_info(info: &crate::discord::UserFriendSourceFlagsInfo) -> Self {
        Self {
            everyone: info.all.unwrap_or(false),
            mutual_friends: info.mutual_friends.unwrap_or(false),
            mutual_guilds: info.mutual_guilds.unwrap_or(false),
        }
    }
}

/// What to change. `None` means leave alone, as elsewhere: this endpoint
/// replaces what it is given, and sending the whole settings object back would
/// overwrite the many fields this client never shows.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PrivacyEdit {
    pub dm_scan_level: Option<DmScanLevel>,
    /// Whether new guilds start with direct messages from their members off.
    pub default_guilds_restricted: Option<bool>,
    pub friend_sources: Option<FriendSources>,
}

impl PrivacyEdit {
    pub fn is_empty(&self) -> bool {
        self.dm_scan_level.is_none()
            && self.default_guilds_restricted.is_none()
            && self.friend_sources.is_none()
    }

    fn to_body(&self) -> Value {
        let mut fields = Map::new();
        if let Some(level) = self.dm_scan_level {
            fields.insert(
                "explicit_content_filter".to_owned(),
                Value::from(level.code()),
            );
        }
        if let Some(restricted) = self.default_guilds_restricted {
            fields.insert(
                "default_guilds_restricted".to_owned(),
                Value::Bool(restricted),
            );
        }
        if let Some(sources) = self.friend_sources {
            fields.insert("friend_source_flags".to_owned(), sources.to_body());
        }
        Value::Object(fields)
    }
}

impl DiscordRest {
    pub async fn modify_privacy_settings(&self, edit: &PrivacyEdit) -> Result<()> {
        if edit.is_empty() {
            return Ok(());
        }

        self.send_unit(
            self.raw_http
                .patch("https://discord.com/api/v9/users/@me/settings")
                .json(&edit.to_body()),
            "privacy settings",
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_edit_is_not_sent() {
        assert!(PrivacyEdit::default().is_empty());
        assert!(
            !PrivacyEdit {
                default_guilds_restricted: Some(true),
                ..PrivacyEdit::default()
            }
            .is_empty()
        );
    }

    #[test]
    fn only_the_changed_fields_are_sent() {
        // This endpoint replaces what it is given. Sending a default for a
        // field nobody touched would silently reset a privacy setting.
        let edit = PrivacyEdit {
            dm_scan_level: Some(DmScanLevel::Everyone),
            ..PrivacyEdit::default()
        };
        let body = edit.to_body();

        assert_eq!(body["explicit_content_filter"], Value::from(2));
        assert!(body.get("friend_source_flags").is_none());
        assert!(body.get("default_guilds_restricted").is_none());
    }

    #[test]
    fn every_scan_level_round_trips_through_its_code() {
        // A transposed code would set a different privacy level than the one
        // the panel showed being chosen.
        for level in DmScanLevel::ALL {
            assert_eq!(DmScanLevel::from_code(u64::from(level.code())), level);
        }
    }

    #[test]
    fn cycling_scan_levels_reaches_every_one_and_returns() {
        let mut level = DmScanLevel::None;
        let mut seen = Vec::new();
        for _ in 0..DmScanLevel::ALL.len() {
            seen.push(level);
            level = level.next();
        }

        assert_eq!(level, DmScanLevel::None, "the cycle does not return");
        for expected in DmScanLevel::ALL {
            assert!(seen.contains(&expected), "{expected:?} is unreachable");
        }
    }

    #[test]
    fn friend_sources_send_only_the_flags_that_are_set() {
        // Discord treats an absent flag as false, so sending `false` is the
        // same as omitting it - but omitting is what its own client does, and
        // matching that avoids finding out the two differ.
        let sources = FriendSources {
            everyone: false,
            mutual_friends: true,
            mutual_guilds: false,
        };
        let body = sources.to_body();

        assert_eq!(body["mutual_friends"], Value::Bool(true));
        assert!(body.get("all").is_none());
        assert!(body.get("mutual_guilds").is_none());
    }

    #[test]
    fn no_source_at_all_is_a_real_state_rather_than_an_empty_form() {
        // All three off means nobody can send a request. A panel that read it
        // as "nothing loaded" would show three blank boxes for a setting that
        // is deliberately set.
        assert!(FriendSources::default().nobody());
        assert!(
            !FriendSources {
                mutual_guilds: true,
                ..FriendSources::default()
            }
            .nobody()
        );
    }
}

/// The privacy panel's rows, shared by both clients.
///
/// In the core rather than in each front end because the interesting part - the
/// three friend-source flags sharing one field, so toggling one must carry the
/// other two - is exactly the kind of thing that drifts when it is written
/// twice. The clients decide how a row looks; this decides what it means.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrivacySetting {
    DmScanning,
    /// Whether servers you join from now on may send you direct messages.
    NewGuildDirectMessages,
    FriendsEveryone,
    FriendsMutualFriends,
    FriendsMutualGuilds,
}

/// What the account currently says, as far as the client has been told.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PrivacyState {
    pub dm_scan_level: Option<DmScanLevel>,
    pub default_guilds_restricted: Option<bool>,
    pub friend_sources: Option<FriendSources>,
}

impl PrivacySetting {
    pub const ALL: [Self; 5] = [
        Self::DmScanning,
        Self::NewGuildDirectMessages,
        Self::FriendsEveryone,
        Self::FriendsMutualFriends,
        Self::FriendsMutualGuilds,
    ];

    pub fn at(index: usize) -> Option<Self> {
        Self::ALL.get(index).copied()
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::DmScanning => "Scan direct messages",
            Self::NewGuildDirectMessages => "New servers may send you direct messages",
            Self::FriendsEveryone => "Friend requests from everyone",
            Self::FriendsMutualFriends => "Friend requests from friends of friends",
            Self::FriendsMutualGuilds => "Friend requests from server members",
        }
    }

    pub const fn detail(self) -> &'static str {
        match self {
            Self::DmScanning => "Whether Discord scans direct messages for explicit images.",
            Self::NewGuildDirectMessages => {
                "Applies to servers you join from now on, not the ones you are in."
            }
            Self::FriendsEveryone => "Covers the other two on its own.",
            Self::FriendsMutualFriends => "Anyone who shares a friend with you.",
            Self::FriendsMutualGuilds => "Anyone in a server you are also in.",
        }
    }

    /// Whether the setting is on, or `None` when it never arrived.
    ///
    /// `None` is not `false`: a panel that showed them alike would report an
    /// unknown setting as permissive.
    pub fn is_on(self, state: &PrivacyState) -> Option<bool> {
        match self {
            Self::DmScanning => state.dm_scan_level.map(|level| level != DmScanLevel::None),
            // The stored flag is the negative - "restricted" - and this row is
            // phrased as the permission, so it is inverted here rather than
            // left for each client to invert and one of them to forget.
            Self::NewGuildDirectMessages => state.default_guilds_restricted.map(|value| !value),
            Self::FriendsEveryone => state.friend_sources.map(|sources| sources.everyone),
            Self::FriendsMutualFriends => {
                state.friend_sources.map(|sources| sources.mutual_friends)
            }
            Self::FriendsMutualGuilds => state.friend_sources.map(|sources| sources.mutual_guilds),
        }
    }

    /// What the row reads on the right, for settings that are not a plain
    /// on/off. Only DM scanning has three states.
    pub fn value(self, state: &PrivacyState) -> Option<&'static str> {
        match self {
            Self::DmScanning => Some(state.dm_scan_level.map_or("Unknown", DmScanLevel::label)),
            _ => None,
        }
    }

    /// The edit that activating this row would send.
    ///
    /// Names only the field it changes: this endpoint replaces what it is
    /// given, so an edit mentioning anything else would reset it.
    pub fn toggled(self, state: &PrivacyState) -> PrivacyEdit {
        let mut edit = PrivacyEdit::default();
        match self {
            Self::DmScanning => {
                edit.dm_scan_level = Some(state.dm_scan_level.unwrap_or(DmScanLevel::None).next());
            }
            Self::NewGuildDirectMessages => {
                edit.default_guilds_restricted =
                    Some(!state.default_guilds_restricted.unwrap_or(false));
            }
            Self::FriendsEveryone | Self::FriendsMutualFriends | Self::FriendsMutualGuilds => {
                // All three go in one field, so the two not being changed are
                // carried over rather than defaulted.
                let mut sources = state.friend_sources.unwrap_or_default();
                match self {
                    Self::FriendsEveryone => sources.everyone = !sources.everyone,
                    Self::FriendsMutualFriends => sources.mutual_friends = !sources.mutual_friends,
                    _ => sources.mutual_guilds = !sources.mutual_guilds,
                }
                edit.friend_sources = Some(sources);
            }
        }
        edit
    }
}

#[cfg(test)]
mod setting_tests {
    use super::*;

    #[test]
    fn a_setting_that_never_arrived_is_unknown_rather_than_off() {
        let state = PrivacyState::default();
        for setting in PrivacySetting::ALL {
            assert_eq!(
                setting.is_on(&state),
                None,
                "{:?} reports a value it was never told",
                setting
            );
        }
        assert_eq!(PrivacySetting::DmScanning.value(&state), Some("Unknown"));
    }

    #[test]
    fn the_new_server_row_is_the_permission_not_the_restriction() {
        // Discord stores the negative. A row showing the stored flag directly
        // would read as checked exactly when direct messages are turned off.
        let restricted = PrivacyState {
            default_guilds_restricted: Some(true),
            ..PrivacyState::default()
        };
        assert_eq!(
            PrivacySetting::NewGuildDirectMessages.is_on(&restricted),
            Some(false)
        );
        assert_eq!(
            PrivacySetting::NewGuildDirectMessages
                .toggled(&restricted)
                .default_guilds_restricted,
            Some(false)
        );
    }

    #[test]
    fn toggling_one_friend_source_carries_the_other_two() {
        let state = PrivacyState {
            friend_sources: Some(FriendSources {
                everyone: false,
                mutual_friends: true,
                mutual_guilds: true,
            }),
            ..PrivacyState::default()
        };

        assert_eq!(
            PrivacySetting::FriendsEveryone
                .toggled(&state)
                .friend_sources,
            Some(FriendSources {
                everyone: true,
                mutual_friends: true,
                mutual_guilds: true,
            })
        );
    }

    #[test]
    fn every_row_edits_exactly_one_field() {
        // The endpoint replaces what it is given, so a row that named a second
        // field would reset a setting nobody touched.
        let state = PrivacyState::default();
        for setting in PrivacySetting::ALL {
            let edit = setting.toggled(&state);
            let named = u8::from(edit.dm_scan_level.is_some())
                + u8::from(edit.default_guilds_restricted.is_some())
                + u8::from(edit.friend_sources.is_some());
            assert_eq!(named, 1, "{setting:?} names {named} fields");
        }
    }

    #[test]
    fn every_row_has_a_distinct_label() {
        // Two rows reading alike would be two controls nobody can tell apart.
        let mut labels: Vec<&str> = PrivacySetting::ALL.iter().map(|s| s.label()).collect();
        labels.sort_unstable();
        let count = labels.len();
        labels.dedup();
        assert_eq!(labels.len(), count);
    }
}
