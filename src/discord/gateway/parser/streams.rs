use serde_json::Value;

use crate::discord::{
    StreamCreateInfo, StreamDeleteInfo, StreamServerInfo, events::AppEvent,
    ids::marker::ChannelMarker,
};

use super::shared::parse_id;

pub(super) fn parse_stream_create(data: &Value) -> Option<AppEvent> {
    let stream_key = required_string(data, "stream_key")?;
    let rtc_server_id = required_string(data, "rtc_server_id")?;
    let rtc_channel_id = data
        .get("rtc_channel_id")
        .and_then(parse_id::<ChannelMarker>)?;

    Some(AppEvent::StreamCreate {
        stream: StreamCreateInfo {
            stream_key,
            rtc_server_id,
            rtc_channel_id,
        },
    })
}

pub(super) fn parse_stream_server_update(data: &Value) -> Option<AppEvent> {
    let stream_key = required_string(data, "stream_key")?;
    let token = required_string(data, "token")?;
    let endpoint = data
        .get("endpoint")
        .filter(|endpoint| !endpoint.is_null())
        .and_then(Value::as_str)
        .filter(|endpoint| !endpoint.is_empty())
        .map(str::to_owned);

    Some(AppEvent::StreamServerUpdate {
        server: StreamServerInfo {
            stream_key,
            endpoint,
            token,
        },
    })
}

pub(super) fn parse_stream_delete(data: &Value) -> Option<AppEvent> {
    Some(AppEvent::StreamDelete {
        stream: StreamDeleteInfo {
            stream_key: required_string(data, "stream_key")?,
            reason: data
                .get("reason")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            unavailable: data
                .get("unavailable")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        },
    })
}

fn required_string(data: &Value, field: &str) -> Option<String> {
    data.get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}
