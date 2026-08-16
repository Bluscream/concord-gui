//! The account-settings form, shared by both clients.
//!
//! In the core for the same reason `PrivacySetting` is: the interesting parts -
//! which fields are credentials, that a new password must be confirmed, that
//! the current password is required whenever anything changes - are exactly
//! what drifts when written twice. The clients decide how a field looks; this
//! decides what it means and whether it may be sent.

use crate::discord::{AccountEdit, AppCommand, Secret, password_problem, username_problem};

/// One field of the form.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccountField {
    Username,
    Email,
    NewPassword,
    /// Typed twice. Discord does not ask for this; the client does, because a
    /// mistyped new password locks you out of your own account and the mistake
    /// is invisible at the time it is made.
    ConfirmPassword,
    /// Required for any change at all. Discord rejects the request without it.
    CurrentPassword,
}

impl AccountField {
    pub const ALL: [Self; 5] = [
        Self::Username,
        Self::Email,
        Self::NewPassword,
        Self::ConfirmPassword,
        Self::CurrentPassword,
    ];

    pub fn at(index: usize) -> Option<Self> {
        Self::ALL.get(index).copied()
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Username => "Username",
            Self::Email => "Email",
            Self::NewPassword => "New password",
            Self::ConfirmPassword => "Confirm new password",
            Self::CurrentPassword => "Current password",
        }
    }

    /// Whether the field holds a credential, and so must be drawn as bullets
    /// and kept out of any debug output.
    pub const fn is_secret(self) -> bool {
        matches!(
            self,
            Self::NewPassword | Self::ConfirmPassword | Self::CurrentPassword
        )
    }

    pub const fn hint(self) -> &'static str {
        match self {
            Self::Username => "2 to 32 characters. No @, #, : or ```.",
            Self::Email => "Changing this signs you out of nothing, but Discord will verify it.",
            Self::NewPassword => "At least 6 characters. Leave blank to keep the current one.",
            Self::ConfirmPassword => "Typed twice, because a mistyped one locks you out.",
            Self::CurrentPassword => "Required for any change. Used once and never stored.",
        }
    }
}

/// Why a form cannot be submitted yet.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccountFormProblem {
    NothingChanged,
    /// The current password is blank; Discord rejects every change without it.
    CurrentPasswordMissing,
    PasswordsDoNotMatch,
    Username(&'static str),
    NewPassword(&'static str),
}

impl AccountFormProblem {
    pub fn message(self) -> String {
        match self {
            Self::NothingChanged => "Nothing to change".to_owned(),
            Self::CurrentPasswordMissing => {
                "Your current password is required for any change".to_owned()
            }
            Self::PasswordsDoNotMatch => "The two new passwords do not match".to_owned(),
            Self::Username(reason) => format!("Username {reason}"),
            Self::NewPassword(reason) => format!("New password {reason}"),
        }
    }
}

/// What has been typed so far.
///
/// The `Debug` impl is hand-written: a derived one would print three passwords
/// in full, and `{:?}` on a whole panel is what a debug log does.
#[derive(Clone, Default, Eq, PartialEq)]
pub struct AccountForm {
    username: String,
    email: String,
    new_password: String,
    confirm_password: String,
    current_password: String,
    /// What the account currently is, so an unchanged field is not sent.
    original_username: String,
    original_email: String,
}

impl std::fmt::Debug for AccountForm {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AccountForm")
            .field("username", &self.username)
            .field("email", &self.email)
            .field("new_password", &"[redacted]")
            .field("confirm_password", &"[redacted]")
            .field("current_password", &"[redacted]")
            .finish()
    }
}

impl AccountForm {
    /// Seeded with what the account is now, so an untouched field compares
    /// equal and is left out of the edit.
    pub fn new(username: &str, email: &str) -> Self {
        Self {
            username: username.to_owned(),
            email: email.to_owned(),
            original_username: username.to_owned(),
            original_email: email.to_owned(),
            ..Self::default()
        }
    }

    pub fn value(&self, field: AccountField) -> &str {
        match field {
            AccountField::Username => &self.username,
            AccountField::Email => &self.email,
            AccountField::NewPassword => &self.new_password,
            AccountField::ConfirmPassword => &self.confirm_password,
            AccountField::CurrentPassword => &self.current_password,
        }
    }

