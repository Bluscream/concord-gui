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
fn guild_parsers_preserve_feature_names_from_lazy_properties() {
    let create = parse_guild_create(&json!({
        "id": "10",
        "properties": {
            "name": "guild",
            "features": ["COMMUNITY", "FUTURE_FEATURE"]
        },
        "channels": [],
        "members": [],
        "roles": [],
        "emojis": []
    }))
    .expect("guild should parse");

    let AppEvent::GuildCreate { features, .. } = create else {
        panic!("expected guild create event");
    };
    assert_eq!(
        features,
        Some(vec!["COMMUNITY".to_owned(), "FUTURE_FEATURE".to_owned()])
    );

    let update = parse_guild_update(&json!({
        "id": "10",
        "properties": {
            "name": "guild",
            "features": ["MEMBER_VERIFICATION_GATE_ENABLED"]
        }
    }))
    .expect("guild update should parse");

    let AppEvent::GuildUpdate { features, .. } = update else {
        panic!("expected guild update event");
    };
    assert_eq!(
        features,
        Some(vec!["MEMBER_VERIFICATION_GATE_ENABLED".to_owned()])
    );
}

#[test]
fn guild_create_parser_preserves_complete_onboarding_payload() {
    let raw_onboarding = json!({
        "guild_id": "10",
        "enabled": false,
        "mode": 1,
        "default_channel_ids": ["30", "40"],
        "prompts": [{
            "id": "50",
            "title": "Choose topics",
            "future_prompt_field": { "kept": true }
        }],
        "future_top_level_field": [1, 2, 3]
    });
    let event = parse_guild_create(&json!({
        "id": "10",
        "name": "guild",
        "guild_onboarding": raw_onboarding,
        "channels": [],
        "members": [],
        "roles": [],
        "emojis": []
    }))
    .expect("guild should parse");

    let AppEvent::GuildCreate {
        onboarding: Some(onboarding),
        ..
    } = event
    else {
        panic!("expected guild onboarding");
    };
    assert_eq!(onboarding.guild_id, Id::new(10));
    assert_eq!(onboarding.enabled, Some(false));
    assert_eq!(onboarding.mode, Some(GuildOnboardingMode::Advanced));
    assert_eq!(
        onboarding.default_channel_ids,
        vec![Id::new(30), Id::new(40)]
    );
    assert_eq!(onboarding.prompts().len(), 1);
    assert_eq!(onboarding.raw["future_top_level_field"], json!([1, 2, 3]));
    assert_eq!(
        onboarding.raw["prompts"][0]["future_prompt_field"],
        json!({ "kept": true })
    );
}

#[test]
fn onboarding_dispatches_preserve_payload() {
    let events = parse_user_account_event(
        &json!({
            "t": "GUILD_ONBOARDING_UPDATE",
            "d": {
                "guild_id": "10",
                "enabled": true,
                "mode": 99,
                "default_channel_ids": [],
                "prompts": [],
                "future_field": "kept"
            }
        })
        .to_string(),
    );

    assert!(matches!(
        events.as_slice(),
        [AppEvent::GuildOnboardingUpdate { guild_id, onboarding }]
            if *guild_id == Id::new(10)
                && onboarding.enabled == Some(true)
                && onboarding.mode == Some(GuildOnboardingMode::Unknown(99))
                && onboarding.raw["future_field"] == json!("kept")
    ));

    let events = parse_user_account_event(
        &json!({
            "t": "READY_SUPPLEMENTAL",
            "d": {
                "guilds": [{
                    "id": "10",
                    "guild_onboarding": {
                        "enabled": true,
                        "mode": 0,
                        "default_channel_ids": [],
                        "prompts": [],
                        "supplemental_field": "kept"
                    }
                }]
            }
        })
        .to_string(),
    );

    assert!(events.iter().any(|event| matches!(
        event,
        AppEvent::GuildOnboardingUpdate { guild_id, onboarding }
            if *guild_id == Id::new(10)
                && onboarding.enabled == Some(true)
                && onboarding.raw["supplemental_field"] == json!("kept")
    )));
}

