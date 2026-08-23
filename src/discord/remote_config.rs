//! Values Discord serves rather than this client deciding.
//!
//! Three layers, in order: what Discord last told us and we wrote to disk, then
//! a fresh fetch, then a compiled-in default. The default is what Discord
//! serves today, so a first run with no network behaves exactly as it would
//! with one - it is a fallback, not a placeholder.
//!
//! The gateway URL is the reason this exists. Discord's own guidance is that
//! clients should cache it and only refetch when the cached one fails to
//! connect, which is a rule about *when* to fetch as much as what.

use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// What Discord serves today. Used until a fetch or a cache says otherwise.
///
/// These are not guesses: each is the value Discord currently returns, so a
/// client that never reaches the network is not a degraded one.
pub mod defaults {
    pub const GATEWAY_URL: &str = "wss://gateway.discord.gg";
}

/// How long a cached value is used before a refetch is attempted.
///
/// A day rather than a session: these change on the scale of Discord's
/// deployments, and refetching every start would spend a request on every run
/// to learn nothing.
const CACHE_LIFETIME: Duration = Duration::from_secs(24 * 60 * 60);

/// Values Discord serves, as last known.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(default)]
pub struct RemoteConfig {
    pub gateway_url: String,
    /// When this was fetched, as Unix seconds. Zero means "compiled in" - a
    /// default has no fetch time, and treating it as fetched-at-the-epoch is
    /// what makes it always stale and therefore always retried.
    pub fetched_at: u64,
}

impl Default for RemoteConfig {
    fn default() -> Self {
        Self {
            gateway_url: defaults::GATEWAY_URL.to_owned(),
            fetched_at: 0,
        }
    }
}

impl RemoteConfig {
    /// Whether a refetch is due.
    ///
    /// A clock that has gone backwards reads as stale rather than as fresh
    /// forever: retrying costs one request, and not retrying could pin a
    /// client to a dead gateway.
    pub fn is_stale(&self, now: SystemTime) -> bool {
        let Some(fetched) = UNIX_EPOCH.checked_add(Duration::from_secs(self.fetched_at)) else {
            return true;
        };
        now.duration_since(fetched)
            .map_or(true, |age| age >= CACHE_LIFETIME)
    }

    /// The websocket URL to connect to, with the query Discord expects.
    ///
    /// Discord returns a bare host, so the parameters are added here rather
    /// than stored - they belong to this client's protocol version, not to
    /// what Discord served.
    pub fn gateway_websocket_url(&self, query: &str) -> String {
        let base = self.gateway_url.trim_end_matches('/');
        format!("{base}/?{query}")
    }

    pub fn mark_fetched(&mut self, now: SystemTime) {
        self.fetched_at = now
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_secs());
    }
}

fn cache_path() -> Option<PathBuf> {
    // State rather than config: nobody edits this, and it should not follow a
    // dotfiles repository between machines.
    Some(crate::support::paths::state_dir()?.join("remote-config.toml"))
}

/// Read the cached values, or the compiled-in ones.
///
/// A cache that cannot be read or parsed is treated as absent rather than as
/// an error: the defaults are correct, and refusing to start over a corrupt
/// cache would be a worse outcome than refetching.
pub fn load() -> RemoteConfig {
    let Some(path) = cache_path() else {
        return RemoteConfig::default();
    };
    let Ok(text) = std::fs::read_to_string(path) else {
        return RemoteConfig::default();
    };
    toml::from_str(&text).unwrap_or_default()
}

/// Write the values back. Failure is ignored on purpose - not caching costs a
/// request next time, which is not worth failing a login over.
pub fn store(config: &RemoteConfig) {
    let Some(path) = cache_path() else {
        return;
    };
    if let Some(parent) = path.parent()
        && std::fs::create_dir_all(parent).is_err()
    {
        return;
    }
    if let Ok(text) = toml::to_string_pretty(config) {
        let _ = std::fs::write(path, text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_compiled_in_default_is_always_stale() {
        // Zero means "never fetched", so the first run always tries once. A
        // default that read as fresh would never be replaced.
        assert!(RemoteConfig::default().is_stale(SystemTime::now()));
    }

    #[test]
    fn a_recent_fetch_is_not_stale_and_an_old_one_is() {
        let now = UNIX_EPOCH + Duration::from_secs(1_000_000);
        let mut config = RemoteConfig::default();
        config.mark_fetched(now);

        assert!(!config.is_stale(now));
        assert!(!config.is_stale(now + CACHE_LIFETIME - Duration::from_secs(1)));
        assert!(config.is_stale(now + CACHE_LIFETIME));
    }

    #[test]
    fn a_clock_that_went_backwards_reads_as_stale() {
        // Retrying costs one request. Reading it as fresh forever could pin a
        // client to a gateway that has since moved.
        let now = UNIX_EPOCH + Duration::from_secs(1_000_000);
        let mut config = RemoteConfig::default();
        config.mark_fetched(now);

        assert!(config.is_stale(now - Duration::from_secs(60)));
    }

    #[test]
    fn the_websocket_url_survives_a_trailing_slash() {
        // Discord has served both forms, and a doubled slash is a URL that
        // fails to connect for a reason nobody would look for here.
        let query = "v=9&encoding=json";
        for served in ["wss://gateway.discord.gg", "wss://gateway.discord.gg/"] {
            let config = RemoteConfig {
                gateway_url: served.to_owned(),
                ..RemoteConfig::default()
            };
            assert_eq!(
                config.gateway_websocket_url(query),
                "wss://gateway.discord.gg/?v=9&encoding=json",
                "for {served}"
            );
        }
    }

    #[test]
    fn the_default_gateway_is_what_the_client_used_before_this_existed() {
        // The point of the fallback: a first run with no network behaves as it
        // did when the URL was compiled in, rather than failing differently.
        let url = RemoteConfig::default().gateway_websocket_url("v=9");
        assert!(url.starts_with("wss://gateway.discord.gg/?"));
    }

    #[test]
    fn a_corrupt_cache_reads_as_the_defaults_rather_than_failing() {
        // Refusing to start over an unreadable cache would be worse than
        // spending one request to refetch.
        let parsed: RemoteConfig = toml::from_str("this is not toml =").unwrap_or_default();
        assert_eq!(parsed, RemoteConfig::default());
    }

    #[test]
    fn a_partial_cache_keeps_the_defaults_for_what_is_missing() {
        // A cache written by an older build has fewer fields; the ones it does
        // not mention must not become zero. A zero `fetched_at` in particular
        // reads as never fetched, which is the harmless direction - it costs
        // one request rather than pinning the client to a stale value.
        let parsed: RemoteConfig =
            toml::from_str(r#"gateway_url = "wss://other.example""#).expect("should parse");

        assert_eq!(parsed.gateway_url, "wss://other.example");
        assert_eq!(parsed.fetched_at, 0);
        assert!(parsed.is_stale(SystemTime::now()));
    }
}
