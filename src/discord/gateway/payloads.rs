use super::*;
use crate::discord::fingerprint::{
    CLIENT_BROWSER, CLIENT_BROWSER_VERSION, ClientFingerprint, DISCORD_REFERRER_CURRENT,
    DISCORD_REFERRING_DOMAIN_CURRENT,
};
use crate::discord::state::ClientCacheState;
use serde_json::{Value, json};

pub(super) fn build_identify_payload(
    token: &str,
    fingerprint: &ClientFingerprint,
    presence: Option<&GatewayPresence>,
    client_state: ClientCacheState,
) -> String {
    let mut properties = json!({
        "os": fingerprint.os,
        "browser": CLIENT_BROWSER,
        "device": "",
        "system_locale": fingerprint.system_locale,
        "browser_user_agent": fingerprint.user_agent,
        "browser_version": CLIENT_BROWSER_VERSION,
        "os_version": fingerprint.os_version,
        "referrer": "",
        "referring_domain": "",
        "referrer_current": DISCORD_REFERRER_CURRENT,
        "referring_domain_current": DISCORD_REFERRING_DOMAIN_CURRENT,
        "release_channel": "stable",
        "client_build_number": fingerprint.client_build_number,
        "client_event_source": Value::Null,
    });
    if let Some(installation_id) = fingerprint.installation_id() {
        properties["installation_id"] = Value::String(installation_id);
    }

    let presence = presence
        .map(|presence| gateway_presence_payload(&presence.status, &presence.activities))
        .unwrap_or_else(|| gateway_presence_payload(&PresenceStatus::Unknown, &[]));

    // Only reuse versions Concord actually tracks. The remaining conservative
    // defaults avoid claiming cache state that this process cannot verify.
    json!({
        "op": 2,
        "d": {
            "token": token,
            "capabilities": USER_ACCOUNT_CAPABILITIES,
            "properties": properties,
            "presence": presence,
            // `zlib-stream` is selected in the Gateway URL. The browser keeps
            // this separate Identify compression mode disabled.
            "compress": false,
            "client_state": {
                "guild_versions": {},
                "highest_last_message_id": client_state
                    .highest_guild_message_id
                    .map(Id::get)
                    .unwrap_or_default()
                    .to_string(),
                "read_state_version": client_state.read_state_version.unwrap_or_default(),
                "user_guild_settings_version": client_state
                    .user_guild_settings_version
                    .unwrap_or(-1),
                "user_settings_version": -1,
                "private_channels_version": client_state
                    .highest_private_message_id
                    .map(Id::get)
                    .unwrap_or_default()
                    .to_string(),
                "api_code_version": 0,
            },
        },
    })
    .to_string()
}

pub(super) fn build_resume_payload(token: &str, session_id: &str, sequence: u64) -> String {
    json!({
        "op": 6,
        "d": {
            "token": token,
            "session_id": session_id,
            "seq": sequence,
        },
    })
    .to_string()
}

pub(super) fn search_guild_members_payload(
    guild_id: Id<GuildMarker>,
    query: &str,
    limit: u16,
    presences: bool,
    nonce: &str,
) -> String {
    json!({
        "op": 8,
        "d": {
            "guild_id": [guild_id.to_string()],
            "query": query,
            "limit": limit,
            "presences": presences,
            "nonce": nonce,
        },
    })
    .to_string()
}

pub(super) fn request_guild_members_by_ids_payload(
    guild_id: Id<GuildMarker>,
    user_ids: &[Id<UserMarker>],
    presences: bool,
    nonce: &str,
) -> String {
    let user_ids = user_ids
        .iter()
        .take(100)
        .map(|user_id| user_id.to_string())
        .collect::<Vec<_>>();
    json!({
        "op": 8,
        "d": {
            "guild_id": [guild_id.to_string()],
            "user_ids": user_ids,
            "presences": presences,
            "nonce": nonce,
        },
    })
    .to_string()
}

pub(super) fn direct_message_subscribe_payload(channel_id: Id<ChannelMarker>) -> String {
    json!({
        "op": 13,
        "d": {
            "channel_id": channel_id.to_string(),
        },
    })
    .to_string()
}

pub(super) fn guild_channel_subscribe_payload(
    guild_id: Id<GuildMarker>,
    channel_id: Id<ChannelMarker>,
    ranges: &[(u32, u32)],
    thread_member_lists: Option<&[Id<ChannelMarker>]>,
) -> String {
    let ranges_json: Vec<[u32; 2]> = ranges.iter().map(|(start, end)| [*start, *end]).collect();
    let mut subscription = json!({
        "typing": true,
        "activities": true,
        "threads": true,
        "member_updates": true,
        "members": [],
        "channels": {
            channel_id.to_string(): ranges_json,
        },
    });
    if let Some(thread_member_lists) = thread_member_lists {
        subscription["thread_member_lists"] = Value::from(
            thread_member_lists
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
        );
    }
    json!({
        "op": 37,
        "d": {
            "subscriptions": {
                guild_id.to_string(): subscription,
            },
        },
    })
    .to_string()
}

