use crate::discord::ids::Id;
use serde_json::{Value, json};

use super::{
    parse_channel_info, parse_guild_create, parse_guild_emojis_update, parse_guild_update,
    parse_message_create, parse_message_info, parse_message_update, parse_user_account_dispatch,
    parse_user_account_event,
};
use crate::discord::{
    ActivityKind, AppEvent, AttachmentUpdate, ChannelVisibilityStats, DiscordState, FriendStatus,
    GuildMemberListItem, GuildMemberListOperation, GuildOnboardingMode, GuildVerificationLevel,
    MentionInfo, MessageKind, NotificationLevel, PollAnswerInfo, PollInfo, PremiumTier,
    PresenceStatus, ReactionEmoji, ReplyInfo,
};



#[test]
fn raw_thread_list_sync_upserts_all_threads() {
    let events = parse_user_account_event(
        &json!({
            "t": "THREAD_LIST_SYNC",
            "d": {
                "guild_id": "1",
                "channel_ids": ["2"],
                "threads": [
                    thread_payload(10, "release notes"),
                    thread_payload(11, "bug reports")
                ],
                "members": [
                    { "id": "10", "user_id": "99", "flags": 3 },
                    { "id": "11", "user_id": "99", "flags": 8 }
                ]
            }
        })
        .to_string(),
    );

    match events.as_slice() {
        [AppEvent::ThreadListSync { sync }] => {
            assert_eq!(sync.guild_id, Id::new(1));
            assert_eq!(sync.channel_ids, Some(vec![Id::new(2)]));
            assert_eq!(sync.threads.len(), 2);
            assert_eq!(sync.threads[0].channel_id, Id::new(10));
            assert_eq!(sync.threads[0].name, "release notes");
            assert_eq!(
                sync.threads[0].current_user_thread_notification_flags,
                Some(3)
            );
            assert_eq!(sync.threads[1].channel_id, Id::new(11));
            assert_eq!(sync.threads[1].name, "bug reports");
            assert_eq!(
                sync.threads[1].current_user_thread_notification_flags,
                Some(8)
            );
        }
        other => panic!("expected one ThreadListSync, got {other:?}"),
    }
}

#[test]
fn raw_empty_thread_list_sync_preserves_the_replacement_scope() {
    let events = parse_user_account_event(
        &json!({
            "t": "THREAD_LIST_SYNC",
            "d": {
                "guild_id": "1",
                "channel_ids": ["2"],
                "threads": [],
                "members": []
            }
        })
        .to_string(),
    );

    assert!(matches!(
        events.as_slice(),
        [AppEvent::ThreadListSync { sync }]
            if sync.guild_id == Id::new(1)
                && sync.channel_ids == Some(vec![Id::new(2)])
                && sync.threads.is_empty()
    ));
}

#[test]
fn raw_group_dm_recipient_events_preserve_the_user_delta() {
    let added = parse_user_account_event(
        &json!({
            "t": "CHANNEL_RECIPIENT_ADD",
            "d": {
                "channel_id": "10",
                "user": {
                    "id": "20",
                    "username": "alice",
                    "global_name": "Alice"
                }
            }
        })
        .to_string(),
    );
    let removed = parse_user_account_event(
        &json!({
            "t": "CHANNEL_RECIPIENT_REMOVE",
            "d": {
                "channel_id": "10",
                "user": { "id": "20" }
            }
        })
        .to_string(),
    );

    assert!(matches!(
        added.as_slice(),
        [AppEvent::ChannelRecipientAdd { channel_id, recipient }]
            if *channel_id == Id::new(10)
                && recipient.user_id == Id::new(20)
                && recipient.display_name == "Alice"
    ));
    assert!(matches!(
        removed.as_slice(),
        [AppEvent::ChannelRecipientRemove { channel_id, user_id }]
            if *channel_id == Id::new(10) && *user_id == Id::new(20)
    ));
}

#[test]
fn message_update_parser_distinguishes_absent_and_empty_attachments() {
    let cases = [
        (
            json!({
                "id": "20",
                "channel_id": "10",
                "content": "edited"
            }),
            false,
        ),
        (
            json!({
                "id": "20",
                "channel_id": "10",
                "content": "edited",
                "attachments": []
            }),
            true,
        ),
    ];

    for (payload, clears_attachments) in cases {
        let event = parse_message_update(&payload).expect("message update should parse");
        let AppEvent::MessageUpdateDispatch { update } = event else {
            panic!("expected message update event");
        };
        if clears_attachments {
            assert!(
                matches!(update.fields.attachments, AttachmentUpdate::Replace(values) if values.is_empty())
            );
        } else {
            assert!(matches!(
                update.fields.attachments,
                AttachmentUpdate::Unchanged
            ));
        }
    }
}

