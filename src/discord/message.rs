mod state;

pub(in crate::discord) use state::{MessageAuthorRoleIds, MessageUpdateFields};
pub use state::{MessageCapabilities, MessageState};

use crate::discord::commands::ReactionEmoji;
use crate::discord::ids::{
    Id,
    marker::{
        AttachmentMarker, ChannelMarker, GuildMarker, MessageMarker, RoleMarker, UserMarker,
        WebhookMarker,
    },
};

pub const MESSAGE_FLAG_SUPPRESS_EMBEDS: u64 = 1 << 2;
pub const MESSAGE_FLAG_IS_COMPONENTS_V2: u64 = 1 << 15;
const MEDIA_FLAG_IS_ANIMATED: u64 = 1 << 5;
const UNFURLED_MEDIA_FLAG_IS_ANIMATED: u64 = 1 << 0;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MentionInfo {
    pub user_id: Id<UserMarker>,
    /// Per-server nickname carried by this message's mention payload. Kept
    /// separate from `display_name` so rendering can prefer a proven guild
    /// alias while still using cached member names when the payload only has a
    /// global display name or username.
    pub guild_nick: Option<String>,
    pub display_name: String,
}

#[cfg(any(test, feature = "fixtures"))]
#[allow(dead_code)]
impl MentionInfo {
    pub fn test(user_id: Id<UserMarker>, display_name: impl Into<String>) -> Self {
        Self {
            user_id,
            guild_nick: None,
            display_name: display_name.into(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AttachmentInfo {
    pub id: Id<AttachmentMarker>,
    pub filename: String,
    pub url: String,
    pub proxy_url: String,
    pub content_type: Option<String>,
    pub size: u64,
    pub width: Option<u64>,
    pub height: Option<u64>,
    pub description: Option<String>,
    pub flags: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AttachmentMediaType {
    Image,
    Video,
    Audio,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ComponentMediaInfo {
    pub url: String,
    pub proxy_url: Option<String>,
    pub content_type: Option<String>,
    pub width: Option<u64>,
    pub height: Option<u64>,
    pub flags: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComponentMediaItemInfo {
    pub media: ComponentMediaInfo,
    pub description: Option<String>,
    pub spoiler: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ComponentSelectKind {
    String,
    User,
    Role,
    Mentionable,
    Channel,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComponentSelectOptionInfo {
    pub label: String,
    pub description: Option<String>,
    pub emoji: Option<String>,
    pub selected: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MessageComponentInfo {
    ActionRow {
        components: Vec<Self>,
    },
    Button {
        label: Option<String>,
        emoji: Option<String>,
        url: Option<String>,
        disabled: bool,
    },
    Select {
        kind: ComponentSelectKind,
        placeholder: Option<String>,
        options: Vec<ComponentSelectOptionInfo>,
        disabled: bool,
    },
    Section {
        components: Vec<Self>,
        accessory: Option<Box<Self>>,
    },
    TextDisplay {
        content: String,
    },
    Thumbnail {
        media: ComponentMediaInfo,
        description: Option<String>,
        spoiler: bool,
    },
    MediaGallery {
        items: Vec<ComponentMediaItemInfo>,
    },
    File {
        file: ComponentMediaInfo,
        name: Option<String>,
        size: Option<u64>,
        spoiler: bool,
    },
    Separator {
        divider: bool,
        spacing: u8,
    },
    Container {
        components: Vec<Self>,
        accent_color: Option<u32>,
        spoiler: bool,
    },
    Unknown {
        kind: u64,
    },
}

#[cfg(any(test, feature = "fixtures"))]
#[allow(dead_code)]
impl AttachmentInfo {
    pub fn test(id: Id<AttachmentMarker>, filename: impl Into<String>) -> Self {
        Self {
            id,
            filename: filename.into(),
            url: String::new(),
            proxy_url: String::new(),
            content_type: None,
            size: 0,
            width: None,
            height: None,
            description: None,
            flags: 0,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmbedFieldInfo {
    pub name: String,
    pub value: String,
    pub inline: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EmbedInfo {
    pub kind: Option<String>,
    pub flags: u64,
    pub color: Option<u32>,
    pub provider_name: Option<String>,
    pub provider_url: Option<String>,
    pub author_name: Option<String>,
    pub author_url: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub timestamp: Option<String>,
    pub fields: Vec<EmbedFieldInfo>,
    pub footer_text: Option<String>,
    pub url: Option<String>,
    pub thumbnail_url: Option<String>,
    pub thumbnail_proxy_url: Option<String>,
    pub thumbnail_width: Option<u64>,
    pub thumbnail_height: Option<u64>,
    pub thumbnail_content_type: Option<String>,
    pub thumbnail_description: Option<String>,
    pub thumbnail_flags: u64,
    pub image_url: Option<String>,
    pub image_proxy_url: Option<String>,
    pub image_width: Option<u64>,
    pub image_height: Option<u64>,
    pub image_content_type: Option<String>,
    pub image_description: Option<String>,
    pub image_flags: u64,
    /// Animated image rendition selected for a `gifv` embed. Some providers
    /// require deriving it from the video URL, while others supply it as the
    /// thumbnail.
    pub gifv_image_url: Option<String>,
    pub gifv_image_proxy_url: Option<String>,
    pub video_url: Option<String>,
    pub video_proxy_url: Option<String>,
    pub video_width: Option<u64>,
    pub video_height: Option<u64>,
    pub video_content_type: Option<String>,
    pub video_description: Option<String>,
    pub video_flags: u64,
}

#[cfg(any(test, feature = "fixtures"))]
#[allow(dead_code)]
impl EmbedInfo {
    pub fn test() -> Self {
        Self {
            kind: None,
            flags: 0,
            color: None,
            provider_name: None,
            provider_url: None,
            author_name: None,
            author_url: None,
            title: None,
            description: None,
            timestamp: None,
            fields: Vec::new(),
            footer_text: None,
            url: None,
            thumbnail_url: None,
            thumbnail_proxy_url: None,
            thumbnail_width: None,
            thumbnail_height: None,
            thumbnail_content_type: None,
            thumbnail_description: None,
            thumbnail_flags: 0,
            image_url: None,
            image_proxy_url: None,
            image_width: None,
            image_height: None,
            image_content_type: None,
            image_description: None,
            image_flags: 0,
            gifv_image_url: None,
            gifv_image_proxy_url: None,
            video_url: None,
            video_proxy_url: None,
            video_width: None,
            video_height: None,
            video_content_type: None,
            video_description: None,
            video_flags: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InlinePreviewInfo<'a> {
    pub url: &'a str,
    pub proxy_url: Option<&'a str>,
    pub filename: &'a str,
    pub width: Option<u64>,
    pub height: Option<u64>,
    pub accent_color: Option<u32>,
    pub animated: bool,
    pub proxy_preview_only: bool,
    pub show_play_marker: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct MessageKind {
    code: u8,
}

impl MessageKind {
    pub const fn new(code: u8) -> Self {
        Self { code }
    }

    pub const fn regular() -> Self {
        Self::new(0)
    }

    pub const fn code(self) -> u8 {
        self.code
    }

    pub const fn is_regular(self) -> bool {
        self.code == 0
    }

    pub const fn is_regular_or_reply(self) -> bool {
        self.code == 0 || self.code == 19
    }

    pub const fn is_poll_result(self) -> bool {
        self.code == 46
    }

    pub const fn is_recipient_remove(self) -> bool {
        self.code == 2
    }

    pub const fn known_label(self) -> Option<&'static str> {
        match self.code {
            0 => Some("Default"),
            1 => Some("Recipient add"),
            2 => Some("Recipient remove"),
            3 => Some("Call"),
            4 => Some("Channel name change"),
            5 => Some("Channel icon change"),
            6 => Some("Pinned message"),
            7 => Some("User join"),
            8 => Some("Guild boost"),
            9 => Some("Guild boost tier 1"),
            10 => Some("Guild boost tier 2"),
            11 => Some("Guild boost tier 3"),
            12 => Some("Channel follow add"),
            13 => Some("Guild stream"),
            14 => Some("Guild discovery disqualified"),
            15 => Some("Guild discovery requalified"),
            16 => Some("Guild discovery initial warning"),
            17 => Some("Guild discovery final warning"),
            18 => Some("Thread created"),
            19 => Some("Reply"),
            20 => Some("Chat input command"),
            21 => Some("Thread starter message"),
            22 => Some("Guild invite reminder"),
            23 => Some("Context menu command"),
            24 => Some("Auto moderation action"),
            25 => Some("Role subscription purchase"),
            26 => Some("Premium upsell"),
            27 => Some("Stage start"),
            28 => Some("Stage end"),
            29 => Some("Stage speaker"),
            30 => Some("Stage raise hand"),
            31 => Some("Stage topic"),
            32 => Some("Application premium subscription"),
            33 => Some("Integration added"),
            34 => Some("Integration removed"),
            35 => Some("Premium referral"),
            36 => Some("Incident alert mode enabled"),
            37 => Some("Incident alert mode disabled"),
            38 => Some("Incident raid report"),
            39 => Some("Incident false alarm report"),
            40 => Some("Revive prompt"),
            41 => Some("Gift"),
            42 => Some("Gaming stats prompt"),
            // 43 is absent from Discord's own table.
            44 => Some("Purchase notification"),
            45 => Some("Voice hangout invite"),
            46 => Some("Poll result"),
            47 => Some("Changelog"),
            48 => Some("Nitro notification"),
            49 => Some("Channel linked to lobby"),
            50 => Some("Gifting prompt"),
            51 => Some("In-game message intro"),
            52 => Some("Join request accepted"),
            53 => Some("Join request rejected"),
            54 => Some("Join request withdrawn"),
            55 => Some("HD streaming upgraded"),
            56 => Some("Chat wallpaper set"),
            57 => Some("Chat wallpaper removed"),
            58 => Some("Moderator deleted a message"),
            59 => Some("Moderator timed a member out"),
            60 => Some("Moderator kicked a member"),
            61 => Some("Moderator banned a member"),
            62 => Some("Moderator closed a report"),
            63 => Some("Emoji added"),
            64 => Some("Premium group invite"),
            65 => Some("Voice session"),
            66 => Some("Guild boost upsell"),
            67 => Some("Friend request accepted"),
            68 => Some("Media mention"),
            _ => None,
        }
    }

    pub const fn label(self) -> &'static str {
        match self.known_label() {
            Some(label) => label,
            None => "Unknown message type",
        }
    }
}

impl Default for MessageKind {
    fn default() -> Self {
        Self::regular()
    }
}

/// A sticker on a message.
///
/// The id is what makes it renderable: a name alone leaves a sticker-only
/// message looking completely empty, which is how it looked before this.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StickerInfo {
    pub id: Id<crate::discord::ids::marker::StickerMarker>,
    pub name: String,
    pub format: StickerFormat,
    /// CDN URL, stored rather than derived so it can be borrowed by an
    /// inline preview. Empty for formats with no image, such as Lottie.
    pub url: String,
}

#[cfg(any(test, feature = "fixtures"))]
impl StickerInfo {
    /// A PNG sticker with the given id and name, for tests and demo data.
    pub fn test(id: u64, name: impl Into<String>) -> Self {
        Self::new(Id::new(id), name, StickerFormat::Png)
    }
}

impl StickerInfo {
    /// Build one, deriving the CDN URL from the id and format.
    pub fn new(
        id: Id<crate::discord::ids::marker::StickerMarker>,
        name: impl Into<String>,
        format: StickerFormat,
    ) -> Self {
        let url = if format.is_image() {
            format!(
                "https://media.discordapp.net/stickers/{}.{}",
                id.get(),
                format.extension()
            )
        } else {
            String::new()
        };
        Self {
            id,
            name: name.into(),
            format,
            url,
        }
    }
}

/// Discord's sticker `format_type`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum StickerFormat {
    #[default]
    Png,
    Apng,
    /// Vector animation. Not renderable as an image, so clients that cannot
    /// play it fall back to the sticker's name rather than a broken picture.
    Lottie,
    Gif,
}

impl StickerFormat {
    pub fn from_wire(value: u64) -> Self {
        match value {
            2 => Self::Apng,
            3 => Self::Lottie,
            4 => Self::Gif,
            // 1 is PNG, and an unknown future value is most safely treated as
            // a still image: a wrong extension fails to load, which is the
            // same outcome as not trying.
            _ => Self::Png,
        }
    }

    /// How the format reads in a management list.
    ///
    /// An unrecognised format reads as PNG, for the reason `from_wire` gives:
    /// it is the safest guess for rendering. That does mean a management row
    /// can name a format the sticker does not have, which is the cost of not
    /// carrying the raw value through.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Png => "PNG",
            Self::Apng => "animated PNG",
            Self::Lottie => "Lottie",
            Self::Gif => "GIF",
        }
    }

    /// Whether this can be shown as an image at all.
    /// The number Discord uses, so a stored format reads back as itself.
    ///
    /// The inverse of `from_wire` for every variant it can produce. An unknown
    /// future value does not survive the round trip - it arrives as `Png` and
    /// leaves as 1 - which is the same fallback `from_wire` already chose.
    pub const fn to_wire(self) -> u64 {
        match self {
            Self::Png => 1,
            Self::Apng => 2,
            Self::Lottie => 3,
            Self::Gif => 4,
        }
    }

    pub const fn is_image(self) -> bool {
        !matches!(self, Self::Lottie)
    }

    pub const fn extension(self) -> &'static str {
        match self {
            Self::Png | Self::Apng => "png",
            Self::Lottie => "json",
            Self::Gif => "gif",
        }
    }
}

impl StickerInfo {
    /// A sticker as an inline preview, so it renders where images do.
    ///
    /// Lottie stickers return `None`: they are vector animations, and no front
    /// end here can play one, so they fall back to the name instead of showing
    /// a broken image.
    pub fn inline_preview_info(&self) -> Option<InlinePreviewInfo<'_>> {
        self.format.is_image().then_some(InlinePreviewInfo {
            url: self.url.as_str(),
            proxy_url: None,
            filename: self.name.as_str(),
            // Discord renders stickers at 160 square. Sending the real size
            // lets a terminal reserve the right number of cells before the
            // image arrives, rather than reflowing when it does.
            width: Some(160),
            height: Some(160),
            accent_color: None,
            animated: matches!(self.format, StickerFormat::Apng | StickerFormat::Gif),
            proxy_preview_only: false,
            show_play_marker: false,
        })
    }

    /// CDN URL for the sticker image, if it has one.
    pub fn image_url(&self) -> Option<String> {
        (!self.url.is_empty()).then(|| self.url.clone())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MessageSnapshotInfo {
    pub content: Option<String>,
    pub sticker_names: Vec<String>,
    /// The same stickers with their ids, so they can be rendered.
    pub stickers: Vec<StickerInfo>,
    pub mentions: Vec<MentionInfo>,
    pub flags: u64,
    pub attachments: Vec<AttachmentInfo>,
    pub embeds: Vec<EmbedInfo>,
    pub components: Vec<MessageComponentInfo>,
    pub source_channel_id: Option<Id<ChannelMarker>>,
    pub timestamp: Option<String>,
}

#[cfg(any(test, feature = "fixtures"))]
#[allow(dead_code)]
impl MessageSnapshotInfo {
    pub fn test() -> Self {
        Self {
            content: None,
            sticker_names: Vec::new(),
            stickers: Vec::new(),
            mentions: Vec::new(),
            flags: 0,
            attachments: Vec::new(),
            embeds: Vec::new(),
            components: Vec::new(),
            source_channel_id: None,
            timestamp: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReplyInfo {
    pub author_id: Option<Id<UserMarker>>,
    pub author: String,
    pub content: Option<String>,
    pub sticker_names: Vec<String>,
    /// The same stickers with their ids, so they can be rendered.
    pub stickers: Vec<StickerInfo>,
    pub mentions: Vec<MentionInfo>,
}

#[cfg(any(test, feature = "fixtures"))]
#[allow(dead_code)]
impl ReplyInfo {
    pub fn test(author: impl Into<String>) -> Self {
        Self {
            author_id: None,
            author: author.into(),
            content: None,
            sticker_names: Vec::new(),
            stickers: Vec::new(),
            mentions: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MessageInteractionInfo {
    pub user_id: Option<Id<UserMarker>>,
    pub user: String,
    pub command_name: Option<String>,
}

#[cfg(any(test, feature = "fixtures"))]
#[allow(dead_code)]
impl MessageInteractionInfo {
    pub fn test(user: impl Into<String>) -> Self {
        Self {
            user_id: None,
            user: user.into(),
            command_name: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MessageReferenceInfo {
    pub guild_id: Option<Id<GuildMarker>>,
    pub channel_id: Option<Id<ChannelMarker>>,
    pub message_id: Option<Id<MessageMarker>>,
}

#[cfg(any(test, feature = "fixtures"))]
#[allow(dead_code)]
impl MessageReferenceInfo {
    pub fn test(message_id: Id<MessageMarker>) -> Self {
        Self {
            guild_id: None,
            channel_id: None,
            message_id: Some(message_id),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PollInfo {
    pub question: String,
    pub answers: Vec<PollAnswerInfo>,
    pub allow_multiselect: bool,
    pub results_finalized: Option<bool>,
    pub total_votes: Option<u64>,
}

#[cfg(any(test, feature = "fixtures"))]
#[allow(dead_code)]
impl PollInfo {
    pub fn test(question: impl Into<String>) -> Self {
        Self {
            question: question.into(),
            answers: Vec::new(),
            allow_multiselect: false,
            results_finalized: None,
            total_votes: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PollAnswerInfo {
    pub answer_id: u8,
    pub text: String,
    pub vote_count: Option<u64>,
    pub me_voted: bool,
}

#[cfg(any(test, feature = "fixtures"))]
#[allow(dead_code)]
impl PollAnswerInfo {
    pub fn test(answer_id: u8, text: impl Into<String>) -> Self {
        Self {
            answer_id,
            text: text.into(),
            vote_count: None,
            me_voted: false,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReactionInfo {
    pub emoji: ReactionEmoji,
    pub count: u64,
    pub me: bool,
}

#[cfg(any(test, feature = "fixtures"))]
#[allow(dead_code)]
impl ReactionInfo {
    pub fn test(emoji: ReactionEmoji) -> Self {
        Self {
            emoji,
            count: 1,
            me: false,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReactionUserInfo {
    pub user_id: Id<UserMarker>,
    pub display_name: String,
}

#[cfg(any(test, feature = "fixtures"))]
#[allow(dead_code)]
impl ReactionUserInfo {
    pub fn test(user_id: Id<UserMarker>, display_name: impl Into<String>) -> Self {
        Self {
            user_id,
            display_name: display_name.into(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MessageInfo {
    pub guild_id: Option<Id<GuildMarker>>,
    pub channel_id: Id<ChannelMarker>,
    pub message_id: Id<MessageMarker>,
    pub nonce: Option<Id<MessageMarker>>,
    pub webhook_id: Option<Id<WebhookMarker>>,
    pub author_id: Id<UserMarker>,
    pub author: String,
    pub author_avatar_url: Option<String>,
    pub author_is_bot: bool,
    pub author_role_ids: Vec<Id<RoleMarker>>,
    pub author_role_ids_present: bool,
    pub message_kind: MessageKind,
    pub interaction: Option<MessageInteractionInfo>,
    pub reference: Option<MessageReferenceInfo>,
    pub reply: Option<ReplyInfo>,
    pub poll: Option<PollInfo>,
    pub pinned: bool,
    pub reactions: Vec<ReactionInfo>,
    pub content: Option<String>,
    pub sticker_names: Vec<String>,
    /// The same stickers with their ids, so they can be rendered.
    pub stickers: Vec<StickerInfo>,
    pub mentions: Vec<MentionInfo>,
    pub mention_everyone: bool,
    pub mention_roles: Vec<Id<RoleMarker>>,
    pub flags: u64,
    pub attachments: Vec<AttachmentInfo>,
    pub embeds: Vec<EmbedInfo>,
    pub components: Vec<MessageComponentInfo>,
    pub forwarded_snapshots: Vec<MessageSnapshotInfo>,
    pub edited_timestamp: Option<String>,
}

impl Default for MessageInfo {
    fn default() -> Self {
        Self {
            guild_id: None,
            channel_id: Id::new(1),
            message_id: Id::new(1),
            nonce: None,
            webhook_id: None,
            author_id: Id::new(1),
            author: String::new(),
            author_avatar_url: None,
            author_is_bot: false,
            author_role_ids: Vec::new(),
            author_role_ids_present: false,
            message_kind: MessageKind::default(),
            interaction: None,
            reference: None,
            reply: None,
            poll: None,
            pinned: false,
            reactions: Vec::new(),
            content: None,
            sticker_names: Vec::new(),
            stickers: Vec::new(),
            mentions: Vec::new(),
            mention_everyone: false,
            mention_roles: Vec::new(),
            flags: 0,
            attachments: Vec::new(),
            embeds: Vec::new(),
            components: Vec::new(),
            forwarded_snapshots: Vec::new(),
            edited_timestamp: None,
        }
    }
}

impl MessageInfo {
    /// The text a compact surface shows for this message: a notification, a
    /// search hit, the channel list's preview line. Components V2 messages
    /// carry their text in the component tree instead of `content`.
    pub fn summary_text(&self) -> Option<&str> {
        let content = self
            .content
            .as_deref()
            .filter(|content| !content.trim().is_empty());
        if self.flags & MESSAGE_FLAG_IS_COMPONENTS_V2 != 0 {
            MessageComponentInfo::first_text(&self.components)
        } else {
            content.or_else(|| MessageComponentInfo::first_text(&self.components))
        }
    }
}

#[cfg(any(test, feature = "fixtures"))]
#[allow(dead_code)]
impl MessageInfo {
    pub fn test(channel_id: Id<ChannelMarker>, message_id: Id<MessageMarker>) -> Self {
        Self {
            channel_id,
            message_id,
            ..Self::default()
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AttachmentUpdate {
    Unchanged,
    Replace(Vec<AttachmentInfo>),
}

impl AttachmentUpdate {
    pub fn replacement(&self) -> Option<&[AttachmentInfo]> {
        match self {
            Self::Unchanged => None,
            Self::Replace(attachments) => Some(attachments),
        }
    }
}

impl AttachmentInfo {
    pub fn preferred_url(&self) -> Option<&str> {
        if self.url.is_empty() {
            (!self.proxy_url.is_empty()).then_some(self.proxy_url.as_str())
        } else {
            Some(self.url.as_str())
        }
    }

    pub fn media_type(&self) -> Option<AttachmentMediaType> {
        if let Some(content_type) = self.content_type.as_deref() {
            if content_type.starts_with("image/") {
                return Some(AttachmentMediaType::Image);
            } else if content_type.starts_with("video/") {
                return Some(AttachmentMediaType::Video);
            } else if content_type.starts_with("audio/") {
                return Some(AttachmentMediaType::Audio);
            }
        }

        if filename_has_extension(
            &self.filename,
            &["avif", "gif", "jpeg", "jpg", "png", "webp"],
        ) {
            return Some(AttachmentMediaType::Image);
        }
        if filename_has_extension(&self.filename, &["m4v", "mov", "mp4", "webm"]) {
            return Some(AttachmentMediaType::Video);
        }
        if filename_has_extension(
            &self.filename,
            &["mp3", "m4a", "opus", "ogg", "flac", "wav", "aiff"],
        ) {
            return Some(AttachmentMediaType::Audio);
        }
        None
    }

    pub fn inline_preview_url(&self) -> Option<&str> {
        self.media_type()
            .filter(|t| *t == AttachmentMediaType::Image)
            .and_then(|_| self.preferred_url())
    }

    pub fn inline_preview_info(&self) -> Option<InlinePreviewInfo<'_>> {
        if self.media_type() == Some(AttachmentMediaType::Video) && !self.proxy_url.is_empty() {
            return Some(InlinePreviewInfo {
                url: self.proxy_url.as_str(),
                proxy_url: Some(self.proxy_url.as_str()),
                filename: self.filename.as_str(),
                width: self.width,
                height: self.height,
                accent_color: None,
                animated: false,
                proxy_preview_only: true,
                show_play_marker: true,
            });
        }

        Some(InlinePreviewInfo {
            url: self.inline_preview_url()?,
            proxy_url: (!self.proxy_url.is_empty()).then_some(self.proxy_url.as_str()),
            filename: self.filename.as_str(),
            width: self.width,
            height: self.height,
            accent_color: None,
            animated: media_is_animated(self.flags, &self.filename, self.url.as_str()),
            proxy_preview_only: false,
            show_play_marker: false,
        })
    }
}

impl EmbedInfo {
    pub fn inline_preview_info(&self) -> Option<InlinePreviewInfo<'_>> {
        self.inline_previews().into_iter().next()
    }

    pub fn inline_previews(&self) -> Vec<InlinePreviewInfo<'_>> {
        if let Some(url) = self.gifv_image_url.as_deref() {
            return vec![InlinePreviewInfo {
                url,
                proxy_url: self.gifv_image_proxy_url.as_deref(),
                filename: "embed-gifv",
                width: self.image_width.or(self.thumbnail_width),
                height: self.image_height.or(self.thumbnail_height),
                accent_color: Some(self.color.unwrap_or(0xff0000)),
                animated: true,
                proxy_preview_only: false,
                show_play_marker: false,
            }];
        }

        let has_video = self.video_url.is_some()
            || self.video_proxy_url.is_some()
            || matches!(self.kind.as_deref(), Some("video" | "gifv"));
        if has_video {
            if let Some(url) = self.thumbnail_url.as_deref() {
                return vec![InlinePreviewInfo {
                    url,
                    proxy_url: self.thumbnail_proxy_url.as_deref(),
                    filename: "embed-thumbnail",
                    width: self.thumbnail_width,
                    height: self.thumbnail_height,
                    accent_color: Some(self.color.unwrap_or(0xff0000)),
                    animated: media_is_animated(self.thumbnail_flags, "", url),
                    proxy_preview_only: false,
                    show_play_marker: true,
                }];
            }
            if let Some(url) = self.image_url.as_deref() {
                return vec![InlinePreviewInfo {
                    url,
                    proxy_url: self.image_proxy_url.as_deref(),
                    filename: "embed-image",
                    width: self.image_width,
                    height: self.image_height,
                    accent_color: Some(self.color.unwrap_or(0xff0000)),
                    animated: media_is_animated(self.image_flags, "", url),
                    proxy_preview_only: false,
                    show_play_marker: true,
                }];
            }
            if let Some(url) = self.video_proxy_url.as_deref() {
                return vec![InlinePreviewInfo {
                    url,
                    proxy_url: Some(url),
                    filename: "embed-video",
                    width: self.video_width,
                    height: self.video_height,
                    accent_color: Some(self.color.unwrap_or(0xff0000)),
                    animated: false,
                    proxy_preview_only: true,
                    show_play_marker: true,
                }];
            }
        }

        let mut previews = Vec::new();
        if let Some(url) = self.image_url.as_deref() {
            previews.push(InlinePreviewInfo {
                url,
                proxy_url: self.image_proxy_url.as_deref(),
                filename: "embed-image",
                width: self.image_width,
                height: self.image_height,
                accent_color: Some(self.color.unwrap_or(0xff0000)),
                animated: media_is_animated(self.image_flags, "", url),
                proxy_preview_only: false,
                show_play_marker: false,
            });
        }
        if let Some(url) = self.thumbnail_url.as_deref()
            && self.image_url.as_deref() != Some(url)
        {
            previews.push(InlinePreviewInfo {
                url,
                proxy_url: self.thumbnail_proxy_url.as_deref(),
                filename: "embed-thumbnail",
                width: self.thumbnail_width,
                height: self.thumbnail_height,
                accent_color: Some(self.color.unwrap_or(0xff0000)),
                animated: media_is_animated(self.thumbnail_flags, "", url),
                proxy_preview_only: false,
                show_play_marker: false,
            });
        }
        previews
    }
}

impl ComponentMediaInfo {
    pub fn attachment_filename(&self) -> Option<&str> {
        self.url.strip_prefix("attachment://")
    }

    pub fn display_filename(&self) -> &str {
        self.attachment_filename()
            .or_else(|| {
                let path = self
                    .url
                    .split_once('?')
                    .map_or(self.url.as_str(), |(path, _)| path);
                path.rsplit('/').next().filter(|value| !value.is_empty())
            })
            .unwrap_or("component-media")
    }

    pub fn media_type(&self) -> Option<AttachmentMediaType> {
        if let Some(content_type) = self.content_type.as_deref() {
            if content_type.starts_with("image/") {
                return Some(AttachmentMediaType::Image);
            } else if content_type.starts_with("video/") {
                return Some(AttachmentMediaType::Video);
            } else if content_type.starts_with("audio/") {
                return Some(AttachmentMediaType::Audio);
            }
        }

        let filename = self.display_filename();
        if filename_has_extension(filename, &["avif", "gif", "jpeg", "jpg", "png", "webp"]) {
            return Some(AttachmentMediaType::Image);
        }
        if filename_has_extension(filename, &["m4v", "mov", "mp4", "webm"]) {
            return Some(AttachmentMediaType::Video);
        }
        if filename_has_extension(
            filename,
            &["mp3", "m4a", "opus", "ogg", "flac", "wav", "aiff"],
        ) {
            return Some(AttachmentMediaType::Audio);
        }
        None
    }

    fn inline_preview_info<'a>(
        &'a self,
        attachments: &'a [AttachmentInfo],
        accent_color: Option<u32>,
    ) -> Option<InlinePreviewInfo<'a>> {
        if let Some(filename) = self.attachment_filename()
            && let Some(attachment) = attachments
                .iter()
                .find(|attachment| attachment.filename == filename)
        {
            let mut preview = attachment.inline_preview_info()?;
            preview.accent_color = accent_color;
            return Some(preview);
        }

        let media_type = self.media_type()?;
        let filename = self.display_filename();
        let url = (!self.url.is_empty()).then_some(self.url.as_str())?;
        let proxy_url = self.proxy_url.as_deref();
        if media_type == AttachmentMediaType::Video {
            return Some(InlinePreviewInfo {
                url: proxy_url.unwrap_or(url),
                proxy_url,
                filename,
                width: self.width,
                height: self.height,
                accent_color,
                animated: false,
                proxy_preview_only: proxy_url.is_some(),
                show_play_marker: true,
            });
        }
        (media_type == AttachmentMediaType::Image).then(|| InlinePreviewInfo {
            url,
            proxy_url,
            filename,
            width: self.width,
            height: self.height,
            accent_color,
            animated: component_media_is_animated(self.flags, filename, url),
            proxy_preview_only: false,
            show_play_marker: false,
        })
    }
}

impl MessageComponentInfo {
    pub(crate) fn first_text(components: &[Self]) -> Option<&str> {
        components.iter().find_map(Self::first_text_value)
    }

    fn first_text_value(&self) -> Option<&str> {
        match self {
            Self::ActionRow { components } | Self::Container { components, .. } => {
                Self::first_text(components)
            }
            Self::Button { label, emoji, .. } => label
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .or_else(|| emoji.as_deref().filter(|value| !value.trim().is_empty())),
            Self::Select {
                placeholder,
                options,
                ..
            } => placeholder
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .or_else(|| {
                    options
                        .iter()
                        .map(|option| option.label.as_str())
                        .find(|label| !label.trim().is_empty())
                }),
            Self::Section {
                components,
                accessory,
            } => Self::first_text(components)
                .or_else(|| accessory.as_deref().and_then(Self::first_text_value)),
            Self::TextDisplay { content } => {
                (!content.trim().is_empty()).then_some(content.as_str())
            }
            Self::Thumbnail {
                media, description, ..
            } => description
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .or_else(|| Some(media.display_filename())),
            Self::MediaGallery { items } => items
                .iter()
                .filter_map(|item| item.description.as_deref())
                .find(|value| !value.trim().is_empty())
                .or_else(|| items.first().map(|item| item.media.display_filename())),
            Self::File { name, file, .. } => name
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .or_else(|| Some(file.display_filename())),
            Self::Separator { .. } | Self::Unknown { .. } => None,
        }
    }

    pub(crate) fn text_display_content(components: &[Self]) -> Option<String> {
        let mut content = Vec::new();
        Self::collect_text_display_content(components, &mut content);
        (!content.is_empty()).then(|| content.join("\n"))
    }

    fn collect_text_display_content<'a>(components: &'a [Self], content: &mut Vec<&'a str>) {
        for component in components {
            match component {
                Self::ActionRow { components } | Self::Container { components, .. } => {
                    Self::collect_text_display_content(components, content);
                }
                Self::Section { components, .. } => {
                    Self::collect_text_display_content(components, content);
                }
                Self::TextDisplay { content: value } if !value.is_empty() => {
                    content.push(value);
                }
                Self::Button { .. }
                | Self::Select { .. }
                | Self::TextDisplay { .. }
                | Self::Thumbnail { .. }
                | Self::MediaGallery { .. }
                | Self::File { .. }
                | Self::Separator { .. }
                | Self::Unknown { .. } => {}
            }
        }
    }

    pub(crate) fn references_attachment(components: &[Self], filename: &str) -> bool {
        components
            .iter()
            .any(|component| component.references_attachment_value(filename))
    }

    fn references_attachment_value(&self, filename: &str) -> bool {
        match self {
            Self::Thumbnail { media, .. } | Self::File { file: media, .. } => {
                media.attachment_filename() == Some(filename)
            }
            Self::MediaGallery { items } => items
                .iter()
                .any(|item| item.media.attachment_filename() == Some(filename)),
            Self::ActionRow { components } | Self::Container { components, .. } => {
                Self::references_attachment(components, filename)
            }
            Self::Section {
                components,
                accessory,
            } => {
                Self::references_attachment(components, filename)
                    || accessory
                        .as_deref()
                        .is_some_and(|accessory| accessory.references_attachment_value(filename))
            }
            Self::Button { .. }
            | Self::Select { .. }
            | Self::TextDisplay { .. }
            | Self::Separator { .. }
            | Self::Unknown { .. } => false,
        }
    }

    pub(crate) fn inline_previews<'a>(
        components: &'a [Self],
        attachments: &'a [AttachmentInfo],
    ) -> Vec<InlinePreviewInfo<'a>> {
        let mut previews = Vec::new();
        Self::collect_inline_previews(components, attachments, None, true, &mut previews);
        previews
    }

    pub(crate) fn flow_inline_previews<'a>(
        components: &'a [Self],
        attachments: &'a [AttachmentInfo],
    ) -> Vec<InlinePreviewInfo<'a>> {
        let mut previews = Vec::new();
        Self::collect_inline_previews(components, attachments, None, false, &mut previews);
        previews
    }

    pub(crate) fn collect_section_thumbnail_previews<'a>(
        components: &'a [Self],
        attachments: &'a [AttachmentInfo],
        next_index: &mut usize,
        previews: &mut Vec<(usize, InlinePreviewInfo<'a>)>,
    ) {
        Self::collect_section_thumbnail_previews_with_accent(
            components,
            attachments,
            None,
            next_index,
            previews,
        );
    }

    fn collect_section_thumbnail_previews_with_accent<'a>(
        components: &'a [Self],
        attachments: &'a [AttachmentInfo],
        accent_color: Option<u32>,
        next_index: &mut usize,
        previews: &mut Vec<(usize, InlinePreviewInfo<'a>)>,
    ) {
        for component in components {
            match component {
                Self::ActionRow { components } => {
                    Self::collect_section_thumbnail_previews_with_accent(
                        components,
                        attachments,
                        accent_color,
                        next_index,
                        previews,
                    )
                }
                Self::Section {
                    components,
                    accessory,
                } => {
                    if let Some(Self::Thumbnail { media, .. }) = accessory.as_deref() {
                        let index = *next_index;
                        *next_index = (*next_index).saturating_add(1);
                        if let Some(preview) = media.inline_preview_info(attachments, accent_color)
                        {
                            previews.push((index, preview));
                        }
                    }
                    Self::collect_section_thumbnail_previews_with_accent(
                        components,
                        attachments,
                        accent_color,
                        next_index,
                        previews,
                    );
                }
                Self::Container {
                    components,
                    accent_color: container_accent,
                    ..
                } => Self::collect_section_thumbnail_previews_with_accent(
                    components,
                    attachments,
                    container_accent.or(accent_color),
                    next_index,
                    previews,
                ),
                Self::Button { .. }
                | Self::Select { .. }
                | Self::TextDisplay { .. }
                | Self::Thumbnail { .. }
                | Self::MediaGallery { .. }
                | Self::File { .. }
                | Self::Separator { .. }
                | Self::Unknown { .. } => {}
            }
        }
    }

    fn collect_inline_previews<'a>(
        components: &'a [Self],
        attachments: &'a [AttachmentInfo],
        accent_color: Option<u32>,
        include_section_thumbnails: bool,
        previews: &mut Vec<InlinePreviewInfo<'a>>,
    ) {
        for component in components {
            match component {
                Self::ActionRow { components } => Self::collect_inline_previews(
                    components,
                    attachments,
                    accent_color,
                    include_section_thumbnails,
                    previews,
                ),
                Self::Section {
                    components,
                    accessory,
                } => {
                    Self::collect_inline_previews(
                        components,
                        attachments,
                        accent_color,
                        include_section_thumbnails,
                        previews,
                    );
                    if let Some(accessory) = accessory
                        && (include_section_thumbnails
                            || !matches!(accessory.as_ref(), Self::Thumbnail { .. }))
                    {
                        Self::collect_inline_previews(
                            std::slice::from_ref(accessory.as_ref()),
                            attachments,
                            accent_color,
                            include_section_thumbnails,
                            previews,
                        );
                    }
                }
                Self::Thumbnail { media, .. } => {
                    if let Some(preview) = media.inline_preview_info(attachments, accent_color) {
                        previews.push(preview);
                    }
                }
                Self::MediaGallery { items } => {
                    for item in items {
                        if let Some(preview) =
                            item.media.inline_preview_info(attachments, accent_color)
                        {
                            previews.push(preview);
                        }
                    }
                }
                Self::Container {
                    components,
                    accent_color: container_accent,
                    ..
                } => Self::collect_inline_previews(
                    components,
                    attachments,
                    container_accent.or(accent_color),
                    include_section_thumbnails,
                    previews,
                ),
                Self::Button { .. }
                | Self::Select { .. }
                | Self::TextDisplay { .. }
                | Self::File { .. }
                | Self::Separator { .. }
                | Self::Unknown { .. } => {}
            }
        }
    }
}

fn media_is_animated(flags: u64, filename: &str, url: &str) -> bool {
    flags & MEDIA_FLAG_IS_ANIMATED != 0 || media_name_is_animated(filename, url)
}

fn component_media_is_animated(flags: u64, filename: &str, url: &str) -> bool {
    // Discord's public app docs and user-client payload docs expose different
    // bit positions for this received field, so accept both representations.
    flags & (UNFURLED_MEDIA_FLAG_IS_ANIMATED | MEDIA_FLAG_IS_ANIMATED) != 0
        || media_name_is_animated(filename, url)
}

fn media_name_is_animated(filename: &str, url: &str) -> bool {
    let url_path = url.split_once('?').map_or(url, |(path, _)| path);
    filename_has_extension(filename, &["gif"])
        || filename_has_extension(url_path, &["gif"])
        || url
            .split_once('?')
            .map(|(_, query)| query.split('&'))
            .into_iter()
            .flatten()
            .any(|param| param.eq_ignore_ascii_case("animated=true"))
}

fn filename_has_extension(filename: &str, extensions: &[&str]) -> bool {
    filename.rsplit_once('.').is_some_and(|(_, extension)| {
        extensions
            .iter()
            .any(|value| extension.eq_ignore_ascii_case(value))
    })
}

#[cfg(test)]
mod sticker_tests {
    use super::*;

    #[test]
    fn a_sticker_url_matches_its_format() {
        let png = StickerInfo::new(Id::new(1), "wave".to_owned(), StickerFormat::Png);
        assert!(png.url.ends_with("/1.png"));

        // APNG is served as .png: the extension is the container, not the
        // animation, and asking for .apng returns nothing.
        let apng = StickerInfo::new(Id::new(2), "spin".to_owned(), StickerFormat::Apng);
        assert!(apng.url.ends_with("/2.png"));

        let gif = StickerInfo::new(Id::new(3), "dance".to_owned(), StickerFormat::Gif);
        assert!(gif.url.ends_with("/3.gif"));
    }

    #[test]
    fn a_lottie_sticker_has_no_image() {
        // A vector animation neither front end can play. It must not produce
        // a preview, or both would show a broken image where a name belongs.
        let lottie = StickerInfo::new(Id::new(4), "bounce".to_owned(), StickerFormat::Lottie);

        assert!(lottie.url.is_empty());
        assert!(lottie.image_url().is_none());
        assert!(lottie.inline_preview_info().is_none());
    }

    #[test]
    fn an_image_sticker_previews_at_discords_own_size() {
        let sticker = StickerInfo::new(Id::new(5), "hello".to_owned(), StickerFormat::Png);
        let preview = sticker
            .inline_preview_info()
            .expect("an image sticker should preview");

        // Sent so a terminal can reserve cells before the image arrives,
        // rather than reflowing the log when it does.
        assert_eq!(preview.width, Some(160));
        assert_eq!(preview.height, Some(160));
        assert_eq!(preview.url, sticker.url);
        assert!(!preview.animated);
    }
}

#[cfg(test)]
mod message_kind_tests {
    use super::MessageKind;

    /// Every message type the live client knows about must have a label.
    ///
    /// The codes were read off a running stable client in August 2026 - the
    /// whole table, not the ones that happened to come up. Anything without a
    /// label renders as "Unknown message type" in the log, which is a blank
    /// stare at a message Discord itself explains.
    ///
    /// 43 is genuinely absent from Discord's own table and is excluded here
    /// rather than left as a hole somebody would try to fill.
    #[test]
    fn every_message_type_the_client_knows_has_a_label() {
        let missing: Vec<u8> = (0..=68u8)
            .filter(|code| *code != 43)
            .filter(|code| MessageKind::new(*code).known_label().is_none())
            .collect();
        assert!(
            missing.is_empty(),
            "these message types would render as \"Unknown message type\": {missing:?}"
        );
    }

    /// A label past the end must still be absent, or the test above passes by
    /// labelling everything and proves nothing.
    #[test]
    fn a_type_discord_does_not_have_is_still_unknown() {
        assert_eq!(MessageKind::new(200).known_label(), None);
        assert_eq!(MessageKind::new(200).label(), "Unknown message type");
        assert_eq!(MessageKind::new(43).known_label(), None);
    }
}
