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
fn message_reaction_remove_dispatch_parses_custom_reaction_event() {
    let events = parse_user_account_event(
        &json!({
            "t": "MESSAGE_REACTION_REMOVE",
            "d": {
                "channel_id": "10",
                "message_id": "20",
                "user_id": "30",
                "emoji": {
                    "id": "40",
                    "name": "party",
                    "animated": true
                }
            }
        })
        .to_string(),
    );

    assert_eq!(events.len(), 1);
    let AppEvent::MessageReactionRemove {
        guild_id,
        channel_id,
        message_id,
        user_id,
        emoji,
    } = &events[0]
    else {
        panic!("expected message reaction remove event");
    };
    assert_eq!(*guild_id, None);
    assert_eq!(*channel_id, Id::new(10));
    assert_eq!(*message_id, Id::new(20));
    assert_eq!(*user_id, Id::new(30));
    assert_eq!(
        emoji,
        &ReactionEmoji::Custom {
            id: Id::new(40),
            name: Some("party".to_owned()),
            animated: true,
        }
    );
}

#[test]
fn message_reaction_remove_all_dispatch_parses_clear_event() {
    let events = parse_user_account_event(
        &json!({
            "t": "MESSAGE_REACTION_REMOVE_ALL",
            "d": {
                "guild_id": "1",
                "channel_id": "10",
                "message_id": "20"
            }
        })
        .to_string(),
    );

    assert_eq!(events.len(), 1);
    let AppEvent::MessageReactionRemoveAll {
        guild_id,
        channel_id,
        message_id,
    } = &events[0]
    else {
        panic!("expected message reaction remove all event");
    };
    assert_eq!(*guild_id, Some(Id::new(1)));
    assert_eq!(*channel_id, Id::new(10));
    assert_eq!(*message_id, Id::new(20));
}

#[test]
fn message_reaction_remove_emoji_dispatch_parses_clear_emoji_event() {
    let events = parse_user_account_event(
        &json!({
            "t": "MESSAGE_REACTION_REMOVE_EMOJI",
            "d": {
                "channel_id": "10",
                "message_id": "20",
                "emoji": { "name": "👍" }
            }
        })
        .to_string(),
    );

    assert_eq!(events.len(), 1);
    let AppEvent::MessageReactionRemoveEmoji {
        guild_id,
        channel_id,
        message_id,
        emoji,
    } = &events[0]
    else {
        panic!("expected message reaction remove emoji event");
    };
    assert_eq!(*guild_id, None);
    assert_eq!(*channel_id, Id::new(10));
    assert_eq!(*message_id, Id::new(20));
    assert_eq!(emoji, &ReactionEmoji::Unicode("👍".to_owned()));
}

#[test]
fn message_create_parser_keeps_regular_embeds() {
    let event = parse_message_create(&json!({
        "id": "20",
        "channel_id": "10",
        "author": { "id": "30", "username": "neo" },
        "content": "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
        "embeds": [{
            "type": "video",
            "color": 16711680,
            "provider": { "name": "YouTube" },
            "title": "Example Video",
            "description": "A video description",
            "timestamp": "2026-05-13T15:22:03+00:00",
            "url": "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
            "thumbnail": {
                "url": "https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg",
                "proxy_url": "https://images-ext-1.discordapp.net/external/thumb/hash/https/i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg",
                "width": 480,
                "height": 360
            },
            "image": {
                "url": "https://i.ytimg.com/vi/dQw4w9WgXcQ/maxresdefault.jpg",
                "proxy_url": "https://images-ext-2.discordapp.net/external/image/hash/https/i.ytimg.com/vi/dQw4w9WgXcQ/maxresdefault.jpg",
                "width": 1280,
                "height": 720
            },
            "video": { "url": "https://www.youtube.com/embed/dQw4w9WgXcQ" }
        }]
    }))
    .expect("message create should parse");

    let AppEvent::MessageCreate { message } = event else {
        panic!("expected message create event");
    };
    assert_eq!(message.embeds.len(), 1);
    assert_eq!(message.embeds[0].color, Some(16711680));
    assert_eq!(message.embeds[0].provider_name.as_deref(), Some("YouTube"));
    assert_eq!(message.embeds[0].title.as_deref(), Some("Example Video"));
    assert_eq!(
        message.embeds[0].timestamp.as_deref(),
        Some("2026-05-13T15:22:03+00:00")
    );
    assert_eq!(
        message.embeds[0].thumbnail_url.as_deref(),
        Some("https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg")
    );
    assert_eq!(
        message.embeds[0].thumbnail_proxy_url.as_deref(),
        Some(
            "https://images-ext-1.discordapp.net/external/thumb/hash/https/i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg"
        )
    );
    assert_eq!(message.embeds[0].thumbnail_width, Some(480));
    assert_eq!(message.embeds[0].thumbnail_height, Some(360));
    assert_eq!(
        message.embeds[0].image_url.as_deref(),
        Some("https://i.ytimg.com/vi/dQw4w9WgXcQ/maxresdefault.jpg")
    );
    assert_eq!(
        message.embeds[0].image_proxy_url.as_deref(),
        Some(
            "https://images-ext-2.discordapp.net/external/image/hash/https/i.ytimg.com/vi/dQw4w9WgXcQ/maxresdefault.jpg"
        )
    );
    assert_eq!(message.embeds[0].image_width, Some(1280));
    assert_eq!(message.embeds[0].image_height, Some(720));
    assert_eq!(
        message.embeds[0].video_url.as_deref(),
        Some("https://www.youtube.com/embed/dQw4w9WgXcQ")
    );
}

