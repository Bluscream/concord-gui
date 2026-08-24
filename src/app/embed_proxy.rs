//! Resolving a link that Discord did not unfurl.
//!
//! Discord only embeds what its own crawler reached. A private site, a page
//! behind Cloudflare, a link posted while its server was down - all arrive
//! with no embed, and the client cannot make one because doing so means
//! fetching an arbitrary address a stranger put in a message.
//!
//! An embed proxy is the way out: a service that does the fetching and reports
//! only what it found. Which service is the user's choice, and leaving it
//! empty turns the whole thing off - because using one means telling a third
//! party which links are being read, and that is a cost somebody may not want
//! to pay.

use std::time::Duration;

use crate::discord::{
    AppEvent, EmbedInfo, Id,
    marker::{ChannelMarker, MessageMarker},
};

/// Percent-encode a URL so it survives being a query parameter.
///
/// Written out rather than pulling in a crate for it: the unreserved set is
/// four lines, and a dependency for four lines is a poor trade.
fn percent_encode(url: &str) -> String {
    let mut out = String::with_capacity(url.len() * 3);
    for byte in url.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// How long to wait on the proxy.
///
/// Short: this is decoration on a message that is already readable, and a
/// preview that arrives a minute later has missed the moment it was for.
const TIMEOUT: Duration = Duration::from_secs(8);

/// Ask the proxy what a link is, and turn the answer into an embed.
pub(super) async fn resolve(
    client: crate::discord::DiscordClient,
    proxy: String,
    channel_id: Id<ChannelMarker>,
    message_id: Id<MessageMarker>,
    url: String,
) {
    if proxy.trim().is_empty() {
        return;
    }

    let request = format!("{}{}", proxy.trim(), percent_encode(&url));
    let fetched = tokio::time::timeout(TIMEOUT, fetch(&request)).await;

    match fetched {
        Err(_) => {
            fail(&client, url, "the embed proxy did not answer in time").await;
        }
        Ok(Err(error)) => {
            fail(&client, url, &error).await;
        }
        Ok(Ok(None)) => {
            // Answered, but had nothing to say about the page. Not a failure
            // worth reporting: most links have no preview and never will.
        }
        Ok(Ok(Some(embed))) => {
            client
                .publish_event(AppEvent::EmbedResolved {
                    channel_id,
                    message_id,
                    embed: Box::new(embed),
                })
                .await;
        }
    }
}

async fn fail(client: &crate::discord::DiscordClient, url: String, message: &str) {
    client
        .publish_event(AppEvent::EmbedResolveFailed {
            url,
            message: message.to_owned(),
        })
        .await;
}

/// Read the proxy's answer.
///
/// Written against the shape microlink and its work-alikes return - a `data`
/// object with `title`, `description`, `image` and `publisher` - but only by
/// looking for those keys, so a proxy that answers in the same shape works
/// without this knowing its name.
async fn fetch(request: &str) -> Result<Option<EmbedInfo>, String> {
    let response = reqwest::get(request)
        .await
        .map_err(|error| format!("the embed proxy could not be reached: {error}"))?;

    if !response.status().is_success() {
        return Err(format!("the embed proxy answered {}", response.status()));
    }

    let body: serde_json::Value = response
        .json()
        .await
        .map_err(|error| format!("the embed proxy's answer was not JSON: {error}"))?;

    // Some proxies wrap their answer in `data`, others do not.
    let data = body.get("data").unwrap_or(&body);
    Ok(embed_from(data))
}

/// Build an embed from whatever of the expected keys are present.
fn embed_from(data: &serde_json::Value) -> Option<EmbedInfo> {
    let text = |key: &str| {
        data.get(key)
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
            .filter(|value| !value.is_empty())
    };

    // An image may be a string or an object with a `url`, depending on the
    // proxy; both are common enough to be worth accepting.
    let nested_url = |key: &str| {
        data.get(key).and_then(|value| match value {
            serde_json::Value::String(url) if !url.is_empty() => Some(url.clone()),
            serde_json::Value::Object(_) => value
                .get("url")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned),
            _ => None,
        })
    };

    let title = text("title");
    let description = text("description");
    let image = nested_url("image");

    // Nothing worth showing. An embed with only a URL in it is a worse
    // version of the link that is already in the message.
    if title.is_none() && description.is_none() && image.is_none() {
        return None;
    }

    Some(EmbedInfo {
        provider_name: text("publisher").or_else(|| text("provider")),
        author_name: text("author"),
        title,
        description,
        url: text("url"),
        image_url: image,
        thumbnail_url: nested_url("logo"),
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::{embed_from, percent_encode};

    #[test]
    fn an_answer_with_nothing_in_it_makes_no_embed() {
        // Most links have no preview. An embed carrying only the address is a
        // worse version of the link already in the message, so it is better
        // not to make one at all.
        let empty = serde_json::json!({ "url": "https://example.com" });
        assert!(embed_from(&empty).is_none());

        let blank = serde_json::json!({ "title": "", "description": "" });
        assert!(embed_from(&blank).is_none());
    }

    #[test]
    fn an_image_is_taken_whether_it_is_a_string_or_an_object() {
        // Proxies differ here and both shapes are common; accepting one and
        // silently dropping the other would look like a proxy with no images.
        let object = serde_json::json!({
            "title": "a page",
            "image": { "url": "https://example.com/a.png" }
        });
        assert_eq!(
            embed_from(&object).and_then(|embed| embed.image_url),
            Some("https://example.com/a.png".to_owned())
        );

        let string = serde_json::json!({
            "title": "a page",
            "image": "https://example.com/b.png"
        });
        assert_eq!(
            embed_from(&string).and_then(|embed| embed.image_url),
            Some("https://example.com/b.png".to_owned())
        );
    }

    #[test]
    fn the_target_is_encoded_so_it_survives_being_a_query_value() {
        // Unencoded, everything after the first `&` or `?` in the target
        // becomes a parameter of the proxy's own URL instead of part of the
        // address being asked about.
        assert_eq!(
            percent_encode("https://a.test/x?y=1&z=2"),
            "https%3A%2F%2Fa.test%2Fx%3Fy%3D1%26z%3D2"
        );
        assert_eq!(percent_encode("safe-._~"), "safe-._~");
    }

    #[test]
    fn the_pieces_that_are_present_are_carried_across() {
        let answer = serde_json::json!({
            "title": "concord",
            "description": "A Discord client in Rust.",
            "publisher": "github.com",
            "url": "https://github.com/bluscream/concord"
        });
        let embed = embed_from(&answer).expect("this has plenty to show");
        assert_eq!(embed.title.as_deref(), Some("concord"));
        assert_eq!(embed.provider_name.as_deref(), Some("github.com"));
        assert_eq!(
            embed.description.as_deref(),
            Some("A Discord client in Rust.")
        );
    }
}

/// Every plain link in a message body, in the order they appear.
///
/// Shared by both front ends so a link the GUI asks the proxy about is the
/// same one the TUI asks about. The GUI's markdown parser finds links too,
/// but it finds them for drawing - masked links and autolinks alike - and a
/// proxy lookup wants only the address, deduplicated.
///
/// `<https://...>` is Discord's "do not unfurl" form. It is skipped for the
/// same reason Discord skips it: the author asked for no preview, and a proxy
/// that ignored that would override them.
pub fn links_in(content: &str) -> Vec<String> {
    let bytes = content.as_bytes();
    let mut found: Vec<String> = Vec::new();
    let mut at = 0;

    while let Some(offset) = content[at..].find("http") {
        let start = at + offset;
        let rest = &content[start..];
        if !rest.starts_with("https://") && !rest.starts_with("http://") {
            at = start + 4;
            continue;
        }

        let end = start
            + rest
                .find(|c: char| c.is_whitespace() || c == '>' || c == ')' || c == '<')
                .unwrap_or(rest.len());
        // Sentence punctuation runs into a bare link; Discord drops it too.
        let url = content[start..end].trim_end_matches(['.', ',', '!', '?', ';', ':', '"', '\'']);

        let suppressed = start > 0 && bytes[start - 1] == b'<';
        if !suppressed && url.len() > 8 && !found.iter().any(|seen| seen == url) {
            found.push(url.to_owned());
        }
        at = end.max(start + 1);
    }

    found
}

#[cfg(test)]
mod link_tests {
    use super::links_in;

    #[test]
    fn finds_bare_links_and_drops_trailing_punctuation() {
        assert_eq!(
            links_in("see https://example.com/a, and http://b.test/x."),
            vec!["https://example.com/a", "http://b.test/x"]
        );
    }

    #[test]
    fn skips_suppressed_links_and_repeats() {
        assert_eq!(
            links_in("<https://quiet.test/x> https://loud.test/y https://loud.test/y"),
            vec!["https://loud.test/y"]
        );
    }

    #[test]
    fn finds_the_target_of_a_masked_link() {
        assert_eq!(
            links_in("[label](https://example.com/page)"),
            vec!["https://example.com/page"]
        );
    }

    #[test]
    fn ignores_text_that_merely_starts_with_http() {
        assert!(links_in("httpsomething and http-only").is_empty());
    }
}
