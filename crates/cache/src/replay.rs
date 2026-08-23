//! Turning cached rows back into events.
//!
//! Everything here builds an `AppEvent` identical in shape to one the gateway
//! would have sent. That is deliberate: a front end should not be able to tell
//! a cached guild from a live one, because the moment it can, there are two
//! rendering paths to keep in step and one of them is only exercised offline.

use std::collections::HashMap;

use concord::discord::AppEvent;
use concord::discord::ids::{Id, marker::ChannelMarker};
use concord::discord::{
    AttachmentInfo, ChannelInfo, GuildBoostTier, MemberInfo, MessageInfo, StickerFormat,
    StickerInfo,
};

use crate::persist::CACHED_MEMBERS_PER_GUILD;
use crate::store::Store;

/// Every cached guild, as the `GuildCreate` events that would have built it.
pub async fn guild_events(store: &Store) -> Vec<AppEvent> {
    let Ok(guilds) = store.guilds().await else {
        return Vec::new();
    };

    let mut events = Vec::new();
    for guild in guilds {
        let Ok(guild_id) = guild.id.parse::<u64>() else {
            continue;
        };
        let guild_id = Id::new(guild_id);

        let channels = store
            .channels(&guild.id)
            .await
            .unwrap_or_default()
            .into_iter()
            .filter_map(|channel| {
                Some(ChannelInfo {
                    guild_id: Some(guild_id),
                    channel_id: Id::new(channel.id.parse().ok()?),
                    parent_id: channel
                        .parent_id
                        .and_then(|id| id.parse().ok())
                        .map(Id::new),
                    name: channel.name.unwrap_or_default(),
                    kind: channel.kind.unwrap_or_default(),
                    position: channel.position.and_then(|at| i32::try_from(at).ok()),
                    topic: channel.topic,
                    ..Default::default()
                })
            })
            .collect();

        // The `_present` flags say what the cache actually holds, so a later
        // partial update from the gateway patches this rather than being
        // treated as a field that was cleared.
        let members = store
            .members(&guild.id, CACHED_MEMBERS_PER_GUILD as u32)
            .await
            .unwrap_or_default()
            .into_iter()
            .filter_map(|member| {
                Some(MemberInfo {
                    user_id: Id::new(member.user_id.parse().ok()?),
                    display_name: member.display_name.unwrap_or_default(),
                    nickname: member.nickname,
                    nickname_present: true,
                    is_bot: member.is_bot,
                    is_bot_present: true,
                    avatar_url: member.avatar_url,
                    avatar_url_present: true,
                    role_ids: member
                        .role_ids
                        .iter()
                        .filter_map(|role| role.parse().ok())
                        .map(Id::new)
                        .collect(),
                    role_ids_present: true,
                    joined_at: member
                        .joined_at
                        .as_deref()
                        .and_then(|joined| chrono::DateTime::parse_from_rfc3339(joined).ok())
                        .map(|joined| joined.with_timezone(&chrono::Utc)),
                    ..Default::default()
                })
            })
            .collect();

        events.push(AppEvent::GuildCreate {
            guild_id,
            name: guild.name.unwrap_or_default(),
            member_count: None,
            owner_id: guild.owner_id.and_then(|id| id.parse().ok()).map(Id::new),
            boost_tier: GuildBoostTier::default(),
            boost_count: 0,
            verification_level: None,
            mfa_level: None,
            features: None,
            onboarding: None,
            channels,
            members,
            presences: Vec::new(),
            roles: None,
            emojis: Vec::new(),
            stickers: Vec::new(),
        });
    }
    events
}

/// A channel's cached messages, as the history event the fetch would produce.
///
/// `None` when there is nothing worth drawing, so the caller publishes nothing
/// rather than an empty page that would read as "this channel is empty".
pub async fn channel_history(
    store: &Store,
    channel_id: Id<ChannelMarker>,
    limit: u32,
) -> Option<AppEvent> {
    let cached = store
        .recent_messages(&channel_id.get().to_string(), limit)
        .await
        .ok()?;

    // Fetched for the whole page in one query: one per message would be fifty
    // round trips before anything is drawn, which costs more than the fetch
    // this is meant to pre-empt.
    let replayable: Vec<String> = cached
        .iter()
        .filter(|row| !row.has_extras)
        .map(|row| row.id.clone())
        .collect();
    let mut attachments = store.attachments_for(&replayable).await.unwrap_or_default();
    let mut stickers = store.stickers_for(&replayable).await.unwrap_or_default();

    let mut messages = Vec::new();
    for row in cached {
        // Messages that had an embed or a poll are skipped: neither is cached,
        // and one drawn without it is wrong in the way that looks like a bug,
        // where one that is simply absent looks like loading.
        if row.has_extras {
            continue;
        }
        let (Some(author_id_text), Ok(message_id)) =
            (row.author_id.as_deref(), row.id.parse::<u64>())
        else {
            continue;
        };
        let Ok(author_id) = author_id_text.parse::<u64>() else {
            continue;
        };
        // The author's name is cached separately; without it the message would
        // render against a blank name, which reads as a broken message rather
        // than a pending one.
        let Ok(Some(author)) = store.user(author_id_text).await else {
            continue;
        };

        messages.push(MessageInfo {
            channel_id,
            message_id: Id::new(message_id),
            author_id: Id::new(author_id),
            // The display name if there is one, as everywhere else - a message
            // that suddenly showed the username would look like a different
            // person had written it.
            author: author.display_name.or(author.username).unwrap_or_default(),
            author_avatar_url: author.avatar_url,
            author_is_bot: author.is_bot,
            content: row.content,
            edited_timestamp: row.edited_timestamp,
            attachments: take_attachments(&mut attachments, &row.id),
            stickers: take_stickers(&mut stickers, &row.id),
            ..Default::default()
        });
    }

    (!messages.is_empty()).then_some(AppEvent::MessageHistoryLoaded {
        channel_id,
        before: None,
        messages,
    })
}

fn take_attachments(
    cached: &mut HashMap<String, Vec<crate::store::CachedAttachment>>,
    message_id: &str,
) -> Vec<AttachmentInfo> {
    cached
        .remove(message_id)
        .unwrap_or_default()
        .into_iter()
        .map(|attachment| AttachmentInfo {
            id: Id::new(attachment.id.parse().unwrap_or(1)),
            filename: attachment.filename.unwrap_or_default(),
            url: attachment.url.clone().unwrap_or_default(),
            // Discord's proxy URL is derivable from neither the id nor the CDN
            // URL, so the original stands in. It is the same picture from a
            // different host.
            proxy_url: attachment.url.unwrap_or_default(),
            content_type: attachment.content_type,
            size: attachment.size,
            width: attachment.width.and_then(|at| u64::try_from(at).ok()),
            height: attachment.height.and_then(|at| u64::try_from(at).ok()),
            description: attachment.description,
            flags: 0,
        })
        .collect()
}

fn take_stickers(
    cached: &mut HashMap<String, Vec<crate::store::CachedSticker>>,
    message_id: &str,
) -> Vec<StickerInfo> {
    cached
        .remove(message_id)
        .unwrap_or_default()
        .into_iter()
        .map(|sticker| {
            // Rebuilt through the same constructor the live path uses, so the
            // CDN URL is derived rather than stored and cannot drift from it.
            StickerInfo::new(
                Id::new(sticker.id.parse().unwrap_or(1)),
                sticker.name.unwrap_or_default(),
                StickerFormat::from_wire(sticker.format),
            )
        })
        .collect()
}
