//! Guild stickers.
//!
//! The last expression type with no management: emoji and sounds both have it.
//! Unlike an emoji, a sticker uploads as multipart form data rather than as a
//! data URI - Discord accepts Lottie JSON here, which is not an image and has
//! no sensible data-URI content type.

use reqwest::multipart::{Form, Part};
use serde::Deserialize;

use crate::Result;
use crate::discord::ids::{Id, marker::GuildMarker};

use super::DiscordRest;
use crate::discord::StickerFormat;

/// Discord's caps on a sticker.
pub const MIN_STICKER_NAME_CHARS: usize = 2;
pub const MAX_STICKER_NAME_CHARS: usize = 30;
pub const MAX_STICKER_TAGS_CHARS: usize = 200;
/// 500 KiB. Larger is refused outright rather than resized.
pub const MAX_STICKER_BYTES: u64 = 500 * 1024;

/// One sticker in a guild.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GuildSticker {
    pub id: u64,
    pub name: String,
    pub description: Option<String>,
    /// Comma-separated suggestion keywords. Discord's own clients put a single
    /// emoji name here, but the field is free text.
    pub tags: String,
    pub format: StickerFormat,
    /// Whether it can currently be used. A server that loses boosts keeps its
    /// stickers but cannot send them, which is not visible from the name.
    pub available: bool,
}

impl GuildSticker {
    pub fn summary(&self) -> String {
        let mut parts = vec![self.format.label().to_owned()];
        if !self.available {
            parts.push("unavailable - the server lost boosts".to_owned());
        }
        if !self.tags.is_empty() {
            parts.push(self.tags.clone());
        }
        if let Some(description) = &self.description
            && !description.is_empty()
        {
            parts.push(description.clone());
        }
        parts.join(" - ")
    }
}

/// Whether Discord would accept this as a sticker name.
///
/// Checked here so a rejected upload costs no round trip, and so the reason
/// names the field - Discord's does not.
pub fn sticker_name_problem(name: &str) -> Option<&'static str> {
    let count = name.trim().chars().count();
    if count < MIN_STICKER_NAME_CHARS {
        return Some("needs a name of at least two characters");
    }
    if count > MAX_STICKER_NAME_CHARS {
        return Some("name is too long");
    }
    None
}

/// The file extensions Discord accepts, and the content type for each.
///
/// Lottie is JSON rather than an image, which is the reason this is a lookup
/// rather than a guess from the bytes.
pub fn sticker_content_type(path: &str) -> Option<&'static str> {
    let extension = path.rsplit('.').next()?.to_lowercase();
    match extension.as_str() {
        "png" | "apng" => Some("image/png"),
        "gif" => Some("image/gif"),
        "json" => Some("application/json"),
        _ => None,
    }
}

#[derive(Deserialize)]
struct StickerBody {
    id: Option<String>,
    name: Option<String>,
    description: Option<String>,
    #[serde(default)]
    tags: String,
    #[serde(default)]
    format_type: u64,
    #[serde(default = "available_by_default")]
    available: bool,
}

/// Absent means usable. Discord omits the field for a sticker that is fine,
/// and defaulting to false would mark every one of them unavailable.
const fn available_by_default() -> bool {
    true
}

impl DiscordRest {
    pub async fn guild_stickers(&self, guild_id: Id<GuildMarker>) -> Result<Vec<GuildSticker>> {
        let stickers: Vec<StickerBody> = self
            .send_json(
                self.raw_http.get(format!(
                    "https://discord.com/api/v9/guilds/{}/stickers",
                    guild_id.get()
                )),
                "stickers",
            )
            .await?;

        Ok(stickers
            .into_iter()
            .filter_map(|sticker| {
                // Without an id there is nothing to rename or delete.
                let id = sticker.id?.parse::<u64>().ok()?;
                Some(GuildSticker {
                    id,
                    name: sticker.name.unwrap_or_default(),
                    description: sticker.description.filter(|text| !text.is_empty()),
                    tags: sticker.tags,
                    format: StickerFormat::from_wire(sticker.format_type),
                    available: sticker.available,
                })
            })
            .collect())
    }

