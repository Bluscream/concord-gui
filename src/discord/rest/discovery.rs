//! Server discovery: finding a server without an invite.
//!
//! The browsing half rather than the owner-facing metadata. Everything else in
//! this client needs someone to hand you a link first; this is the one way in
//! that does not.

use serde::Deserialize;

use crate::Result;
use crate::discord::ids::{Id, marker::GuildMarker};

use super::DiscordRest;

/// Discord's cap on one page of results.
pub const MAX_DISCOVERY_LIMIT: u16 = 48;

/// One server as discovery lists it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiscoverableGuild {
    pub id: Id<GuildMarker>,
    pub name: String,
    pub description: Option<String>,
    /// Roughly how many people are in it. Approximate by Discord's own
    /// admission - the field is named for it - so the client says "about".
    pub approximate_member_count: Option<u64>,
    pub approximate_presence_count: Option<u64>,
    /// The server's vanity invite, when it has one.
    ///
    /// Joining goes through the ordinary invite path rather than a discovery
    /// endpoint of its own: that path is already written and tested, and
    /// guessing at a second one would ship something that silently fails.
    /// A server without a vanity code simply cannot be joined from here yet,
    /// which the row says.
    pub vanity_url_code: Option<String>,
}

impl DiscoverableGuild {
    /// Whether this server can be joined from the list.
    pub fn is_joinable(&self) -> bool {
        self.vanity_url_code.is_some()
    }

    /// The line under the name.
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        match (
            self.approximate_member_count,
            self.approximate_presence_count,
        ) {
            (Some(members), Some(online)) => {
                parts.push(format!("about {members} members, {online} online"));
            }
            (Some(members), None) => parts.push(format!("about {members} members")),
            // Said rather than left out: a server with no counts is one
            // Discord did not report them for, not an empty one.
            (None, _) => parts.push("member count unknown".to_owned()),
        }
        if !self.is_joinable() {
            // Said plainly rather than by a greyed button with no reason: this
            // is a limit of the client, not of the server.
            parts.push("no invite link - open it in a browser to join".to_owned());
        }
        if let Some(description) = &self.description
            && !description.is_empty()
        {
            parts.push(description.clone());
        }
        parts.join(" - ")
    }
}

#[derive(Deserialize)]
struct DiscoveryResponse {
    #[serde(default)]
    guilds: Vec<GuildBody>,
    /// Some Discord builds answer with a bare array instead; handled by the
    /// caller rather than here.
    #[serde(default)]
    hits: Option<Vec<GuildBody>>,
}

#[derive(Deserialize)]
struct GuildBody {
    id: Option<String>,
    name: Option<String>,
    description: Option<String>,
    approximate_member_count: Option<u64>,
    approximate_presence_count: Option<u64>,
    vanity_url_code: Option<String>,
}

fn to_guilds(bodies: Vec<GuildBody>) -> Vec<DiscoverableGuild> {
    bodies
        .into_iter()
        .filter_map(|body| {
            // Without an id there is nothing to join, so the row could not do
            // the one thing this list is for.
            let id = body.id?.parse::<u64>().ok()?;
            Some(DiscoverableGuild {
                id: Id::new(id),
                name: body.name.unwrap_or_default(),
                description: body.description.filter(|text| !text.is_empty()),
                approximate_member_count: body.approximate_member_count,
                approximate_presence_count: body.approximate_presence_count,
                vanity_url_code: body.vanity_url_code.filter(|code| !code.is_empty()),
            })
        })
        .collect()
}

impl DiscordRest {
    /// Servers Discord will show to anyone, optionally matching a query.
    pub async fn discoverable_guilds(&self, query: &str) -> Result<Vec<DiscoverableGuild>> {
        let query = query.trim();
        let url = if query.is_empty() {
            format!("https://discord.com/api/v9/discoverable-guilds?limit={MAX_DISCOVERY_LIMIT}")
        } else {
            format!(
                "https://discord.com/api/v9/discoverable-guilds?limit={MAX_DISCOVERY_LIMIT}&query={}",
                urlencoding(query)
            )
        };

        let response: DiscoveryResponse =
            self.send_json(self.raw_http.get(url), "discovery").await?;
        // Discord has answered under both names; taking whichever is present
        // beats picking one and finding out later which build we are talking to.
        Ok(to_guilds(response.hits.unwrap_or(response.guilds)))
    }
}

/// Percent-encode a query for a URL.
///
/// Deliberately strict: the allow-list is RFC 3986's unreserved set, so a
/// character nobody thought about is encoded rather than passed through and
/// silently ending the query.
fn urlencoding(value: &str) -> String {
    let mut out = String::new();
    for byte in value.as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(*byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guild(members: Option<u64>, online: Option<u64>) -> DiscoverableGuild {
        DiscoverableGuild {
            id: Id::new(1),
            name: "Rustaceans".to_owned(),
            description: Some("We talk about Rust".to_owned()),
            approximate_member_count: members,
            approximate_presence_count: online,
            vanity_url_code: Some("rustaceans".to_owned()),
        }
    }

    #[test]
    fn counts_are_described_as_approximate() {
        // Discord's own field says approximate, and a bare number reads as
        // exact - which it is not, sometimes by thousands.
        assert!(guild(Some(1200), Some(80)).summary().contains("about 1200"));
        assert!(guild(Some(1200), Some(80)).summary().contains("80 online"));
    }

    #[test]
    fn a_server_with_no_counts_says_so_rather_than_showing_nothing() {
        // Discord did not report them; the server is not empty.
        assert!(guild(None, None).summary().contains("unknown"));
    }

    #[test]
    fn a_query_cannot_break_out_of_the_url() {
        // A search containing & or = would otherwise start a parameter of its
        // own, and the results would quietly be for something else.
        let encoded = urlencoding("rust & friends=1");
        assert!(!encoded.contains('&'));
        assert!(!encoded.contains('='));
        assert!(encoded.contains("%26"));
    }

    #[test]
    fn an_ordinary_query_is_left_readable() {
        assert_eq!(urlencoding("rust"), "rust");
    }

    #[test]
    fn a_server_without_a_vanity_code_says_why_it_cannot_be_joined() {
        // A greyed button with no reason reads as a bug; this is a limit of
        // the client rather than of the server.
        let mut guild = guild(Some(10), Some(2));
        guild.vanity_url_code = None;

        assert!(!guild.is_joinable());
        assert!(guild.summary().contains("no invite link"));
        assert!(
            !self::guild(Some(10), Some(2))
                .summary()
                .contains("no invite link")
        );
    }

    #[test]
    fn a_guild_with_no_id_is_dropped_rather_than_shown_unjoinable() {
        let guilds = to_guilds(vec![
            GuildBody {
                id: None,
                name: Some("Nameless".to_owned()),
                description: None,
                approximate_member_count: None,
                approximate_presence_count: None,
                vanity_url_code: None,
            },
            GuildBody {
                id: Some("7".to_owned()),
                name: Some("Real".to_owned()),
                description: None,
                approximate_member_count: None,
                approximate_presence_count: None,
                vanity_url_code: None,
            },
        ]);

        assert_eq!(guilds.len(), 1);
        assert_eq!(guilds[0].name, "Real");
    }
}
