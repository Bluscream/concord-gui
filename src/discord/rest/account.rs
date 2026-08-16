//! Credentials and two-factor enrolment.
//!
//! Every function here takes what the user typed and sends it. Nothing is
//! stored, defaulted or remembered: the client has nowhere to keep a password
//! and no reason to. Passwords arrive as `Secret` so they cannot reach a log
//! on the way here.

use serde::Deserialize;
use serde_json::{Map, Value};

use crate::Result;
use crate::discord::Secret;

use super::DiscordRest;

/// Discord's caps on a username.
pub const MIN_USERNAME_CHARS: usize = 2;
pub const MAX_USERNAME_CHARS: usize = 32;
/// Discord's minimum password length.
pub const MIN_PASSWORD_CHARS: usize = 6;

/// What to change about the account.
///
/// `None` means leave alone. The current password is separate and always
/// required: Discord rejects any of these without it.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AccountEdit {
    pub username: Option<String>,
    pub email: Option<String>,
    pub new_password: Option<Secret>,
}

impl AccountEdit {
    pub fn is_empty(&self) -> bool {
        self.username.is_none() && self.email.is_none() && self.new_password.is_none()
    }

    fn to_body(&self, current_password: &Secret) -> Value {
        let mut fields = Map::new();
        fields.insert(
            "password".to_owned(),
            Value::from(current_password.expose()),
        );
        if let Some(username) = &self.username {
            fields.insert("username".to_owned(), Value::from(username.as_str()));
        }
        if let Some(email) = &self.email {
            fields.insert("email".to_owned(), Value::from(email.as_str()));
        }
        if let Some(password) = &self.new_password {
            fields.insert("new_password".to_owned(), Value::from(password.expose()));
        }
        Value::Object(fields)
    }
}

/// Whether Discord would accept this as a username.
///
/// Checked here so a rejected change costs no round trip, and so the reason is
/// specific rather than Discord's generic complaint.
pub fn username_problem(username: &str) -> Option<&'static str> {
    let count = username.chars().count();
    if count < MIN_USERNAME_CHARS {
        return Some("too short");
    }
    if count > MAX_USERNAME_CHARS {
        return Some("too long");
    }
    // Discord's own rule. `@#:` and triple backtick are refused because they
    // would be ambiguous in a mention or a code fence.
    if username.contains(['@', '#', ':']) || username.contains("```") {
        return Some("cannot contain @, #, : or ```");
    }
    None
}

pub fn password_problem(password: &str) -> Option<&'static str> {
    if password.chars().count() < MIN_PASSWORD_CHARS {
        return Some("too short");
    }
    None
}

#[derive(Deserialize)]
struct BackupCodeBody {
    code: Option<String>,
    #[serde(default)]
    consumed: bool,
}

#[derive(Deserialize)]
struct BackupCodesResponse {
    #[serde(default)]
    backup_codes: Vec<BackupCodeBody>,
}

#[derive(Deserialize)]
struct EnableTotpResponse {
    #[serde(default)]
    backup_codes: Vec<BackupCodeBody>,
}

/// One backup code.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackupCode {
    pub code: String,
    /// A used code is shown struck through rather than hidden, so the count on
    /// screen matches the count Discord issued.
    pub consumed: bool,
}

impl DiscordRest {
    /// Change username, email or password.
    pub async fn modify_account(
        &self,
        edit: &AccountEdit,
        current_password: &Secret,
    ) -> Result<()> {
        if edit.is_empty() {
            return Ok(());
        }

        self.send_unit(
            self.raw_http
                .patch("https://discord.com/api/v9/users/@me")
                .json(&edit.to_body(current_password)),
            "account settings",
        )
        .await
    }

    /// Turn on two-factor authentication.
    ///
    /// The secret was generated locally and shown to the user; the code proves
    /// their authenticator app has it. Returns the backup codes, which are the
    /// only thing standing between a lost phone and a lost account.
    pub async fn enable_totp(
        &self,
        secret: &str,
        code: &str,
        password: &Secret,
    ) -> Result<Vec<BackupCode>> {
        let response: EnableTotpResponse = self
            .send_json(
                self.raw_http
                    .post("https://discord.com/api/v9/users/@me/mfa/totp/enable")
                    .json(&serde_json::json!({
                        "password": password.expose(),
                        "secret": secret,
                        "code": code,
                    })),
                "enable two-factor",
            )
            .await?;

        Ok(collect_codes(response.backup_codes))
    }