#[test]
fn message_create_parser_builds_giphy_animation_url_for_gifv() {
    let event = parse_message_create(&json!({
        "id": "20",
        "channel_id": "10",
        "author": { "id": "30", "username": "neo" },
        "content": "https://giphy.com/gifs/hvY8Ahy9r340SU8xLY",
        "embeds": [{
            "type": "gifv",
            "url": "https://giphy.com/gifs/hvY8Ahy9r340SU8xLY",
            "thumbnail": {
                "url": "https://media2.giphy.com/media/hvY8Ahy9r340SU8xLY/giphy_s.gif",
                "width": 500,
                "height": 599
            },
            "video": {
                "url": "https://media2.giphy.com/media/hvY8Ahy9r340SU8xLY/giphy.mp4?cid=discord",
                "width": 500,
                "height": 599
            }
        }]
    }))
    .expect("message create should parse");

    let AppEvent::MessageCreate { message } = event else {
        panic!("expected message create event");
    };
    assert_eq!(
        message.embeds[0].gifv_image_url.as_deref(),
        Some("https://media2.giphy.com/media/hvY8Ahy9r340SU8xLY/giphy.webp?cid=discord")
    );
}

#[test]
fn message_create_parser_keeps_timestamp_only_embeds() {
    let event = parse_message_create(&json!({
        "id": "20",
        "channel_id": "10",
        "author": { "id": "30", "username": "neo" },
        "content": "",
        "embeds": [{
            "timestamp": "2026-05-13T15:22:03+00:00"
        }]
    }))
    .expect("message create should parse");

    let AppEvent::MessageCreate { message } = event else {
        panic!("expected message create event");
    };
    assert_eq!(message.embeds.len(), 1);
    assert_eq!(
        message.embeds[0].timestamp.as_deref(),
        Some("2026-05-13T15:22:03+00:00")
    );
}

#[test]
fn message_create_parser_keeps_message_type() {
    let event = parse_message_create(&json!({
        "id": "20",
        "channel_id": "10",
        "author": { "id": "30", "username": "mee6", "bot": true },
        "type": 20,
        "content": "",
        "attachments": [],
        "interaction": {
            "name": "anime search",
            "user": { "id": "40", "global_name": "Casey", "username": "casey" }
        },
        "interaction_metadata": {
            "user": { "id": "40", "global_name": "Casey", "username": "casey" }
        }
    }))
    .expect("message create should parse");

    let AppEvent::MessageCreate { message } = event else {
        panic!("expected message create event");
    };
    assert_eq!(message.message_kind, MessageKind::new(20));
    assert!(message.author_is_bot);
    let interaction = message
        .interaction
        .expect("interaction metadata should parse");
    assert_eq!(interaction.user_id, Some(Id::new(40)));
    assert_eq!(interaction.user, "Casey");
    assert_eq!(interaction.command_name.as_deref(), Some("anime search"));
}

