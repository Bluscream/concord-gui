//! Managing a server: its invites, its emoji, and what has been done in it.
//!
//! Endpoints cross-checked against Abaddon, which is the only surveyed
//! third-party client that offers any of this.

use serde::Deserialize;
use serde_json::json;

use crate::Result;
use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, EmojiMarker, GuildMarker, UserMarker},
};

use super::DiscordRest;

/// Discord's cap on how long an invite may last, in seconds.
///
/// Seven days, the same figure as the ban message-deletion window and equally
/// easy to exceed by passing days where seconds were meant.
pub const MAX_INVITE_MAX_AGE_SECONDS: u32 = 604_800;

/// Discord's cap on how many times an invite may be used.
pub const MAX_INVITE_MAX_USES: u32 = 100;

/// An invite to somewhere in this guild.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GuildInviteInfo {
    pub code: String,
    pub channel_id: Option<Id<ChannelMarker>>,
    pub channel_name: Option<String>,
    /// Who made it, when Discord says.
    pub inviter: Option<String>,
    pub uses: u32,
    /// `None` means unlimited, which is what Discord's 0 means here.
    pub max_uses: Option<u32>,
    /// `None` means it never expires, again from Discord's 0.
    pub max_age_seconds: Option<u32>,
    /// Membership lasts only until the member disconnects.
    pub temporary: bool,
}

/// One thing somebody did, as the audit log records it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuditLogEntryInfo {
    pub id: Id<UserMarker>,
    /// Who did it, resolved against the users the response carries.
    pub actor: Option<String>,
    /// What was done, as Discord's numeric action type.
    pub action: AuditLogAction,
    /// What it was done to, when the entry names one.
    pub target: Option<String>,
    pub reason: Option<String>,
}

/// The audit log actions worth naming.
///
/// Discord has around fifty and adds more; the ones this client can itself
/// cause are named, and the rest keep their number rather than being dropped -
/// an unexplained entry is still evidence that something happened.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuditLogAction {
    GuildUpdate,
    ChannelCreate,
    ChannelUpdate,
    ChannelDelete,
    MemberKick,
    MemberBanAdd,
    MemberBanRemove,
    MemberUpdate,
    MemberRoleUpdate,
    RoleCreate,
    RoleUpdate,
    RoleDelete,
    InviteCreate,
    InviteDelete,
    MessageDelete,
    EmojiCreate,
    EmojiUpdate,
    EmojiDelete,
    Other(u16),
}

impl AuditLogAction {
    pub fn from_code(code: u16) -> Self {
        match code {
            1 => Self::GuildUpdate,
            10 => Self::ChannelCreate,
            11 => Self::ChannelUpdate,
            12 => Self::ChannelDelete,
            20 => Self::MemberKick,
            22 => Self::MemberBanAdd,
            23 => Self::MemberBanRemove,
            24 => Self::MemberUpdate,
            25 => Self::MemberRoleUpdate,
            30 => Self::RoleCreate,
            31 => Self::RoleUpdate,
            32 => Self::RoleDelete,
            40 => Self::InviteCreate,
            42 => Self::InviteDelete,
            72 => Self::MessageDelete,
            60 => Self::EmojiCreate,
            61 => Self::EmojiUpdate,
            62 => Self::EmojiDelete,
            other => Self::Other(other),
        }
    }

    /// A short description, for a log that is meant to be skimmed.
    pub fn label(self) -> String {
        match self {
            Self::GuildUpdate => "updated the server".to_owned(),
            Self::ChannelCreate => "created a channel".to_owned(),
            Self::ChannelUpdate => "updated a channel".to_owned(),
            Self::ChannelDelete => "deleted a channel".to_owned(),
            Self::MemberKick => "kicked".to_owned(),
            Self::MemberBanAdd => "banned".to_owned(),
            Self::MemberBanRemove => "unbanned".to_owned(),
            Self::MemberUpdate => "updated".to_owned(),
            Self::MemberRoleUpdate => "changed roles for".to_owned(),
            Self::RoleCreate => "created a role".to_owned(),
            Self::RoleUpdate => "updated a role".to_owned(),
            Self::RoleDelete => "deleted a role".to_owned(),
            Self::InviteCreate => "created an invite".to_owned(),
            Self::InviteDelete => "revoked an invite".to_owned(),
            Self::MessageDelete => "deleted a message".to_owned(),
            Self::EmojiCreate => "added an emoji".to_owned(),
            Self::EmojiUpdate => "renamed an emoji".to_owned(),
            Self::EmojiDelete => "removed an emoji".to_owned(),
            // Kept rather than dropped: an entry nobody can name is still
            // evidence that something was done.
            Self::Other(code) => format!("did something (action {code})"),
        }
    }
}

