//! Linked accounts.
//!
//! Listing, changing what a connection shows, and removing one. Adding a
//! connection is an OAuth flow through a browser and is deliberately not here:
//! it would mean handling somebody else's credentials, which this client does
//! not do.

use serde::Deserialize;
use serde_json::json;

use crate::Result;

use super::DiscordRest;

/// Who can see a connection on a profile.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConnectionVisibility {
    /// Only you.
    Hidden,
    Everyone,
}

impl ConnectionVisibility {
    pub const fn from_code(code: u64) -> Self {
        match code {
            1 => Self::Everyone,
            _ => Self::Hidden,
        }
    }

    pub const fn code(self) -> u8 {
        match self {
            Self::Hidden => 0,
            Self::Everyone => 1,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Hidden => "Only you",
            Self::Everyone => "Everyone",
        }
    }

    pub const fn toggled(self) -> Self {
        match self {
            Self::Hidden => Self::Everyone,
            Self::Everyone => Self::Hidden,
        }
    }
}

/// One linked account.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Connection {
    pub id: String,
    /// Discord's own name for the service - `spotify`, `github`, `steam`.
    pub kind: String,
    /// The username on that service.
    pub name: String,
    /// An unverified connection is one the service never confirmed; Discord
    /// keeps it but will not show it on a profile.
    pub verified: bool,
    /// Whether what you do there appears in your presence.
    pub show_activity: bool,
    pub visibility: ConnectionVisibility,
}

impl Connection {
    /// How the row reads.
    pub fn summary(&self) -> String {
        let mut parts = vec![self.visibility.label().to_owned()];
        if !self.verified {
            // Worth saying: an unverified connection looks linked but does not
            // appear on the profile, which otherwise looks like a bug.
            parts.push("unverified".to_owned());
        }
        if self.show_activity {
            parts.push("shows activity".to_owned());
        }
        parts.join(" - ")
    }
}

#[derive(Deserialize)]
struct ConnectionBody {
    id: Option<String>,
    #[serde(rename = "type")]
    kind: Option<String>,
    name: Option<String>,
    #[serde(default)]
    verified: bool,
    #[serde(default)]
    show_activity: bool,
    #[serde(default)]
    visibility: u64,
}

impl DiscordRest {
    /// Every linked account.
    pub async fn connections(&self) -> Result<Vec<Connection>> {
        let connections: Vec<ConnectionBody> = self
            .send_json(
                self.raw_http
                    .get("https://discord.com/api/v9/users/@me/connections"),
                "connections",
            )
            .await?;

        Ok(connections
            .into_iter()
            .filter_map(|connection| {
                // Without both an id and a type there is nothing to address,
                // so a row for it could neither be changed nor removed.
                Some(Connection {
                    id: connection.id?,
                    kind: connection.kind?,
                    name: connection.name.unwrap_or_default(),
                    verified: connection.verified,
                    show_activity: connection.show_activity,
                    visibility: ConnectionVisibility::from_code(connection.visibility),
                })
            })
            .collect())
    }

    /// Change what a connection shows.
    pub async fn modify_connection(
        &self,
        kind: &str,
        id: &str,
        visibility: ConnectionVisibility,
        show_activity: bool,
    ) -> Result<()> {
        self.send_unit(
            self.raw_http
                .patch(format!(
                    "https://discord.com/api/v9/users/@me/connections/{kind}/{id}"
                ))
                .json(&json!({
                    "visibility": visibility.code(),
                    "show_activity": show_activity,
                })),
            "modify connection",
        )
        .await
    }

    /// Unlink an account.
    pub async fn delete_connection(&self, kind: &str, id: &str) -> Result<()> {
        self.send_unit(
            self.raw_http.delete(format!(
                "https://discord.com/api/v9/users/@me/connections/{kind}/{id}"
            )),
            "remove connection",
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn connection(verified: bool, show_activity: bool) -> Connection {
        Connection {
            id: "1".to_owned(),
            kind: "github".to_owned(),
            name: "someone".to_owned(),
            verified,
            show_activity,
            visibility: ConnectionVisibility::Everyone,
        }
    }

    #[test]
    fn an_unverified_connection_says_so() {
        // It looks linked but does not appear on the profile, which otherwise
        // reads as a bug in this client.
        assert!(connection(false, false).summary().contains("unverified"));
        assert!(!connection(true, false).summary().contains("unverified"));
    }

    #[test]
    fn visibility_round_trips_and_toggles() {
        for visibility in [ConnectionVisibility::Hidden, ConnectionVisibility::Everyone] {
            assert_eq!(
                ConnectionVisibility::from_code(u64::from(visibility.code())),
                visibility
            );
            // Toggling twice returns to where it started, or the control would
            // not be a toggle.
            assert_eq!(visibility.toggled().toggled(), visibility);
        }
    }

    #[test]
    fn an_unknown_visibility_reads_as_hidden() {
        // The safer of the two: showing something on a profile because a code
        // was unrecognised would be the wrong way to be wrong.
        assert_eq!(
            ConnectionVisibility::from_code(99),
            ConnectionVisibility::Hidden
        );
    }
}
