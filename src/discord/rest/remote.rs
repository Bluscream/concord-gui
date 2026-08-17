//! Fetching the values Discord serves.
//!
//! The reading half is `crate::discord::remote_config`; this is the half that
//! talks to Discord. Split because the reader is used on every connection and
//! must not need a client, while this needs one and runs rarely.

use serde::Deserialize;

use super::DiscordRest;

#[derive(Deserialize)]
struct GatewayBody {
    url: Option<String>,
}

impl DiscordRest {
    /// Refresh the cached values if they are stale.
    ///
    /// Failure is not an error worth surfacing: the cached or compiled-in
    /// values still work, and a client that refused to start because it could
    /// not check for a newer gateway URL would be worse than one using the
    /// URL that has worked for years.
    pub async fn refresh_remote_config(&self) {
        let mut config = crate::discord::remote_config::load();
        if !config.is_stale(std::time::SystemTime::now()) {
            return;
        }

        let fetched: Result<GatewayBody, _> = self
            .send_json(
                self.raw_http.get("https://discord.com/api/v9/gateway"),
                "gateway url",
            )
            .await;
        let Ok(body) = fetched else {
            // Left stale on purpose, so the next start tries again rather than
            // waiting a day to retry a request that failed once.
            return;
        };

        // An empty or non-websocket URL is refused rather than cached: writing
        // one would break every start until the cache expired, which is a far
        // worse outcome than not updating.
        if let Some(url) = body.url.filter(|url| url.starts_with("wss://")) {
            config.gateway_url = url;
        }
        config.mark_fetched(std::time::SystemTime::now());
        crate::discord::remote_config::store(&config);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_url_that_is_not_a_websocket_is_refused() {
        // Caching one would break every start until the cache expired, which
        // is worse than never updating at all.
        for served in ["", "https://gateway.discord.gg", "gateway.discord.gg"] {
            let body = GatewayBody {
                url: Some(served.to_owned()),
            };
            assert!(
                body.url.filter(|url| url.starts_with("wss://")).is_none(),
                "{served} was accepted"
            );
        }
    }

    #[test]
    fn a_websocket_url_is_accepted() {
        let body = GatewayBody {
            url: Some("wss://gateway.discord.gg".to_owned()),
        };
        assert!(body.url.filter(|url| url.starts_with("wss://")).is_some());
    }

    #[test]
    fn an_absent_url_leaves_the_cached_value_alone() {
        // Discord answering without one is not a reason to forget what works.
        let body: GatewayBody = serde_json::from_str("{}").expect("should parse");
        let mut config = crate::discord::RemoteConfig::default();
        let before = config.gateway_url.clone();
        if let Some(url) = body.url.filter(|url| url.starts_with("wss://")) {
            config.gateway_url = url;
        }
        assert_eq!(config.gateway_url, before);
    }
}