#[test]
fn guild_create_parser_keeps_custom_emojis() {
    let event = parse_guild_create(&json!({
        "id": "1",
        "name": "guild",
        "member_count": 123,
        "channels": [],
        "members": [],
        "presences": [],
        "emojis": [
            {
                "id": "50",
                "name": "party",
                "animated": true,
                "available": true
            },
            {
                "id": "51",
                "name": "sleep",
                "available": false
            }
        ]
    }))
    .expect("guild create should parse");

    let AppEvent::GuildCreate {
        member_count,
        emojis,
        ..
    } = event
    else {
        panic!("expected guild create event");
    };
    assert_eq!(member_count, Some(123));
    assert_eq!(emojis.len(), 2);
    assert_eq!(emojis[0].id, Id::new(50));
    assert_eq!(emojis[0].name, "party");
    assert!(emojis[0].animated);
    assert!(emojis[0].available);
    assert!(!emojis[1].available);
}

#[test]
fn guild_create_parser_keeps_roles() {
    let event = parse_guild_create(&json!({
        "id": "1",
        "name": "guild",
        "channels": [],
        "members": [],
        "presences": [],
        "roles": [{
            "id": "90",
            "name": "Admin",
            "color": 16755200,
            "position": 10,
            "hoist": true
        }],
        "emojis": []
    }))
    .expect("guild create should parse");

    let AppEvent::GuildCreate { roles, .. } = event else {
        panic!("expected guild create event");
    };

    let roles = roles.expect("guild roles should be present");
    assert_eq!(roles.len(), 1);
    assert_eq!(roles[0].id, Id::new(90));
    assert_eq!(roles[0].name, "Admin");
    assert_eq!(roles[0].color, Some(16755200));
    assert_eq!(roles[0].position, 10);
    assert!(roles[0].hoist);
}

#[test]
fn raw_guild_role_events_patch_single_roles() {
    let created = parse_user_account_event(
        &json!({
            "t": "GUILD_ROLE_CREATE",
            "d": {
                "guild_id": "1",
                "role": {
                    "id": "90",
                    "name": "Admin",
                    "color": 16755200,
                    "position": 10,
                    "hoist": true,
                    "permissions": "1024"
                }
            }
        })
        .to_string(),
    );
    let updated = parse_user_account_event(
        &json!({
            "t": "GUILD_ROLE_UPDATE",
            "d": {
                "guild_id": "1",
                "role": {
                    "id": "90",
                    "name": "Owner",
                    "color": 0,
                    "position": 11,
                    "hoist": false,
                    "permissions": "2048"
                }
            }
        })
        .to_string(),
    );
    let deleted = parse_user_account_event(
        &json!({
            "t": "GUILD_ROLE_DELETE",
            "d": {
                "guild_id": "1",
                "role_id": "90"
            }
        })
        .to_string(),
    );

    assert!(matches!(
        created.as_slice(),
        [AppEvent::GuildRoleUpsert { guild_id, role }]
            if *guild_id == Id::new(1)
                && role.id == Id::new(90)
                && role.name == "Admin"
                && role.color == Some(16755200)
                && role.position == 10
                && role.hoist
                && role.permissions == 1024
    ));
    assert!(matches!(
        updated.as_slice(),
        [AppEvent::GuildRoleUpsert { guild_id, role }]
            if *guild_id == Id::new(1)
                && role.id == Id::new(90)
                && role.name == "Owner"
                && role.color.is_none()
                && role.position == 11
                && !role.hoist
                && role.permissions == 2048
    ));
    assert!(matches!(
        deleted.as_slice(),
        [AppEvent::GuildRoleDelete { guild_id, role_id }]
            if *guild_id == Id::new(1) && *role_id == Id::new(90)
    ));
}

