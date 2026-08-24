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
fn channel_parser_reads_forum_tags_and_media_type() {
    let channel = parse_channel_info(
        &json!({
            "id": "10",
            "type": 16,
            "name": "support",
            "flags": 16,
            "available_tags": [{
                "id": "101",
                "name": "Resolved",
                "moderated": true,
                "emoji_id": "201"
            }]
        }),
        None,
    )
    .expect("media channel should parse");

    assert_eq!(channel.kind, "media");
    assert!(channel.requires_forum_tag());
    assert_eq!(channel.available_tags.len(), 1);
    assert_eq!(channel.available_tags[0].id.get(), 101);
    assert_eq!(channel.available_tags[0].name, "Resolved");
    assert!(channel.available_tags[0].moderated);
    assert_eq!(
        channel.available_tags[0].emoji_id.map(|id| id.get()),
        Some(201)
    );
}

#[test]
fn channel_parser_reads_thread_applied_tags() {
    let channel = parse_channel_info(
        &json!({
            "id": "20",
            "type": 11,
            "name": "post",
            "parent_id": "10",
            "thread_metadata": {
                "archived": false,
                "locked": false
            },
            "applied_tags": ["101", "102"]
        }),
        None,
    )
    .expect("thread should parse");

    assert_eq!(
        channel
            .applied_tags
            .iter()
            .map(|tag_id| tag_id.get())
            .collect::<Vec<_>>(),
        vec![101, 102]
    );
}

#[test]
fn raw_ready_parser_adds_current_user_to_group_dm_recipients() {
    let events = parse_user_account_event(
        &json!({
            "t": "READY",
            "d": {
                "user": {
                    "id": "99",
                    "username": "neo"
                },
                "sessions": [{ "status": "idle" }],
                "guilds": [],
                "merged_presences": {
                    "friends": [
                        { "user": { "id": "20" }, "status": "online" },
                        { "user": { "id": "30" }, "status": "idle" }
                    ]
                },
                "private_channels": [{
                    "id": "10",
                    "type": 3,
                    "name": "project chat",
                    "recipients": [
                        {
                            "id": "20",
                            "username": "alice",
                            "global_name": "Alice",
                            "bot": false
                        },
                        {
                            "id": "30",
                            "username": "helper-bot",
                            "bot": true
                        }
                    ]
                }]
            }
        })
        .to_string(),
    );

    let channel = events
        .iter()
        .find_map(|event| match event {
            AppEvent::ChannelUpsert(channel) => Some(channel),
            _ => None,
        })
        .expect("ready should emit a private channel upsert");
    let recipients = channel
        .recipients
        .as_ref()
        .expect("group dm should carry recipients");

    assert_eq!(channel.kind, "group-dm");
    assert_eq!(recipients.len(), 3);
    assert_eq!(recipients[0].user_id, Id::new(20));
    assert_eq!(recipients[0].display_name, "Alice");
    assert!(!recipients[0].is_bot);
    assert_eq!(recipients[0].status, Some(PresenceStatus::Online));
    assert_eq!(recipients[1].display_name, "helper-bot");
    assert!(recipients[1].is_bot);
    assert_eq!(recipients[1].status, Some(PresenceStatus::Idle));
    assert_eq!(recipients[2].user_id, Id::new(99));
    assert_eq!(recipients[2].display_name, "neo");
    assert_eq!(recipients[2].status, Some(PresenceStatus::Idle));
    assert!(events.iter().any(|event| matches!(
        event,
        AppEvent::PresenceUpdate { guild_id: None, presence }
            if presence.user_id == Id::new(99) && presence.status == PresenceStatus::Idle
    )));
}

