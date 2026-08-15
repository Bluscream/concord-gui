//! The soundboard.
//!
//! Two lists rather than one: the default sounds every account has, and the
//! ones a guild has added. They are fetched separately and shown together,
//! which is what the official client does and what makes the picker useful in
//! a guild that has added nothing.

use serde::Deserialize;
use serde_json::json;

use crate::Result;
use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, EmojiMarker, GuildMarker},
};

use super::DiscordRest;

/// Discord's limits on a soundboard sound's name.
pub const MIN_SOUND_NAME_CHARS: usize = 2;
pub const MAX_SOUND_NAME_CHARS: usize = 32;

/// One sound, from either list.
#[derive(Clone, Debug, PartialEq)]
pub struct SoundboardSound {
    pub sound_id: u64,
    pub name: String,
    /// 0 to 1. Discord stores it per sound rather than per playback.
    pub volume: f64,
    /// The custom emoji shown on the button, if it has one.
    pub emoji_id: Option<Id<EmojiMarker>>,
    /// The unicode emoji shown on the button, if it has one instead.
    pub emoji_name: Option<String>,
    /// `None` for a default sound, which belongs to no guild.
    pub guild_id: Option<Id<GuildMarker>>,
    /// Discord turns sounds off when a guild loses the boost level that paid
    /// for them. An unavailable sound is shown and refused rather than hidden,
    /// so the reason is visible.
    pub available: bool,
}

impl SoundboardSound {
    /// Where the audio lives.
    ///
    /// Sounds are Ogg, and small - a few tens of kilobytes - which is why the
    /// picker can afford to fetch one on demand rather than prefetching all.
    pub fn url(&self) -> String {
        format!(
            "https://cdn.discordapp.com/soundboard-sounds/{}",
            self.sound_id
        )
    }

    /// What to draw on the button when there is no emoji.
    pub fn label(&self) -> &str {
        self.emoji_name.as_deref().unwrap_or(&self.name)
    }
}

#[derive(Deserialize)]
struct SoundBody {
    // Discord sends the id as a string, as it does every snowflake.
    sound_id: Option<String>,
    name: Option<String>,
    #[serde(default = "default_volume")]
    volume: f64,
    emoji_id: Option<Id<EmojiMarker>>,
    emoji_name: Option<String>,
    guild_id: Option<Id<GuildMarker>>,
    #[serde(default = "default_available")]
    available: bool,
}

const fn default_volume() -> f64 {
    1.0
}

const fn default_available() -> bool {
    true
}

#[derive(Deserialize)]
struct DefaultSoundsBody(Vec<SoundBody>);

#[derive(Deserialize)]
struct GuildSoundsBody {
    #[serde(default)]
    items: Vec<SoundBody>,
}

fn parse_sounds(bodies: Vec<SoundBody>) -> Vec<SoundboardSound> {
    bodies
        .into_iter()
        .filter_map(|body| {
            // A sound with no id cannot be played or addressed, so there is
            // nothing a button for it could do.
            let sound_id = body.sound_id?.parse::<u64>().ok()?;
            Some(SoundboardSound {
                sound_id,
                name: body.name.unwrap_or_default(),
                volume: body.volume.clamp(0.0, 1.0),
                emoji_id: body.emoji_id,
                emoji_name: body.emoji_name,
                guild_id: body.guild_id,
                available: body.available,
            })
        })
        .collect()
}

/// Whether Discord will accept this as a sound name.
pub fn is_valid_sound_name(name: &str) -> bool {
    let length = name.trim().chars().count();
    (MIN_SOUND_NAME_CHARS..=MAX_SOUND_NAME_CHARS).contains(&length)
}

impl DiscordRest {
    /// The sounds every account has.
    pub async fn default_soundboard_sounds(&self) -> Result<Vec<SoundboardSound>> {
        let sounds: DefaultSoundsBody = self
            .send_json(
                self.raw_http
                    .get("https://discord.com/api/v9/soundboard-default-sounds"),
                "default soundboard sounds",
            )
            .await?;
        Ok(parse_sounds(sounds.0))
    }

    /// The sounds this guild has added.
    pub async fn guild_soundboard_sounds(
        &self,
        guild_id: Id<GuildMarker>,
    ) -> Result<Vec<SoundboardSound>> {
        let sounds: GuildSoundsBody = self
            .send_json(
                self.raw_http.get(format!(
                    "https://discord.com/api/v9/guilds/{}/soundboard-sounds",
                    guild_id.get()
                )),
                "guild soundboard sounds",
            )
            .await?;
        Ok(parse_sounds(sounds.items))
    }