#[test]
fn guild_parser_keeps_message_verification_inputs() {
    let event = parse_guild_create(&json!({
        "id": "10",
        "name": "guild",
        "verification_level": 3,
        "mfa_level": 1,
        "channels": [],
        "roles": [],
        "emojis": [],
        "members": [{
            "user": { "id": "20", "username": "neo" },
            "roles": [],
            "joined_at": "2026-07-14T23:51:00+00:00",
            "flags": 4,
            "pending": true,
            "communication_disabled_until": "2026-07-15T01:00:00+00:00"
        }]
    }))
    .expect("guild should parse");

    let AppEvent::GuildCreate {
        verification_level,
        mfa_level,
        members,
        ..
    } = event
    else {
        panic!("expected GuildCreate event");
    };
    assert_eq!(verification_level, Some(GuildVerificationLevel::High));
    assert_eq!(mfa_level, Some(1));
    assert_eq!(members[0].flags, Some(4));
    assert_eq!(members[0].pending, Some(true));
    assert!(members[0].communication_disabled_until_present);
    assert_eq!(
        members[0]
            .communication_disabled_until
            .expect("communication_disabled_until should parse")
            .to_rfc3339(),
        "2026-07-15T01:00:00+00:00"
    );
    assert_eq!(
        members[0]
            .joined_at
            .expect("joined_at should parse")
            .to_rfc3339(),
        "2026-07-14T23:51:00+00:00"
    );
}

#[test]
fn guild_parser_preserves_missing_authorization_inputs() {
    let event = parse_guild_create(&json!({
        "id": "10",
        "name": "guild",
        "channels": [],
        "members": [],
        "emojis": []
    }))
    .expect("partial lazy guild should parse");

    let AppEvent::GuildCreate {
        verification_level,
        mfa_level,
        features,
        roles,
        ..
    } = event
    else {
        panic!("expected GuildCreate event");
    };
    assert_eq!(verification_level, None);
    assert_eq!(mfa_level, None);
    assert_eq!(features, None);
    assert_eq!(roles, None);
}

#[test]
fn current_user_verification_status_is_loaded_and_refreshed() {
    let events = parse_user_account_event(
        &json!({
            "t": "READY",
            "d": {
                "user": {
                    "id": "20",
                    "username": "neo",
                    "verified": true,
                    "phone": "+10000000000",
                    "mfa_enabled": true
                },
                "guilds": []
            }
        })
        .to_string(),
    );

    assert!(events.iter().any(|event| matches!(
        event,
        AppEvent::CurrentUserVerification {
            email_verified: Some(true),
            phone_verified: Some(true),
            mfa_enabled: Some(true),
        }
    )));

    let events = parse_user_account_event(
        &json!({
            "t": "USER_UPDATE",
            "d": {
                "id": "20",
                "username": "neo",
                "verified": true,
                "phone": null
            }
        })
        .to_string(),
    );

    assert!(events.iter().any(|event| matches!(
        event,
        AppEvent::CurrentUserVerification {
            email_verified: Some(true),
            phone_verified: Some(false),
            mfa_enabled: _,
        }
    )));
}

#[test]
fn raw_dispatch_parser_keeps_original_payload_for_future_fields() {
    let parsed = parse_user_account_dispatch(json!({
        "t": "MESSAGE_CREATE",
        "d": {
            "id": "101",
            "channel_id": "20",
            "author": { "id": "30", "username": "neo" },
            "type": 0,
            "pinned": false,
            "content": "hello",
            "mentions": [],
            "attachments": [],
            "embeds": [],
            "future_discord_field": { "value": true }
        }
    }))
    .expect("dispatch should parse");

    assert_eq!(parsed.dispatch.event_type, "MESSAGE_CREATE");
    assert_eq!(
        parsed.dispatch.payload["future_discord_field"]["value"],
        true
    );
    assert!(matches!(
        parsed.events.as_slice(),
        [AppEvent::MessageCreate { .. }]
    ));
}

