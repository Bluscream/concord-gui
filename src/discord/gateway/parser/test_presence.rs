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
fn message_ack_preserves_optional_read_state_fields() {
    let present = parse_user_account_event(
        &json!({
            "t": "MESSAGE_ACK",
            "d": {
                "channel_id": "42",
                "message_id": "99",
                "mention_count": 2,
                "flags": 5,
                "last_viewed": 20_000,
                "version": 6,
            }
        })
        .to_string(),
    );

    match present.as_slice() {
        [
            AppEvent::MessageAck {
                channel_id,
                message_id,
                mention_count,
                flags,
                last_viewed,
                version,
            },
        ] => {
            assert_eq!(*channel_id, Id::new(42));
            assert_eq!(*message_id, Id::new(99));
            assert_eq!(*mention_count, Some(2));
            assert_eq!(*flags, Some(5));
            assert_eq!(*last_viewed, Some(20_000));
            assert_eq!(*version, Some(6));
        }
        other => panic!("expected one MessageAck, got {other:?}"),
    }

    for payload in [
        json!({
            "t": "MESSAGE_ACK",
            "d": {
                "channel_id": "42",
                "message_id": "100",
                "version": 7
            }
        }),
        json!({
            "t": "MESSAGE_ACK",
            "d": {
                "channel_id": "42",
                "message_id": "101",
                "mention_count": null,
                "version": 8
            }
        }),
    ] {
        assert!(matches!(
            parse_user_account_event(&payload.to_string()).as_slice(),
            [AppEvent::MessageAck {
                mention_count: None,
                flags: None,
                last_viewed: None,
                version: Some(_),
                ..
            }]
        ));
    }

    let missing_version = json!({
        "t": "MESSAGE_ACK",
        "d": {
            "channel_id": "42",
            "message_id": "102"
        }
    });
    assert!(parse_user_account_event(&missing_version.to_string()).is_empty());
}

#[test]
fn read_state_dispatches_preserve_ack_and_unread_fields() {
    let feature_ack = parse_user_account_event(
        &json!({
            "t": "USER_NON_CHANNEL_ACK",
            "d": {
                "ack_type": 2,
                "resource_id": "10",
                "entity_id": "20",
                "version": 3
            }
        })
        .to_string(),
    );
    assert!(matches!(
        feature_ack.as_slice(),
        [AppEvent::FeatureReadStateAck {
            read_state_type: 2,
            resource_id: 10,
            entity_id: 20,
            version: 3,
        }]
    ));

    let pins_ack = parse_user_account_event(
        &json!({
            "t": "CHANNEL_PINS_ACK",
            "d": {
                "channel_id": "42",
                "timestamp": "2026-07-24T00:00:00+00:00",
                "version": 4
            }
        })
        .to_string(),
    );
    assert!(matches!(
        pins_ack.as_slice(),
        [AppEvent::ChannelPinsAck {
            channel_id,
            timestamp,
            version: 4,
        }] if *channel_id == Id::new(42) && timestamp == "2026-07-24T00:00:00+00:00"
    ));

    let unread = parse_user_account_event(
        &json!({
            "t": "CHANNEL_UNREAD_UPDATE",
            "d": {
                "guild_id": "1",
                "channel_unread_updates": [{
                    "id": "42",
                    "last_message_id": null,
                    "last_pin_timestamp": null
                }]
            }
        })
        .to_string(),
    );
    assert!(matches!(
        unread.as_slice(),
        [AppEvent::ChannelUnreadUpdate { guild_id, channels }]
            if *guild_id == Id::new(1)
                && channels.len() == 1
                && channels[0].channel_id == Id::new(42)
                && channels[0].last_message_id == Some(None)
                && channels[0].last_pin_timestamp == Some(None)
    ));
}