pub(super) fn voice_state_update_payload(
    guild_id: Option<Id<GuildMarker>>,
    channel_id: Option<Id<ChannelMarker>>,
    self_mute: bool,
    self_deaf: bool,
) -> String {
    // A null `guild_id` tells Discord this is a DM or group-DM call. The
    // `channel_id` then points at the DM channel rather than a guild voice channel.
    json!({
        "op": 4,
        "d": {
            "guild_id": guild_id.map(|guild_id| guild_id.to_string()),
            "channel_id": channel_id.map(|channel_id| channel_id.to_string()),
            "self_mute": self_mute,
            "self_deaf": self_deaf,
        },
    })
    .to_string()
}

pub(super) fn watch_stream_payload(stream_key: &str) -> String {
    json!({
        "op": 20,
        "d": {
            "stream_key": stream_key,
        },
    })
    .to_string()
}

pub(super) fn create_stream_payload(scope: VoiceScope, channel_id: Id<ChannelMarker>) -> String {
    let (stream_type, guild_id) = match scope {
        VoiceScope::Guild(guild_id) => ("guild", Some(guild_id.to_string())),
        VoiceScope::Private(_) => ("call", None),
    };
    json!({
        "op": 18,
        "d": {
            "type": stream_type,
            "guild_id": guild_id,
            "channel_id": channel_id.to_string(),
            "preferred_region": Value::Null,
        },
    })
    .to_string()
}

pub(super) fn delete_stream_payload(stream_key: &str) -> String {
    json!({
        "op": 19,
        "d": {
            "stream_key": stream_key,
        },
    })
    .to_string()
}

pub(super) fn presence_update_payload(
    status: PresenceStatus,
    activities: &[ActivityInfo],
) -> String {
    json!({
        "op": 3,
        "d": gateway_presence_payload(&status, activities),
    })
    .to_string()
}

pub(super) fn gateway_presence_payload(
    status: &PresenceStatus,
    activities: &[ActivityInfo],
) -> Value {
    json!({
        "since": 0,
        "activities": activities.iter().map(activity_gateway_payload).collect::<Vec<_>>(),
        "status": status.gateway_status(),
        "afk": false,
    })
}

pub(super) fn current_gateway_presence(state: &DiscordState) -> Option<GatewayPresence> {
    let user_id = state.current_user_id()?;
    Some(GatewayPresence {
        status: state.user_presence(user_id)?,
        activities: state.user_activities(user_id).to_vec(),
    })
}

