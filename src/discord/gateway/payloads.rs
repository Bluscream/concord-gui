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
) -> String {
    let ranges_json: Vec<[u32; 2]> = ranges.iter().map(|(start, end)| [*start, *end]).collect();
    json!({
        "op": 37,
        "d": {
            "subscriptions": {
                guild_id.to_string(): {
                    "typing": true,
                    "activities": true,
                    "threads": true,
                    "channels": {
                        channel_id.to_string(): ranges_json,
                    },
                },
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
    let mut value = json!({
        "name": activity.name.as_str(),
        "type": activity.kind.gateway_code(),
    });
    if let Some(details) = activity.details.as_deref() {
        value["details"] = json!(details);
    }
    if let Some(state) = activity.state.as_deref() {
        value["state"] = json!(state);
    }
    if let Some(url) = activity.url.as_deref() {
        value["url"] = json!(url);
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
        value["emoji"] = node;
    }
    if let Some(application_id) = activity.application_id.as_deref() {
        value["application_id"] = json!(application_id);
    }
    if let Some(timestamps) = activity.timestamps.as_ref() {
        let mut node = json!({});
        if let Some(start) = timestamps.start {
            node["start"] = json!(start);
        }
        if let Some(end) = timestamps.end {
            node["end"] = json!(end);
        }
        value["timestamps"] = node;
    }
    if let Some(assets) = activity.assets.as_ref() {
        let mut node = json!({});
        if let Some(large_image) = assets.large_image.as_deref() {
            node["large_image"] = json!(large_image);
        }
        if let Some(large_text) = assets.large_text.as_deref() {
            node["large_text"] = json!(large_text);
        }
        if let Some(small_image) = assets.small_image.as_deref() {
            node["small_image"] = json!(small_image);
        }
        if let Some(small_text) = assets.small_text.as_deref() {
            node["small_text"] = json!(small_text);
        }
        value["assets"] = node;
    }
    if let Some(party) = activity.party.as_ref() {
        let mut node = json!({});
        if let Some(id) = party.id.as_deref() {
            node["id"] = json!(id);
        }
        if let Some((current, max)) = party.size {
            node["size"] = json!([current, max]);
        }
        value["party"] = node;
    }
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
        value["buttons"] = json!(labels);
        value["metadata"] = json!({ "button_urls": urls });
    }
    value
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