#[test]
fn raw_member_list_update_preserves_operations_and_member_data() {
    let events = parse_user_account_event(
        &json!({
            "t": "GUILD_MEMBER_LIST_UPDATE",
            "d": {
                "guild_id": "10",
                "groups": [{ "id": "admin", "count": 4 }],
                "ops": [
                    {
                        "op": "SYNC",
                        "range": [0, 99],
                        "items": [
                            { "group": { "id": "admin" } },
                            {
                                "member": {
                                    "user": {
                                        "id": "20",
                                        "username": "alice",
                                        "global_name": "Alice",
                                        "avatar": "global_hash"
                                    },
                                    "avatar": "guild_hash",
                                    "nick": "Alice Nick",
                                    "roles": ["30"]
                                },
                                "presence": { "status": "idle" }
                            }
                        ]
                    },
                    {
                        "op": "SYNC",
                        "range": [100, 199],
                        "items": [{
                            "member": {
                                "user": { "id": "21", "username": "bob" },
                                "roles": []
                            },
                            "presence": { "status": "idle" }
                        }]
                    },
                    {
                        "op": "INSERT",
                        "index": 200,
                        "item": {
                            "member": {
                                "user": { "id": "22", "username": "carol" },
                                "roles": []
                            },
                            "presence": { "status": "online" }
                        }
                    },
                    {
                        "op": "UPDATE",
                        "index": 201,
                        "item": {
                            "member": {
                                "user": { "id": "23", "username": "dave" },
                                "roles": []
                            },
                            "presence": { "status": "dnd" }
                        }
                    },
                    { "op": "DELETE", "index": 12 },
                    { "op": "INVALIDATE", "range": [200, 299] },
                    { "op": "FUTURE_OPERATION", "index": 4 }
                ]
            }
        })
        .to_string(),
    );

    let [AppEvent::GuildMemberListUpdate { update }] = events.as_slice() else {
        panic!("expected one GuildMemberListUpdate");
    };
    assert_eq!(update.guild_id, Id::new(10));
    assert_eq!(update.ops.len(), 7);

    let GuildMemberListOperation::Sync { range, items } = &update.ops[0] else {
        panic!("expected first sync operation");
    };
    assert_eq!(*range, (0, 99));
    assert!(matches!(
        &items[0],
        GuildMemberListItem::Group { id, count } if id == "admin" && *count == 4
    ));
    let GuildMemberListItem::Member { member, presence } = &items[1] else {
        panic!("expected member list item");
    };
    assert_eq!(member.user_id, Id::new(20));
    assert_eq!(member.display_name, "Alice Nick");
    assert_eq!(member.nickname.as_deref(), Some("Alice Nick"));
    assert!(member.nickname_present);
    assert_eq!(
        member.avatar_url.as_deref(),
        Some("https://cdn.discordapp.com/guilds/10/users/20/avatars/guild_hash.png")
    );
    assert_eq!(member.role_ids, vec![Id::new(30)]);
    assert_eq!(
        presence.as_ref().map(|value| (value.user_id, value.status)),
        Some((Id::new(20), PresenceStatus::Idle))
    );

    assert!(matches!(
        &update.ops[1],
        GuildMemberListOperation::Sync { range: (100, 199), items }
            if matches!(
                &items[0],
                GuildMemberListItem::Member { member, presence: Some(presence) }
                    if member.user_id == Id::new(21)
                        && presence.status == PresenceStatus::Idle
            )
    ));
    assert!(matches!(
        &update.ops[2],
        GuildMemberListOperation::Insert {
            index: 200,
            item: GuildMemberListItem::Member { presence: Some(presence), .. }
        } if presence.user_id == Id::new(22)
            && presence.status == PresenceStatus::Online
    ));
    assert!(matches!(
        &update.ops[3],
        GuildMemberListOperation::Update {
            index: 201,
            item: GuildMemberListItem::Member { presence: Some(presence), .. }
        } if presence.user_id == Id::new(23)
            && presence.status == PresenceStatus::DoNotDisturb
    ));
    assert!(matches!(
        &update.ops[4..],
        [
            GuildMemberListOperation::Delete { index: 12 },
            GuildMemberListOperation::Invalidate { range: (200, 299) },
            GuildMemberListOperation::Unknown { name: Some(name), raw }
        ] if name == "FUTURE_OPERATION" && raw["index"] == json!(4)
    ));
}