/// One of a guild's custom emoji.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GuildEmojiInfo {
    pub id: Id<EmojiMarker>,
    pub name: String,
    pub animated: bool,
    /// Emoji can be limited to particular roles, in which case most members
    /// cannot use them and should be told why rather than left wondering.
    pub role_restricted: bool,
}

#[derive(Deserialize)]
struct InviteBody {
    code: String,
    channel: Option<InviteChannel>,
    inviter: Option<InviteUser>,
    #[serde(default)]
    uses: u32,
    #[serde(default)]
    max_uses: u32,
    #[serde(default)]
    max_age: u32,
    #[serde(default)]
    temporary: bool,
}

#[derive(Deserialize)]
struct InviteChannel {
    id: Option<Id<ChannelMarker>>,
    name: Option<String>,
}

#[derive(Deserialize)]
struct InviteUser {
    username: Option<String>,
    global_name: Option<String>,
}

#[derive(Deserialize)]
struct AuditLogBody {
    #[serde(default)]
    audit_log_entries: Vec<AuditLogEntryBody>,
    #[serde(default)]
    users: Vec<AuditLogUser>,
}

#[derive(Deserialize)]
struct AuditLogEntryBody {
    id: Option<Id<UserMarker>>,
    user_id: Option<Id<UserMarker>>,
    target_id: Option<String>,
    #[serde(default)]
    action_type: u16,
    reason: Option<String>,
}

#[derive(Deserialize)]
struct AuditLogUser {
    id: Id<UserMarker>,
    username: Option<String>,
    global_name: Option<String>,
}

#[derive(Deserialize)]
struct EmojiBody {
    id: Option<Id<EmojiMarker>>,
    name: Option<String>,
    #[serde(default)]
    animated: bool,
    #[serde(default)]
    roles: Vec<String>,
}

/// Clamp an invite's lifetime to what Discord accepts.
///
/// Zero is meaningful - it means "never expires" - so it passes through rather
/// than being treated as unset.
pub fn clamp_invite_max_age(seconds: u32) -> u32 {
    seconds.min(MAX_INVITE_MAX_AGE_SECONDS)
}

/// Clamp an invite's use count. Zero means unlimited.
pub fn clamp_invite_max_uses(uses: u32) -> u32 {
    uses.min(MAX_INVITE_MAX_USES)
}

impl DiscordRest {
    /// Every invite pointing into this guild.
    pub async fn guild_invites(&self, guild_id: Id<GuildMarker>) -> Result<Vec<GuildInviteInfo>> {
        let invites: Vec<InviteBody> = self
            .send_json(
                self.raw_http.get(format!(
                    "https://discord.com/api/v9/guilds/{}/invites",
                    guild_id.get()
                )),
                "guild invites",
            )
            .await?;

        Ok(invites
            .into_iter()
            .map(|invite| GuildInviteInfo {
                code: invite.code,
                channel_id: invite.channel.as_ref().and_then(|channel| channel.id),
                channel_name: invite
                    .channel
                    .as_ref()
                    .and_then(|channel| channel.name.clone()),
                inviter: invite
                    .inviter
                    .and_then(|user| user.global_name.or(user.username)),
                uses: invite.uses,
                // Discord writes "no limit" as 0 in both fields; carrying that
                // through as a limit of zero would read as "already used up".
                max_uses: (invite.max_uses > 0).then_some(invite.max_uses),
                max_age_seconds: (invite.max_age > 0).then_some(invite.max_age),
                temporary: invite.temporary,
            })
            .collect())
    }

    /// Make an invite to a channel.
    pub async fn create_channel_invite(
        &self,
        channel_id: Id<ChannelMarker>,
        max_age_seconds: u32,
        max_uses: u32,
        temporary: bool,
    ) -> Result<String> {
        #[derive(Deserialize)]
        struct Created {
            code: String,
        }

        let created: Created = self
            .send_json(
                self.raw_http
                    .post(format!(
                        "https://discord.com/api/v9/channels/{}/invites",
                        channel_id.get()
                    ))
                    .json(&json!({
                        "max_age": clamp_invite_max_age(max_age_seconds),
                        "max_uses": clamp_invite_max_uses(max_uses),
                        "temporary": temporary,
                    })),
                "create invite",
            )
            .await?;

        Ok(created.code)
    }

    /// Revoke an invite so the code stops working.
    pub async fn revoke_invite(&self, code: &str) -> Result<()> {
        self.send_unit(
            self.raw_http
                .delete(format!("https://discord.com/api/v9/invites/{code}")),
            "revoke invite",
        )
        .await
    }