#[test]
fn raw_channel_pins_update_invalidates_channel_pins() {
    let full = parse_user_account_event(
        &json!({
            "t": "CHANNEL_PINS_UPDATE",
            "d": {
                "guild_id": "1",
                "channel_id": "10",
                "last_pin_timestamp": "2026-05-25T12:34:56.000000+00:00"
            }
        })
        .to_string(),
    );
    assert!(matches!(
        full.as_slice(),
        [AppEvent::ChannelPinsUpdate { guild_id, channel_id, last_pin_timestamp }]
            if *guild_id == Some(Id::new(1))
                && *channel_id == Id::new(10)
                && last_pin_timestamp.as_deref() == Some("2026-05-25T12:34:56.000000+00:00")
    ));

    let minimal = parse_user_account_event(
        &json!({ "t": "CHANNEL_PINS_UPDATE", "d": { "channel_id": "10" } }).to_string(),
    );
    assert!(matches!(
        minimal.as_slice(),
        [AppEvent::ChannelPinsUpdate { guild_id, channel_id, last_pin_timestamp }]
            if guild_id.is_none() && *channel_id == Id::new(10) && last_pin_timestamp.is_none()
    ));

    // Without a channel there is nothing to invalidate, so no event at all.
    let channelless = parse_user_account_event(
        &json!({
            "t": "CHANNEL_PINS_UPDATE",
            "d": { "guild_id": "1", "last_pin_timestamp": null }
        })
        .to_string(),
    );
    assert!(channelless.is_empty());
}

#[test]
fn guild_create_parser_accepts_member_user_id_without_nested_user() {
    let event = parse_guild_create(&json!({
        "id": "1",
        "name": "guild",
        "channels": [],
        "members": [{
            "user_id": "10",
            "roles": [20]
        }],
        "presences": [],
        "roles": [],
        "emojis": []
    }))
    .expect("guild create should parse");

    let AppEvent::GuildCreate { members, .. } = event else {
        panic!("expected guild create event");
    };

    assert_eq!(members.len(), 1);
    assert_eq!(members[0].user_id, Id::new(10));
    assert_eq!(members[0].role_ids, vec![Id::new(20)]);
}

#[test]
fn raw_guild_create_with_thin_current_member_hides_denied_channel() {
    let event = parse_guild_create(&json!({
        "id": "1",
        "name": "guild",
        "owner_id": "11",
        "channels": [{
            "id": "2",
            "type": 0,
            "name": "secret",
            "permission_overwrites": [{
                "id": "1",
                "type": 0,
                "allow": "0",
                "deny": "1024"
            }]
        }],
        "members": [{
            "user_id": "10",
            "roles": []
        }],
        "presences": [],
        "roles": [{
            "id": "1",
            "name": "@everyone",
            "permissions": "1024",
            "position": 0,
            "hoist": false
        }],
        "emojis": []
    }))
    .expect("guild create should parse");
    let mut state = DiscordState::default();
    state.apply_event(&AppEvent::Ready {
        user: "me".to_owned(),
        user_id: Some(Id::new(10)),
    });
    state.apply_event(&event);

    assert_eq!(
        state.channel_visibility_stats(Some(Id::new(1))),
        ChannelVisibilityStats {
            visible: 0,
            hidden: 1,
        }
    );
    assert!(
        state
            .viewable_channels_for_guild(Some(Id::new(1)))
            .is_empty()
    );
}

#[test]
fn raw_guild_create_with_thin_current_member_keeps_role_based_access() {
    let event = parse_guild_create(&json!({
        "id": "1",
        "name": "guild",
        "owner_id": "11",
        "channels": [{
            "id": "2",
            "type": 0,
            "name": "staff",
            "permission_overwrites": [{
                "id": "1",
                "type": 0,
                "allow": "0",
                "deny": "1024"
            }, {
                "id": "20",
                "type": 0,
                "allow": "1024",
                "deny": "0"
            }]
        }],
        "members": [{
            "user_id": "10",
            "roles": [20]
        }],
        "presences": [],
        "roles": [{
            "id": "1",
            "name": "@everyone",
            "permissions": "1024",
            "position": 0,
            "hoist": false
        }, {
            "id": "20",
            "name": "Staff",
            "permissions": "0",
            "position": 1,
            "hoist": false
        }],
        "emojis": []
    }))
    .expect("guild create should parse");
    let mut state = DiscordState::default();
    state.apply_event(&AppEvent::Ready {
        user: "me".to_owned(),
        user_id: Some(Id::new(10)),
    });
    state.apply_event(&event);

    assert_eq!(
        state.channel_visibility_stats(Some(Id::new(1))),
        ChannelVisibilityStats {
            visible: 1,
            hidden: 0,
        }
    );
    assert_eq!(state.viewable_channels_for_guild(Some(Id::new(1))).len(), 1);
}