#[test]
fn raw_voice_state_update_extracts_channel_and_member() {
    let events = parse_user_account_event(
        &json!({
            "t": "VOICE_STATE_UPDATE",
            "d": {
                "guild_id": "10",
                "channel_id": "30",
                "user_id": "20",
                "deaf": false,
                "mute": true,
                "self_deaf": false,
                "self_mute": true,
                "self_stream": true,
                "session_id": "voice-session-1",
                "member": {
                    "user": {
                        "id": "20",
                        "username": "alice",
                        "global_name": "Alice"
                    },
                    "nick": "Alice Nick",
                    "roles": ["40"]
                }
            }
        })
        .to_string(),
    );

    assert!(events.iter().any(|event| matches!(
        event,
        AppEvent::VoiceStateUpdate { state }
            if state.guild_id == Some(Id::new(10))
                && state.channel_id == Some(Id::new(30))
                && state.user_id == Id::new(20)
                && state.mute
                && state.self_mute
                && state.self_stream
                && state.session_id.as_deref() == Some("voice-session-1")
                && state.member.as_ref().is_some_and(|member|
                    member.display_name == "Alice Nick" && member.role_ids == vec![Id::new(40)]
                )
    )));
}

#[test]
fn dm_call_voice_states_parse_without_a_guild() {
    use crate::discord::VoiceScope;

    // A DM/group-DM voice state arrives with a null guild and the DM channel id.
    let dm_state = parse_user_account_event(
        &json!({
            "t": "VOICE_STATE_UPDATE",
            "d": {
                "guild_id": null,
                "channel_id": "30",
                "user_id": "20",
                "session_id": "dm-voice-session"
            }
        })
        .to_string(),
    );
    assert!(dm_state.iter().any(|event| matches!(
        event,
        AppEvent::VoiceStateUpdate { state }
            if state.guild_id.is_none()
                && state.channel_id == Some(Id::new(30))
                && state.scope() == Some(VoiceScope::Private(Id::new(30)))
    )));

    // CALL_CREATE describes an in-progress DM call and seeds its participants.
    let call = parse_user_account_event(
        &json!({
            "t": "CALL_CREATE",
            "d": {
                "channel_id": "30",
                "voice_states": [
                    { "user_id": "20", "channel_id": "30" },
                    { "user_id": "21" }
                ]
            }
        })
        .to_string(),
    );
    let call_users: Vec<_> = call
        .iter()
        .filter_map(|event| match event {
            AppEvent::VoiceStateUpdate { state } => Some(state),
            _ => None,
        })
        .collect();
    assert_eq!(call_users.len(), 2);
    // A participant whose state omits its channel inherits the call's channel.
    assert!(
        call_users
            .iter()
            .all(|state| state.channel_id == Some(Id::new(30)) && state.guild_id.is_none())
    );

    // CALL_DELETE ends the call and clears its channel.
    let deleted = parse_user_account_event(
        &json!({ "t": "CALL_DELETE", "d": { "channel_id": "30" } }).to_string(),
    );
    assert!(deleted.iter().any(|event| matches!(
        event,
        AppEvent::CallDelete { channel_id } if *channel_id == Id::new(30)
    )));
}

#[test]
fn raw_voice_server_update_extracts_endpoint_without_exposing_token_in_debug() {
    let events = parse_user_account_event(
        &json!({
            "t": "VOICE_SERVER_UPDATE",
            "d": {
                "guild_id": "10",
                "endpoint": "voice.example.com",
                "token": "secret-voice-token"
            }
        })
        .to_string(),
    );

    let server = events
        .iter()
        .find_map(|event| match event {
            AppEvent::VoiceServerUpdate { server } => Some(server),
            _ => None,
        })
        .expect("voice server update should parse");

    assert_eq!(server.guild_id, Some(Id::new(10)));
    assert_eq!(server.endpoint.as_deref(), Some("voice.example.com"));
    assert_eq!(server.token, "secret-voice-token");
    assert!(!format!("{server:?}").contains("secret-voice-token"));
}