    /// Upload a sticker.
    ///
    /// Multipart rather than a data URI, unlike an emoji: Discord accepts
    /// Lottie JSON here, which is not an image.
    pub async fn create_sticker(
        &self,
        guild_id: Id<GuildMarker>,
        name: &str,
        tags: &str,
        path: &str,
    ) -> Result<()> {
        if let Some(problem) = sticker_name_problem(name) {
            return Err(crate::AppError::DiscordRequest(format!(
                "a sticker {problem}"
            )));
        }
        let Some(content_type) = sticker_content_type(path) else {
            return Err(crate::AppError::DiscordRequest(
                "a sticker must be a PNG, APNG, GIF or Lottie JSON file".to_owned(),
            ));
        };

        let bytes = tokio::fs::read(path)
            .await
            .map_err(|error| crate::AppError::DiscordRequest(format!("{path}: {error}")))?;
        if bytes.len() as u64 > MAX_STICKER_BYTES {
            return Err(crate::AppError::DiscordRequest(format!(
                "a sticker must be under {} KiB",
                MAX_STICKER_BYTES / 1024
            )));
        }

        let file_name = path.rsplit('/').next().unwrap_or("sticker").to_owned();
        let form = Form::new()
            .text(
                "name",
                name.chars()
                    .take(MAX_STICKER_NAME_CHARS)
                    .collect::<String>(),
            )
            // Required even when empty. Not collected: the upload form is one
            // line already, and a description nobody sees is not worth a
            // second separator to type past.
            .text("description", String::new())
            .text(
                "tags",
                tags.chars()
                    .take(MAX_STICKER_TAGS_CHARS)
                    .collect::<String>(),
            )
            .part(
                "file",
                Part::bytes(bytes)
                    .file_name(file_name)
                    .mime_str(content_type)
                    .map_err(|error| crate::AppError::DiscordRequest(error.to_string()))?,
            );

        self.send_unit(
            self.raw_http
                .post(format!(
                    "https://discord.com/api/v9/guilds/{}/stickers",
                    guild_id.get()
                ))
                .multipart(form),
            "create sticker",
        )
        .await
    }

    pub async fn rename_sticker(
        &self,
        guild_id: Id<GuildMarker>,
        sticker_id: u64,
        name: &str,
    ) -> Result<()> {
        if let Some(problem) = sticker_name_problem(name) {
            return Err(crate::AppError::DiscordRequest(format!(
                "a sticker {problem}"
            )));
        }

        self.send_unit(
            self.raw_http
                .patch(format!(
                    "https://discord.com/api/v9/guilds/{}/stickers/{sticker_id}",
                    guild_id.get()
                ))
                .json(&serde_json::json!({
                    "name": name.chars().take(MAX_STICKER_NAME_CHARS).collect::<String>(),
                })),
            "rename sticker",
        )
        .await
    }

    pub async fn delete_sticker(&self, guild_id: Id<GuildMarker>, sticker_id: u64) -> Result<()> {
        self.send_unit(
            self.raw_http.delete(format!(
                "https://discord.com/api/v9/guilds/{}/stickers/{sticker_id}",
                guild_id.get()
            )),
            "delete sticker",
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sticker(available: bool, format: StickerFormat) -> GuildSticker {
        GuildSticker {
            id: 1,
            name: "wave".to_owned(),
            description: None,
            tags: "wave".to_owned(),
            format,
            available,
        }
    }

    #[test]
    fn a_sticker_the_server_can_no_longer_send_says_why() {
        // A server that loses boosts keeps its stickers and cannot send them.
        // Nothing about the name reveals that.
        assert!(
            sticker(false, StickerFormat::Png)
                .summary()
                .contains("lost boosts")
        );
        assert!(
            !sticker(true, StickerFormat::Png)
                .summary()
                .contains("lost boosts")
        );
    }

    #[test]
    fn an_absent_available_field_means_usable() {
        // Discord omits it for a sticker that is fine, so defaulting to false
        // would mark every sticker in a healthy server as unavailable.
        let body: StickerBody =
            serde_json::from_str(r#"{"id":"1","name":"wave"}"#).expect("should parse");
        assert!(body.available);
    }

    #[test]
    fn names_discord_would_reject_are_refused_here() {
        assert_eq!(sticker_name_problem("wave"), None);
        assert!(sticker_name_problem("a").is_some());
        assert!(sticker_name_problem(" ").is_some());
        assert!(sticker_name_problem(&"a".repeat(MAX_STICKER_NAME_CHARS + 1)).is_some());
    }

    #[test]
    fn name_length_is_counted_in_characters_not_bytes() {
        assert_eq!(
            sticker_name_problem(&"é".repeat(MAX_STICKER_NAME_CHARS)),
            None
        );
        assert!(sticker_name_problem(&"é".repeat(MAX_STICKER_NAME_CHARS + 1)).is_some());
    }

    #[test]
    fn every_format_discord_accepts_has_a_content_type() {
        // Lottie is JSON rather than an image, which is why this is a lookup
        // rather than a guess from the first bytes of the file.
        assert_eq!(sticker_content_type("a.png"), Some("image/png"));
        assert_eq!(sticker_content_type("a.APNG"), Some("image/png"));
        assert_eq!(sticker_content_type("a.gif"), Some("image/gif"));
        assert_eq!(sticker_content_type("a.json"), Some("application/json"));
    }

    #[test]
    fn anything_else_is_refused_rather_than_sent_as_a_guess() {
        // Discord rejects it, and a guessed content type would make that a
        // failure nobody can explain from the filename.
        assert_eq!(sticker_content_type("a.jpg"), None);
        assert_eq!(sticker_content_type("a.webp"), None);
        assert_eq!(sticker_content_type("noextension"), None);
    }
}