    /// What to draw. Bullets for a credential, so it is never on screen.
    pub fn display_value(&self, field: AccountField) -> String {
        let value = self.value(field);
        if field.is_secret() {
            // Characters, not bytes: a multi-byte password would otherwise
            // show more bullets than it has characters.
            "•".repeat(value.chars().count())
        } else {
            value.to_owned()
        }
    }

    pub fn set(&mut self, field: AccountField, value: String) {
        match field {
            AccountField::Username => self.username = value,
            AccountField::Email => self.email = value,
            AccountField::NewPassword => self.new_password = value,
            AccountField::ConfirmPassword => self.confirm_password = value,
            AccountField::CurrentPassword => self.current_password = value,
        }
    }

    pub fn push(&mut self, field: AccountField, character: char) {
        let mut value = self.value(field).to_owned();
        value.push(character);
        self.set(field, value);
    }

    pub fn pop(&mut self, field: AccountField) {
        let mut value = self.value(field).to_owned();
        value.pop();
        self.set(field, value);
    }

    /// The edit this form describes, with unchanged fields left out.
    fn edit(&self) -> AccountEdit {
        AccountEdit {
            username: (self.username != self.original_username).then(|| self.username.clone()),
            email: (self.email != self.original_email).then(|| self.email.clone()),
            new_password: (!self.new_password.is_empty())
                .then(|| Secret::new(self.new_password.clone())),
        }
    }

    /// Why this cannot be submitted, or `None` when it can.
    pub fn problem(&self) -> Option<AccountFormProblem> {
        let edit = self.edit();
        if edit.is_empty() {
            return Some(AccountFormProblem::NothingChanged);
        }
        if let Some(username) = &edit.username
            && let Some(reason) = username_problem(username)
        {
            return Some(AccountFormProblem::Username(reason));
        }
        if let Some(password) = &edit.new_password {
            if let Some(reason) = password_problem(password.expose()) {
                return Some(AccountFormProblem::NewPassword(reason));
            }
            // Checked before the round trip: Discord accepts whatever it is
            // given here, so a typo would be locked in with no way to notice.
            if self.new_password != self.confirm_password {
                return Some(AccountFormProblem::PasswordsDoNotMatch);
            }
        }
        if self.current_password.is_empty() {
            return Some(AccountFormProblem::CurrentPasswordMissing);
        }
        None
    }