#[test]
fn raw_stream_events_supply_the_separate_rtc_connection() {
    let created = parse_user_account_event(
        &json!({
            "t": "STREAM_CREATE",
            "d": {
                "stream_key": "guild:10:20:30",
                "rtc_server_id": "400",
                "rtc_channel_id": "401",
                "viewer_ids": ["50", "60"],
                "paused": true
            }
        })
        .to_string(),
    );
    assert!(created.iter().any(|event| matches!(
        event,
        AppEvent::StreamCreate { stream }
            if stream.stream_key == "guild:10:20:30"
                && stream.rtc_server_id == "400"
                && stream.rtc_channel_id == Id::new(401)
                && stream.viewer_ids == vec![Id::new(50), Id::new(60)]
                && stream.paused
    )));

    let updated = parse_user_account_event(
        &json!({
            "t": "STREAM_UPDATE",
            "d": {
                "stream_key": "guild:10:20:30",
                "viewer_ids": ["50", "70"],
                "paused": false
            }
        })
        .to_string(),
    );
    assert!(updated.iter().any(|event| matches!(
        event,
        AppEvent::StreamUpdate { stream }
            if stream.stream_key == "guild:10:20:30"
                && stream.viewer_ids == vec![Id::new(50), Id::new(70)]
                && !stream.paused
    )));

    let server = parse_user_account_event(
        &json!({
            "t": "STREAM_SERVER_UPDATE",
            "d": {
                "stream_key": "guild:10:20:30",
                "endpoint": "stream.example.com",
                "token": "secret-stream-token"
            }
        })
        .to_string(),
    );
    let server = server
        .iter()
        .find_map(|event| match event {
            AppEvent::StreamServerUpdate { server } => Some(server),
            _ => None,
        })
        .expect("stream server update should parse");
    assert_eq!(server.endpoint.as_deref(), Some("stream.example.com"));
    assert!(!format!("{server:?}").contains("secret-stream-token"));

    let deleted = parse_user_account_event(
        &json!({
            "t": "STREAM_DELETE",
            "d": {
                "stream_key": "guild:10:20:30",
                "reason": "stream_ended",
                "unavailable": true
            }
        })
        .to_string(),
    );
    assert!(deleted.iter().any(|event| matches!(
        event,
        AppEvent::StreamDelete { stream }
            if stream.reason == "stream_ended" && stream.unavailable
    )));
}

#[test]
fn stream_update_without_viewer_ids_is_ignored() {
    let events = parse_user_account_event(
        &json!({
            "t": "STREAM_UPDATE",
            "d": {
                "stream_key": "guild:10:20:30",
                "paused": false
            }
        })
        .to_string(),
    );

    assert!(events.is_empty());
}

#[test]
fn raw_voice_state_update_extracts_leave_payload() {
    let events = parse_user_account_event(
        &json!({
            "t": "VOICE_STATE_UPDATE",
            "d": {
                "guild_id": "10",
                "channel_id": null,
                "user_id": "20"
            }
        })
        .to_string(),
    );

    assert!(events.iter().any(|event| matches!(
        event,
        AppEvent::VoiceStateUpdate { state }
            if state.guild_id == Some(Id::new(10))
                && state.channel_id.is_none()
                && state.user_id == Id::new(20)
    )));
}

#[test]
fn raw_guild_create_emits_initial_voice_states() {
    let events = parse_user_account_event(
        &json!({
            "t": "GUILD_CREATE",
            "d": {
                "id": "10",
                "name": "guild",
                "channels": [],
                "voice_states": [{
                    "channel_id": "30",
                    "user_id": "20",
                    "self_stream": true
                }]
            }
        })
        .to_string(),
    );

    assert!(events.iter().any(|event| matches!(
        event,
        AppEvent::GuildCreate { guild_id, .. } if *guild_id == Id::new(10)
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        AppEvent::VoiceStateUpdate { state }
            if state.guild_id == Some(Id::new(10))
                && state.channel_id == Some(Id::new(30))
                && state.user_id == Id::new(20)
                && state.self_stream
    )));
}

