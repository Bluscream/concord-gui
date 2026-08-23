//! When two accounts see the same thing, and when they only appear to.
//!
//! This is the whole problem. Everything a merged view does - one guild list,
//! one inbox, one unread count - is a decision about which rows collapse into
//! one and which stay apart, and the right answer is different per entity in
//! ways that look arbitrary until you hit them.
//!
//! The rule is not "same snowflake, same thing". A snowflake identifies an
//! object on Discord's side; it does not say the two accounts have the same
//! relationship to it. Sometimes that relationship *is* the thing being shown.

use std::collections::BTreeSet;

use crate::account::AccountId;

/// Something one or more accounts can see.
///
/// The accounts are a set rather than a count: "which" is the question a
/// person asks of a merged row - and a count cannot answer whether the guild
/// in front of them is the one their work account is in.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Shared<T> {
    pub value: T,
    accounts: BTreeSet<AccountId>,
}

impl<T> Shared<T> {
    pub fn new(value: T, account: AccountId) -> Self {
        Self {
            value,
            accounts: BTreeSet::from([account]),
        }
    }

    /// Note that another account sees this too.
    pub fn also_seen_by(&mut self, account: AccountId) {
        self.accounts.insert(account);
    }

    pub fn accounts(&self) -> impl Iterator<Item = AccountId> + '_ {
        self.accounts.iter().copied()
    }

    /// Whether more than one account sees this.
    ///
    /// Worth knowing on screen: a guild both accounts are in behaves
    /// differently from one only a single account can act in, and a person who
    /// cannot tell will eventually post from the wrong identity.
    pub fn is_shared(&self) -> bool {
        self.accounts.len() > 1
    }

    /// The account to act as, when something has to pick one.
    ///
    /// The lowest-numbered, which is the earliest added. Arbitrary but stable:
    /// an arbitrary rule that changed between runs would send from a different
    /// identity each time, which is the failure worth avoiding.
    pub fn primary(&self) -> AccountId {
        self.accounts
            .iter()
            .next()
            .copied()
            .expect("a shared value always has at least one account")
    }
}

/// Whether two sightings of the same snowflake are one row or two.
///
/// Named per entity rather than decided by a single rule, because a single
/// rule is wrong for at least one of them and the wrongness is quiet.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SharedBy {
    /// One row. The object is the same and both accounts relate to it the
    /// same way: a server is a server whoever is looking.
    Snowflake,
    /// One row per account, even for the same snowflake. Membership is what is
    /// being shown, and each account has its own.
    AccountAndSnowflake,
    /// Never collapsed. The object belongs to one account by definition.
    Account,
}

impl SharedBy {
    /// How a guild merges. Two accounts in one server see one server.
    pub const GUILD: Self = Self::Snowflake;

    /// How a guild channel merges.
    ///
    /// One row, like the guild: the channel is the same channel. What differs
    /// is permissions and read state, which hang off the row rather than
    /// splitting it.
    pub const GUILD_CHANNEL: Self = Self::Snowflake;

    /// How a direct message merges.
    ///
    /// Never. A DM channel is a conversation *between* two users, so the same
    /// snowflake cannot be seen by two accounts - and two accounts talking to
    /// the same person have two different conversations with two different
    /// histories. Merging them would interleave messages that were never in
    /// one thread.
    pub const DIRECT_MESSAGE: Self = Self::Account;

    /// How a friend merges.
    ///
    /// Per account: the same person can be a friend of one account and a
    /// stranger to the other, and a merged list that showed them once would
    /// have to lie about which.
    pub const FRIEND: Self = Self::AccountAndSnowflake;

    /// Whether rows with this rule may be collapsed at all.
    pub const fn collapses(self) -> bool {
        matches!(self, Self::Snowflake)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(index: u8) -> AccountId {
        AccountId::new(index)
    }

    #[test]
    fn a_server_both_accounts_are_in_is_one_row_that_names_both() {
        let mut guild = Shared::new("a server", account(0));
        guild.also_seen_by(account(1));

        assert!(guild.is_shared());
        assert_eq!(
            guild.accounts().collect::<Vec<_>>(),
            [account(0), account(1)]
        );
    }

    #[test]
    fn seeing_the_same_thing_twice_from_one_account_does_not_make_it_shared() {
        // The same guild arrives on reconnect. A count would reach two and
        // claim both accounts are in a server only one of them is in.
        let mut guild = Shared::new("a server", account(0));
        guild.also_seen_by(account(0));

        assert!(!guild.is_shared());
    }

    #[test]
    fn the_account_to_act_as_is_the_same_one_every_run() {
        // An arbitrary rule is fine; an unstable one sends from a different
        // identity each time, which is the failure worth avoiding.
        let mut from_low = Shared::new((), account(0));
        from_low.also_seen_by(account(2));
        let mut from_high = Shared::new((), account(2));
        from_high.also_seen_by(account(0));

        assert_eq!(from_low.primary(), from_high.primary());
        assert_eq!(from_low.primary(), account(0));
    }

    #[test]
    fn direct_messages_are_never_collapsed() {
        // Two accounts talking to the same person have two conversations with
        // two histories. Collapsing them would interleave messages that were
        // never in one thread.
        assert!(!SharedBy::DIRECT_MESSAGE.collapses());
    }

    #[test]
    fn friends_are_per_account_even_though_a_user_id_is_global() {
        // The snowflake is the same person; the friendship is not the same
        // friendship. This is the case a single "same id, same row" rule gets
        // wrong quietly.
        assert!(!SharedBy::FRIEND.collapses());
        assert_eq!(SharedBy::FRIEND, SharedBy::AccountAndSnowflake);
    }

    #[test]
    fn servers_and_their_channels_agree() {
        // A channel that split while its guild collapsed would put one
        // server's channels under two headings.
        assert!(SharedBy::GUILD.collapses());
        assert!(SharedBy::GUILD_CHANNEL.collapses());
    }
}
