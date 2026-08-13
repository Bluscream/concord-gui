use std::fmt;

const CUSTOM_EMOJI_CDN_BASE: &str = "https://cdn.discordapp.com/emojis";

/// Builds the CDN URL Discord documents for a custom emoji.
///
/// Animated emoji may originate as WebP or AVIF and Discord does not convert
/// those uploads to GIF. Requesting animated WebP works for every supported
/// source format and matches the format used by Discord's own client.
pub(crate) fn custom_emoji_image_url(id: impl fmt::Display, animated: bool) -> String {
    if animated {
        format!("{CUSTOM_EMOJI_CDN_BASE}/{id}.webp?animated=true")
    } else {
        format!("{CUSTOM_EMOJI_CDN_BASE}/{id}.png")
    }
}