#[test]
fn ready_parsers_emit_authoritative_snapshot_boundaries() {
    let ready = parse_user_account_event(
        &json!({
            "t": "READY",
            "d": {
                "user": { "id": "99", "username": "neo" },
                "guilds": [{
                    "id": "10",
                    "name": "guild",
                    "channels": [{ "id": "20", "type": 0, "name": "general" }],
                    "threads": [{
                        "id": "21",
                        "type": 11,
                        "name": "thread",
                        "parent_id": "20",
                        "thread_metadata": {
                            "archived": false,
                            "archive_timestamp": "2026-08-10T00:00:00.000000+00:00",
                            "auto_archive_duration": 1440,
                            "locked": false
                        }
                    }]
                }],
                "private_channels": [{ "id": "30", "type": 1 }]
            }
        })
        .to_string(),
    );
    assert!(matches!(
        ready.last(),
        Some(AppEvent::ReadySnapshotComplete { snapshot })
            if snapshot.guild_ids.as_deref() == Some(&[Id::new(10)])
                && snapshot.guild_channel_ids.get(&Id::new(10)).map(Vec::as_slice)
                    == Some(&[Id::new(20), Id::new(21)])
                && snapshot.private_channel_ids.as_deref() == Some(&[Id::new(30)])
    ));

    let supplemental = parse_user_account_event(
        &json!({
            "t": "READY_SUPPLEMENTAL",
            "d": {
                "lazy_private_channels": [{ "id": "31", "type": 1 }]
            }
        })
        .to_string(),
    );
    assert!(matches!(
        supplemental.last(),
        Some(AppEvent::ReadySupplementalComplete { private_channel_ids })
            if private_channel_ids == &[Id::new(31)]
    ));
}

#[test]
fn raw_ready_parser_exposes_current_user_premium_capability() {
    for (premium_type, expected) in [(0, PremiumTier::None), (2, PremiumTier::Nitro)] {
        let events = parse_user_account_event(
            &json!({
                "t": "READY",
                "d": {
                    "user": {
                        "id": "99",
                        "username": "neo",
                        "premium_type": premium_type
                    },
                    "guilds": []
                }
            })
            .to_string(),
        );

        assert!(
            events.iter().any(|event| matches!(
                event,
                AppEvent::CurrentUserCapabilities { premium_tier } if *premium_tier == expected
            )),
            "premium_type {premium_type}"
        );
    }
}

#[test]
fn raw_ready_parser_applies_guild_merged_presence_to_dm_recipient() {
    let events = parse_user_account_event(
        &json!({
            "t": "READY",
            "d": {
                "user": {
                    "id": "99",
                    "username": "neo"
                },
                "guilds": [],
                "merged_presences": {
                    "friends": [],
                    "guilds": [[
                        { "user_id": "20", "status": "idle" }
                    ]]
                },
                "private_channels": [{
                    "id": "10",
                    "type": 1,
                    "recipients": [{
                        "id": "20",
                        "username": "alice"
                    }]
                }]
            }
        })
        .to_string(),
    );

    let channel = events
        .iter()
        .find_map(|event| match event {
            AppEvent::ChannelUpsert(channel) => Some(channel),
            _ => None,
        })
        .expect("ready should emit a private channel upsert");
    let recipients = channel
        .recipients
        .as_ref()
        .expect("dm should carry recipients");

    assert_eq!(channel.kind, "dm");
    assert_eq!(recipients[0].user_id, Id::new(20));
    assert_eq!(recipients[0].status, Some(PresenceStatus::Idle));
}

#[test]
fn raw_ready_supplemental_updates_user_presences() {
    let events = parse_user_account_event(
        &json!({
            "t": "READY_SUPPLEMENTAL",
            "d": {
                "merged_presences": {
                    "friends": [
                        { "user_id": "20", "status": "online" }
                    ],
                    "guilds": [[
                        { "user_id": "30", "status": "idle" }
                    ]]
                }
            }
        })
        .to_string(),
    );

    assert_eq!(events.len(), 2);
    assert!(events.iter().any(|event| matches!(
        event,
        AppEvent::PresenceUpdate { guild_id: None, presence }
            if presence.user_id == Id::new(20) && presence.status == PresenceStatus::Online
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        AppEvent::PresenceUpdate { guild_id: None, presence }
            if presence.user_id == Id::new(30) && presence.status == PresenceStatus::Idle
    )));
}

