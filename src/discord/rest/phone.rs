//! Phone number as a credential, and SMS two-factor.
//!
//! Mobile-only in the official clients, and in the core here for the same
//! reason: a future mobile front end would need it, and a core that only
//! serves the two clients in this repository is one that has to be reopened
//! later. Neither the TUI nor the GUI surfaces all of it today.
//!
//! Adding a number is three steps, not one. Discord sends a code, the code is
//! exchanged for a token, and only then does the number attach - and the last
//! step wants the account password as well. A client that modelled it as a
//! single call would appear to work and leave the number unattached.

use serde::Deserialize;
use serde_json::json;

use crate::Result;
use crate::discord::Secret;

use super::DiscordRest;

/// Why a phone number is being added, which Discord records.
///
/// Sent because it changes what Discord does afterwards: a number added to
/// satisfy a server's verification level is treated differently from one added
/// to turn on SMS two-factor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PhoneChangeReason {
    /// A server demands a verified phone before you may participate.
    GuildVerification,
    /// Turning on SMS two-factor.
    EnableSmsMfa,
}

impl PhoneChangeReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::GuildVerification => "guild_phone_required",
            Self::EnableSmsMfa => "mfa_phone_update",
        }
    }
}

/// A phone number on its way to being verified.
///
/// Carries the token from step two into step three. Not a `Secret`: it is not
/// a credential on its own, it is single-use, and hiding it would make the
/// three-step flow impossible to debug from a log.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PhoneVerificationToken(String);

impl PhoneVerificationToken {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Deserialize)]
struct VerificationBody {
    token: Option<String>,
}

/// Whether Discord would accept this as a phone number.
///
/// E.164: a leading `+`, then digits. Checked here because Discord's rejection
/// does not distinguish a badly formatted number from one it refuses for
/// another reason, and the two need different things from the user.
pub fn phone_number_problem(phone: &str) -> Option<&'static str> {
    let trimmed = phone.trim();
    let Some(digits) = trimmed.strip_prefix('+') else {
        return Some("must start with + and a country code");
    };
    if !digits.chars().all(|character| character.is_ascii_digit()) {
        return Some("may only contain digits after the +");
    }
    // E.164 allows at most fifteen digits, and at least a country code plus a
    // subscriber number.
    if !(4..=15).contains(&digits.chars().count()) {
        return Some("is not a valid length");
    }
    None
}

impl DiscordRest {
    /// Step one: ask Discord to send a code to this number.
    pub async fn send_phone_code(&self, phone: &str, reason: PhoneChangeReason) -> Result<()> {
        if let Some(problem) = phone_number_problem(phone) {
            return Err(crate::AppError::DiscordRequest(format!(
                "the phone number {problem}"
            )));
        }

        self.send_unit(
            self.raw_http
                .post("https://discord.com/api/v9/users/@me/phone")
                .json(&json!({
                    "phone": phone.trim(),
                    "change_phone_reason": reason.as_str(),
                })),
            "send phone code",
        )
        .await
    }

    /// Step two: exchange the code for a token.
    pub async fn verify_phone_code(
        &self,
        phone: &str,
        code: &str,
    ) -> Result<PhoneVerificationToken> {
        let body: VerificationBody = self
            .send_json(
                self.raw_http
                    .post("https://discord.com/api/v9/phone-verifications/verify")
                    .json(&json!({ "phone": phone.trim(), "code": code.trim() })),
                "verify phone code",
            )
            .await?;

        body.token.map(PhoneVerificationToken).ok_or_else(|| {
            crate::AppError::DiscordRequest(
                "Discord accepted the code but returned no verification token".to_owned(),
            )
        })
    }

    /// Ask for the code again, for a number that never received the first.
    pub async fn resend_phone_code(&self, phone: &str) -> Result<()> {
        self.send_unit(
            self.raw_http
                .post("https://discord.com/api/v9/phone-verifications/resend")
                .json(&json!({ "phone": phone.trim() })),
            "resend phone code",
        )
        .await
    }