#[test]
fn guild_create_parser_keeps_active_threads() {
    let event = parse_guild_create(&json!({
        "id": "1",
        "name": "guild",
        "channels": [],
        "threads": [thread_payload(10, "release notes")],
        "members": [],
        "presences": [],
        "emojis": []
    }))
    .expect("guild create should parse");

    let AppEvent::GuildCreate { channels, .. } = event else {
        panic!("expected guild create event");
    };

    assert_eq!(channels.len(), 1);
    assert_eq!(channels[0].channel_id, Id::new(10));
    assert_eq!(channels[0].kind, "GuildPublicThread");
    assert_eq!(channels[0].name, "release notes");
}

#[test]
fn raw_member_chunk_upserts_members_and_presences() {
    let events = parse_user_account_event(
        &json!({
            "t": "GUILD_MEMBERS_CHUNK",
            "d": {
                "guild_id": "1",
                "chunk_index": 0,
                "chunk_count": 1,
                "members": [
                    {
                        "nick": "Alice Nick",
                        "roles": ["30", "31"],
                        "user": {
                            "id": "10",
                            "username": "alice",
                            "global_name": "Alice Global",
                            "avatar": "avatarhash"
                        }
                    },
                    {
                        "user": {
                            "id": "20",
                            "username": "bob",
                            "bot": true
                        }
                    }
                ],
                "presences": [
                    { "user": { "id": "10" }, "status": "online" },
                    { "user": { "id": "20" }, "status": "idle" }
                ]
            }
        })
        .to_string(),
    );

    match events.as_slice() {
        [AppEvent::GuildMembersChunk { chunk }] => {
            assert_eq!(chunk.guild_id, Id::new(1));
            assert_eq!(chunk.members.len(), 2);
            assert_eq!(chunk.members[0].user_id, Id::new(10));
            assert_eq!(chunk.members[0].display_name, "Alice Nick");
            assert_eq!(chunk.members[0].role_ids, vec![Id::new(30), Id::new(31)]);
            assert!(!chunk.members[0].is_bot);
            assert_eq!(chunk.members[1].user_id, Id::new(20));
            assert_eq!(chunk.members[1].display_name, "bob");
            assert!(chunk.members[1].is_bot);
            assert_eq!(chunk.presences[0].user_id, Id::new(10));
            assert_eq!(chunk.presences[0].status, PresenceStatus::Online);
            assert_eq!(chunk.presences[1].user_id, Id::new(20));
            assert_eq!(chunk.presences[1].status, PresenceStatus::Idle);
        }
        other => panic!("expected one GuildMembersChunk, got {other:?}"),
    }
}

#[test]
fn raw_member_add_keeps_real_join_semantics() {
    let events = parse_user_account_event(
        &json!({
            "t": "GUILD_MEMBER_ADD",
            "d": {
                "guild_id": "1",
                "nick": "Alice Nick",
                "user": {
                    "id": "10",
                    "username": "alice"
                }
            }
        })
        .to_string(),
    );

    assert_eq!(events.len(), 1);
    assert!(matches!(
        &events[0],
        AppEvent::GuildMemberAdd { guild_id, member }
            if *guild_id == Id::new(1)
                && member.user_id == Id::new(10)
                && member.display_name == "Alice Nick"
    ));
}

#[test]
fn guild_emojis_update_parser_replaces_custom_emojis() {
    let event = parse_guild_emojis_update(&json!({
        "guild_id": "1",
        "emojis": [
            {
                "id": "60",
                "name": "wave",
                "animated": false,
                "available": true
            }
        ]
    }))
    .expect("guild emojis update should parse");

    let AppEvent::GuildEmojisUpdate { guild_id, emojis } = event else {
        panic!("expected guild emojis update event");
    };
    assert_eq!(guild_id, Id::new(1));
    assert_eq!(emojis.len(), 1);
    assert_eq!(emojis[0].id, Id::new(60));
    assert_eq!(emojis[0].name, "wave");
    assert!(emojis[0].available);
}

