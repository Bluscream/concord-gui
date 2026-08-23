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
    Channel(super::CachedChannel),
    Member(super::CachedMember),
    Attachment(super::CachedAttachment),
    Sticker(super::CachedSticker),
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

/// A stand-in revision for rows Discord does not stamp.
///
/// Wall-clock milliseconds. Not a real clock guarantee - two clients whose
/// clocks disagree will disagree about which write is newer - but the rows
/// this is used for carry the same server's truth either way, so the loser of
/// a race writes the same bytes as the winner.
///
/// It has to move forward for a different reason: a tombstone is a revision
/// too. With a constant revision, a deleted channel could never be recreated
/// and a guild you rejoined would stay invisible, because the create could
/// never outrank the delete.
pub fn wall_clock_revision() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64)
}

/// A guild's revision.
///
/// Discord stamps guilds with a monotonic `version`, but the gateway event
/// this client parses does not carry it through yet. Falls back to the wall
/// clock until it does, so that a rejoined guild outranks the tombstone from
/// when it was left.
///
/// That is safe here and would not be everywhere: both clients are caching the
/// same server's truth, so the loser of a race writes the same bytes as the
/// winner. It stops being safe the moment anything is written that Discord did
/// not send, which is why this is a named function rather than a bare clock
/// read.
pub fn guild_revision(version: Option<u64>) -> u64 {
    version.unwrap_or_else(wall_clock_revision)
}