    /// Step three: attach the number, which needs the token and the password.
    pub async fn attach_phone(
        &self,
        phone: &str,
        token: &PhoneVerificationToken,
        password: &Secret,
        reason: PhoneChangeReason,
    ) -> Result<()> {
        self.send_unit(
            self.raw_http
                .post("https://discord.com/api/v9/users/@me/phone")
                .json(&json!({
                    "phone": phone.trim(),
                    "phone_token": token.as_str(),
                    "password": password.expose(),
                    "change_phone_reason": reason.as_str(),
                })),
            "attach phone",
        )
        .await
    }

    /// Reverify a number Discord has asked about, which is the same flow with
    /// a different last step.
    pub async fn reverify_phone(&self, phone: &str, token: &PhoneVerificationToken) -> Result<()> {
        self.send_unit(
            self.raw_http
                .post("https://discord.com/api/v9/users/@me/phone/reverify")
                .json(&json!({
                    "phone": phone.trim(),
                    "phone_token": token.as_str(),
                })),
            "reverify phone",
        )
        .await
    }

    pub async fn remove_phone(&self, password: &Secret) -> Result<()> {
        self.send_unit(
            self.raw_http
                .delete("https://discord.com/api/v9/users/@me/phone")
                .json(&json!({ "password": password.expose() })),
            "remove phone",
        )
        .await
    }

    /// Turn SMS two-factor on or off.
    ///
    /// Needs a phone number already attached; Discord refuses otherwise, which
    /// is why the two live in one module.
    pub async fn set_sms_mfa(&self, enabled: bool, password: &Secret) -> Result<()> {
        let action = if enabled { "enable" } else { "disable" };
        self.send_unit(
            self.raw_http
                .post(format!(
                    "https://discord.com/api/v9/users/@me/mfa/sms/{action}"
                ))
                .json(&json!({ "password": password.expose() })),
            "sms two-factor",
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_well_formed_number_is_accepted() {
        assert_eq!(phone_number_problem("+441632960001"), None);
        // Surrounding space is trimmed rather than refused: it arrives from a
        // paste more often than from typing.
        assert_eq!(phone_number_problem("  +441632960001 "), None);
    }

    #[test]
    fn a_number_without_a_country_code_is_refused_with_the_reason() {
        // Discord's own rejection does not distinguish a badly formatted
        // number from one it refuses for another reason, and the two need
        // different things from the person typing.
        assert!(
            phone_number_problem("07700900001")
                .is_some_and(|problem| problem.contains("country code"))
        );
    }

    #[test]
    fn separators_people_type_are_refused_rather_than_silently_stripped() {
        // Stripping them would send a different number from the one on screen,
        // and the code goes to whatever was sent.
        for input in ["+44 1632 960001", "+44-1632-960001", "+44(1632)960001"] {
            assert!(
                phone_number_problem(input).is_some(),
                "{input} was accepted"
            );
        }
    }

    #[test]
    fn lengths_outside_e164_are_refused() {
        assert!(phone_number_problem("+1").is_some());
        assert!(phone_number_problem(&format!("+{}", "1".repeat(16))).is_some());
        assert_eq!(phone_number_problem(&format!("+{}", "1".repeat(15))), None);
    }

    #[test]
    fn an_empty_number_is_refused() {
        assert!(phone_number_problem("").is_some());
        assert!(phone_number_problem("+").is_some());
    }

    #[test]
    fn the_reason_discord_records_is_sent_verbatim() {
        // It changes what Discord does afterwards, so a transposed string
        // records the wrong reason for a change that otherwise succeeds.
        assert_eq!(
            PhoneChangeReason::GuildVerification.as_str(),
            "guild_phone_required"
        );
        assert_eq!(PhoneChangeReason::EnableSmsMfa.as_str(), "mfa_phone_update");
    }

    #[test]
    fn a_missing_token_is_an_error_rather_than_an_empty_one() {
        // Step three would otherwise send an empty token and fail with a
        // message about the phone number rather than about the code.
        let body: VerificationBody = serde_json::from_str("{}").expect("should parse");
        assert!(body.token.is_none());
    }
}
