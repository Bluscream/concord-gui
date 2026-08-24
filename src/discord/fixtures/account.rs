//! The signed-in account's own settings.
//!
//! Connections, sessions, authorised apps, two-factor and privacy. These
//! describe a real Discord account, and the backend used to refuse all of
//! them on the grounds that inventing one would put made-up sessions and
//! linked accounts on screen as though they were real.
//!
//! That reasoning does not survive contact with a demo whose entire purpose
//! is to be made up: every server, message and person in it is invented
//! already. What the objection was really pointing at is that these must not
//! *behave* as though they reached a real account - so nothing here talks to
//! anything, and the parts that would need a password or an SMS say plainly
//! that they cannot be done offline rather than pretending they succeeded.

use std::sync::Arc;

use crate::discord::{BackupCode, ConnectionVisibility, DiscordState, PrivacyEdit};

/// Apply an edit to a linked account.
pub fn modify_connection(
    connections: &mut [crate::discord::Connection],
    kind: &str,
    id: &str,
    visibility: ConnectionVisibility,
    show_activity: bool,
) {
    if let Some(connection) = connections
        .iter_mut()
        .find(|entry| entry.kind == kind && entry.id == id)
    {
        connection.visibility = visibility;
        connection.show_activity = show_activity;
    }
}

/// Regenerated backup codes.
///
/// A different set each time, because the point of regenerating is that the
/// old ones stop working - handing back the same list would make the button
/// look broken.
pub fn fresh_backup_codes(seed: u64) -> Vec<BackupCode> {
    (0..8)
        .map(|index| BackupCode {
            code: format!(
                "{:04}-{:04}",
                (seed + index * 37) % 10_000,
                (seed + index * 91) % 10_000
            ),
            consumed: false,
        })
        .collect()
}

/// Apply a privacy edit to the session record the panel reads back.
pub fn apply_privacy(state: &mut DiscordState, edit: &PrivacyEdit) {
    let session = Arc::make_mut(&mut state.session);

    if let Some(level) = edit.dm_scan_level {
        session.dm_scan_level = Some(level);
    }
    if let Some(restricted) = edit.default_guilds_restricted {
        session.default_guilds_restricted = Some(restricted);
    }
    if let Some(sources) = edit.friend_sources {
        session.friend_sources = Some(sources);
    }
    if let Some(discovery) = edit.friend_discovery {
        session.friend_discovery = Some(discovery);
    }
    if let Some(detect) = edit.detect_platform_accounts {
        session.detect_platform_accounts = Some(detect);
    }
}

/// Restrict or unrestrict direct messages from one server.
///
/// Kept separate because it is a list edit rather than a flag, and it is the
/// one privacy control reachable from outside the settings window - the
/// server context menu has it.
pub fn set_guild_restricted(
    state: &mut DiscordState,
    guild: crate::discord::Id<crate::discord::marker::GuildMarker>,
    restricted: bool,
) {
    let session = Arc::make_mut(&mut state.session);
    // Absent means "never set", which is not the same as an empty list, so
    // the first restriction has to create one.
    let list = session.restricted_guilds.get_or_insert_with(Vec::new);

    list.retain(|entry| *entry != guild);
    if restricted {
        list.push(guild);
    }
}