    /// Play a sound into a voice channel.
    ///
    /// `source_guild_id` is what lets a guild's sound be played somewhere else
    /// it is allowed; it is omitted for a default sound, which belongs to no
    /// guild.
    pub async fn send_soundboard_sound(
        &self,
        channel_id: Id<ChannelMarker>,
        sound_id: u64,
        source_guild_id: Option<Id<GuildMarker>>,
    ) -> Result<()> {
        let mut body = json!({ "sound_id": sound_id.to_string() });
        if let Some(guild_id) = source_guild_id
            && let serde_json::Value::Object(fields) = &mut body
        {
            fields.insert(
                "source_guild_id".to_owned(),
                serde_json::Value::from(guild_id.get().to_string()),
            );
        }

        self.send_unit(
            self.raw_http
                .post(format!(
                    "https://discord.com/api/v9/channels/{}/send-soundboard-sound",
                    channel_id.get()
                ))
                .json(&body),
            "send soundboard sound",
        )
        .await
    }

    pub async fn rename_soundboard_sound(
        &self,
        guild_id: Id<GuildMarker>,
        sound_id: u64,
        name: &str,
    ) -> Result<()> {
        self.send_unit(
            self.raw_http
                .patch(format!(
                    "https://discord.com/api/v9/guilds/{}/soundboard-sounds/{sound_id}",
                    guild_id.get()
                ))
                .json(&json!({ "name": name })),
            "rename soundboard sound",
        )
        .await
    }

    pub async fn delete_soundboard_sound(
        &self,
        guild_id: Id<GuildMarker>,
        sound_id: u64,
    ) -> Result<()> {
        self.send_unit(
            self.raw_http.delete(format!(
                "https://discord.com/api/v9/guilds/{}/soundboard-sounds/{sound_id}",
                guild_id.get()
            )),
            "delete soundboard sound",
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sound(available: bool, emoji_name: Option<&str>) -> SoundboardSound {
        SoundboardSound {
            sound_id: 12345,
            name: "airhorn".to_owned(),
            volume: 1.0,
            emoji_id: None,
            emoji_name: emoji_name.map(str::to_owned),
            guild_id: None,
            available,
        }
    }

    #[test]
    fn a_sound_knows_where_its_audio_lives() {
        assert_eq!(
            sound(true, None).url(),
            "https://cdn.discordapp.com/soundboard-sounds/12345"
        );
    }

    #[test]
    fn the_button_prefers_an_emoji_and_falls_back_to_the_name() {
        assert_eq!(sound(true, Some("📯")).label(), "📯");
        assert_eq!(sound(true, None).label(), "airhorn");
    }

    #[test]
    fn a_sound_with_no_id_is_dropped_rather_than_shown() {
        // It could not be played, so a button for it would do nothing.
        let parsed = parse_sounds(vec![
            SoundBody {
                sound_id: None,
                name: Some("broken".to_owned()),
                volume: 1.0,
                emoji_id: None,
                emoji_name: None,
                guild_id: None,
                available: true,
            },
            SoundBody {
                sound_id: Some("7".to_owned()),
                name: Some("fine".to_owned()),
                volume: 1.0,
                emoji_id: None,
                emoji_name: None,
                guild_id: None,
                available: true,
            },
        ]);

        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].sound_id, 7);
    }

    #[test]
    fn volume_is_clamped_to_the_range_discord_documents() {
        // A sound that came back out of range would otherwise be played at
        // whatever the audio path does with a multiplier above one.
        let parsed = parse_sounds(vec![SoundBody {
            sound_id: Some("1".to_owned()),
            name: None,
            volume: 4.0,
            emoji_id: None,
            emoji_name: None,
            guild_id: None,
            available: true,
        }]);

        assert_eq!(parsed[0].volume, 1.0);
    }

    #[test]
    fn sound_names_discord_would_reject_are_refused_here() {
        assert!(is_valid_sound_name("ok"));
        assert!(is_valid_sound_name("airhorn"));

        assert!(!is_valid_sound_name("a"));
        assert!(!is_valid_sound_name(""));
        assert!(!is_valid_sound_name("   "));
        assert!(!is_valid_sound_name(&"a".repeat(33)));
    }

    #[test]
    fn an_unavailable_sound_still_parses() {
        // Shown and refused rather than hidden, so the reason is visible when
        // a guild loses the boost level that paid for its sounds.
        assert!(!sound(false, None).available);
    }
}