/// What to write for a message.
///
/// Takes the event's own type rather than a list of fields: nine parameters of
/// mostly-`u64` is a signature where two can be swapped without the compiler
/// noticing, and this one is called from exactly one place anyway.
pub fn message_writes(message: &concord::discord::MessageInfo) -> Vec<Write> {
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
            // Recorded rather than the contents themselves: knowing a message
            // had a picture is enough to not replay it wrongly, and caching
            // every attachment is a different and larger job.
            // Attachments are cached, so they no longer count. Embeds,
            // stickers and polls still do: a message replayed without them
            // is wrong in the way that looks like a bug.
            // Stickers are cached too now. Embeds and polls still are not.
            has_extras: !message.embeds.is_empty() || message.poll.is_some(),
            revision: message_revision(edited),
        }),
    ]
    .into_iter()
    .chain(message.attachments.iter().map(|attachment| {
        Write::Attachment(super::CachedAttachment {
            id: attachment.id.get().to_string(),
            message_id: message.message_id.get().to_string(),
            filename: Some(attachment.filename.clone()),
            url: Some(attachment.url.clone()),
            content_type: attachment.content_type.clone(),
            size: attachment.size,
            width: attachment.width.and_then(|width| i64::try_from(width).ok()),
            height: attachment
                .height
                .and_then(|height| i64::try_from(height).ok()),
            description: attachment.description.clone(),
            // Attachments do not change once posted; an edit that removes one
            // arrives as an edit of the message, not of the attachment.
            revision: 0,
        })
    }))
    .chain(message.stickers.iter().map(|sticker| {
        Write::Sticker(super::CachedSticker {
            id: sticker.id.get().to_string(),
            message_id: message.message_id.get().to_string(),
            name: Some(sticker.name.clone()),
            format: sticker.format.to_wire(),
        })
    }))
    .collect()
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
    #[test]
    fn a_sticker_format_reads_back_as_the_format_it_was() {
        // The cache stores Discord's number, so an inverse that disagreed
        // with `from_wire` would turn every cached Lottie into a broken image.
        use concord::discord::StickerFormat;
        for format in [
            StickerFormat::Png,
            StickerFormat::Apng,
            StickerFormat::Lottie,
            StickerFormat::Gif,
        ] {
            assert_eq!(StickerFormat::from_wire(format.to_wire()), format);
        }
    }

    #[test]
    fn a_message_with_a_sticker_is_replayable_because_the_sticker_is_cached() {
        let message = concord::discord::MessageInfo {
            stickers: vec![concord::discord::StickerInfo::new(
                concord::discord::ids::Id::new(4),
                "wave".to_owned(),
                concord::discord::StickerFormat::Lottie,
            )],
            ..Default::default()
        };

        assert!(replayable(&message));
        assert!(
            message_writes(&message)
                .iter()
                .any(|write| matches!(write, Write::Sticker(_)))
        );
    }
    fn replayable(message: &concord::discord::MessageInfo) -> bool {
        message_writes(message)
            .into_iter()
            .find_map(|write| match write {
                Write::Message(cached) => Some(!cached.has_extras),
                _ => None,
            })
            .expect("a message write")
    }

    #[test]
    fn a_message_with_a_picture_is_replayable_because_the_picture_is_cached() {
        // Attachments used to force a skip. Now that the metadata is stored,
        // the message can be drawn with its picture rather than withheld.
        let message = concord::discord::MessageInfo {
            content: Some("look at this".to_owned()),
            attachments: vec![concord::discord::AttachmentInfo {
                id: concord::discord::ids::Id::new(4),
                filename: "cat.png".to_owned(),
                url: "https://cdn.example/cat.png".to_owned(),
                proxy_url: String::new(),
                content_type: Some("image/png".to_owned()),
                size: 10,
                width: Some(2),
                height: Some(3),
                description: None,
                flags: 0,
            }],
            ..Default::default()
        };

        assert!(replayable(&message));
        assert!(
            message_writes(&message)
                .iter()
                .any(|write| matches!(write, Write::Attachment(_))),
            "the attachment itself should be cached"
        );
    }

    #[test]
    fn a_message_with_something_uncacheable_is_still_skipped() {
        // The column earns its keep for whatever is left uncached. A poll
        // replayed as bare text reads as a broken message rather than a
        // pending one, so the message is withheld until the fetch lands.
        let mut message = concord::discord::MessageInfo {
            content: Some("vote".to_owned()),
            ..Default::default()
        };
        assert!(replayable(&message));

        message.poll = Some(concord::discord::PollInfo {
            question: "tea or coffee".to_owned(),
            answers: Vec::new(),
            allow_multiselect: false,
            results_finalized: None,
            total_votes: None,
        });
        assert!(!replayable(&message));
    }

    #[test]
    fn a_plain_message_is_replayable() {
        let message = concord::discord::MessageInfo {
            content: Some("morning".to_owned()),
            ..Default::default()
        };
        assert!(replayable(&message));
    }

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
    fn a_guild_with_a_version_uses_it() {
        assert_eq!(guild_revision(Some(7)), 7);
    }

    #[test]
    fn a_guild_without_a_version_can_still_outrank_an_earlier_tombstone() {
        // The reason this is a clock read rather than a constant: leaving a
        // guild writes a tombstone, and with a fixed revision the rejoin could
        // never outrank it, so the guild would stay invisible for good.
        let left = wall_clock_revision();
        let rejoined = guild_revision(None);

        assert!(rejoined >= left);
        assert!(rejoined > 0);
    }

    #[test]
    fn leaving_a_guild_and_losing_a_channel_are_both_remembered() {
        // Without these, a channel deleted while the client was closed comes
        // back on the next start and stays until the gateway contradicts it.
        let guild = writes_for(&concord::discord::AppEvent::GuildDelete {
            guild_id: concord::discord::ids::Id::new(5),
        });
        assert!(matches!(
            guild.as_slice(),
            [Write::Tombstone { table: "guilds", id, .. }] if id == "5"
        ));

        let channel = writes_for(&concord::discord::AppEvent::ChannelDelete {
            guild_id: None,
            channel_id: concord::discord::ids::Id::new(9),
        });
        assert!(matches!(
            channel.as_slice(),
            [Write::Tombstone { table: "channels", id, .. }] if id == "9"
        ));
    }

    #[test]
    fn a_guild_caches_its_channels_and_members_and_not_just_itself() {
        // Tested through the event rather than the store: a `member_write`
        // that nothing calls stores members perfectly in isolation and caches
        // nothing at all in practice.
        let event = concord::discord::AppEvent::GuildCreate {
            guild_id: concord::discord::ids::Id::new(1),
            name: "server".to_owned(),
            member_count: None,
            owner_id: None,
            boost_tier: concord::discord::GuildBoostTier::default(),
            boost_count: 0,
            verification_level: None,
            mfa_level: None,
            features: None,
            onboarding: None,
            channels: vec![concord::discord::ChannelInfo {
                channel_id: concord::discord::ids::Id::new(2),
                ..Default::default()
            }],
            members: vec![concord::discord::MemberInfo {
                user_id: concord::discord::ids::Id::new(3),
                ..Default::default()
            }],
            presences: Vec::new(),
            roles: None,
            emojis: Vec::new(),
            stickers: Vec::new(),
        };

        let writes = writes_for(&event);
        assert!(writes.iter().any(|write| matches!(write, Write::Guild(_))));
        assert!(
            writes
                .iter()
                .any(|write| matches!(write, Write::Channel(_)))
        );
        assert!(writes.iter().any(|write| matches!(write, Write::Member(_))));
    }

    #[test]
    fn a_member_who_left_is_remembered_under_the_key_the_row_uses() {
        // A tombstone under a different key silently marks nothing, and the
        // member keeps being drawn after every restart.
        let writes = writes_for(&concord::discord::AppEvent::GuildMemberRemove {
            guild_id: concord::discord::ids::Id::new(3),
            user_id: concord::discord::ids::Id::new(7),
        });
        let [Write::Tombstone { table, id, .. }] = writes.as_slice() else {
            panic!("should be one tombstone");
        };
        assert_eq!(*table, "members");
        assert_eq!(
            *id,
            super::super::CachedMember {
                guild_id: "3".to_owned(),
                user_id: "7".to_owned(),
                ..Default::default()
            }
            .id()
        );
    }

    #[test]
    fn a_guild_that_is_merely_unreachable_is_not_forgotten() {
        // An outage arrives as `GuildUnavailable`. Treating it as a removal
        // would empty the sidebar on the next start for a server having a bad
        // day, which is the opposite of what a cache is for.
        assert!(
            writes_for(&concord::discord::AppEvent::GuildUnavailable {
                guild_id: concord::discord::ids::Id::new(5),
            })
            .is_empty()
        );
    }

    #[test]
    fn a_message_also_caches_its_author() {
        // A restart that drew messages with no author names would be worse
        // than one that drew nothing at all.
        let writes = message_writes(&concord::discord::MessageInfo {
            message_id: concord::discord::ids::Id::new(100),
            channel_id: concord::discord::ids::Id::new(200),
            author_id: concord::discord::ids::Id::new(300),
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

/// One channel, as the sidebar needs it.
///
/// Only the columns that draw a sidebar entry. The rest of a channel - its
/// overwrites, thread state, forum tags - is answered by the gateway within
/// seconds and is not worth a schema that has to migrate every time Discord
/// adds a field.
/// How many members of one guild are worth keeping.
///
/// A large server has far more members than anyone scrolls, and the cache is
/// for drawing the list quickly rather than for holding a copy of the server.
/// The gateway sends the rest as the list is scrolled.
pub const CACHED_MEMBERS_PER_GUILD: usize = 200;

/// One member, as the member list needs it.
fn member_write(guild_id: u64, member: &concord::discord::MemberInfo) -> Write {
    Write::Member(super::CachedMember {
        guild_id: guild_id.to_string(),
        user_id: member.user_id.get().to_string(),
        display_name: Some(member.display_name.clone()),
        nickname: member.nickname.clone(),
        avatar_url: member.avatar_url.clone(),
        is_bot: member.is_bot,
        joined_at: member.joined_at.map(|joined| joined.to_rfc3339()),
        role_ids: member
            .role_ids
            .iter()
            .map(|role| role.get().to_string())
            .collect(),
        revision: wall_clock_revision(),
    })
}

fn channel_write(guild_id: u64, channel: &concord::discord::ChannelInfo) -> Write {
    Write::Channel(super::CachedChannel {
        id: channel.channel_id.get().to_string(),
        guild_id: Some(guild_id.to_string()),
        parent_id: channel.parent_id.map(|id| id.get().to_string()),
        name: Some(channel.name.clone()),
        kind: Some(channel.kind.clone()),
        position: channel.position.map(i64::from),
        topic: channel.topic.clone(),
        // No version on the gateway event, as with guilds: the last writer
        // wins, which is correct while every client is caching the same
        // server's truth.
        revision: wall_clock_revision(),
    })
}

/// What one gateway event asks the cache to do.
///
/// Empty for the events that carry nothing worth keeping between runs, which
/// is most of them - typing, presence and voice state are all about right now.
pub fn writes_for(event: &concord::discord::AppEvent) -> Vec<Write> {
    use concord::discord::AppEvent;
    match event {
        AppEvent::MessageCreate { message } => message_writes(message),
        AppEvent::GuildCreate {
            guild_id,
            name,
            owner_id,
            channels,
            members,
            ..
        } => {
            let mut writes = vec![guild_write(
                guild_id.get(),
                name,
                owner_id.map(concord::discord::ids::Id::get),
                None,
                // The gateway event does not carry Discord's guild version
                // yet, so every write wins. Safe while both clients cache the
                // same server's truth; see `guild_revision`.
                None,
            )];
            writes.extend(
                channels
                    .iter()
                    .map(|channel| channel_write(guild_id.get(), channel)),
            );
            writes.extend(
                members
                    .iter()
                    .take(CACHED_MEMBERS_PER_GUILD)
                    .map(|member| member_write(guild_id.get(), member)),
            );
            writes
        }
        // Both are real removals rather than an outage: an unreachable guild
        // arrives as `GuildUnavailable`, which caches nothing, so a server
        // having a bad day does not empty the sidebar on the next start.
        AppEvent::GuildDelete { guild_id } => vec![Write::Tombstone {
            table: "guilds",
            id: guild_id.get().to_string(),
            revision: wall_clock_revision(),
        }],
        AppEvent::GuildMemberRemove { guild_id, user_id } => vec![Write::Tombstone {
            table: "members",
            // The same key `CachedMember::id` builds, since a member row is a
            // guild and a user together.
            id: format!("{}:{}", guild_id.get(), user_id.get()),
            revision: wall_clock_revision(),
        }],
        AppEvent::ChannelDelete { channel_id, .. } => vec![Write::Tombstone {
            table: "channels",
            id: channel_id.get().to_string(),
            revision: wall_clock_revision(),
        }],
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
        let event = concord::discord::AppEvent::TypingStart {
            channel_id: concord::discord::ids::Id::new(1),
            user_id: concord::discord::ids::Id::new(2),
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
        let event = concord::discord::AppEvent::MessageDelete {
            channel_id: concord::discord::ids::Id::new(1),
            message_id: concord::discord::ids::Id::new(2),
            guild_id: None,
        };
        let Some(Write::Tombstone { revision, .. }) = writes_for(&event).first().cloned() else {
            panic!("a delete should tombstone");
        };
        assert!(revision > latest_edit);
    }
}
