//! Turning gateway events into cache writes.
//!
//! The translation is a pure function so the interesting part - what revision
//! each entity gets - is testable without a database. Applying the result is
//! the thin half.
//!
//! Choosing a revision is the whole problem. It must come from Discord, not
//! from either client's clock, and it must increase when the entity changes.
//! What serves varies by entity, and each choice is argued where it is made.

use super::store::{CachedGuild, CachedMessage, CachedUser};

/// What one event asks the cache to do.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Write {
    User(CachedUser),
    Guild(CachedGuild),
    Message(CachedMessage),
    /// Mark a row deleted rather than removing it, so a client with stale
    /// state cannot write it back.
    Tombstone {
        table: &'static str,
        id: String,
        revision: u64,
    },
}

/// A message's revision, from its edit timestamp.
///
/// An unedited message never changes, so it is revision zero and any write
/// wins - every writer has identical content, and letting them through beats
/// leaving a partial row from an interrupted write.
///
/// An edit has to beat the original, and a later edit a earlier one. The id
/// cannot serve: it is monotonic but never changes, so an edit would tie with
/// the message it edits and the guard would keep whichever landed first.
///
/// So: the digits of Discord's own ISO 8601 timestamp, which sort the way the
/// times do. Not parsed into a real date - a parse that failed would have to
/// fall back to something, and there is nothing safe to fall back to.
pub fn message_revision(edited_timestamp: Option<&str>) -> u64 {
    let Some(edited) = edited_timestamp else {
        return 0;
    };
    // YYYYMMDDHHMMSS. Fourteen digits is seconds, which is finer than anyone
    // edits, and fits a u64 with room to spare.
    let digits: String = edited
        .chars()
        .filter(char::is_ascii_digit)
        .take(14)
        .collect();
    digits.parse().unwrap_or(0)
}

/// A guild's revision.
///
/// Discord stamps guilds with a monotonic `version`, but the gateway event
/// this client parses does not carry it through yet. Zero until it does, which
/// makes every write win - last-writer-wins with no guard.
///
/// That is safe here and would not be everywhere: both clients are caching the
/// same server's truth, so the loser of a race writes the same bytes as the
/// winner. It stops being safe the moment anything is written that Discord did
/// not send, which is why this is a named function rather than a bare zero.
pub const fn guild_revision(version: Option<u64>) -> u64 {
    match version {
        Some(version) => version,
        None => 0,
    }
}

/// What to write for a message.
///
/// Takes the event's own type rather than a list of fields: nine parameters of
/// mostly-`u64` is a signature where two can be swapped without the compiler
/// noticing, and this one is called from exactly one place anyway.
pub fn message_writes(message: &crate::discord::MessageInfo) -> Vec<Write> {
    let edited = message.edited_timestamp.as_deref();
    vec![
        // The author too: a restart that drew messages with no author names
        // would be worse than one that drew nothing.
        Write::User(CachedUser {
            id: message.author_id.get().to_string(),
            username: Some(message.author.clone()),
            display_name: Some(message.author.clone()),
            avatar_url: message.author_avatar_url.clone(),
            is_bot: message.author_is_bot,
            revision: 0,
        }),
        Write::Message(CachedMessage {
            id: message.message_id.get().to_string(),
            channel_id: message.channel_id.get().to_string(),
            author_id: Some(message.author_id.get().to_string()),
            content: message.content.clone(),
            // Not carried: the event has no creation timestamp because the
            // snowflake already encodes one, which is the same reason the
            // store orders by id. The column stays for a reader that wants it
            // spelled out without decoding an id.
            timestamp: None,
            edited_timestamp: edited.map(str::to_owned),
            revision: message_revision(edited),
        }),
    ]
}

