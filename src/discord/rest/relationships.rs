//! Friends, friend requests and blocks.
//!
//! Endpoints cross-checked against Abaddon, endcord and discordgo, which agree
//! on all four. They disagree only on how a friend request names its target,
//! because all three predate pomelo - see [`friend_request_target`].
//!
//! Rule 6 applies to everything here: managing the friends list is one of the
//! actions Discord's anti-spam checks watch, so front ends warn before it.

use serde_json::{Value, json};

use crate::Result;
use crate::discord::ids::{Id, marker::UserMarker};

use super::DiscordRest;

/// Discord's relationship type for a block.
const RELATIONSHIP_TYPE_BLOCKED: u8 = 2;

/// How a friend request names its target.
///
/// Legacy names carried a four-digit discriminator after a `#`; pomelo names
/// have none, and sending a discriminator for one is rejected. Every surveyed
/// client sends the discriminator unconditionally because all of them predate
/// the change, so this is the one place their behaviour is deliberately not
/// copied.
pub fn friend_request_target(input: &str) -> Option<(String, Option<u16>)> {
    let input = input.trim().trim_start_matches('@');
    if input.is_empty() {
        return None;
    }

    match input.split_once('#') {
        Some((username, discriminator)) => {
            let username = username.trim();
            // A `#` with nothing usable after it is a typo, not a legacy name.
            let discriminator = discriminator.trim().parse::<u16>().ok()?;
            (!username.is_empty()).then(|| (username.to_owned(), Some(discriminator)))
        }
        None => Some((input.to_owned(), None)),
    }
}

impl DiscordRest {
    /// Ask to be someone's friend, by username.
    pub async fn send_friend_request(
        &self,
        username: &str,
        discriminator: Option<u16>,
    ) -> Result<()> {
        let mut body = json!({ "username": username });
        if let Some(discriminator) = discriminator
            && let Value::Object(fields) = &mut body
        {
            fields.insert(
                "discriminator".to_owned(),
                Value::from(discriminator.to_string()),
            );
        }

        self.send_unit(
            self.raw_http
                .post("https://discord.com/api/v9/users/@me/relationships")
                .json(&body),
            "send friend request",
        )
        .await
    }

    /// Accept an incoming request, or send one to a known user id.
    ///
    /// The same call does both, which is what the web client does: an empty
    /// body means "friend", and Discord works out which direction applies.
    pub async fn add_friend(&self, user_id: Id<UserMarker>) -> Result<()> {
        self.send_unit(
            self.raw_http
                .put(format!(
                    "https://discord.com/api/v9/users/@me/relationships/{}",
                    user_id.get()
                ))
                .json(&json!({})),
            "add friend",
        )
        .await
    }

    /// Block someone.
    ///
    /// Blocking replaces any existing relationship rather than requiring it to
    /// be removed first, so a friend can be blocked in one step.
    pub async fn block_user(&self, user_id: Id<UserMarker>) -> Result<()> {
        self.send_unit(
            self.raw_http
                .put(format!(
                    "https://discord.com/api/v9/users/@me/relationships/{}",
                    user_id.get()
                ))
                .json(&json!({ "type": RELATIONSHIP_TYPE_BLOCKED })),
            "block user",
        )
        .await
    }

    /// Drop a relationship entirely.
    ///
    /// One endpoint for four outcomes - unfriend, cancel an outgoing request,
    /// decline an incoming one, unblock - because Discord models all of them
    /// as removing the relationship. Front ends should still name the one they
    /// mean, since "remove" alone does not say which is about to happen.
    pub async fn remove_relationship(&self, user_id: Id<UserMarker>) -> Result<()> {
        self.send_unit(
            self.raw_http.delete(format!(
                "https://discord.com/api/v9/users/@me/relationships/{}",
                user_id.get()
            )),
            "remove relationship",
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pomelo_name_carries_no_discriminator() {
        assert_eq!(
            friend_request_target("someone"),
            Some(("someone".to_owned(), None))
        );
        // Pasted from a profile, where the name is shown with an @.
        assert_eq!(
            friend_request_target(" @someone "),
            Some(("someone".to_owned(), None))
        );
    }

    #[test]
    fn a_legacy_name_keeps_its_discriminator() {
        assert_eq!(
            friend_request_target("someone#0001"),
            Some(("someone".to_owned(), Some(1)))
        );
    }

    #[test]
    fn nothing_usable_is_refused_rather_than_sent() {
        // Sending these would spend a request to be told they are wrong, and
        // failed friend requests are exactly what the spam filter counts.
        assert_eq!(friend_request_target(""), None);
        assert_eq!(friend_request_target("   "), None);
        assert_eq!(friend_request_target("someone#"), None);
        assert_eq!(friend_request_target("someone#abcd"), None);
        assert_eq!(friend_request_target("#0001"), None);
    }
}
