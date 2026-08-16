//! Active sessions and authorised applications.
//!
//! The two live together because they answer the same question - what else has
//! access to this account - and because both are things people go looking for
//! after a scare, when hunting through two panels is the last thing wanted.
//!
//! Revoking a session needs the account password. This module takes one as an
//! argument and never stores it: it goes into the request and is dropped. The
//! client has nowhere to keep it and no reason to.

use serde::Deserialize;
use serde_json::json;

use crate::Result;

use super::DiscordRest;

/// One logged-in session.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthSession {
    /// Discord identifies a session by a hash, not by an id.
    pub id_hash: String,
    /// The operating system as Discord reported it.
    pub os: String,
    pub platform: String,
    /// Roughly where it signed in from. Discord gives a city and country, and
    /// omits both for a session it cannot place.
    pub location: Option<String>,
    /// Roughly when it was last active, as Discord reported it. Approximate by
    /// Discord's own admission - the field is named for it.
    pub last_used: Option<String>,
    /// Whether this is the session doing the asking.
    ///
    /// Worth knowing before revoking: logging yourself out is a valid thing to
    /// want and a surprising thing to do by accident.
    pub current: bool,
}

impl AuthSession {
    /// How the row reads under the platform.
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if self.current {
            parts.push("this session".to_owned());
        }
        match &self.location {
            Some(location) => parts.push(location.clone()),
            // Said rather than left blank: an empty location reads as a
            // rendering fault rather than as one Discord could not place.
            None => parts.push("location unknown".to_owned()),
        }
        if !self.os.is_empty() {
            parts.push(self.os.clone());
        }
        if let Some(last_used) = &self.last_used {
            parts.push(format!("last used {last_used}"));
        }
        parts.join(" - ")
    }
}

/// One authorised application or bot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthorisedApp {
    /// The token id, which is what revoking addresses.
    pub id: String,
    pub name: String,
    /// What it was granted, in Discord's own words.
    pub scopes: Vec<String>,
}

impl AuthorisedApp {
    pub fn summary(&self) -> String {
        if self.scopes.is_empty() {
            // Discord allows it, and an empty line would read as a failure to
            // load rather than as an app with no scopes.
            return "no permissions granted".to_owned();
        }
        self.scopes.join(", ")
    }
}

#[derive(Deserialize)]
struct SessionBody {
    id_hash: Option<String>,
    #[serde(default)]
    approx_last_used_time: Option<String>,
    client_info: Option<ClientInfoBody>,
}

#[derive(Deserialize)]
struct ClientInfoBody {
    #[serde(default)]
    os: Option<String>,
    #[serde(default)]
    platform: Option<String>,
    #[serde(default)]
    location: Option<String>,
}

#[derive(Deserialize)]
struct SessionsResponse {
    #[serde(default)]
    user_sessions: Vec<SessionBody>,
}

#[derive(Deserialize)]
struct TokenBody {
    id: Option<String>,
    #[serde(default)]
    scopes: Vec<String>,
    application: Option<ApplicationBody>,
}

#[derive(Deserialize)]
struct ApplicationBody {
    #[serde(default)]
    name: Option<String>,
}

impl DiscordRest {
    /// Every session signed in to this account.
    ///
    /// `current_session_id_hash` is the one doing the asking, when known, so a
    /// row can say so.
    pub async fn auth_sessions(
        &self,
        current_session_id_hash: Option<&str>,
    ) -> Result<Vec<AuthSession>> {
        let response: SessionsResponse = self
            .send_json(
                self.raw_http
                    .get("https://discord.com/api/v9/auth/sessions"),
                "sessions",
            )
            .await?;

        Ok(response
            .user_sessions
            .into_iter()
            .filter_map(|session| {
                // Without a hash there is nothing to revoke, so a row for it
                // could not do the one thing the panel is for.
                let id_hash = session.id_hash?;
                let info = session.client_info.unwrap_or(ClientInfoBody {
                    os: None,
                    platform: None,
                    location: None,
                });
                Some(AuthSession {
                    last_used: session
                        .approx_last_used_time
                        .filter(|value| !value.is_empty()),
                    current: current_session_id_hash == Some(id_hash.as_str()),
                    id_hash,
                    os: info.os.unwrap_or_default(),
                    platform: info.platform.unwrap_or_default(),
                    location: info.location.filter(|value| !value.is_empty()),
                })
            })
            .collect())
    }