#[test]
fn raw_ready_parser_emits_initial_voice_states_from_embedded_guilds() {
    let events = parse_user_account_event(
        &json!({
            "t": "READY",
            "d": {
                "user": { "id": "1", "username": "me" },
                "guilds": [{
                    "id": "10",
                    "name": "guild",
                    "channels": [],
                    "voice_states": [{
                        "channel_id": "30",
                        "user_id": "20"
                    }]
                }]
            }
        })
        .to_string(),
    );

    assert!(events.iter().any(|event| matches!(
        event,
        AppEvent::GuildCreate { guild_id, .. } if *guild_id == Id::new(10)
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        AppEvent::VoiceStateUpdate { state }
            if state.guild_id == Some(Id::new(10))
                && state.channel_id == Some(Id::new(30))
                && state.user_id == Id::new(20)
    )));
}

#[test]
fn relationship_payloads_emit_upserts_and_authoritative_empty_lists() {
    let events = parse_user_account_event(
        &json!({
            "t": "RELATIONSHIP_ADD",
            "d": {
                "id": "20",
                "type": 1,
                "nickname": "Bestie",
                "user": {
                    "id": "20",
                    "global_name": "Alice Global",
                    "username": "alice"
                }
            }
        })
        .to_string(),
    );
    assert_eq!(events.len(), 1);
    assert!(matches!(
        &events[0],
        AppEvent::RelationshipUpsert { relationship }
            if relationship.user_id == Id::new(20)
                && relationship.status == FriendStatus::Friend
                && relationship.nickname.as_deref() == Some("Bestie")
                && relationship.display_name.as_deref() == Some("Alice Global")
                && relationship.username.as_deref() == Some("alice")
    ));

    let ready = parse_user_account_event(
        &json!({
            "t": "READY",
            "d": {
                "user": { "id": "10", "username": "me" },
                "relationships": []
            }
        })
        .to_string(),
    );
    assert!(ready.iter().any(|event| matches!(
        event,
        AppEvent::RelationshipsLoaded { relationships } if relationships.is_empty()
    )));
}

#[test]
fn relationship_update_accepts_a_partial_nickname_patch() {
    let events = parse_user_account_event(
        &json!({
            "t": "RELATIONSHIP_UPDATE",
            "d": {
                "id": "20",
                "nickname": "New nickname"
            }
        })
        .to_string(),
    );

    assert!(matches!(
        events.as_slice(),
        [AppEvent::RelationshipUpdate { update }]
            if update.user_id == Id::new(20)
                && update.status.is_none()
                && update.nickname == Some(Some("New nickname".to_owned()))
                && update.display_name.is_none()
                && update.username.is_none()
    ));
}

#[test]
fn relationship_remove_emits_event() {
    let events = parse_user_account_event(
        &json!({
            "t": "RELATIONSHIP_REMOVE",
            "d": {"id": "20", "type": 3}
        })
        .to_string(),
    );
    assert_eq!(events.len(), 1);
    assert!(matches!(
        &events[0],
        AppEvent::RelationshipRemove { user_id } if *user_id == Id::new(20)
    ));
}

#[test]
fn channel_parser_keeps_last_message_id() {
    let channel = parse_channel_info(
        &json!({
            "id": "10",
            "type": 1,
            "last_message_id": "99",
            "recipients": [{ "username": "neo" }]
        }),
        None,
    )
    .expect("dm channel should parse");

    assert_eq!(channel.last_message_id.map(|id| id.get()), Some(99));
}

#[test]
fn channel_parser_reads_dm_message_request_and_spam_flags() {
    let channel = parse_channel_info(
        &json!({
            "id": "10",
            "type": 1,
            "is_message_request": true,
            "is_spam": true,
            "recipients": [{ "username": "stranger" }]
        }),
        None,
    )
    .expect("dm channel should parse");

    assert_eq!(channel.is_message_request, Some(true));
    assert_eq!(channel.is_spam, Some(true));
}
