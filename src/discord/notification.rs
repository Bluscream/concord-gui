mod state;

pub use state::ChannelUnreadState;
pub(in crate::discord) use state::{
    GuildNotificationSettingsState, MessageNotificationInput, MessageNotificationKind,
    READ_STATE_MENTION_LOW_IMPORTANCE,
};

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NotificationLevel {
    AllMessages,
    OnlyMentions,
    NoMessages,
    ParentDefault,
}

impl NotificationLevel {
    pub const fn from_code(code: u64) -> Option<Self> {
        match code {
            0 => Some(Self::AllMessages),
            1 => Some(Self::OnlyMentions),
            2 => Some(Self::NoMessages),
            3 => Some(Self::ParentDefault),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChannelNotificationOverrideInfo {
    pub channel_id: Id<ChannelMarker>,
    pub message_notifications: Option<NotificationLevel>,
    pub muted: bool,
    pub mute_end_time: Option<String>,
    pub collapsed: bool,
    pub flags: u64,
}

#[cfg(any(test, feature = "fixtures"))]
#[allow(dead_code)]
impl ChannelNotificationOverrideInfo {
    pub fn test(channel_id: Id<ChannelMarker>) -> Self {
        Self {
            channel_id,
            message_notifications: None,
            muted: false,
            mute_end_time: None,
            collapsed: false,
            flags: 0,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GuildNotificationSettingsInfo {
    pub guild_id: Option<Id<GuildMarker>>,
    pub message_notifications: Option<NotificationLevel>,
    pub muted: bool,
    pub mute_end_time: Option<String>,
    pub suppress_everyone: bool,
    pub suppress_roles: bool,
    pub flags: u64,
    pub hide_muted_channels: bool,
    pub mobile_push: bool,
    pub mute_scheduled_events: bool,
    pub notify_highlights: u64,
    pub version: u64,
    pub channel_overrides: Vec<ChannelNotificationOverrideInfo>,
}

#[cfg(any(test, feature = "fixtures"))]
#[allow(dead_code)]
impl GuildNotificationSettingsInfo {
    pub fn test(guild_id: Option<Id<GuildMarker>>) -> Self {
        Self {
            guild_id,
            message_notifications: None,
            muted: false,
            mute_end_time: None,
            suppress_everyone: false,
            suppress_roles: false,
            flags: 0,
            hide_muted_channels: false,
            mobile_push: true,
            mute_scheduled_events: false,
            notify_highlights: 0,
            version: 0,
            channel_overrides: Vec::new(),
        }
    }
}