pub(super) fn activity_gateway_payload(activity: &ActivityInfo) -> Value {
    let mut fields: serde_json::Map<String, Value> =
        activity.extra_fields.clone().into_iter().collect();
    fields.insert("name".to_owned(), json!(activity.name));
    fields.insert("type".to_owned(), json!(activity.kind.gateway_code()));
    if let Some(details) = activity.details.as_deref() {
        fields.insert("details".to_owned(), json!(details));
    }
    if let Some(details_url) = activity.details_url.as_deref() {
        fields.insert("details_url".to_owned(), json!(details_url));
    }
    if let Some(state) = activity.state.as_deref() {
        fields.insert("state".to_owned(), json!(state));
    }
    if let Some(state_url) = activity.state_url.as_deref() {
        fields.insert("state_url".to_owned(), json!(state_url));
    }
    if let Some(url) = activity.url.as_deref() {
        fields.insert("url".to_owned(), json!(url));
    }
    if let Some(platform) = activity.platform.as_deref() {
        fields.insert("platform".to_owned(), json!(platform));
    }
    if !activity.supported_platforms.is_empty() {
        fields.insert(
            "supported_platforms".to_owned(),
            json!(activity.supported_platforms),
        );
    }
    // A Custom status carries its emoji here. Without it a status change would
    // re-broadcast the activity and drop the emoji.
    if let Some(emoji) = activity.emoji.as_ref() {
        let mut node = json!({ "name": emoji.name.as_str() });
        if let Some(id) = emoji.id {
            node["id"] = json!(id.get().to_string());
        }
        if emoji.animated {
            node["animated"] = json!(true);
        }
        fields.insert("emoji".to_owned(), node);
    }
    if let Some(application_id) = activity.application_id.as_deref() {
        fields.insert("application_id".to_owned(), json!(application_id));
    }
    if let Some(parent_application_id) = activity.parent_application_id.as_deref() {
        fields.insert(
            "parent_application_id".to_owned(),
            json!(parent_application_id),
        );
    }
    if let Some(status_display_type) = activity.status_display_type {
        fields.insert("status_display_type".to_owned(), json!(status_display_type));
    }
    if let Some(sync_id) = activity.sync_id.as_deref() {
        fields.insert("sync_id".to_owned(), json!(sync_id));
    }
    if let Some(timestamps) = activity.timestamps.as_ref() {
        let mut node = json!({});
        if let Some(start) = timestamps.start {
            node["start"] = json!(start);
        }
        if let Some(end) = timestamps.end {
            node["end"] = json!(end);
        }
        fields.insert("timestamps".to_owned(), node);
    }
    if let Some(assets) = activity.assets.as_ref() {
        let mut node: serde_json::Map<String, Value> =
            assets.extra_fields.clone().into_iter().collect();
        if let Some(large_image) = assets.large_image.as_deref() {
            node.insert("large_image".to_owned(), json!(large_image));
        }
        if let Some(large_text) = assets.large_text.as_deref() {
            node.insert("large_text".to_owned(), json!(large_text));
        }
        if let Some(large_url) = assets.large_url.as_deref() {
            node.insert("large_url".to_owned(), json!(large_url));
        }
        if let Some(small_image) = assets.small_image.as_deref() {
            node.insert("small_image".to_owned(), json!(small_image));
        }
        if let Some(small_text) = assets.small_text.as_deref() {
            node.insert("small_text".to_owned(), json!(small_text));
        }
        if let Some(small_url) = assets.small_url.as_deref() {
            node.insert("small_url".to_owned(), json!(small_url));
        }
        if let Some(invite_cover_image) = assets.invite_cover_image.as_deref() {
            node.insert("invite_cover_image".to_owned(), json!(invite_cover_image));
        }
        fields.insert("assets".to_owned(), Value::Object(node));
    }
    if let Some(party) = activity.party.as_ref() {
        let mut node: serde_json::Map<String, Value> =
            party.extra_fields.clone().into_iter().collect();
        if let Some(id) = party.id.as_deref() {
            node.insert("id".to_owned(), json!(id));
        }
        if let Some((current, max)) = party.size {
            node.insert("size".to_owned(), json!([current, max]));
        }
        if let Some(privacy) = party.privacy {
            node.insert("privacy".to_owned(), json!(privacy));
        }
        fields.insert("party".to_owned(), Value::Object(node));
    }
    if let Some(secrets) = activity.secrets.as_ref() {
        let mut node: serde_json::Map<String, Value> =
            secrets.extra_fields.clone().into_iter().collect();
        if let Some(join) = secrets.join.as_deref() {
            node.insert("join".to_owned(), json!(join));
        }
        if let Some(spectate) = secrets.spectate.as_deref() {
            node.insert("spectate".to_owned(), json!(spectate));
        }
        fields.insert("secrets".to_owned(), Value::Object(node));
    }

    let mut flags = activity.flags;
    if let Some(instance) = activity.instance {
        let updated = if instance {
            flags.unwrap_or_default() | 1
        } else {
            flags.unwrap_or_default() & !1
        };
        flags = Some(updated);
    }
    if let Some(flags) = flags {
        fields.insert("flags".to_owned(), json!(flags));
    }

    let mut metadata: serde_json::Map<String, Value> =
        activity.metadata.clone().into_iter().collect();
    // User-account presence encodes buttons as a parallel pair: an array of
    // labels under `buttons` and their URLs under `metadata.button_urls`. This
    // differs from the bot `[{label, url}]` shape.
    if !activity.buttons.is_empty() {
        let labels: Vec<&str> = activity
            .buttons
            .iter()
            .map(|button| button.label.as_str())
            .collect();
        let urls: Vec<&str> = activity
            .buttons
            .iter()
            .map(|button| button.url.as_str())
            .collect();
        fields.insert("buttons".to_owned(), json!(labels));
        metadata.insert("button_urls".to_owned(), json!(urls));
    }
    if !metadata.is_empty() {
        fields.insert("metadata".to_owned(), Value::Object(metadata));
    }

    Value::Object(fields)
}

#[cfg(test)]
mod redaction_tests {
    use super::super::frame_handler::redact_token;

    #[test]
    pub(super) fn an_identify_frame_never_carries_its_token_into_the_log() {
        let frame = r#"{"op":2,"d":{"token":"hunter2.abc.def","properties":{}}}"#;
        let traced = redact_token(frame);

        assert!(!traced.contains("hunter2"), "got {traced}");
        assert!(traced.contains("[redacted]"));
        // The rest of the frame survives, which is the point of redacting
        // rather than dropping it.
        assert!(traced.contains(r#""op":2"#));
        assert!(traced.contains("properties"));
    }

    #[test]
    pub(super) fn a_frame_with_no_token_is_untouched() {
        let frame = r#"{"op":1,"d":42}"#;
        assert_eq!(redact_token(frame), frame);
    }

    #[test]
    pub(super) fn a_truncated_frame_is_withheld_rather_than_guessed_at() {
        // The failure that matters: an unterminated value could otherwise be
        // printed whole, token and all.
        let traced = redact_token(r#"{"op":2,"d":{"token":"hunter2"#);
        assert!(!traced.contains("hunter2"), "got {traced}");
    }
}
