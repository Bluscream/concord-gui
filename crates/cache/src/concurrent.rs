//! Two clients writing one store.
//!
//! Worth stating plainly, because it decides how much machinery this needs:
//! **Discord is the single writer.** Neither client invents data. Both are
//! caching what one server told them, and that server stamps what it sends.
//!
//! So this is not the general distributed-write problem, and reaching for
//! vector clocks or a consensus library would be solving a problem we do not
//! have. Two properties do the work:
//!
//! 1. **Every entity carries an id Discord assigned.** A snowflake is unique
//!    across all of Discord, so two clients inserting the same message insert
//!    the same primary key. "Already added by another client" is answered by
//!    the key, not by a check - and a check would be a race anyway, since
//!    another client can write between the read and the insert.
//!
//! 2. **Mutable entities carry a version Discord assigned.** Guilds and
//!    channels have a monotonic `version`; messages have `edited_timestamp`,
//!    and an unedited message never changes. So a writer can be told to lose:
//!    the update only applies when the incoming revision is at least the
//!    stored one.
//!
//! Together those make each row a last-write-wins register where the clock
//! comes from Discord rather than from any client. Nothing depends on the two
//! clients agreeing about time, which matters because they will not.
//!
//! What is left is deletion, which the two properties above do not cover: a
//! client holding stale state can re-insert a row another client has just
//! learned was deleted. That needs a tombstone, below.

/// Discord's own ordering stamp for a row.
///
/// Not a timestamp from either client. Two clients disagree about the time by
/// however far their clocks differ, and a cache that resolved conflicts with a
/// local clock would let the more wrong one win.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub struct Revision(pub u64);

impl Revision {
    /// Whether a write carrying this revision should replace what is stored.
    ///
    /// Equal revisions are allowed through rather than skipped: the same
    /// version arriving twice is the common case when two clients receive the
    /// same gateway event, and rewriting identical data is harmless while
    /// skipping it would leave a partial row from an interrupted write.
    pub const fn supersedes(self, stored: Self) -> bool {
        self.0 >= stored.0
    }
}

/// Why a row is absent.
///
/// Distinguished because they behave differently on a shared store: a row
/// nobody has fetched should be fetched, and a row Discord deleted should not
/// be re-inserted by a client that has not noticed yet.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Absence {
    NeverFetched,
    Deleted,
}

/// Whether a write should proceed, given what the store already holds.
///
/// The decision is made here and applied in one SQL statement rather than as a
/// read followed by a write - between those two another client can write, and
/// the result is the sort of loss that only appears under load.
pub fn should_write(incoming: Revision, stored: Option<Revision>, tombstoned: bool) -> bool {
    if tombstoned {
        // A tombstone is only lifted by a revision that beats it, which is how
        // a delete followed by a genuine re-creation still works: Discord
        // would give the new row a higher version.
        return stored.is_none_or(|stored| incoming.0 > stored.0);
    }
    stored.is_none_or(|stored| incoming.supersedes(stored))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_row_nobody_has_written_is_written() {
        assert!(should_write(Revision(1), None, false));
    }

    #[test]
    fn a_newer_revision_replaces_an_older_one() {
        assert!(should_write(Revision(2), Some(Revision(1)), false));
    }

    #[test]
    fn a_stale_writer_does_not_clobber_fresh_data() {
        // The case this module exists for: two clients hold different versions
        // of a guild, and the one that is behind must not win by writing last.
        assert!(!should_write(Revision(1), Some(Revision(2)), false));
    }

    #[test]
    fn the_same_revision_twice_is_allowed_through() {
        // Two clients receiving the same gateway event is the common case, not
        // an anomaly. Skipping it would leave a partial row from a write that
        // was interrupted at the same revision.
        assert!(should_write(Revision(2), Some(Revision(2)), false));
    }

    #[test]
    fn a_deleted_row_is_not_resurrected_by_a_client_that_has_not_noticed() {
        // Without this, a client holding stale state re-inserts what another
        // client has just learned was deleted, and the row comes back.
        assert!(!should_write(Revision(2), Some(Revision(2)), true));
        assert!(!should_write(Revision(1), Some(Revision(2)), true));
    }

    #[test]
    fn a_genuine_recreation_lifts_the_tombstone() {
        // Discord gives a re-created entity a higher version, so beating the
        // tombstone is exactly the right condition - a delete is not permanent
        // and treating it as such would strand the new row.
        assert!(should_write(Revision(3), Some(Revision(2)), true));
    }

    #[test]
    fn revisions_order_the_way_a_version_number_does() {
        assert!(Revision(2).supersedes(Revision(1)));
        assert!(Revision(1).supersedes(Revision(1)));
        assert!(!Revision(1).supersedes(Revision(2)));
    }

    #[test]
    fn an_unfetched_row_and_a_deleted_one_are_different_answers() {
        // A row nobody has fetched should be fetched. A deleted one should not
        // be, and a store that returned the same "missing" for both would send
        // every client back to Discord for rows it knows are gone.
        assert_ne!(Absence::NeverFetched, Absence::Deleted);
    }
}
