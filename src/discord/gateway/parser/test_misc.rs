use crate::discord::ids::Id;
use serde_json::json;

use super::{parse_guild_create, parse_message_create, parse_user_account_event};
use crate::discord::{AppEvent, DiscordState, MentionInfo};

#[test]
fn message_create_parser_preserves_content_and_sticker_names() {
    let cases = [
        (
            "",
            vec![json!({ "id": "11", "name": "Wave", "format_type": 1 })],
            vec!["Wave"],
        ),
        (
            "hello",
            vec![
                json!({ "id": "11", "name": "Wave", "format_type": 1 }),
                json!({ "id": "12", "name": "Heart", "format_type": 1 }),
            ],
            vec!["Wave", "Heart"],
        ),
    ];

    for (raw_content, sticker_items, expected_stickers) in cases {
        let event = parse_message_create(&json!({
            "id": "20",
            "channel_id": "10",
            "author": { "id": "30", "username": "neo" },
            "content": raw_content,
            "sticker_items": sticker_items
        }))
        .expect("message create should parse");
        let AppEvent::MessageCreate { message } = event else {
            panic!("expected message create event");
        };
        assert_eq!(message.content.as_deref(), Some(raw_content));
        assert_eq!(
            message.sticker_names,
            expected_stickers
                .into_iter()
                .map(str::to_owned)
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn message_create_parser_keeps_forwarded_snapshot_fields() {
    let event = parse_message_create(&json!({
        "id": "20",
        "channel_id": "10",
        "author": { "id": "30", "username": "neo" },
        "content": "",
        "attachments": [],
        "message_reference": { "channel_id": "11" },
        "message_snapshots": [{
            "message": {
                "content": "hello <@40>",
                "timestamp": "2026-04-30T12:34:56.000000+00:00",
                "mentions": [{ "id": "40", "username": "alice" }],
                "attachments": [{
                    "id": "41",
                    "filename": "cat.png",
                    "url": "https://cdn.discordapp.com/cat.png",
                    "proxy_url": "https://media.discordapp.net/cat.png",
                    "content_type": "image/png",
                    "size": 2048,
                    "width": 640,
                    "height": 480
                }],
                "sticker_items": [
                    { "id": "42", "name": "Wave", "format_type": 1 }
                ]
            }
        }, {
            "message": {
                "content": ""
            }
        }]
    }))
    .expect("message create should parse");

    let AppEvent::MessageCreate { message } = event else {
        panic!("expected message create event");
    };
    assert_eq!(message.forwarded_snapshots.len(), 2);
    assert_eq!(
        message.forwarded_snapshots[0].content.as_deref(),
        Some("hello <@40>")
    );
    assert_eq!(
        message.forwarded_snapshots[0].source_channel_id,
        Some(Id::new(11))
    );
    assert_eq!(
        message.forwarded_snapshots[0].timestamp.as_deref(),
        Some("2026-04-30T12:34:56.000000+00:00")
    );
    assert_eq!(
        message.forwarded_snapshots[0].mentions,
        vec![mention_info(40, "alice")]
    );
    assert_eq!(
        message.forwarded_snapshots[0].sticker_names,
        vec!["Wave".to_owned()]
    );
    assert_eq!(message.forwarded_snapshots[0].attachments.len(), 1);
    assert_eq!(
        message.forwarded_snapshots[0].attachments[0].filename,
        "cat.png"
    );
    assert_eq!(message.forwarded_snapshots[1].content.as_deref(), Some(""));
}

pub(super) fn mention_info(user_id: u64, display_name: &str) -> MentionInfo {
    MentionInfo::test(Id::new(user_id), display_name.to_owned())
}

pub(super) fn mention_info_with_nick(user_id: u64, nick: &str) -> MentionInfo {
    MentionInfo {
        guild_nick: Some(nick.to_owned()),
        ..MentionInfo::test(Id::new(user_id), nick.to_owned())
    }
}

pub(super) fn thread_payload(id: u64, name: &str) -> serde_json::Value {
    json!({
        "id": id.to_string(),
        "guild_id": "1",
        "parent_id": "2",
        "type": 11,
        "name": name,
        "message_count": 12,
        "total_message_sent": 14,
        "thread_metadata": { "archived": false, "locked": false }
    })
}

#[test]
fn parse_guild_create_reads_name_from_lazy_properties_object() {
    // With user-account capabilities containing LAZY_USER_NOTIFICATIONS,
    // Discord nests guild metadata under `properties` instead of placing
    // `name` / `owner_id` at the root. Concord must look in both places
    // or every guild renders as "unknown".
    let event = parse_guild_create(&json!({
        "id": "100",
        "member_count": 7,
        "channels": [],
        "roles": [],
        "emojis": [],
        "properties": {
            "name": "Lazy Server",
            "owner_id": "42",
        },
    }))
    .expect("guild_create payload should map");

    let AppEvent::GuildCreate {
        guild_id,
        name,
        owner_id,
        member_count,
        ..
    } = event
    else {
        panic!("expected GuildCreate event");
    };
    assert_eq!(guild_id, Id::new(100));
    assert_eq!(name, "Lazy Server");
    assert_eq!(owner_id, Some(Id::new(42)));
    assert_eq!(member_count, Some(7));
}

#[test]
fn parse_guild_create_prefers_root_name_when_both_locations_set() {
    // Guard against future Discord shape drift: if both root-level and
    // nested name are present, the root wins (matches what the official
    // client does).
    let event = parse_guild_create(&json!({
        "id": "100",
        "name": "Root Name",
        "properties": {"name": "Properties Name"},
    }))
    .expect("guild_create payload should map");

    let AppEvent::GuildCreate { name, .. } = event else {
        panic!("expected GuildCreate event");
    };
    assert_eq!(name, "Root Name");
}

#[test]
fn typing_start_extracts_channel_and_user_from_dm_payload() {
    // DM TYPING_START omits guild_id and embeds user_id directly.
    let events = parse_user_account_event(
        &json!({
            "t": "TYPING_START",
            "d": {
                "channel_id": "12345",
                "user_id": "99",
                "timestamp": 1_700_000_000
            }
        })
        .to_string(),
    );
    assert!(matches!(
        events.as_slice(),
        [AppEvent::TypingStart { guild_id, channel_id, user_id, member }]
            if *channel_id == Id::new(12345)
                && *user_id == Id::new(99)
                && guild_id.is_none()
                && member.is_none()
    ));
}

#[test]
fn typing_start_falls_back_to_member_user_id_when_top_level_missing() {
    // Some guild TYPING_START payloads only embed the user id under
    // `member.user.id`. Make sure we still surface the typer.
    let events = parse_user_account_event(
        &json!({
            "t": "TYPING_START",
            "d": {
                "channel_id": "55",
                "guild_id": "77",
                "member": {
                    "nick": "Live Nick",
                    "roles": ["90"],
                    "user": {
                        "id": "42",
                        "username": "typing-user",
                        "global_name": "Typing Global",
                        "bot": true,
                        "avatar": "typing-avatar"
                    }
                },
                "timestamp": 1_700_000_000
            }
        })
        .to_string(),
    );
    assert!(matches!(
        events.as_slice(),
        [AppEvent::TypingStart { guild_id, channel_id, user_id, member }]
            if *channel_id == Id::new(55)
                && *user_id == Id::new(42)
                && *guild_id == Some(Id::new(77))
                && member.as_ref().is_some_and(|member|
                    member.display_name == "Live Nick"
                        && member.username.as_deref() == Some("typing-user")
                        && member.is_bot
                        && member.role_ids == vec![Id::new(90)]
                        && member.role_ids_present
                )
    ));
}

#[test]
fn ready_hydrates_dm_recipients_from_dedupe_user_ids() {
    // With DEDUPE_USER_OBJECTS in capabilities, READY puts users at the
    // top level once and each private channel only carries
    // `recipient_ids`. The dashboard must still show the peer's name
    // and not `dm-{channel_id}`.
    let events = parse_user_account_event(
        &json!({
            "t": "READY",
            "d": {
                "user": { "id": "10", "username": "me" },
                "users": [
                    {
                        "id": "20",
                        "username": "asdf",
                        "global_name": "global",
                        "discriminator": "0",
                    }
                ],
                "private_channels": [
                    {
                        "id": "12345",
                        "type": 1,
                        "recipient_ids": ["20"]
                    }
                ]
            }
        })
        .to_string(),
    );

    let dm = events
        .iter()
        .find_map(|event| match event {
            AppEvent::ChannelUpsert(info) if info.kind == "dm" => Some(info),
            _ => None,
        })
        .expect("dm channel upsert should be emitted");
    assert_eq!(dm.name, "global");
    let recipients = dm.recipients.as_ref().expect("recipients hydrated");
    assert_eq!(recipients.len(), 1);
    assert_eq!(recipients[0].user_id, Id::new(20));
    assert_eq!(recipients[0].display_name, "global");
    assert_eq!(recipients[0].username.as_deref(), Some("asdf"));

    let mut state = DiscordState::default();
    for event in &events {
        state.apply_event(event);
    }
    let supplemental = parse_user_account_event(
        &json!({
            "t": "READY_SUPPLEMENTAL",
            "d": {
                "lazy_private_channels": [{
                    "id": "54321",
                    "type": 3,
                    "recipient_ids": ["20"]
                }]
            }
        })
        .to_string(),
    );
    for event in &supplemental {
        state.apply_event(event);
    }

    let group_dm = state
        .channel(Id::new(54321))
        .expect("supplemental group DM should be cached");
    assert_eq!(group_dm.name, "global, me");
    assert_eq!(
        group_dm
            .recipients
            .iter()
            .map(|recipient| recipient.user_id)
            .collect::<Vec<_>>(),
        vec![Id::new(20), Id::new(10)]
    );
}

#[test]
fn guild_delete_distinguishes_outages_from_membership_removal() {
    let unavailable = parse_user_account_event(
        &json!({
            "t": "GUILD_DELETE",
            "d": { "id": "10", "unavailable": true }
        })
        .to_string(),
    );
    let removed = parse_user_account_event(
        &json!({
            "t": "GUILD_DELETE",
            "d": { "id": "10" }
        })
        .to_string(),
    );

    assert!(matches!(
        unavailable.as_slice(),
        [AppEvent::GuildUnavailable { guild_id }] if *guild_id == Id::new(10)
    ));
    assert!(matches!(
        removed.as_slice(),
        [AppEvent::GuildDelete { guild_id }] if *guild_id == Id::new(10)
    ));
}