#[test]
fn application_command_gateway_events_keep_index_and_interaction_data() {
    let index = parse_user_account_event(
        &json!({
            "t": "GUILD_APPLICATION_COMMAND_INDEX_UPDATE",
            "d": { "guild_id": "10" }
        })
        .to_string(),
    );
    let success = parse_user_account_event(
        &json!({
            "t": "INTERACTION_SUCCESS",
            "d": { "id": "20", "nonce": "request-1" }
        })
        .to_string(),
    );
    let failure = parse_user_account_event(
        &json!({
            "t": "INTERACTION_FAILURE",
            "d": { "id": "21", "nonce": 22, "reason_code": 18 }
        })
        .to_string(),
    );
    let autocomplete = parse_user_account_event(
        &json!({
            "t": "APPLICATION_COMMAND_AUTOCOMPLETE_RESPONSE",
            "d": {
                "nonce": "request-2",
                "choices": [{ "name": "first", "value": 1 }]
            }
        })
        .to_string(),
    );

    assert!(matches!(
        index.as_slice(),
        [AppEvent::ApplicationCommandIndexUpdated { guild_id }]
            if *guild_id == Id::new(10)
    ));
    assert!(matches!(
        success.as_slice(),
        [AppEvent::InteractionSucceeded {
            interaction_id: 20,
            nonce: Some(nonce),
            correlated: false,
        }] if nonce == "request-1"
    ));
    assert!(matches!(
        failure.as_slice(),
        [AppEvent::InteractionFailed {
            interaction_id: 21,
            nonce: Some(nonce),
            reason_code: 18,
            correlated: false,
        }] if nonce == "22"
    ));
    assert!(matches!(
        autocomplete.as_slice(),
        [AppEvent::ApplicationCommandAutocompleteResponse {
            nonce: Some(nonce),
            choices,
        }] if nonce == "request-2"
            && choices.len() == 1
            && choices[0].name == "first"
            && choices[0].value == json!(1)
    ));
}

#[test]
fn user_update_refreshes_global_identity() {
    let events = parse_user_account_event(
        &json!({
            "t": "USER_UPDATE",
            "d": {
                "id": "42",
                "username": "neo",
                "global_name": "Neo Global",
                "avatar": "avatar_hash",
                "discriminator": "0"
            }
        })
        .to_string(),
    );

    match events.as_slice() {
        [
            AppEvent::UserIdentityUpdate {
                user_id,
                username,
                global_name,
                avatar_url,
                is_bot,
            },
        ] => {
            assert_eq!(*user_id, Id::new(42));
            assert_eq!(username, "neo");
            assert_eq!(global_name.as_deref(), Some("Neo Global"));
            assert_eq!(
                avatar_url.as_deref(),
                Some("https://cdn.discordapp.com/avatars/42/avatar_hash.png"),
            );
            assert!(!is_bot);
        }
        other => panic!("expected one UserIdentityUpdate, got {other:?}"),
    }
}

#[test]
fn ready_payload_emits_read_state_sync_with_ack_pointers() {
    // Minimal READY: a `user`, an empty guild list (so the test stays
    // light), and a `read_state.entries[]` array with two channels.
    let events = parse_user_account_event(
        &json!({
            "t": "READY",
            "d": {
                "user": { "id": "1", "username": "neo" },
                "guilds": [],
                "read_state": {
                    "entries": [
                        {
                            "id": "11",
                            "last_message_id": "20",
                            "mention_count": 0,
                            "last_pin_timestamp": "2026-07-24T00:00:00.000Z",
                            "flags": 3,
                            "last_viewed": 1234
                        },
                        { "id": "12", "last_message_id": "30", "mention_count": 4 },
                        {
                            "id": "11",
                            "read_state_type": 1,
                            "last_acked_id": "40",
                            "badge_count": 7
                        }
                    ]
                }
            }
        })
        .to_string(),
    );

    let entries = events
        .iter()
        .find_map(|event| match event {
            AppEvent::ReadStateSync {
                entries,
                partial,
                version,
            } if !partial && version.is_none() => Some(entries.clone()),
            _ => None,
        })
        .expect("READY should emit a full ReadStateSync");
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0].channel_id, Id::new(11));
    assert_eq!(entries[0].last_acked_message_id, Some(Id::new(20)));
    assert_eq!(entries[0].mention_count, 0);
    assert_eq!(
        entries[0].last_pin_timestamp.as_deref(),
        Some("2026-07-24T00:00:00.000Z")
    );
    assert_eq!(entries[0].flags, 3);
    assert_eq!(entries[0].last_viewed, Some(1234));
    assert_eq!(entries[1].channel_id, Id::new(12));
    assert_eq!(entries[1].mention_count, 4);
    assert_eq!(entries[2].read_state_type, 1);
    assert_eq!(entries[2].channel_id, Id::new(11));
    assert_eq!(entries[2].last_acked_message_id, Some(Id::new(40)));
    assert_eq!(entries[2].badge_count, 7);
}

#[test]
fn ready_payload_treats_zero_read_state_ack_pointer_as_absent() {
    let events = parse_user_account_event(
        &json!({
            "t": "READY",
            "d": {
                "user": { "id": "1", "username": "neo" },
                "guilds": [],
                "read_state": {
                    "entries": [
                        { "id": "11", "last_message_id": "0", "mention_count": 0 },
                        { "id": "12", "last_message_id": 0, "mention_count": 1 },
                    ]
                }
            }
        })
        .to_string(),
    );

    let entries = events
        .iter()
        .find_map(|event| match event {
            AppEvent::ReadStateSync { entries, .. } => Some(entries.clone()),
            _ => None,
        })
        .expect("READY should emit a ReadStateSync");
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].channel_id, Id::new(11));
    assert_eq!(entries[0].last_acked_message_id, None);
    assert_eq!(entries[0].mention_count, 0);
    assert_eq!(entries[1].channel_id, Id::new(12));
    assert_eq!(entries[1].last_acked_message_id, None);
    assert_eq!(entries[1].mention_count, 1);
}