pub fn guild_write(
    guild_id: u64,
    name: &str,
    owner_id: Option<u64>,
    icon_url: Option<&str>,
    version: Option<u64>,
) -> Write {
    Write::Guild(CachedGuild {
        id: guild_id.to_string(),
        name: Some(name.to_owned()),
        icon_url: icon_url.map(str::to_owned),
        owner_id: owner_id.map(|id| id.to_string()),
        revision: guild_revision(version),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unedited_message_is_revision_zero() {
        // Every writer has identical content, so letting them all through
        // beats leaving a partial row from an interrupted write.
        assert_eq!(message_revision(None), 0);
    }

    #[test]
    fn an_edit_beats_the_message_it_edits() {
        // The case the id cannot serve: an id is monotonic but never changes,
        // so an edit would tie with its original and the guard would keep
        // whichever landed first - which is the unedited one.
        let original = message_revision(None);
        let edited = message_revision(Some("2026-09-01T19:00:00.000000+00:00"));
        assert!(edited > original);
    }

    #[test]
    fn a_later_edit_beats_an_earlier_one() {
        let first = message_revision(Some("2026-09-01T19:00:00+00:00"));
        let second = message_revision(Some("2026-09-01T19:00:01+00:00"));
        assert!(second > first, "{second} should beat {first}");
    }

    #[test]
    fn edits_order_across_a_day_and_a_year_boundary() {
        // Digits of an ISO timestamp sort the way the times do only because
        // the format is fixed-width and big-endian. Worth checking at the
        // boundaries, where a format that was not would break.
        let before_midnight = message_revision(Some("2026-09-01T23:59:59+00:00"));
        let after_midnight = message_revision(Some("2026-09-02T00:00:00+00:00"));
        assert!(after_midnight > before_midnight);

        let old_year = message_revision(Some("2026-12-31T23:59:59+00:00"));
        let new_year = message_revision(Some("2027-01-01T00:00:00+00:00"));
        assert!(new_year > old_year);
    }

    #[test]
    fn a_timestamp_discord_sent_in_a_shape_we_do_not_expect_is_revision_zero() {
        // Zero means "any write wins", which for a message is the same
        // behaviour as before edits were tracked. Guessing a high number would
        // pin the row against every future edit.
        assert_eq!(message_revision(Some("")), 0);
        assert_eq!(message_revision(Some("not a timestamp")), 0);
    }

    #[test]
    fn a_guild_with_a_version_uses_it_and_one_without_takes_any_write() {
        assert_eq!(guild_revision(Some(7)), 7);
        assert_eq!(guild_revision(None), 0);
    }

    #[test]
    fn a_message_also_caches_its_author() {
        // A restart that drew messages with no author names would be worse
        // than one that drew nothing at all.
        let writes = message_writes(&crate::discord::MessageInfo {
            message_id: crate::discord::ids::Id::new(100),
            channel_id: crate::discord::ids::Id::new(200),
            author_id: crate::discord::ids::Id::new(300),
            author: "sam".to_owned(),
            content: Some("hello".to_owned()),
            ..Default::default()
        });

        assert!(writes.iter().any(|write| matches!(
            write,
            Write::User(user) if user.id == "300" && user.username.as_deref() == Some("sam")
        )));
        assert!(writes.iter().any(|write| matches!(
            write,
            Write::Message(message) if message.id == "100" && message.channel_id == "200"
        )));
    }

    #[test]
    fn ids_are_written_as_text_so_a_snowflake_survives() {
        // The column is text for this reason; writing a number here would undo
        // it at the last step.
        let Write::Guild(guild) = guild_write(u64::MAX, "big", None, None, None) else {
            panic!("should be a guild");
        };
        assert_eq!(guild.id, u64::MAX.to_string());
    }
}

/// What one gateway event asks the cache to do.
///
/// Empty for the events that carry nothing worth keeping between runs, which
/// is most of them - typing, presence and voice state are all about right now.
pub fn writes_for(event: &crate::discord::AppEvent) -> Vec<Write> {
    use crate::discord::AppEvent;
    match event {
        AppEvent::MessageCreate { message } => message_writes(message),
        AppEvent::GuildCreate {
            guild_id,
            name,
            owner_id,
            ..
        } => vec![guild_write(
            guild_id.get(),
            name,
            owner_id.map(crate::discord::ids::Id::get),
            None,
            // The gateway event does not carry Discord's guild version yet, so
            // every write wins. Safe while both clients cache the same
            // server's truth; see `guild_revision`.
            None,
        )],
        AppEvent::MessageDelete { message_id, .. } => vec![Write::Tombstone {
            table: "messages",
            id: message_id.get().to_string(),
            // Higher than any edit timestamp, so a delete beats an edit that
            // is still in flight from another client. Nothing legitimate
            // produces a fourteen-digit revision above this.
            revision: u64::MAX,
        }],
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod event_tests {
    use super::*;

    #[test]
    fn an_event_with_nothing_worth_keeping_writes_nothing() {
        // Typing, presence and voice state are about right now, and caching
        // them would fill the store with rows that are wrong by the time it
        // is read.
        let event = crate::discord::AppEvent::TypingStart {
            channel_id: crate::discord::ids::Id::new(1),
            user_id: crate::discord::ids::Id::new(2),
            member: None,
            guild_id: None,
        };
        assert!(writes_for(&event).is_empty());
    }

    #[test]
    fn a_delete_outranks_an_edit_still_in_flight() {
        // Two clients: one sends the edit it just received, the other the
        // delete. The delete has to win, or the message comes back.
        let latest_edit = message_revision(Some("9999-12-31T23:59:59+00:00"));
        let event = crate::discord::AppEvent::MessageDelete {
            channel_id: crate::discord::ids::Id::new(1),
            message_id: crate::discord::ids::Id::new(2),
            guild_id: None,
        };
        let Some(Write::Tombstone { revision, .. }) = writes_for(&event).first().cloned() else {
            panic!("a delete should tombstone");
        };
        assert!(revision > latest_edit);
    }
}