#[test]
fn raw_presence_update_extracts_activities() {
    let events = parse_user_account_event(
        &json!({
            "t": "PRESENCE_UPDATE",
            "d": {
                "guild_id": "10",
                "user": { "id": "20" },
                "status": "online",
                "activities": [
                    {
                        "type": 4,
                        "name": "Custom Status",
                        "state": "Coding hard",
                        "emoji": { "name": "🦀" }
                    },
                    {
                        "type": 2,
                        "name": "Spotify",
                        "details": "Bohemian Rhapsody",
                        "state": "Queen"
                    },
                    {
                        "type": 0,
                        "name": "Concord"
                    }
                ]
            }
        })
        .to_string(),
    );

    let (guild_id, activities) = events
        .iter()
        .find_map(|event| match event {
            AppEvent::PresenceUpdate { guild_id, presence } => {
                Some((*guild_id, &presence.activities))
            }
            _ => None,
        })
        .expect("PRESENCE_UPDATE should produce a PresenceUpdate event");

    assert_eq!(guild_id, Some(Id::new(10)));
    assert_eq!(activities.len(), 3);
    assert_eq!(activities[0].kind, ActivityKind::Custom);
    assert_eq!(activities[0].state.as_deref(), Some("Coding hard"));
    assert_eq!(
        activities[0].emoji.as_ref().map(|e| e.name.as_str()),
        Some("🦀")
    );
    assert_eq!(activities[1].kind, ActivityKind::Listening);
    assert_eq!(activities[1].name, "Spotify");
    assert_eq!(activities[1].details.as_deref(), Some("Bohemian Rhapsody"));
    assert_eq!(activities[1].state.as_deref(), Some("Queen"));
    assert_eq!(activities[2].kind, ActivityKind::Playing);
    assert_eq!(activities[2].name, "Concord");
}

#[test]
fn raw_presence_update_without_guild_id_emits_user_event_with_activities() {
    let events = parse_user_account_event(
        &json!({
            "t": "PRESENCE_UPDATE",
            "d": {
                "user": { "id": "20" },
                "status": "dnd",
                "activities": [
                    { "type": 1, "name": "Twitch", "url": "https://twitch.tv/foo" }
                ]
            }
        })
        .to_string(),
    );

    let activities = events
        .iter()
        .find_map(|event| match event {
            AppEvent::PresenceUpdate {
                guild_id: None,
                presence,
            } => Some(&presence.activities),
            _ => None,
        })
        .expect("PRESENCE_UPDATE without guild_id should produce a PresenceUpdate without guild");

    assert_eq!(activities.len(), 1);
    assert_eq!(activities[0].kind, ActivityKind::Streaming);
    assert_eq!(activities[0].name, "Twitch");
    assert_eq!(activities[0].url.as_deref(), Some("https://twitch.tv/foo"));
}

#[test]
fn raw_ready_supplemental_aligns_merged_members_by_guild_index() {
    let events = parse_user_account_event(
        &json!({
            "t": "READY_SUPPLEMENTAL",
            "d": {
                "guilds": [{ "id": "1" }, { "id": "2" }],
                "merged_members": [[{
                    "user_id": "10",
                    "roles": ["20"]
                }], [{
                    "user_id": "10",
                    "roles": ["30"]
                }]]
            }
        })
        .to_string(),
    );

    assert!(events.iter().any(|event| matches!(
        event,
        AppEvent::GuildMemberUpsert { guild_id, member }
            if *guild_id == Id::new(1)
                && member.user_id == Id::new(10)
                && member.role_ids == vec![Id::new(20)]
                && member.role_ids_present
                && !member.is_bot_present
                && !member.avatar_url_present
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        AppEvent::GuildMemberUpsert { guild_id, member }
            if *guild_id == Id::new(2)
                && member.user_id == Id::new(10)
                && member.role_ids == vec![Id::new(30)]
    )));
}