    /// The guild's custom emoji.
    pub async fn guild_emojis(&self, guild_id: Id<GuildMarker>) -> Result<Vec<GuildEmojiInfo>> {
        let emojis: Vec<EmojiBody> = self
            .send_json(
                self.raw_http.get(format!(
                    "https://discord.com/api/v9/guilds/{}/emojis",
                    guild_id.get()
                )),
                "guild emojis",
            )
            .await?;

        Ok(emojis
            .into_iter()
            .filter_map(|emoji| {
                // An emoji with no id is not addressable, so nothing here could
                // rename or delete it.
                Some(GuildEmojiInfo {
                    id: emoji.id?,
                    name: emoji.name.unwrap_or_default(),
                    animated: emoji.animated,
                    role_restricted: !emoji.roles.is_empty(),
                })
            })
            .collect())
    }

    pub async fn rename_emoji(
        &self,
        guild_id: Id<GuildMarker>,
        emoji_id: Id<EmojiMarker>,
        name: &str,
    ) -> Result<()> {
        self.send_unit(
            self.raw_http
                .patch(format!(
                    "https://discord.com/api/v9/guilds/{}/emojis/{}",
                    guild_id.get(),
                    emoji_id.get()
                ))
                .json(&json!({ "name": name })),
            "rename emoji",
        )
        .await
    }

    pub async fn delete_emoji(
        &self,
        guild_id: Id<GuildMarker>,
        emoji_id: Id<EmojiMarker>,
    ) -> Result<()> {
        self.send_unit(
            self.raw_http.delete(format!(
                "https://discord.com/api/v9/guilds/{}/emojis/{}",
                guild_id.get(),
                emoji_id.get()
            )),
            "delete emoji",
        )
        .await
    }

    /// What has been done in this guild lately.
    pub async fn guild_audit_log(
        &self,
        guild_id: Id<GuildMarker>,
    ) -> Result<Vec<AuditLogEntryInfo>> {
        let log: AuditLogBody = self
            .send_json(
                self.raw_http.get(format!(
                    "https://discord.com/api/v9/guilds/{}/audit-logs",
                    guild_id.get()
                )),
                "guild audit log",
            )
            .await?;

        // Names arrive alongside the entries rather than inside them, so the
        // actor of every entry has to be looked up in the same response.
        let name_of = |user_id: Id<UserMarker>| -> Option<String> {
            log.users
                .iter()
                .find(|user| user.id == user_id)
                .and_then(|user| user.global_name.clone().or_else(|| user.username.clone()))
        };

        Ok(log
            .audit_log_entries
            .iter()
            .filter_map(|entry| {
                Some(AuditLogEntryInfo {
                    id: entry.id?,
                    actor: entry.user_id.and_then(name_of),
                    action: AuditLogAction::from_code(entry.action_type),
                    // The target id is a snowflake for whatever kind of thing
                    // the action was about, so it is resolved as a user when
                    // it is one and left as an id when it is not.
                    target: entry.target_id.as_ref().and_then(|target| {
                        target
                            .parse::<u64>()
                            .ok()
                            .and_then(|raw| name_of(Id::new(raw)))
                            .or_else(|| Some(target.clone()))
                    }),
                    reason: entry.reason.clone(),
                })
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invite_limits_are_clamped_to_what_discord_accepts() {
        assert_eq!(
            clamp_invite_max_age(MAX_INVITE_MAX_AGE_SECONDS + 1),
            MAX_INVITE_MAX_AGE_SECONDS
        );
        assert_eq!(clamp_invite_max_uses(u32::MAX), MAX_INVITE_MAX_USES);

        // Zero is meaningful in both - "never expires" and "unlimited uses" -
        // so it must survive rather than being read as unset.
        assert_eq!(clamp_invite_max_age(0), 0);
        assert_eq!(clamp_invite_max_uses(0), 0);

        // And an ordinary value is left alone.
        assert_eq!(clamp_invite_max_age(3600), 3600);
        assert_eq!(clamp_invite_max_uses(5), 5);
    }

    #[test]
    fn known_audit_actions_are_named_and_unknown_ones_survive() {
        assert_eq!(AuditLogAction::from_code(20), AuditLogAction::MemberKick);
        assert_eq!(AuditLogAction::from_code(22), AuditLogAction::MemberBanAdd);

        // Discord keeps adding action types. An unrecognised one keeps its
        // number rather than vanishing, because "somebody did something" is
        // still worth showing in a log people read to find out what happened.
        let unknown = AuditLogAction::from_code(999);
        assert_eq!(unknown, AuditLogAction::Other(999));
        assert!(unknown.label().contains("999"));
    }

    #[test]
    fn every_named_action_says_something_useful() {
        for code in [
            1, 10, 11, 12, 20, 22, 23, 24, 25, 30, 31, 32, 40, 42, 60, 61, 62, 72,
        ] {
            let label = AuditLogAction::from_code(code);
            assert!(
                !matches!(label, AuditLogAction::Other(_)),
                "action {code} should be named"
            );
            assert!(!label.label().is_empty());
        }
    }
}
