//! Kicking, banning and role assignment.
//!
//! Endpoints cross-checked against Abaddon, which is the only surveyed
//! third-party client with working moderation.

use serde::Deserialize;
use serde_json::json;

use crate::Result;
use crate::discord::ids::{
    Id,
    marker::{GuildMarker, RoleMarker, UserMarker},
};

use super::DiscordRest;

/// Discord's cap on how much of a banned user's history can be purged.
pub const MAX_BAN_DELETE_MESSAGE_SECONDS: u32 = 604_800;

/// One entry in a guild's ban list.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GuildBanInfo {
    pub user_id: Id<UserMarker>,
    pub username: String,
    /// The moderator's stated reason, when one was given.
    pub reason: Option<String>,
}

#[derive(Deserialize)]
struct BulkBanBody {
    /// Absent when everything succeeded, which is why this is optional rather
    /// than defaulted to empty - an empty list would read as nothing banned.
    banned_users: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct BanBody {
    user: Option<BanUser>,
    reason: Option<String>,
}

#[derive(Deserialize)]
struct BanUser {
    id: Option<Id<UserMarker>>,
    username: Option<String>,
    global_name: Option<String>,
}

impl DiscordRest {
    /// Remove a member from a guild. They can rejoin with a new invite.
    pub async fn kick_member(
        &self,
        guild_id: Id<GuildMarker>,
        user_id: Id<UserMarker>,
    ) -> Result<()> {
        self.send_unit(
            self.raw_http.delete(format!(
                "https://discord.com/api/v9/guilds/{}/members/{}",
                guild_id.get(),
                user_id.get()
            )),
            "kick member",
        )
        .await
    }

    /// Ban a member, optionally deleting their recent messages.
    ///
    /// `delete_message_seconds` is clamped rather than rejected: the value
    /// comes from a UI choice, and Discord refuses the whole request if it is
    /// over the maximum.
    pub async fn ban_member(
        &self,
        guild_id: Id<GuildMarker>,
        user_id: Id<UserMarker>,
        delete_message_seconds: u32,
    ) -> Result<()> {
        self.send_unit(
            self.raw_http
                .put(format!(
                    "https://discord.com/api/v9/guilds/{}/bans/{}",
                    guild_id.get(),
                    user_id.get()
                ))
                .json(&json!({
                    "delete_message_seconds": delete_message_seconds
                        .min(MAX_BAN_DELETE_MESSAGE_SECONDS),
                })),
            "ban member",
        )
        .await
    }

    /// Discord's cap on one bulk-ban request.
    ///
    /// More than this is refused outright, so the caller sends batches rather
    /// than losing the whole request.
    pub const MAX_BULK_BAN_USERS: usize = 200;

    /// Ban several people at once.
    ///
    /// Discord reports each outcome separately: it bans who it can and returns
    /// the rest as failures, so a partial success is the normal case rather
    /// than an error. The count banned is returned for that reason.
    pub async fn bulk_ban(
        &self,
        guild_id: Id<GuildMarker>,
        user_ids: &[Id<UserMarker>],
        delete_message_seconds: u32,
    ) -> Result<usize> {
        if user_ids.is_empty() {
            return Ok(0);
        }

        let ids: Vec<String> = user_ids
            .iter()
            .take(Self::MAX_BULK_BAN_USERS)
            .map(|id| id.get().to_string())
            .collect();
        let attempted = ids.len();

        let response: BulkBanBody = self
            .send_json(
                self.raw_http
                    .post(format!(
                        "https://discord.com/api/v9/guilds/{}/bulk-ban",
                        guild_id.get()
                    ))
                    .json(&json!({
                        "user_ids": ids,
                        "delete_message_seconds": delete_message_seconds
                            .min(MAX_BAN_DELETE_MESSAGE_SECONDS),
                    })),
                "bulk ban",
            )
            .await?;

        // Discord omits the list when everything succeeded, so an absent field
        // means all of them rather than none.
        Ok(response
            .banned_users
            .map_or(attempted, |banned| banned.len()))
    }

    /// The guild's ban list.
    ///
    /// Without this a ban is a one-way door: `unban_member` needs a user id,
    /// and nothing else in the client knows who is banned.
    pub async fn guild_bans(&self, guild_id: Id<GuildMarker>) -> Result<Vec<GuildBanInfo>> {
        let bans: Vec<BanBody> = self
            .send_json(
                self.raw_http.get(format!(
                    "https://discord.com/api/v9/guilds/{}/bans",
                    guild_id.get()
                )),
                "guild bans",
            )
            .await?;

        Ok(bans
            .into_iter()
            .filter_map(|ban| {
                let user = ban.user?;
                Some(GuildBanInfo {
                    // An entry without a user id cannot be unbanned, so it is
                    // dropped rather than shown as an un-actionable row.
                    user_id: user.id?,
                    username: user
                        .global_name
                        .or(user.username)
                        .unwrap_or_else(|| "unknown".to_owned()),
                    reason: ban.reason,
                })
            })
            .collect())
    }

    pub async fn unban_member(
        &self,
        guild_id: Id<GuildMarker>,
        user_id: Id<UserMarker>,
    ) -> Result<()> {
        self.send_unit(
            self.raw_http.delete(format!(
                "https://discord.com/api/v9/guilds/{}/bans/{}",
                guild_id.get(),
                user_id.get()
            )),
            "unban member",
        )
        .await
    }