#[test]
fn ready_preserves_empty_and_partial_versioned_snapshots() {
    let events = parse_user_account_event(
        &json!({
            "t": "READY",
            "d": {
                "user": { "id": "1", "username": "neo" },
                "guilds": [],
                "read_state": {
                    "entries": [],
                    "partial": false,
                    "version": 12
                },
                "user_guild_settings": {
                    "entries": [],
                    "partial": true,
                    "version": 13
                }
            }
        })
        .to_string(),
    );

    assert!(events.iter().any(|event| matches!(
        event,
        AppEvent::ReadStateSync { entries, partial: false, version: Some(12) }
            if entries.is_empty()
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        AppEvent::UserGuildSettingsSync {
            settings,
            partial: true,
            version: Some(13),
        } if settings.is_empty()
    )));
}

#[test]
fn notification_settings_are_preserved_from_ready_and_updates() {
    let events = parse_user_account_event(
        &json!({
            "t": "READY",
            "d": {
                "user": { "id": "1", "username": "neo" },
                "guilds": [],
                "notification_settings": { "flags": 32 },
                "user_guild_settings": {
                    "entries": [{
                        "guild_id": "10",
                        "message_notifications": 1,
                        "muted": false,
                        "flags": 16384,
                        "hide_muted_channels": true,
                        "mobile_push": false,
                        "mute_scheduled_events": true,
                        "notify_highlights": 2,
                        "version": 9,
                        "suppress_everyone": true,
                        "suppress_roles": true,
                        "channel_overrides": [{
                            "channel_id": "20",
                            "message_notifications": 0,
                            "muted": true,
                            "collapsed": true,
                            "flags": 5120,
                            "mute_config": { "end_time": "2099-01-01T00:00:00.000Z" }
                        }]
                    }]
                }
            }
        })
        .to_string(),
    );

    let settings = events
        .iter()
        .find_map(|event| match event {
            AppEvent::UserGuildSettingsSync { settings, .. } => Some(settings),
            _ => None,
        })
        .expect("READY should emit user guild settings");
    assert_eq!(settings.len(), 1);
    let notification_settings = &settings[0].notification_settings;
    assert_eq!(notification_settings.guild_id, Some(Id::new(10)));
    assert_eq!(
        notification_settings.message_notifications,
        Some(NotificationLevel::OnlyMentions)
    );
    assert!(notification_settings.suppress_everyone);
    assert!(notification_settings.suppress_roles);
    assert_eq!(notification_settings.flags, 16384);
    assert!(notification_settings.hide_muted_channels);
    assert!(!notification_settings.mobile_push);
    assert!(notification_settings.mute_scheduled_events);
    assert_eq!(notification_settings.notify_highlights, 2);
    assert_eq!(notification_settings.version, 9);
    assert_eq!(notification_settings.channel_overrides.len(), 1);
    assert_eq!(
        notification_settings.channel_overrides[0].channel_id,
        Id::new(20)
    );
    assert_eq!(
        notification_settings.channel_overrides[0].message_notifications,
        Some(NotificationLevel::AllMessages)
    );
    assert!(notification_settings.channel_overrides[0].muted);
    assert!(notification_settings.channel_overrides[0].collapsed);
    assert_eq!(notification_settings.channel_overrides[0].flags, 5120);
    assert!(events.iter().any(|event| matches!(
        event,
        AppEvent::UserNotificationSettingsUpdate { flags } if *flags == 32
    )));

    let update = parse_user_account_event(
        &json!({
            "t": "NOTIFICATION_SETTINGS_UPDATE",
            "d": { "flags": 64 }
        })
        .to_string(),
    );
    assert!(matches!(
        update.as_slice(),
        [AppEvent::UserNotificationSettingsUpdate { flags }] if *flags == 64
    ));
}