#[test]
fn message_create_parser_resolves_author_name_by_precedence() {
    // Server nick beats global name beats username.
    let cases = [
        (
            json!({ "nick": "server alias" }),
            Some(Id::new(1)),
            "server alias",
        ),
        (json!(null), None, "global alias"),
    ];

    for (member, guild_id, expected_author) in cases {
        let event = parse_message_create(&json!({
            "id": "20",
            "channel_id": "10",
            "guild_id": guild_id.map(|id: Id<_>| id.get().to_string()),
            "author": { "id": "30", "global_name": "global alias", "username": "neo" },
            "member": member,
            "content": "hello",
            "attachments": []
        }))
        .expect("message create should parse");

        let AppEvent::MessageCreate { message } = event else {
            panic!("expected message create event");
        };
        assert_eq!(message.guild_id, guild_id);
        assert_eq!(message.author, expected_author);
    }
}

#[test]
fn message_info_parser_preserves_webhook_identity() {
    let message = parse_message_info(&json!({
        "id": "20",
        "channel_id": "10",
        "webhook_id": "40",
        "author": {
            "id": "30",
            "global_name": "cached bot name",
            "username": "Persona One",
            "avatar": "avatarhash",
            "bot": true
        },
        "content": "hello"
    }))
    .expect("webhook message should parse");

    assert_eq!(message.webhook_id, Some(Id::new(40)));
    assert_eq!(message.author, "Persona One");
    assert_eq!(
        message.author_avatar_url.as_deref(),
        Some("https://cdn.discordapp.com/avatars/30/avatarhash.png")
    );
}

#[test]
fn message_info_parser_tracks_author_role_payload_presence() {
    let cases = [
        (
            "roles present",
            json!({ "roles": ["90", "91"] }),
            vec![Id::new(90), Id::new(91)],
            true,
        ),
        (
            "roles explicitly empty",
            json!({ "roles": [] }),
            vec![],
            true,
        ),
        ("member omitted", Value::Null, vec![], false),
    ];

    for (label, member, expected_roles, expected_presence) in cases {
        let mut payload = json!({
            "id": "20",
            "channel_id": "10",
            "guild_id": "1",
            "author": { "id": "30", "username": "neo" },
            "content": "hello",
            "attachments": []
        });
        if !member.is_null() {
            payload["member"] = member;
        }
        let message = parse_message_info(&payload).expect("message should parse");

        assert_eq!(message.author_role_ids, expected_roles, "{label}");
        assert_eq!(
            message.author_role_ids_present, expected_presence,
            "{label}"
        );
    }
}

#[test]
fn message_info_parser_keeps_outgoing_nonce() {
    let message = parse_message_info(&json!({
        "id": "20",
        "channel_id": "10",
        "nonce": "99",
        "author": { "id": "30", "username": "neo" },
        "content": "hello",
        "attachments": []
    }))
    .expect("message should parse");

    assert_eq!(message.nonce, Some(Id::new(99)));
}

#[test]
fn message_create_parser_builds_author_avatar_url() {
    let event = parse_message_create(&json!({
        "id": "20",
        "channel_id": "10",
        "author": {
            "id": "30",
            "username": "neo",
            "avatar": "a_avatarhash"
        },
        "content": "hello",
        "attachments": []
    }))
    .expect("message create should parse");

    let AppEvent::MessageCreate { message } = event else {
        panic!("expected message create event");
    };
    assert_eq!(
        message.author_avatar_url.as_deref(),
        Some("https://cdn.discordapp.com/avatars/30/a_avatarhash.gif")
    );
}

#[test]
fn message_create_parser_keeps_mention_display_names() {
    let event = parse_message_create(&json!({
        "id": "20",
        "channel_id": "10",
        "author": { "id": "30", "username": "neo" },
        "content": "hello <@40> <@41> <@42> <@43>",
        "mention_everyone": true,
        "mention_roles": ["50", "51"],
        "flags": 4096,
        "mentions": [
            {
                "id": "40",
                "username": "alpha",
                "global_name": "Alpha Global",
                "member": { "nick": "Alpha Nick" }
            },
            {
                "id": "41",
                "username": "beta",
                "global_name": "Beta Global"
            },
            {
                "id": "42",
                "username": "gamma"
            },
            { "id": "43" }
        ],
        "attachments": []
    }))
    .expect("message create should parse");

    let AppEvent::MessageCreate { message } = event else {
        panic!("expected message create event");
    };
    assert!(message.mention_everyone);
    assert_eq!(message.mention_roles, vec![Id::new(50), Id::new(51)]);
    assert_eq!(message.flags, 4096);
    assert_eq!(
        message.mentions,
        vec![
            mention_info_with_nick(40, "Alpha Nick"),
            mention_info(41, "Beta Global"),
            mention_info(42, "gamma"),
            mention_info(43, "unknown"),
        ]
    );
}