    /// The command to send, or `None` when the form is not ready.
    ///
    /// Takes `self` by value: the form is consumed by submitting it, so there
    /// is no copy of three passwords left behind in panel state.
    pub fn submit(self) -> Option<AppCommand> {
        if self.problem().is_some() {
            return None;
        }
        Some(AppCommand::ModifyAccount {
            edit: self.edit(),
            current_password: Secret::new(self.current_password),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form() -> AccountForm {
        AccountForm::new("someone", "someone@example.com")
    }

    fn ready() -> AccountForm {
        let mut form = form();
        form.set(AccountField::Username, "somebody".to_owned());
        form.set(AccountField::CurrentPassword, "hunter2".to_owned());
        form
    }

    #[test]
    fn an_untouched_form_has_nothing_to_send() {
        // Seeded with the current values, so every field compares equal.
        assert_eq!(form().problem(), Some(AccountFormProblem::NothingChanged));
        assert!(form().submit().is_none());
    }

    #[test]
    fn only_changed_fields_are_sent() {
        // Sending an unchanged email would ask Discord to reverify an address
        // that never changed.
        let Some(AppCommand::ModifyAccount { edit, .. }) = ready().submit() else {
            panic!("form should be ready");
        };

        assert_eq!(edit.username.as_deref(), Some("somebody"));
        assert!(edit.email.is_none());
        assert!(edit.new_password.is_none());
    }

    #[test]
    fn every_change_needs_the_current_password() {
        // Discord rejects the request without it, so the form says so rather
        // than spending a round trip to be told.
        let mut incomplete = form();
        incomplete.set(AccountField::Username, "somebody".to_owned());

        assert_eq!(
            incomplete.problem(),
            Some(AccountFormProblem::CurrentPasswordMissing)
        );
        assert!(incomplete.submit().is_none());
    }

    #[test]
    fn a_mistyped_new_password_is_caught_before_it_is_locked_in() {
        // Discord accepts whatever it is given here. A typo would become the
        // real password with no way to notice until the next sign-in.
        let mut mismatched = form();
        mismatched.set(AccountField::NewPassword, "hunter22".to_owned());
        mismatched.set(AccountField::ConfirmPassword, "hunter23".to_owned());
        mismatched.set(AccountField::CurrentPassword, "hunter2".to_owned());

        assert_eq!(
            mismatched.problem(),
            Some(AccountFormProblem::PasswordsDoNotMatch)
        );
        assert!(mismatched.submit().is_none());
    }

    #[test]
    fn a_matching_new_password_goes_through_separately_from_the_current_one() {
        let mut changing = form();
        changing.set(AccountField::NewPassword, "hunter22".to_owned());
        changing.set(AccountField::ConfirmPassword, "hunter22".to_owned());
        changing.set(AccountField::CurrentPassword, "old-one".to_owned());

        let Some(AppCommand::ModifyAccount {
            edit,
            current_password,
        }) = changing.submit()
        else {
            panic!("form should be ready");
        };

        assert_eq!(
            edit.new_password.map(|p| p.expose().to_owned()),
            Some("hunter22".to_owned())
        );
        assert_eq!(current_password.expose(), "old-one");
    }

    #[test]
    fn the_confirmation_is_not_required_when_the_password_is_not_changing() {
        // It would otherwise block a username change for no reason.
        assert_eq!(ready().problem(), None);
    }

    #[test]
    fn an_invalid_username_is_refused_with_a_specific_reason() {
        let mut invalid = form();
        invalid.set(AccountField::Username, "some@one".to_owned());
        invalid.set(AccountField::CurrentPassword, "hunter2".to_owned());

        assert!(matches!(
            invalid.problem(),
            Some(AccountFormProblem::Username(_))
        ));
        assert!(invalid.problem().unwrap().message().contains("@"));
    }

    #[test]
    fn passwords_are_drawn_as_bullets_and_ordinary_fields_are_not() {
        let mut typed = form();
        typed.set(AccountField::CurrentPassword, "hunter2".to_owned());

        assert_eq!(
            typed.display_value(AccountField::CurrentPassword),
            "•••••••"
        );
        assert_eq!(typed.display_value(AccountField::Username), "someone");
    }

    #[test]
    fn bullets_count_characters_not_bytes() {
        let mut typed = form();
        typed.set(AccountField::CurrentPassword, "héllo".to_owned());

        assert_eq!(
            typed
                .display_value(AccountField::CurrentPassword)
                .chars()
                .count(),
            5
        );
    }

    #[test]
    fn no_password_is_ever_printed() {
        // `{:?}` on a whole panel is what a debug log or a failing assertion
        // does, and a derived Debug here would print three passwords.
        let mut typed = form();
        typed.set(AccountField::NewPassword, "new-secret".to_owned());
        typed.set(AccountField::ConfirmPassword, "new-secret".to_owned());
        typed.set(AccountField::CurrentPassword, "old-secret".to_owned());

        let printed = format!("{typed:?}");
        assert!(!printed.contains("new-secret"));
        assert!(!printed.contains("old-secret"));
        // The command it produces must not print them either.
        let command = typed.submit().expect("form should be ready");
        let printed = format!("{command:?}");
        assert!(!printed.contains("new-secret"));
        assert!(!printed.contains("old-secret"));
    }

    #[test]
    fn every_credential_field_is_marked_as_one() {
        // A field missed here would be drawn in plain text on screen.
        for field in AccountField::ALL {
            let expected = matches!(
                field,
                AccountField::NewPassword
                    | AccountField::ConfirmPassword
                    | AccountField::CurrentPassword
            );
            assert_eq!(field.is_secret(), expected, "{field:?}");
        }
    }

    #[test]
    fn typing_and_deleting_reach_every_field() {
        for field in AccountField::ALL {
            let mut typed = form();
            typed.set(field, String::new());
            typed.push(field, 'a');
            typed.push(field, 'b');
            assert_eq!(typed.value(field), "ab", "{field:?}");
            typed.pop(field);
            assert_eq!(typed.value(field), "a", "{field:?}");
        }
    }
}
