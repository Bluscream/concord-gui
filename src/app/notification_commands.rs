use std::collections::BTreeMap;

use chrono::{Duration as ChronoDuration, SecondsFormat, Utc};

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker},
};
use crate::{
    DiscordClient,
    discord::{
        AppEvent, ChannelNotificationOverrideInfo, GuildNotificationSettingsInfo, MuteDuration,
        UserGuildSettingsInfo,
    },
};

use super::command_loop::publish_app_error;

pub(super) async fn set_guild_muted(
    client: DiscordClient,
    guild_id: Id<GuildMarker>,
    muted: bool,
    duration: Option<MuteDuration>,
) {
    let mute_end_time = mute_end_time_from_duration(duration, muted);
    let selected_time_window = selected_time_window_from_duration(duration, muted);
    match client
        .set_guild_muted(guild_id, muted, mute_end_time, selected_time_window)
        .await
    {
        Ok(()) => {
            publish_settings_update(&client, Some(guild_id), Some((muted, mute_end_time)), None)
                .await;
        }
        Err(error) => publish_app_error(&client, "set guild mute failed", &error).await,
    }
}

pub(super) async fn set_channel_muted(
    client: DiscordClient,
    guild_id: Option<Id<GuildMarker>>,
    channel_id: Id<ChannelMarker>,
    muted: bool,
    duration: Option<MuteDuration>,
) {
    let mute_end_time = mute_end_time_from_duration(duration, muted);
    let selected_time_window = selected_time_window_from_duration(duration, muted);
    match client
        .set_channel_muted(
            guild_id,
            channel_id,
            muted,
            mute_end_time,
            selected_time_window,
        )
        .await
    {
        Ok(()) => {
            publish_settings_update(
                &client,
                guild_id,
                None,
                Some((channel_id, muted, mute_end_time)),
            )
            .await;
        }
        Err(error) => publish_app_error(&client, "set channel mute failed", &error).await,
    }
}

pub(super) async fn set_thread_muted(
    client: DiscordClient,
    channel_id: Id<ChannelMarker>,
    muted: bool,
    duration: Option<MuteDuration>,
) {
    let mute_end_time = mute_end_time_from_duration(duration, muted);
    let selected_time_window = selected_time_window_from_duration(duration, muted);
    match client
        .set_thread_muted(channel_id, muted, mute_end_time, selected_time_window)
        .await
    {
        Ok(()) => {
            client
                .publish_event(AppEvent::ThreadMuteUpdate {
                    channel_id,
                    muted,
                    mute_end_time: mute_end_time
                        .map(|end_time| end_time.to_rfc3339_opts(SecondsFormat::Millis, true)),
                })
                .await;
        }
        Err(error) => publish_app_error(&client, "set post mute failed", &error).await,
    }
}

pub(super) async fn set_thread_notification_level(
    client: DiscordClient,
    channel_id: Id<ChannelMarker>,
    flags: u64,
) {
    match client
        .set_thread_notification_level(channel_id, flags)
        .await
    {
        Ok(()) => {
            client
                .publish_event(AppEvent::ThreadNotificationLevelUpdate { channel_id, flags })
                .await;
        }
        Err(error) => {
            publish_app_error(&client, "set post notifications failed", &error).await;
        }
    }
}

pub(super) async fn set_thread_followed(
    client: DiscordClient,
    channel_id: Id<ChannelMarker>,
    followed: bool,
) {
    let result = if followed {
        client.follow_thread(channel_id).await
    } else {
        client.unfollow_thread(channel_id).await
    };
    // Discord echoes a THREAD_MEMBERS_UPDATE for the join/leave, which
    // updates `current_user_joined_thread`, so no optimistic event here.
    if let Err(error) = result {
        let context = if followed {
            "follow post failed"
        } else {
            "unfollow post failed"
        };
        publish_app_error(&client, context, &error).await;
    }
}

async fn publish_settings_update(
    client: &DiscordClient,
    guild_id: Option<Id<GuildMarker>>,
    guild_update: Option<(bool, Option<chrono::DateTime<Utc>>)>,
    channel_override: Option<(Id<ChannelMarker>, bool, Option<chrono::DateTime<Utc>>)>,
) {
    client
        .publish_event(AppEvent::UserGuildSettingsUpdate {
            settings: UserGuildSettingsInfo {
                notification_settings: guild_notification_settings_update(
                    client,
                    guild_id,
                    guild_update,
                    channel_override,
                ),
                extra_fields: BTreeMap::new(),
            },
        })
        .await;
}

fn mute_end_time_from_duration(
    duration: Option<MuteDuration>,
    muted: bool,
) -> Option<chrono::DateTime<Utc>> {
    if !muted {
        return None;
    }
    duration
        .and_then(MuteDuration::minutes)
        .filter(|minutes| *minutes > 0)
        .and_then(|minutes| i64::try_from(minutes).ok())
        .map(|minutes| Utc::now() + ChronoDuration::minutes(minutes))
}

fn selected_time_window_from_duration(duration: Option<MuteDuration>, muted: bool) -> Option<i64> {
    muted.then(|| {
        duration
            .unwrap_or(MuteDuration::Permanent)
            .selected_time_window_seconds()
    })
}

fn guild_notification_settings_update(
    client: &DiscordClient,
    guild_id: Option<Id<GuildMarker>>,
    guild_update: Option<(bool, Option<chrono::DateTime<Utc>>)>,
    channel_override: Option<(Id<ChannelMarker>, bool, Option<chrono::DateTime<Utc>>)>,
) -> GuildNotificationSettingsInfo {
    let snapshot = client.current_discord_snapshot();
    let mut settings = snapshot
        .to_state()
        .guild_notification_settings_info(guild_id);
    if let Some((muted, mute_end_time)) = guild_update {
        settings.muted = muted;
        settings.mute_end_time =
            mute_end_time.map(|value| value.to_rfc3339_opts(SecondsFormat::Millis, true));
    }
    if let Some((channel_id, muted, mute_end_time)) = channel_override {
        if let Some(override_info) = settings
            .channel_overrides
            .iter_mut()
            .find(|override_info| override_info.channel_id == channel_id)
        {
            override_info.muted = muted;
            override_info.mute_end_time =
                mute_end_time.map(|value| value.to_rfc3339_opts(SecondsFormat::Millis, true));
        } else {
            settings
                .channel_overrides
                .push(ChannelNotificationOverrideInfo {
                    channel_id,
                    message_notifications: None,
                    muted,
                    mute_end_time: mute_end_time
                        .map(|value| value.to_rfc3339_opts(SecondsFormat::Millis, true)),
                    collapsed: false,
                    flags: 0,
                });
        }
    }
    settings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mute_duration_only_produces_an_end_time_while_muting() {
        // (duration, muted) -> (has end time, selected window)
        let cases = [
            (Some(MuteDuration::Minutes(60)), true, true, Some(3600)),
            (Some(MuteDuration::Permanent), true, false, Some(-1)),
            (None, true, false, Some(-1)),
            (Some(MuteDuration::Minutes(0)), true, false, Some(0)),
            (Some(MuteDuration::Minutes(60)), false, false, None),
            (Some(MuteDuration::Permanent), false, false, None),
        ];

        for (duration, muted, expects_end_time, expected_window) in cases {
            let label = format!("{duration:?} muted={muted}");
            assert_eq!(
                mute_end_time_from_duration(duration, muted).is_some(),
                expects_end_time,
                "{label}"
            );
            assert_eq!(
                selected_time_window_from_duration(duration, muted),
                expected_window,
                "{label}"
            );
        }
    }
}
