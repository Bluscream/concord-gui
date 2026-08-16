//! Actions worth warning about before they happen.
//!
//! Discord's anti-spam heuristics treat third-party clients more harshly than
//! the official one, and a false positive costs the user their account.
//! Abaddon's README names the actions that most often trip them; this is that
//! list, with the explanation each one gets.
//!
//! Nothing here refuses anything. The account is the user's and so is the
//! decision - this exists to make it an informed one, and every warning can be
//! turned off permanently once it has been read.
//!
//! Lives in the core so both front ends warn about the same things in the same
//! words, and a translator has one set of strings to work with.

use crate::config::WarningOptions;

/// What kind of risk a warning is about.
///
/// Deliberately carries no payload: what has to survive the confirmation
/// differs per front end - a nickname here, a built command there - so each
/// keeps its own wrapper and shares only the part that must not diverge.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RiskKind {
    JoinGuild,
    LeaveGuild,
    /// Changing your profile while connected through a third-party client.
    ProfileEdit,
    /// Adding, removing or blocking someone.
    FriendAction,
    /// Removing inactive members. Irreversible, and unlike the others it acts
    /// on people who are not here to notice.
    PruneMembers,
}

impl RiskKind {
    pub const ALL: &'static [Self] = &[
        Self::JoinGuild,
        Self::LeaveGuild,
        Self::ProfileEdit,
        Self::FriendAction,
        Self::PruneMembers,
    ];

    /// Why this is worth a pause, in the active language.
    pub fn explanation(self) -> String {
        match self {
            Self::JoinGuild => crate::t!("warning-join-guild"),
            Self::LeaveGuild => crate::t!("warning-leave-guild"),
            Self::ProfileEdit => crate::t!("warning-profile-edit"),
            Self::FriendAction => crate::t!("warning-friend-action"),
            Self::PruneMembers => crate::t!("warning-prune-members"),
        }
    }

    /// Whether the user has already asked not to be warned about this.
    pub fn suppressed(self, options: &WarningOptions) -> bool {
        match self {
            Self::JoinGuild => options.suppress_join_guild,
            Self::LeaveGuild => options.suppress_leave_guild,
            Self::ProfileEdit => options.suppress_profile_edit,
            Self::FriendAction => options.suppress_friend_action,
            Self::PruneMembers => options.suppress_prune_members,
        }
    }

    /// Stop warning about this.
    pub fn suppress(self, options: &mut WarningOptions) {
        match self {
            Self::JoinGuild => options.suppress_join_guild = true,
            Self::LeaveGuild => options.suppress_leave_guild = true,
            Self::ProfileEdit => options.suppress_profile_edit = true,
            Self::FriendAction => options.suppress_friend_action = true,
            Self::PruneMembers => options.suppress_prune_members = true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_risk_has_its_own_switch() {
        // A shared flag would silence warnings the user never read, which is
        // the one way "don't ask again" can do harm.
        for kind in RiskKind::ALL {
            let mut options = WarningOptions::default();
            kind.suppress(&mut options);

            for other in RiskKind::ALL {
                assert_eq!(
                    other.suppressed(&options),
                    other == kind,
                    "suppressing {kind:?} must not affect {other:?}"
                );
            }
        }
    }

    #[test]
    fn nothing_is_suppressed_to_begin_with() {
        let options = WarningOptions::default();
        for kind in RiskKind::ALL {
            assert!(!kind.suppressed(&options));
        }
    }

    #[test]
    fn every_risk_explains_itself() {
        // An unknown key returns itself, so a missing string shows up here as
        // a body that is just the key.
        for kind in RiskKind::ALL {
            let explanation = kind.explanation();
            assert!(!explanation.starts_with("warning-"), "{kind:?} has no text");
            assert!(explanation.len() > 20, "{kind:?} does not explain much");
        }
    }
}
