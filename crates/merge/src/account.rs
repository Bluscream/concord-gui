//! Which signed-in account something came from.

use std::fmt;

/// One signed-in account, as this crate refers to it.
///
/// Deliberately not the Discord user id. An account is a *session* here: the
/// same person signed in twice is two accounts with one user id, and a merged
/// view that collapsed them would show one inbox where there are two. The id
/// is assigned locally, in the order accounts were added.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AccountId(u8);

impl AccountId {
    /// Accounts are numbered from zero in the order they were added.
    ///
    /// A `u8` because the ceiling is what a person can plausibly read at once,
    /// not what a machine can hold. Anything that wanted thousands would be a
    /// different program.
    pub const fn new(index: u8) -> Self {
        Self(index)
    }

    pub const fn index(self) -> u8 {
        self.0
    }
}

impl fmt::Display for AccountId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "account {}", self.0)
    }
}

/// What to call an account on screen.
///
/// Held separately from the account itself because it is a display concern
/// that changes without the session changing: renaming an account must not
/// look like signing out and back in.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountLabel {
    pub account: AccountId,
    /// What the person calls it - "work", "main". Empty until they say.
    pub name: String,
    /// The account's own username, as a fallback and a disambiguator.
    pub username: String,
}

impl AccountLabel {
    /// What to show, preferring what the person chose.
    ///
    /// Falls back to the username rather than to the account number: "account
    /// 1" tells somebody nothing about which of their accounts it is.
    pub fn display(&self) -> &str {
        if self.name.trim().is_empty() {
            &self.username
        } else {
            &self.name
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_chosen_name_wins_over_the_username() {
        let label = AccountLabel {
            account: AccountId::new(0),
            name: "work".to_owned(),
            username: "sam".to_owned(),
        };
        assert_eq!(label.display(), "work");
    }

    #[test]
    fn an_unnamed_account_shows_its_username_rather_than_its_number() {
        // "account 1" tells somebody nothing about which of their accounts it
        // is, which is the only question the label exists to answer.
        let label = AccountLabel {
            account: AccountId::new(1),
            name: "   ".to_owned(),
            username: "sam".to_owned(),
        };
        assert_eq!(label.display(), "sam");
    }

    #[test]
    fn accounts_sort_in_the_order_they_were_added() {
        // The order is the person's own, so a merged list that reordered
        // itself between runs would look like it had lost track.
        let mut accounts = [AccountId::new(2), AccountId::new(0), AccountId::new(1)];
        accounts.sort();
        assert_eq!(
            accounts,
            [AccountId::new(0), AccountId::new(1), AccountId::new(2)]
        );
    }
}