#[test]
fn user_guild_settings_update_emits_single_update_event() {
    let events = parse_user_account_event(
        &json!({
            "t": "USER_GUILD_SETTINGS_UPDATE",
            "d": {
                "guild_id": "10",
                "message_notifications": 2,
                "muted": true,
                "mute_config": { "end_time": "2099-01-01T00:00:00.000Z" },
                "channel_overrides": [],
                "version": 11
            }
        })
        .to_string(),
    );

    match events.as_slice() {
        [AppEvent::UserGuildSettingsUpdate { settings }] => {
            let notification_settings = &settings.notification_settings;
            assert_eq!(notification_settings.guild_id, Some(Id::new(10)));
            assert_eq!(
                notification_settings.message_notifications,
                Some(NotificationLevel::NoMessages)
            );
            assert!(notification_settings.muted);
            assert_eq!(notification_settings.version, 11);
        }
        other => panic!("expected one UserGuildSettingsUpdate, got {other:?}"),
    }

    let missing_version = json!({
        "t": "USER_GUILD_SETTINGS_UPDATE",
        "d": {
            "guild_id": "10",
            "message_notifications": 2,
            "channel_overrides": []
        }
    });
    assert!(parse_user_account_event(&missing_version.to_string()).is_empty());
}

#[test]
fn user_settings_update_emits_guild_folder_order() {
    let events = parse_user_account_event(
        &json!({
            "t": "USER_SETTINGS_UPDATE",
            "d": {
                "activity_restricted_guild_ids": ["40"],
                "custom_status": {
                    "text": "working",
                    "emoji_id": "50",
                    "expires_at": null
                },
                "friend_source_flags": {
                    "all": true,
                    "mutual_friends": false,
                    "mutual_guilds": true
                },
                "guild_folders": [
                    {
                        "id": null,
                        "name": null,
                        "color": null,
                        "guild_ids": ["20"]
                    },
                    {
                        "id": 42,
                        "name": "work",
                        "color": 16711680,
                        "guild_ids": ["10", "30"]
                    }
                ],
                "status": "online",
                "theme": "dark",
                "future_setting": { "preserved": true }
            }
        })
        .to_string(),
    );

    match events.as_slice() {
        [AppEvent::UserSettingsUpdate { settings }] => {
            assert_eq!(
                settings.activity_restricted_guild_ids,
                Some(vec![Id::new(40)])
            );
            assert_eq!(settings.status.as_deref(), Some("online"));
            assert_eq!(settings.theme.as_deref(), Some("dark"));
            assert_eq!(
                settings
                    .custom_status
                    .as_ref()
                    .and_then(Option::as_ref)
                    .and_then(|status| status.text.as_deref()),
                Some("working")
            );
            assert_eq!(
                settings
                    .custom_status
                    .as_ref()
                    .and_then(Option::as_ref)
                    .and_then(|status| status.emoji_id),
                Some(Id::new(50))
            );
            assert_eq!(
                settings
                    .friend_source_flags
                    .as_ref()
                    .and_then(|flags| flags.all),
                Some(true)
            );
            assert!(settings.extra_fields.contains_key("future_setting"));
            let folders = settings
                .guild_folders
                .as_ref()
                .expect("user settings update should keep guild folders");
            assert_eq!(folders.len(), 2);
            assert_eq!(folders[0].id, None);
            assert_eq!(folders[0].guild_ids, vec![Id::new(20)]);
            assert_eq!(folders[1].id, Some(42));
            assert_eq!(folders[1].name.as_deref(), Some("work"));
            assert_eq!(folders[1].color, Some(16_711_680));
            assert_eq!(folders[1].guild_ids, vec![Id::new(10), Id::new(30)]);
        }
        other => panic!("expected one UserSettingsUpdate, got {other:?}"),
    }
}

#[test]
fn ready_payload_parses_private_channel_notification_settings() {
    let events = parse_user_account_event(
        &json!({
            "t": "READY",
            "d": {
                "user": { "id": "1", "username": "neo" },
                "guilds": [],
                "user_guild_settings": {
                    "entries": [{
                        "guild_id": null,
                        "message_notifications": 1,
                        "channel_overrides": {
                            "20": {
                                "message_notifications": 2,
                                "muted": true,
                                "mute_config": null
                            }
                        }
                    }]
                }
            }
        })
        .to_string(),
    );

    let settings = events
        .iter()
        .find_map(|event| match event {
            AppEvent::UserGuildSettingsSync { settings, .. } => Some(settings),
            _ => None,
        })
        .expect("READY should emit private channel guild settings");
    assert_eq!(settings.len(), 1);
    let notification_settings = &settings[0].notification_settings;
    assert_eq!(notification_settings.guild_id, None);
    assert_eq!(
        notification_settings.message_notifications,
        Some(NotificationLevel::OnlyMentions)
    );
    assert_eq!(notification_settings.channel_overrides.len(), 1);
    assert_eq!(
        notification_settings.channel_overrides[0].channel_id,
        Id::new(20)
    );
    assert_eq!(
        notification_settings.channel_overrides[0].message_notifications,
        Some(NotificationLevel::NoMessages)
    );
    assert!(notification_settings.channel_overrides[0].muted);
}
