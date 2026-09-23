use std::collections::BTreeMap;

use serde_json::Value;

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, MessageMarker, RoleMarker, UserMarker},
};

use crate::discord::{
    ActivityInfo, AttachmentUpdate, EmbedInfo, GuildNotificationSettingsInfo, MemberInfo,
    MentionInfo, PollInfo, PresenceStatus,
};

#[derive(Clone, Debug, PartialEq)]
pub struct GatewayDispatchInfo {
    pub event_type: String,
    pub payload: Value,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChannelUnreadInfo {
    pub channel_id: Id<ChannelMarker>,
    pub last_message_id: Option<Option<Id<MessageMarker>>>,
    pub last_pin_timestamp: Option<Option<String>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MessageUpdateEventFields {
    pub poll: Option<PollInfo>,
    pub content: Option<String>,
    pub sticker_names: Option<Vec<String>>,
    pub stickers: Option<Vec<crate::discord::StickerInfo>>,
    pub mentions: Option<Vec<MentionInfo>>,
    pub mention_everyone: Option<bool>,
    pub mention_roles: Option<Vec<Id<RoleMarker>>>,
    pub flags: Option<u64>,
    pub pinned: Option<bool>,
    pub attachments: AttachmentUpdate,
    pub embeds: Option<Vec<EmbedInfo>>,
    pub edited_timestamp: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MessageUpdateDispatchInfo {
    pub guild_id: Option<Id<GuildMarker>>,
    pub channel_id: Id<ChannelMarker>,
    pub message_id: Id<MessageMarker>,
    pub fields: MessageUpdateEventFields,
    pub extra_fields: BTreeMap<String, Value>,
}

impl Default for MessageUpdateEventFields {
    fn default() -> Self {
        Self {
            poll: None,
            content: None,
            pinned: None,
            sticker_names: None,
            stickers: None,
            mentions: None,
            mention_everyone: None,
            mention_roles: None,
            flags: None,
            attachments: AttachmentUpdate::Unchanged,
            embeds: None,
            edited_timestamp: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PresenceEventFields {
    pub user_id: Id<UserMarker>,
    pub status: PresenceStatus,
    pub activities: Vec<ActivityInfo>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct UserGuildSettingsInfo {
    pub notification_settings: GuildNotificationSettingsInfo,
    pub extra_fields: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum GuildMemberListItem {
    Member {
        member: MemberInfo,
        presence: Option<PresenceEventFields>,
    },
    Group {
        id: String,
        count: u64,
    },
    Unknown {
        raw: Value,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum GuildMemberListOperation {
    Sync {
        range: (u32, u32),
        items: Vec<GuildMemberListItem>,
    },
    Insert {
        index: u32,
        item: GuildMemberListItem,
    },
    Update {
        index: u32,
        item: GuildMemberListItem,
    },
    Delete {
        index: u32,
    },
    Invalidate {
        range: (u32, u32),
    },
    /// An operation Concord does not understand cannot be treated as a no-op.
    /// Keeping the raw value lets state invalidate the list conservatively and
    /// preserves enough data to add support once Discord introduces it.
    Unknown {
        name: Option<String>,
        raw: Value,
    },
}

impl GuildMemberListOperation {
    pub fn items(&self) -> &[GuildMemberListItem] {
        match self {
            Self::Sync { items, .. } => items,
            Self::Insert { item, .. } | Self::Update { item, .. } => std::slice::from_ref(item),
            Self::Delete { .. } | Self::Invalidate { .. } | Self::Unknown { .. } => &[],
        }
    }
}

impl GuildMemberListItem {
    pub fn member(&self) -> Option<(&MemberInfo, Option<&PresenceEventFields>)> {
        match self {
            Self::Member { member, presence } => Some((member, presence.as_ref())),
            Self::Group { .. } | Self::Unknown { .. } => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct GuildMemberListUpdateInfo {
    pub guild_id: Id<GuildMarker>,
    pub list_id: Option<String>,
    pub member_count: Option<u64>,
    pub online_count: Option<u32>,
    pub groups: Vec<Value>,
    pub ops: Vec<GuildMemberListOperation>,
    pub extra_fields: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReadySnapshotInfo {
    /// `None` means the source payload omitted the field, so existing state
    /// must not be reconciled from an incomplete test or future payload.
    pub guild_ids: Option<Vec<Id<GuildMarker>>>,
    /// Guild channel collections are authoritative when present in READY.
    pub guild_channel_ids: BTreeMap<Id<GuildMarker>, Vec<Id<ChannelMarker>>>,
    /// READY and READY_SUPPLEMENTAL together form the private-channel snapshot.
    pub private_channel_ids: Option<Vec<Id<ChannelMarker>>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GuildMembersChunkInfo {
    pub guild_id: Id<GuildMarker>,
    pub members: Vec<MemberInfo>,
    pub presences: Vec<PresenceEventFields>,
    pub chunk_index: Option<u64>,
    pub chunk_count: Option<u64>,
    pub nonce: Option<String>,
    pub not_found: Vec<Id<UserMarker>>,
    pub extra_fields: BTreeMap<String, Value>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MessageHistoryLoadTarget {
    Latest,
    Older { before: Id<MessageMarker> },
    Newer { after: Id<MessageMarker> },
    Around { message_id: Id<MessageMarker> },
}