#[test]
fn partial_member_fields_only_replace_cached_data_when_their_types_are_valid() {
    let events = parse_user_account_event(
        &json!({
            "t": "GUILD_MEMBER_UPDATE",
            "d": {
                "guild_id": "1",
                "user_id": "10",
                "avatar": null,
                "roles": null,
                "user": { "id": "10", "bot": null }
            }
        })
        .to_string(),
    );

    assert!(matches!(
        events.as_slice(),
        [AppEvent::GuildMemberUpsert { guild_id, member }]
            if *guild_id == Id::new(1)
                && member.user_id == Id::new(10)
                && member.avatar_url.as_deref()
                    == Some("https://cdn.discordapp.com/embed/avatars/0.png")
                && member.avatar_url_present
                && !member.role_ids_present
                && !member.is_bot_present
    ));
}

#[test]
fn raw_ready_supplemental_member_roles_hide_role_denied_channel() {
    let ready_events = parse_user_account_event(
        &json!({
            "t": "READY",
            "d": {
                "user": { "id": "10", "username": "me" },
                "guilds": [{
                    "id": "1",
                    "name": "guild",
                    "owner_id": "11",
                    "channels": [{
                        "id": "2",
                        "type": 0,
                        "name": "staff-hidden",
                        "permission_overwrites": [{
                            "id": "20",
                            "type": 0,
                            "allow": "0",
                            "deny": "1024"
                        }]
                    }],
                    "members": [],
                    "presences": [],
                    "roles": [],
                    "emojis": []
                }],
                "private_channels": []
            }
        })
        .to_string(),
    );
    let supplemental_events = parse_user_account_event(
        &json!({
            "t": "READY_SUPPLEMENTAL",
            "d": {
                "guilds": [{
                    "id": "1",
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
                    }]
                }],
                "merged_members": [[{
                    "user_id": "10",
                    "roles": ["20"]
                }]]
            }
        })
        .to_string(),
    );
    let mut state = DiscordState::default();
    for event in ready_events.iter().chain(supplemental_events.iter()) {
        state.apply_event(event);
    }

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
fn raw_ready_supplemental_accepts_bare_id_presence_entries() {
    let events = parse_user_account_event(
        &json!({
            "t": "READY_SUPPLEMENTAL",
            "d": {
                "merged_presences": {
                    "friends": [
                        { "id": "20", "status": "online" }
                    ]
                }
            }
        })
        .to_string(),
    );

    assert!(matches!(
        events.as_slice(),
        [AppEvent::PresenceUpdate { guild_id: None, presence }]
            if presence.user_id == Id::new(20) && presence.status == PresenceStatus::Online
    ));
}

#[test]
fn raw_ready_supplemental_ignores_non_presence_ids() {
    let events = parse_user_account_event(
        &json!({
            "t": "READY_SUPPLEMENTAL",
            "d": {
                "merged_presences": {
                    "friends": [],
                    "metadata": { "id": "20" }
                }
            }
        })
        .to_string(),
    );

    assert!(events.is_empty());
}

#[test]
fn raw_presence_update_accepts_user_id_field() {
    let events = parse_user_account_event(
        &json!({
            "t": "PRESENCE_UPDATE",
            "d": {
                "user_id": "20",
                "status": "online"
            }
        })
        .to_string(),
    );

    assert!(matches!(
        events.as_slice(),
        [AppEvent::PresenceUpdate { guild_id: None, presence }]
            if presence.user_id == Id::new(20) && presence.status == PresenceStatus::Online
    ));
}