#[test]
fn guild_update_parser_distinguishes_present_and_absent_custom_emojis() {
    let event = parse_guild_update(&json!({
        "id": "1",
        "name": "guild renamed",
        "emojis": [{
            "id": "70",
            "name": "dance",
            "animated": true,
            "available": true
        }]
    }))
    .expect("guild update should parse");

    let AppEvent::GuildUpdate {
        guild_id,
        name,
        roles,
        emojis,
        ..
    } = event
    else {
        panic!("expected guild update event");
    };
    assert_eq!(guild_id, Id::new(1));
    assert_eq!(name, "guild renamed");
    assert_eq!(roles, None);
    let emojis = emojis.expect("emoji field should be preserved when present");
    assert_eq!(emojis.len(), 1);
    assert_eq!(emojis[0].id, Id::new(70));
    assert_eq!(emojis[0].name, "dance");
    assert!(emojis[0].animated);

    // An absent field must stay `None` rather than collapsing to an empty
    // list, or applying the update would wipe the guild's emojis.
    let event = parse_guild_update(&json!({ "id": "1", "name": "guild renamed" }))
        .expect("guild update should parse");
    let AppEvent::GuildUpdate { roles, emojis, .. } = event else {
        panic!("expected guild update event");
    };
    assert_eq!(roles, None);
    assert_eq!(emojis, None);
}

#[test]
fn message_update_parser_keeps_mentions_when_present() {
    let event = parse_message_update(&json!({
        "id": "20",
        "channel_id": "10",
        "content": "edited <@40>",
        "mentions": [{ "id": "40", "username": "alice" }]
    }))
    .expect("message update should parse");

    let AppEvent::MessageUpdateDispatch { update } = event else {
        panic!("expected message update event");
    };
    assert_eq!(
        update.fields.mentions,
        Some(vec![mention_info(40, "alice")])
    );
}

#[test]
fn message_update_parser_keeps_poll_results() {
    let event = parse_message_update(&json!({
        "id": "20",
        "channel_id": "10",
        "poll": {
            "question": { "text": "오늘 뭐 먹지?" },
            "answers": [
                { "answer_id": 1, "poll_media": { "text": "김치찌개" } },
                { "answer_id": 2, "poll_media": { "text": "라멘" } }
            ],
            "results": {
                "is_finalized": true,
                "answer_counts": [
                    { "id": 1, "count": 5, "me_voted": true },
                    { "id": 2, "count": 3, "me_voted": false }
                ]
            }
        }
    }))
    .expect("message update should parse");

    let AppEvent::MessageUpdateDispatch { update } = event else {
        panic!("expected message update event");
    };
    let poll = update.fields.poll.expect("poll payload should be kept");
    assert_eq!(poll.results_finalized, Some(true));
    assert_eq!(poll.answers[0].vote_count, Some(5));
    assert!(poll.answers[0].me_voted);
}

#[test]
fn message_delete_bulk_dispatch_parses_deleted_message_ids() {
    let events = parse_user_account_event(
        &json!({
            "t": "MESSAGE_DELETE_BULK",
            "d": {
                "guild_id": "1",
                "channel_id": "10",
                "ids": ["20", "30"]
            }
        })
        .to_string(),
    );

    assert_eq!(events.len(), 1);
    let AppEvent::MessageDeleteBulk {
        guild_id,
        channel_id,
        message_ids,
    } = &events[0]
    else {
        panic!("expected message delete bulk event");
    };
    assert_eq!(*guild_id, Some(Id::new(1)));
    assert_eq!(*channel_id, Id::new(10));
    assert_eq!(message_ids, &vec![Id::new(20), Id::new(30)]);
}

#[test]
fn message_delete_bulk_dispatch_ignores_empty_deleted_message_ids() {
    let events = parse_user_account_event(
        &json!({
            "t": "MESSAGE_DELETE_BULK",
            "d": {
                "channel_id": "10",
                "ids": []
            }
        })
        .to_string(),
    );

    assert!(events.is_empty());
}

#[test]
fn message_reaction_add_dispatch_parses_reaction_event() {
    let events = parse_user_account_event(
        &json!({
            "t": "MESSAGE_REACTION_ADD",
            "d": {
                "guild_id": "1",
                "channel_id": "10",
                "message_id": "20",
                "user_id": "30",
                "emoji": { "name": "👍" }
            }
        })
        .to_string(),
    );

    assert_eq!(events.len(), 1);
    let AppEvent::MessageReactionAdd {
        guild_id,
        channel_id,
        message_id,
        user_id,
        emoji,
    } = &events[0]
    else {
        panic!("expected message reaction add event");
    };
    assert_eq!(*guild_id, Some(Id::new(1)));
    assert_eq!(*channel_id, Id::new(10));
    assert_eq!(*message_id, Id::new(20));
    assert_eq!(*user_id, Id::new(30));
    assert_eq!(emoji, &ReactionEmoji::Unicode("👍".to_owned()));
}
