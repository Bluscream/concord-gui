//! Invite resolution and acceptance.
//!
//! Until now a client could leave a guild but never join one, which made
//! joining a server something you had to open the official client to do.

use serde::Deserialize;

use crate::Result;
use crate::discord::ids::{Id, marker::GuildMarker};

use super::DiscordRest;

/// What an invite points at, before accepting it.
///
/// Shown to the user first: an invite code says nothing about where it leads,
/// and joining a server sight unseen is not a decision to make on their behalf.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InvitePreview {
    pub code: String,
    pub guild_id: Option<Id<GuildMarker>>,
    pub guild_name: String,
    /// Channel the invite drops into, when it names one.
    pub channel_name: Option<String>,
    pub inviter: Option<String>,
    /// Approximate counts, which Discord only sends when asked for them.
    pub member_count: Option<u64>,
    pub online_count: Option<u64>,
    /// Whether the account is already in this guild, so the UI can say "open"
    /// rather than offering to join something already joined.
    pub already_joined: bool,
}

#[derive(Deserialize)]
struct InviteBody {
    code: String,
    guild: Option<InviteGuild>,
    channel: Option<InviteChannel>,
    inviter: Option<InviteUser>,
    approximate_member_count: Option<u64>,
    approximate_presence_count: Option<u64>,
}

#[derive(Deserialize)]
struct InviteGuild {
    id: Option<Id<GuildMarker>>,
    name: Option<String>,
}

#[derive(Deserialize)]
struct InviteChannel {
    name: Option<String>,
}

#[derive(Deserialize)]
struct InviteUser {
    username: Option<String>,
    global_name: Option<String>,
}

/// Extract the code from whatever the user pasted.
///
/// Accepts a bare code or any of the URL forms, because a user who has copied
/// an invite has copied a link, not a code.
pub fn invite_code_from(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Strip the scheme and any known host, then take the last path segment.
    // Query strings are dropped: `?event=` and friends are not part of the
    // code, and sending them makes the lookup fail.
    let without_scheme = trimmed
        .trim_start_matches("https://")
        .trim_start_matches("http://");

    let path = without_scheme
        .split(['?', '#'])
        .next()
        .unwrap_or(without_scheme)
        .trim_end_matches('/');

    let candidate = match path.split_once('/') {
        // A host was present, so the code is the final segment - this also
        // handles discord.com/invite/CODE, which has two.
        Some((host, rest)) if host.contains('.') => rest.rsplit('/').next().unwrap_or(rest),
        _ => path,
    };

    // Invite codes are alphanumeric with dashes; anything else means the user
    // pasted something that is not an invite.
    let valid = !candidate.is_empty()
        && candidate
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '-');

    valid.then(|| candidate.to_string())
}

impl DiscordRest {
    /// Look up an invite without joining.
    pub async fn resolve_invite(&self, code: &str) -> Result<InvitePreview> {
        let body: InviteBody = self
            .send_json(
                self.raw_http
                    .get(format!("https://discord.com/api/v9/invites/{code}"))
                    // Counts are omitted unless requested, and they are most of
                    // what tells someone whether this is the server they meant.
                    .query(&[("with_counts", "true"), ("with_expiration", "true")]),
                "resolve invite",
            )
            .await?;

        Ok(InvitePreview {
            code: body.code,
            guild_id: body.guild.as_ref().and_then(|guild| guild.id),
            guild_name: body
                .guild
                .as_ref()
                .and_then(|guild| guild.name.clone())
                .unwrap_or_else(|| "Unknown server".to_string()),
            channel_name: body.channel.and_then(|channel| channel.name),
            inviter: body.inviter.and_then(|user| {
                // Display name where there is one; Discord moved to global
                // names and the username is now often an unhelpful handle.
                user.global_name.or(user.username)
            }),
            member_count: body.approximate_member_count,
            online_count: body.approximate_presence_count,
            already_joined: false,
        })
    }

    /// Accept an invite, joining the guild.
    pub async fn accept_invite(&self, code: &str) -> Result<Option<Id<GuildMarker>>> {
        let body: InviteBody = self
            .send_json(
                self.raw_http
                    .post(format!("https://discord.com/api/v9/invites/{code}"))
                    // Discord expects a body on this route; an empty object is
                    // what the web client sends.
                    .json(&serde_json::json!({})),
                "accept invite",
            )
            .await?;

        Ok(body.guild.and_then(|guild| guild.id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_extracted_from_every_form_a_user_might_paste() {
        let code = Some("aBc-123".to_string());

        assert_eq!(invite_code_from("aBc-123"), code);
        assert_eq!(invite_code_from("discord.gg/aBc-123"), code);
        assert_eq!(invite_code_from("https://discord.gg/aBc-123"), code);
        assert_eq!(invite_code_from("http://discord.gg/aBc-123"), code);
        assert_eq!(invite_code_from("https://discord.com/invite/aBc-123"), code);
        assert_eq!(invite_code_from("  https://discord.gg/aBc-123/  "), code);

        // Query strings are not part of the code, and sending them 404s.
        assert_eq!(
            invite_code_from("https://discord.gg/aBc-123?event=98765"),
            code
        );
    }

    #[test]
    fn things_that_are_not_invites_are_rejected() {
        assert_eq!(invite_code_from(""), None);
        assert_eq!(invite_code_from("   "), None);
        // A message link is not an invite, and treating it as one would send a
        // meaningless lookup.
        assert_eq!(invite_code_from("https://discord.gg/a b"), None);
        assert_eq!(invite_code_from("hello there"), None);
    }
}