    /// Replace a member's roles.
    ///
    /// The whole set is sent rather than a single add or remove. Discord has
    /// per-role endpoints, but the official client does not use them and
    /// neither does Abaddon; sending the set avoids a race where two edits
    /// each drop the other's change.
    pub async fn set_member_roles(
        &self,
        guild_id: Id<GuildMarker>,
        user_id: Id<UserMarker>,
        role_ids: &[Id<RoleMarker>],
    ) -> Result<()> {
        let roles: Vec<String> = role_ids.iter().map(|role| role.get().to_string()).collect();

        self.send_unit(
            self.raw_http
                .patch(format!(
                    "https://discord.com/api/v9/guilds/{}/members/{}",
                    guild_id.get(),
                    user_id.get()
                ))
                .json(&json!({ "roles": roles })),
            "set member roles",
        )
        .await
    }

    /// Time a member out, or clear a timeout by passing `None`.
    ///
    /// Discord expects an RFC 3339 instant rather than a duration, so the
    /// caller's minutes are resolved against the current time here.
    pub async fn timeout_member(
        &self,
        guild_id: Id<GuildMarker>,
        user_id: Id<UserMarker>,
        minutes: Option<u32>,
    ) -> Result<()> {
        let until = minutes.map(|minutes| {
            (chrono::Utc::now() + chrono::Duration::minutes(i64::from(minutes))).to_rfc3339()
        });

        self.send_unit(
            self.raw_http
                .patch(format!(
                    "https://discord.com/api/v9/guilds/{}/members/{}",
                    guild_id.get(),
                    user_id.get()
                ))
                // Null clears the timeout; omitting the field would leave it.
                .json(&json!({ "communication_disabled_until": until })),
            "timeout member",
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The clamp `ban_member` applies before sending.
    fn clamped(seconds: u32) -> u32 {
        seconds.min(MAX_BAN_DELETE_MESSAGE_SECONDS)
    }

    #[test]
    fn ban_history_purge_is_clamped_to_what_discord_accepts() {
        // Over the maximum, Discord rejects the whole request rather than
        // trimming it, so a UI offering "all time" must not send it verbatim.
        assert_eq!(clamped(u32::MAX), MAX_BAN_DELETE_MESSAGE_SECONDS);
        assert_eq!(
            clamped(MAX_BAN_DELETE_MESSAGE_SECONDS + 1),
            MAX_BAN_DELETE_MESSAGE_SECONDS
        );

        // Values within range pass through, including zero - "ban but keep
        // their messages" is the default and must not be rewritten.
        assert_eq!(clamped(0), 0);
        assert_eq!(clamped(3_600), 3_600);

        // Seven days is the documented maximum.
        assert_eq!(MAX_BAN_DELETE_MESSAGE_SECONDS, 7 * 24 * 60 * 60);
    }
}

#[cfg(test)]
mod bulk_ban_tests {
    use super::*;

    #[test]
    fn an_absent_banned_list_means_everyone_rather_than_nobody() {
        // Discord omits the field when nothing failed. Reading that as an
        // empty list would report a successful mass ban as having banned
        // nobody, which is the most alarming way to be wrong about it.
        let body: BulkBanBody = serde_json::from_str("{}").expect("should parse");
        assert!(body.banned_users.is_none());

        let body: BulkBanBody =
            serde_json::from_str(r#"{"banned_users":["1","2"]}"#).expect("should parse");
        assert_eq!(body.banned_users.map(|users| users.len()), Some(2));
    }

    #[test]
    fn the_batch_cap_is_the_one_discord_enforces() {
        // Over it the whole request is refused, so the caller batches rather
        // than losing every ban in it.
        assert_eq!(DiscordRest::MAX_BULK_BAN_USERS, 200);
    }
}

/// Read a typed list of user ids for a bulk ban.
///
/// Separators are deliberately loose - spaces, commas, newlines - because the
/// list is usually pasted from somewhere else, and a paste that fails on its
/// delimiter is a paste that gets retyped by hand.
///
/// In the core so both clients accept exactly the same input.
pub fn parse_user_id_list(text: &str) -> Vec<Id<UserMarker>> {
    let mut seen = std::collections::BTreeSet::new();
    text.split(|c: char| !c.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .filter_map(|part| part.parse::<u64>().ok())
        // Zero is not a snowflake, and Discord refuses the whole request over
        // one bad id rather than skipping it.
        .filter(|id| *id != 0)
        .filter(|id| seen.insert(*id))
        .map(Id::new)
        .collect()
}

#[cfg(test)]
mod user_id_list_tests {
    use super::*;

    #[test]
    fn any_reasonable_separator_works() {
        // The list is usually pasted, and a paste that fails on its delimiter
        // is a paste that gets retyped by hand.
        let expected = vec![Id::new(1), Id::new(2), Id::new(3)];
        for text in ["1 2 3", "1,2,3", "1\n2\n3", "1, 2,  3", "<@1> <@2> <@3>"] {
            assert_eq!(parse_user_id_list(text), expected, "failed on {text:?}");
        }
    }

    #[test]
    fn a_repeated_id_is_listed_once() {
        // Discord counts the request against its cap by length, so duplicates
        // cost room that a real id could have used.
        assert_eq!(parse_user_id_list("7 7 7"), vec![Id::new(7)]);
    }

    #[test]
    fn order_is_kept_so_the_list_reads_as_typed() {
        assert_eq!(
            parse_user_id_list("3 1 2"),
            vec![Id::new(3), Id::new(1), Id::new(2)]
        );
    }

    #[test]
    fn nothing_usable_yields_nothing_rather_than_a_bogus_id() {
        // Discord refuses the whole request over one bad id, so a stray word
        // must not become a zero that takes the rest down with it.
        assert!(parse_user_id_list("").is_empty());
        assert!(parse_user_id_list("nobody here").is_empty());
        assert!(parse_user_id_list("0").is_empty());
    }
}
