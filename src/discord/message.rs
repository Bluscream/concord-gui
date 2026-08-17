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
const MEDIA_FLAG_IS_ANIMATED: u64 = 1 << 5;

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

#[cfg(test)]
#[allow(dead_code)]
impl MentionInfo {
    pub(crate) fn test(user_id: Id<UserMarker>, display_name: impl Into<String>) -> Self {
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

#[cfg(test)]
#[allow(dead_code)]
impl AttachmentInfo {
    pub(crate) fn test(id: Id<AttachmentMarker>, filename: impl Into<String>) -> Self {
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
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmbedInfo {
    pub color: Option<u32>,
    pub provider_name: Option<String>,
    pub author_name: Option<String>,
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
    pub thumbnail_flags: u64,
    pub image_url: Option<String>,
    pub image_proxy_url: Option<String>,
    pub image_width: Option<u64>,
    pub image_height: Option<u64>,
    pub image_flags: u64,
    /// Animated image rendition recovered for a `gifv` embed when its provider
    /// exposes only a video URL in Discord's payload.
    pub gifv_image_url: Option<String>,
    pub video_url: Option<String>,
}

#[cfg(test)]
#[allow(dead_code)]
impl EmbedInfo {
    pub(crate) fn test() -> Self {
        Self {
            color: None,
            provider_name: None,
            author_name: None,
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
            thumbnail_flags: 0,
            image_url: None,
            image_proxy_url: None,
            image_width: None,
            image_height: None,
            image_flags: 0,
            gifv_image_url: None,
            video_url: None,
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
            31 => Some("Stage topic"),
            32 => Some("Application premium subscription"),
            36 => Some("Incident alert mode enabled"),
            37 => Some("Incident alert mode disabled"),
            38 => Some("Incident raid report"),
            39 => Some("Incident false alarm report"),
            44 => Some("Purchase notification"),
            46 => Some("Poll result"),
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

impl StickerInfo {
    /// Build one, deriving the CDN URL from the id and format.
    pub fn new(
        id: Id<crate::discord::ids::marker::StickerMarker>,
        name: String,
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
            name,
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
    pub attachments: Vec<AttachmentInfo>,
    pub embeds: Vec<EmbedInfo>,
    pub source_channel_id: Option<Id<ChannelMarker>>,
    pub timestamp: Option<String>,
}

#[cfg(test)]
#[allow(dead_code)]
impl MessageSnapshotInfo {
    pub(crate) fn test() -> Self {
        Self {
            content: None,
            sticker_names: Vec::new(),
            stickers: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
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

#[cfg(test)]
#[allow(dead_code)]
impl ReplyInfo {
    pub(crate) fn test(author: impl Into<String>) -> Self {
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

#[cfg(test)]
#[allow(dead_code)]
impl MessageInteractionInfo {
    pub(crate) fn test(user: impl Into<String>) -> Self {
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

#[cfg(test)]
#[allow(dead_code)]
impl MessageReferenceInfo {
    pub(crate) fn test(message_id: Id<MessageMarker>) -> Self {
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

#[cfg(test)]
#[allow(dead_code)]
impl PollInfo {
    pub(crate) fn test(question: impl Into<String>) -> Self {
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

#[cfg(test)]
#[allow(dead_code)]
impl PollAnswerInfo {
    pub(crate) fn test(answer_id: u8, text: impl Into<String>) -> Self {
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

#[cfg(test)]
#[allow(dead_code)]
impl ReactionInfo {
    pub(crate) fn test(emoji: ReactionEmoji) -> Self {
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

#[cfg(test)]
#[allow(dead_code)]
impl ReactionUserInfo {
    pub(crate) fn test(user_id: Id<UserMarker>, display_name: impl Into<String>) -> Self {
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
            forwarded_snapshots: Vec::new(),
            edited_timestamp: None,
        }
    }
}

#[cfg(test)]
#[allow(dead_code)]
impl MessageInfo {
    pub(crate) fn test(channel_id: Id<ChannelMarker>, message_id: Id<MessageMarker>) -> Self {
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
        if let Some(url) = self.gifv_image_url.as_deref() {
            return Some(InlinePreviewInfo {
                url,
                proxy_url: None,
                filename: "embed-gifv",
                width: self.image_width.or(self.thumbnail_width),
                height: self.image_height.or(self.thumbnail_height),
                accent_color: Some(self.color.unwrap_or(0xff0000)),
                animated: true,
                proxy_preview_only: false,
                show_play_marker: false,
            });
        }

        let show_play_marker = self.video_url.is_some();

        if let Some(url) = self.thumbnail_url.as_deref() {
            return Some(InlinePreviewInfo {
                url,
                proxy_url: self.thumbnail_proxy_url.as_deref(),
                filename: "embed-thumbnail",
                width: self.thumbnail_width,
                height: self.thumbnail_height,
                accent_color: Some(self.color.unwrap_or(0xff0000)),
                animated: media_is_animated(self.thumbnail_flags, "", url),
                proxy_preview_only: false,
                show_play_marker,
            });
        }

        self.image_url.as_deref().map(|url| InlinePreviewInfo {
            url,
            proxy_url: self.image_proxy_url.as_deref(),
            filename: "embed-image",
            width: self.image_width,
            height: self.image_height,
            accent_color: Some(self.color.unwrap_or(0xff0000)),
            animated: media_is_animated(self.image_flags, "", url),
            proxy_preview_only: false,
            show_play_marker,
        })
    }
}

fn media_is_animated(flags: u64, filename: &str, url: &str) -> bool {
    let url_path = url.split_once('?').map_or(url, |(path, _)| path);
    flags & MEDIA_FLAG_IS_ANIMATED != 0
        || filename_has_extension(filename, &["gif"])
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