#[test]
fn message_create_parser_does_not_store_empty_mention_nick() {
    let event = parse_message_create(&json!({
        "id": "20",
        "channel_id": "10",
        "author": { "id": "30", "username": "neo" },
        "content": "hello <@40>",
        "mentions": [{
            "id": "40",
            "username": "alpha",
            "member": { "nick": "" }
        }],
        "attachments": []
    }))
    .expect("message create should parse");

    let AppEvent::MessageCreate { message } = event else {
        panic!("expected message create event");
    };
    assert_eq!(message.mentions, vec![mention_info(40, "alpha")]);
}

#[test]
fn message_create_parser_keeps_reply_preview() {
    let event = parse_message_create(&json!({
        "id": "20",
        "channel_id": "10",
        "author": { "id": "30", "username": "neo" },
        "type": 19,
        "content": "reply",
        "attachments": [],
        "referenced_message": {
            "id": "19",
            "channel_id": "10",
            "author": { "id": "31", "global_name": "Alex", "username": "alex" },
            "content": "잘되는군",
            "attachments": []
        }
    }))
    .expect("message create should parse");

    let AppEvent::MessageCreate { message } = event else {
        panic!("expected message create event");
    };
    assert_eq!(
        message.reply,
        Some(ReplyInfo {
            author_id: Some(Id::new(31)),
            author: "Alex".to_owned(),
            content: Some("잘되는군".to_owned()),
            sticker_names: Vec::new(),
            stickers: Vec::new(),
            mentions: Vec::new(),
        })
    );
}

#[test]
fn message_create_parser_keeps_reply_mentions() {
    let event = parse_message_create(&json!({
        "id": "20",
        "channel_id": "10",
        "author": { "id": "30", "username": "neo" },
        "type": 19,
        "content": "reply",
        "attachments": [],
        "referenced_message": {
            "id": "19",
            "channel_id": "10",
            "author": { "id": "31", "username": "alex" },
            "content": "hello <@40>",
            "mentions": [{ "id": "40", "username": "alice" }],
            "attachments": []
        }
    }))
    .expect("message create should parse");

    let AppEvent::MessageCreate { message } = event else {
        panic!("expected message create event");
    };
    assert_eq!(
        message
            .reply
            .and_then(|reply| reply.mentions.into_iter().next()),
        Some(mention_info(40, "alice"))
    );
}

#[test]
fn message_create_parser_keeps_poll_payload() {
    let event = parse_message_create(&json!({
        "id": "20",
        "channel_id": "10",
        "author": { "id": "30", "username": "neo" },
        "type": 0,
        "content": "",
        "attachments": [],
        "poll": {
            "question": { "text": "오늘 뭐 먹지?" },
            "answers": [
                { "answer_id": 1, "poll_media": { "text": "김치찌개" } },
                { "answer_id": 2, "poll_media": { "text": "라멘" } }
            ],
            "results": {
                "is_finalized": false,
                "answer_counts": [
                    { "id": 1, "count": 2, "me_voted": true },
                    { "id": 2, "count": 1, "me_voted": false }
                ]
            },
            "allow_multiselect": true
        }
    }))
    .expect("message create should parse");

    let AppEvent::MessageCreate { message } = event else {
        panic!("expected message create event");
    };
    assert_eq!(
        message.poll,
        Some(PollInfo {
            question: "오늘 뭐 먹지?".to_owned(),
            answers: vec![
                PollAnswerInfo {
                    answer_id: 1,
                    text: "김치찌개".to_owned(),
                    vote_count: Some(2),
                    me_voted: true,
                },
                PollAnswerInfo {
                    answer_id: 2,
                    text: "라멘".to_owned(),
                    vote_count: Some(1),
                    me_voted: false,
                },
            ],
            allow_multiselect: true,
            results_finalized: Some(false),
            total_votes: Some(3),
        })
    );
}

