use serde_json::Value;

use crate::discord::ids::{
    Id,
    marker::{GuildMarker, UserMarker},
};

pub(crate) fn user_avatar_url(user_id: Id<UserMarker>, user: &Value) -> Option<String> {
    match custom_avatar_hash(user) {
        Some(hash) => Some(format!(
            "https://cdn.discordapp.com/avatars/{user_id}/{hash}.{extension}",
            extension = avatar_hash_extension(hash)
        )),
        None => Some(default_avatar_url(user_id, discriminator(user))),
    }
}

pub(crate) fn member_avatar_url(
    guild_id: Option<Id<GuildMarker>>,
    user_id: Id<UserMarker>,
    member: Option<&Value>,
    user: Option<&Value>,
) -> Option<String> {
    if let Some(guild_id) = guild_id
        && let Some(hash) = member.and_then(custom_avatar_hash)
    {
        let extension = avatar_hash_extension(hash);
        return Some(format!(
            "https://cdn.discordapp.com/guilds/{guild_id}/users/{user_id}/avatars/{hash}.{extension}"
        ));
    }

    user.and_then(|user| user_avatar_url(user_id, user))
}

pub(crate) fn default_avatar_url(user_id: Id<UserMarker>, discriminator: u16) -> String {
    let index = if discriminator == 0 {
        (user_id.get() >> 22) % 6
    } else {
        u64::from(discriminator % 5)
    };

    format!("https://cdn.discordapp.com/embed/avatars/{index}.png")
}

pub(crate) fn avatar_hash_extension(hash: &str) -> &'static str {
    if hash.starts_with("a_") { "gif" } else { "png" }
}

/// The still version of an avatar URL.
///
/// Discord serves every animated avatar as a PNG at the same path, so turning
/// animation off is an extension swap rather than a different request. Useful
/// on hardware where decoding a wall of GIFs costs more than it is worth -
/// this client is meant to run on a Raspberry Pi.
pub fn still_avatar_url(url: &str) -> String {
    match url.strip_suffix(".gif") {
        Some(base) => format!("{base}.png"),
        None => url.to_owned(),
    }
}

fn custom_avatar_hash(value: &Value) -> Option<&str> {
    value
        .get("avatar")
        .and_then(Value::as_str)
        .filter(|hash| !hash.is_empty())
}

fn discriminator(user: &Value) -> u16 {
    user.get("discriminator")
        .and_then(|value| {
            value
                .as_str()
                .and_then(|value| value.parse::<u16>().ok())
                .or_else(|| value.as_u64().and_then(|value| u16::try_from(value).ok()))
        })
        .unwrap_or(0)
}

#[cfg(test)]
mod still_avatar_tests {
    use super::*;

    #[test]
    fn an_animated_avatar_has_a_still_version_at_the_same_path() {
        // Discord serves both, so turning animation off is an extension swap
        // rather than a different request or a paused decode.
        assert_eq!(
            still_avatar_url("https://cdn.discordapp.com/avatars/1/a_abc.gif"),
            "https://cdn.discordapp.com/avatars/1/a_abc.png"
        );
    }

    #[test]
    fn a_still_avatar_is_left_alone() {
        let url = "https://cdn.discordapp.com/avatars/1/abc.png";
        assert_eq!(still_avatar_url(url), url);

        // And a default avatar, which has no hash to inspect at all.
        let default = "https://cdn.discordapp.com/embed/avatars/3.png";
        assert_eq!(still_avatar_url(default), default);
    }

    #[test]
    fn only_the_extension_is_touched() {
        // A path containing "gif" elsewhere must not be rewritten.
        let url = "https://cdn.discordapp.com/avatars/1/gifted.png";
        assert_eq!(still_avatar_url(url), url);
    }
}