    /// Log other sessions out.
    ///
    /// The password is used for this request and dropped. Discord requires it
    /// here and nowhere else in this module.
    pub async fn revoke_auth_sessions(&self, id_hashes: &[String], password: &str) -> Result<()> {
        self.send_unit(
            self.raw_http
                .post("https://discord.com/api/v9/auth/sessions/logout")
                .json(&json!({
                    "session_id_hashes": id_hashes,
                    "password": password,
                })),
            "log out sessions",
        )
        .await
    }

    /// Every authorised application and bot.
    pub async fn authorised_apps(&self) -> Result<Vec<AuthorisedApp>> {
        let tokens: Vec<TokenBody> = self
            .send_json(
                self.raw_http
                    .get("https://discord.com/api/v9/oauth2/tokens"),
                "authorised apps",
            )
            .await?;

        Ok(tokens
            .into_iter()
            .filter_map(|token| {
                let id = token.id?;
                let name = token
                    .application
                    .and_then(|application| application.name)
                    .unwrap_or_default();
                Some(AuthorisedApp {
                    id,
                    // An unnamed app still has access, so it is shown with a
                    // placeholder rather than dropped from the list.
                    name: if name.is_empty() {
                        "Unnamed application".to_owned()
                    } else {
                        name
                    },
                    scopes: token.scopes,
                })
            })
            .collect())
    }

    /// Revoke an application's access. No password needed for this one.
    pub async fn revoke_authorised_app(&self, id: &str) -> Result<()> {
        self.send_unit(
            self.raw_http
                .delete(format!("https://discord.com/api/v9/oauth2/tokens/{id}")),
            "revoke application",
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(current: bool, location: Option<&str>) -> AuthSession {
        AuthSession {
            id_hash: "abc".to_owned(),
            os: "Linux".to_owned(),
            platform: "Desktop".to_owned(),
            location: location.map(str::to_owned),
            last_used: None,
            current,
        }
    }

    #[test]
    fn the_session_doing_the_asking_says_so() {
        // Logging yourself out is a valid thing to want and a surprising thing
        // to do by accident, so the row that would do it is marked.
        assert!(
            session(true, Some("Berlin"))
                .summary()
                .contains("this session")
        );
        assert!(
            !session(false, Some("Berlin"))
                .summary()
                .contains("this session")
        );
    }

    #[test]
    fn a_session_discord_could_not_place_says_so_rather_than_showing_a_gap() {
        // A blank location reads as a rendering fault rather than as one
        // Discord has no data for.
        assert!(session(false, None).summary().contains("location unknown"));
    }

    #[test]
    fn a_last_used_time_appears_when_discord_gives_one() {
        let mut session = session(false, Some("Berlin"));
        assert!(!session.summary().contains("last used"));

        session.last_used = Some("2026-08-01".to_owned());
        assert!(session.summary().contains("last used 2026-08-01"));
    }

    #[test]
    fn an_app_with_no_scopes_says_so_rather_than_reading_as_broken() {
        let app = AuthorisedApp {
            id: "1".to_owned(),
            name: "Bot".to_owned(),
            scopes: Vec::new(),
        };
        assert!(app.summary().contains("no permissions"));
    }

    #[test]
    fn an_app_lists_every_scope_it_was_granted() {
        // Truncating would hide access that is really held, which is the
        // opposite of what this panel is for.
        let app = AuthorisedApp {
            id: "1".to_owned(),
            name: "Bot".to_owned(),
            scopes: vec!["identify".to_owned(), "guilds.join".to_owned()],
        };
        let summary = app.summary();

        assert!(summary.contains("identify"));
        assert!(summary.contains("guilds.join"));
    }
}