#[test]
fn message_create_parser_keeps_poll_result_embed() {
    let event = parse_message_create(&json!({
        "id": "20",
        "channel_id": "10",
        "author": { "id": "30", "username": "neo" },
        "type": 46,
        "content": "",
        "attachments": [],
        "embeds": [{
            "type": "poll_result",
            "fields": [
                { "name": "poll_question_text", "value": "오늘 뭐 먹지?" },
                { "name": "victor_answer_id", "value": "1" },
                { "name": "victor_answer_text", "value": "김치찌개" },
                { "name": "victor_answer_votes", "value": "5" },
                { "name": "total_votes", "value": "7" }
            ]
        }]
    }))
    .expect("poll result message should parse");

    let AppEvent::MessageCreate { message } = event else {
        panic!("expected message create event");
    };
    assert_eq!(
        message
            .poll
            .expect("poll result should map to poll info")
            .total_votes,
        Some(7)
    );
}

#[test]
fn message_create_parser_uses_proxy_url_when_url_is_missing() {
    let event = parse_message_create(&json!({
        "id": "20",
        "channel_id": "10",
        "author": { "id": "30", "username": "neo" },
        "content": "",
        "attachments": [{
            "id": "40",
            "filename": "cat.png",
            "proxy_url": "https://media.discordapp.net/cat.png",
            "content_type": "image/png"
        }]
    }))
    .expect("message create should parse");

    let AppEvent::MessageCreate { message } = event else {
        panic!("expected message create event");
    };
    assert_eq!(message.attachments.len(), 1);
    assert_eq!(
        message.attachments[0].url,
        "https://media.discordapp.net/cat.png"
    );
    assert_eq!(
        message.attachments[0].proxy_url,
        "https://media.discordapp.net/cat.png"
    );
}

#[test]
fn message_create_parser_keeps_video_attachment_metadata() {
    let event = parse_message_create(&json!({
        "id": "20",
        "channel_id": "10",
        "author": { "id": "30", "username": "neo" },
        "content": "",
        "attachments": [{
            "id": "40",
            "filename": "clip.mp4",
            "url": "https://cdn.discordapp.com/clip.mp4",
            "proxy_url": "https://media.discordapp.net/clip.mp4",
            "content_type": "video/mp4",
            "size": 78364758,
            "width": 1920,
            "height": 1080,
            "description": "clip"
        }]
    }))
    .expect("message create should parse");

    let AppEvent::MessageCreate { message } = event else {
        panic!("expected message create event");
    };
    assert_eq!(message.attachments.len(), 1);
    assert_eq!(message.attachments[0].filename, "clip.mp4");
    assert_eq!(
        message.attachments[0].content_type.as_deref(),
        Some("video/mp4")
    );
    assert_eq!(message.attachments[0].size, 78_364_758);
    assert_eq!(message.attachments[0].width, Some(1920));
    assert_eq!(message.attachments[0].height, Some(1080));
}

#[test]
fn message_create_parser_keeps_animated_media_flags() {
    let event = parse_message_create(&json!({
        "id": "20",
        "channel_id": "10",
        "author": { "id": "30", "username": "neo" },
        "content": "",
        "attachments": [{
            "id": "40",
            "filename": "dance.webp",
            "url": "https://cdn.discordapp.com/dance.webp",
            "proxy_url": "https://media.discordapp.net/dance.webp",
            "content_type": "image/webp",
            "flags": 32
        }],
        "embeds": [{
            "type": "image",
            "thumbnail": {
                "url": "https://example.com/thumb.webp",
                "flags": 32
            },
            "image": {
                "url": "https://example.com/image.webp",
                "flags": 32
            }
        }]
    }))
    .expect("message create should parse");

    let AppEvent::MessageCreate { message } = event else {
        panic!("expected message create event");
    };
    assert_eq!(message.attachments[0].flags, 1 << 5);
    assert_eq!(message.embeds[0].thumbnail_flags, 1 << 5);
    assert_eq!(message.embeds[0].image_flags, 1 << 5);
}