    /// Turn two-factor authentication off.
    ///
    /// Takes a current code rather than a password, which is Discord's rule:
    /// the point is to prove the second factor still works before removing it.
    pub async fn disable_totp(&self, code: &str) -> Result<()> {
        self.send_unit(
            self.raw_http
                .post("https://discord.com/api/v9/users/@me/mfa/totp/disable")
                .json(&serde_json::json!({ "code": code })),
            "disable two-factor",
        )
        .await
    }

    /// Fetch or regenerate the backup codes.
    ///
    /// Regenerating invalidates the old ones, so it is a separate argument
    /// rather than something this does on its own.
    pub async fn backup_codes(
        &self,
        password: &Secret,
        regenerate: bool,
    ) -> Result<Vec<BackupCode>> {
        let response: BackupCodesResponse = self
            .send_json(
                self.raw_http
                    .post("https://discord.com/api/v9/users/@me/mfa/codes-verification")
                    .json(&serde_json::json!({
                        "password": password.expose(),
                        "regenerate": regenerate,
                    })),
                "backup codes",
            )
            .await?;

        Ok(collect_codes(response.backup_codes))
    }
}

fn collect_codes(bodies: Vec<BackupCodeBody>) -> Vec<BackupCode> {
    bodies
        .into_iter()
        .filter_map(|body| {
            Some(BackupCode {
                code: body.code?,
                consumed: body.consumed,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_edit_is_not_sent() {
        assert!(AccountEdit::default().is_empty());
        assert!(
            !AccountEdit {
                username: Some("someone".to_owned()),
                ..AccountEdit::default()
            }
            .is_empty()
        );
    }

    #[test]
    fn the_current_password_is_always_sent_and_only_the_changed_fields_are() {
        // Discord rejects any of these without the current password, and
        // sending a field nobody touched would change it to a default.
        let edit = AccountEdit {
            username: Some("someone".to_owned()),
            ..AccountEdit::default()
        };
        let body = edit.to_body(&Secret::new("hunter2"));

        assert_eq!(body["password"], Value::from("hunter2"));
        assert_eq!(body["username"], Value::from("someone"));
        assert!(body.get("email").is_none());
        assert!(body.get("new_password").is_none());
    }

    #[test]
    fn a_new_password_is_sent_separately_from_the_current_one() {
        // Transposing these would set the password to itself and report
        // success, which is the worst possible way for this to fail.
        let edit = AccountEdit {
            new_password: Some(Secret::new("new-one")),
            ..AccountEdit::default()
        };
        let body = edit.to_body(&Secret::new("old-one"));

        assert_eq!(body["password"], Value::from("old-one"));
        assert_eq!(body["new_password"], Value::from("new-one"));
    }

    #[test]
    fn an_edit_carrying_a_password_does_not_print_it() {
        // AccountEdit derives Debug, and `{:?}` on it is what a debug log or a
        // failing assertion does.
        let edit = AccountEdit {
            new_password: Some(Secret::new("hunter2")),
            ..AccountEdit::default()
        };

        assert!(!format!("{edit:?}").contains("hunter2"));
    }

    #[test]
    fn usernames_discord_would_reject_are_refused_here() {
        // Refused locally so the reason is specific rather than Discord's
        // generic complaint, and so a rejection costs no round trip.
        assert_eq!(username_problem("someone"), None);
        assert!(username_problem("a").is_some());
        assert!(username_problem(&"a".repeat(33)).is_some());
        assert!(username_problem("some@one").is_some());
        assert!(username_problem("some#one").is_some());
        assert!(username_problem("some:one").is_some());
        assert!(username_problem("some```one").is_some());
    }

    #[test]
    fn username_length_is_counted_in_characters_not_bytes() {
        // A name of multi-byte characters would otherwise be refused while
        // being well within Discord's limit.
        assert_eq!(username_problem(&"é".repeat(MAX_USERNAME_CHARS)), None);
        assert!(username_problem(&"é".repeat(MAX_USERNAME_CHARS + 1)).is_some());
    }

    #[test]
    fn short_passwords_are_refused_before_the_round_trip() {
        assert_eq!(password_problem("hunter22"), None);
        assert!(password_problem("short").is_some());
    }

    #[test]
    fn a_used_backup_code_is_kept_rather_than_dropped() {
        // Hiding it would make the count on screen disagree with the count
        // Discord issued, which reads as codes having gone missing.
        let codes = collect_codes(vec![
            BackupCodeBody {
                code: Some("aaaa".to_owned()),
                consumed: true,
            },
            BackupCodeBody {
                code: Some("bbbb".to_owned()),
                consumed: false,
            },
            // No code at all: nothing to show or type, so it is dropped.
            BackupCodeBody {
                code: None,
                consumed: false,
            },
        ]);

        assert_eq!(codes.len(), 2);
        assert!(codes[0].consumed);
        assert!(!codes[1].consumed);
    }
}