#[test]
fn raw_presence_update_parses_rich_activity_fields() {
    let events = parse_user_account_event(
        &json!({
            "t": "PRESENCE_UPDATE",
            "d": {
                "user": { "id": "20" },
                "status": "online",
                "activities": [{
                    "type": 0,
                    "name": "Concord",
                    "application_id": "12345",
                    "timestamps": { "start": 1_700_000_000_000i64 },
                    "assets": { "large_image": "cover", "large_text": "Main menu" },
                    "party": { "id": "party-1", "size": [2, 5] },
                    "buttons": ["Join"],
                    "metadata": { "button_urls": ["https://example.com/join"] }
                }]
            }
        })
        .to_string(),
    );

    let [AppEvent::PresenceUpdate { presence, .. }] = events.as_slice() else {
        panic!("expected a single presence update, got {events:?}");
    };
    let activity = &presence.activities[0];
    assert_eq!(
        activity.timestamps.and_then(|t| t.start),
        Some(1_700_000_000_000)
    );
    let assets = activity.assets.as_ref().expect("assets parsed");
    assert_eq!(assets.large_image.as_deref(), Some("cover"));
    assert_eq!(assets.large_text.as_deref(), Some("Main menu"));
    let party = activity.party.as_ref().expect("party parsed");
    assert_eq!(party.size, Some((2, 5)));
    assert_eq!(activity.buttons.len(), 1);
    assert_eq!(activity.buttons[0].label, "Join");
    assert_eq!(activity.buttons[0].url, "https://example.com/join");
}

#[test]
fn thread_channel_parser_keeps_counts_and_status() {
    let channel = parse_channel_info(
        &json!({
            "id": "10",
            "guild_id": "1",
            "parent_id": "2",
            "type": 11,
            "name": "release notes",
            "message_count": 12,
            "total_message_sent": 14,
            "thread_metadata": { "archived": true, "locked": false }
        }),
        None,
    )
    .expect("thread channel should parse");

    assert_eq!(channel.kind, "GuildPublicThread");
    assert_eq!(channel.message_count, Some(12));
    assert_eq!(channel.total_message_sent, Some(14));
    assert_eq!(channel.thread_archived(), Some(true));
    assert_eq!(channel.thread_locked(), Some(false));
}

#[test]
fn thread_channel_parser_uses_only_real_current_user_member_settings() {
    let channel = parse_channel_info(
        &json!({
            "id": "10",
            "guild_id": "1",
            "parent_id": "2",
            "type": 11,
            "name": "release notes",
            "member": {
                "id": "10",
                "user_id": "99",
                "flags": 9,
                "muted": true,
                "mute_config": { "end_time": "2099-01-01T00:00:00.000Z" }
            },
            "thread_metadata": { "archived": false, "locked": false }
        }),
        None,
    )
    .expect("thread channel should parse");

    assert_eq!(channel.current_user_joined_thread, Some(true));
    assert_eq!(channel.current_user_thread_notification_flags, Some(9));
    assert_eq!(channel.current_user_thread_muted, Some(true));
    assert_eq!(
        channel.current_user_thread_mute_end_time.as_deref(),
        Some("2099-01-01T00:00:00.000Z")
    );

    for (name, member) in [("null", Some(Value::Null)), ("absent", None)] {
        let mut payload = thread_payload(10, "release notes");
        if let Some(member) = member {
            payload["member"] = member;
        }
        let channel = parse_channel_info(&payload, None).expect("thread channel should parse");
        assert_eq!(channel.current_user_joined_thread, None, "{name}");
        assert_eq!(
            channel.current_user_thread_notification_flags, None,
            "{name}"
        );
        assert_eq!(channel.current_user_thread_muted, None, "{name}");
        assert_eq!(channel.current_user_thread_mute_end_time, None, "{name}");
    }
}

#[test]
fn raw_thread_member_updates_keep_current_user_state() {
    let joined = parse_user_account_event(
        &json!({
            "t": "THREAD_MEMBERS_UPDATE",
            "d": {
                "id": "10",
                "guild_id": "1",
                "added_members": [{ "user_id": "99" }]
            }
        })
        .to_string(),
    );
    let left = parse_user_account_event(
        &json!({
            "t": "THREAD_MEMBERS_UPDATE",
            "d": {
                "id": "10",
                "guild_id": "1",
                "removed_member_ids": ["99"]
            }
        })
        .to_string(),
    );
    let current_user = parse_user_account_event(
        &json!({
            "t": "THREAD_MEMBER_UPDATE",
            "d": {
                "id": "10",
                "guild_id": "1",
                "user_id": "99",
                "flags": 9,
                "muted": true,
                "mute_config": { "end_time": "2099-01-01T00:00:00.000Z" }
            }
        })
        .to_string(),
    );

    assert!(matches!(
        joined.as_slice(),
        [AppEvent::ThreadMembersUpdateDispatch { update }]
            if update.channel_id == Id::new(10)
                && update.guild_id == Some(Id::new(1))
                && update.added_members.iter().map(|member| member.user_id).collect::<Vec<_>>()
                    == vec![Id::new(99)]
                && update.removed_user_ids.is_empty()
    ));
    assert!(matches!(
        left.as_slice(),
        [AppEvent::ThreadMembersUpdateDispatch { update }]
            if update.channel_id == Id::new(10)
                && update.added_members.is_empty()
                && update.removed_user_ids == vec![Id::new(99)]
    ));
    assert!(matches!(
        current_user.as_slice(),
        [AppEvent::ThreadMemberUpdate {
            guild_id,
            channel_id,
            flags: Some(9),
            muted: Some(true),
            mute_end_time,
        }] if *guild_id == Some(Id::new(1))
            && *channel_id == Id::new(10)
            && mute_end_time.as_deref() == Some("2099-01-01T00:00:00.000Z")
    ));

    let mut state = DiscordState::default();
    let thread = parse_channel_info(&thread_payload(10, "release notes"), None)
        .expect("thread channel should parse");
    state.apply_event(&AppEvent::ChannelUpsert(thread));
    for event in &current_user {
        state.apply_event(event);
    }

    let thread = state
        .channel(Id::new(10))
        .expect("thread should stay cached");
    assert!(thread.current_user_joined_thread);
    assert_eq!(thread.current_user_thread_notification_flags, Some(9));
    assert!(thread.current_user_thread_muted);
    assert_eq!(
        thread.current_user_thread_mute_end_time.as_deref(),
        Some("2099-01-01T00:00:00.000Z")
    );
}

#[test]
fn raw_thread_create_upserts_thread_channel() {
    let events = parse_user_account_event(
        &json!({
            "t": "THREAD_CREATE",
            "d": thread_payload(10, "release notes")
        })
        .to_string(),
    );

    assert!(matches!(
        events.as_slice(),
        [AppEvent::ChannelUpsert(channel)]
            if channel.channel_id == Id::new(10)
                && channel.guild_id == Some(Id::new(1))
                && channel.parent_id == Some(Id::new(2))
                && channel.name == "release notes"
                && channel.kind == "GuildPublicThread"
                && channel.message_count == Some(12)
                && channel.total_message_sent == Some(14)
                && channel.thread_archived() == Some(false)
                && channel.thread_locked() == Some(false)
    ));
}

#[test]
fn raw_thread_delete_removes_thread_channel() {
    let events = parse_user_account_event(
        &json!({
            "t": "THREAD_DELETE",
            "d": {
                "id": "10",
                "guild_id": "1",
                "parent_id": "2",
                "type": 11
            }
        })
        .to_string(),
    );

    assert!(matches!(
        events.as_slice(),
        [AppEvent::ChannelDelete { guild_id, channel_id }]
            if *guild_id == Some(Id::new(1)) && *channel_id == Id::new(10)
    ));
}
