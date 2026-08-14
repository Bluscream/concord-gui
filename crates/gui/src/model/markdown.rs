//! Discord-flavoured markdown parsing.
//!
//! Produces a flat string plus styled ranges, rather than nested elements, so
//! the renderer can hand it to GPUI's `StyledText` and get correct word
//! wrapping across style boundaries. Building a row of separately-styled
//! elements would wrap at segment boundaries instead, which looks wrong as
//! soon as a bold word lands near the end of a line.
//!
//! Supported: `**bold**`, `*italic*`, `_italic_`, `__underline__`,
//! `~~strike~~`, `` `code` ``, ```` ```block``` ````, `||spoiler||`,
//! `> quote`, mentions (`<@id>`, `<#id>`, `<@&id>`), custom emoji
//! (`<:name:id>`, `<a:name:id>`), timestamps (`<t:unix:style>`) and bare URLs.
//!
//! Deliberately not supported: nested blockquotes, lists, headings and tables.
//! Discord renders those, but they are rare in practice and a partial
//! implementation is worse than a clearly-scoped one.

use std::ops::Range;

/// How a run of text should be drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub code: bool,
    pub spoiler: bool,
    pub quote: bool,
    pub kind: Kind,
}

/// Semantic classification, which drives colour rather than weight.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Kind {
    #[default]
    Text,
    /// `<@id>` - a user mention.
    Mention,
    /// `<#id>` - a channel link.
    Channel,
    /// `<@&id>` - a role mention.
    Role,
    /// `<:name:id>` - a custom emoji, rendered as `:name:` until images land.
    Emoji,
    Url,
    /// `<t:unix:style>` - a rendered timestamp.
    Timestamp,
}

/// A parsed message body: display text plus the ranges that carry styling.
#[derive(Debug, Default)]
pub struct Parsed {
    pub text: String,
    pub runs: Vec<(Range<usize>, Style)>,
}

impl Parsed {
    fn push(&mut self, text: &str, style: Style) {
        if text.is_empty() {
            return;
        }
        let start = self.text.len();
        self.text.push_str(text);
        if style != Style::default() {
            self.runs.push((start..self.text.len(), style));
        }
    }
}

/// Parse a message body.
///
/// Fenced blocks are extracted *before* line splitting, because they span
/// lines - splitting first would tear them apart and leak the fences into the
/// rendered text.
pub fn parse(input: &str) -> Parsed {
    let mut out = Parsed::default();
    let mut rest = input;

    while let Some(start) = rest.find("```") {
        let (before, remainder) = rest.split_at(start);
        // An unterminated fence is literal text, not a block to end-of-message.
        let Some(end) = remainder[3..].find("```") else {
            break;
        };

        parse_lines(before, &mut out);

        let body = &remainder[3..3 + end];
        let body = match body.split_once('\n') {
            // ```rust\n... - drop a bare language tag.
            Some((first, tail)) if !first.is_empty() && !first.contains(' ') => tail,
            _ => body,
        };
        out.push(
            body,
            Style {
                code: true,
                ..Style::default()
            },
        );

        rest = &remainder[3 + end + 3..];
    }

    parse_lines(rest, &mut out);
    out
}

/// Line-oriented parsing for everything outside a fenced block.
fn parse_lines(input: &str, out: &mut Parsed) {
    if input.is_empty() {
        return;
    }

    for (index, line) in input.split('\n').enumerate() {
        if index > 0 {
            out.text.push('\n');
        }

        let (line, quote) = match line.strip_prefix("> ") {
            Some(rest) => (rest, true),
            None => (line, false),
        };

        parse_inline(
            line,
            Style {
                quote,
                ..Style::default()
            },
            out,
        );
    }
}

/// Scan a single line, tracking active delimiters.
fn parse_inline(input: &str, base: Style, out: &mut Parsed) {
    let bytes = input.as_bytes();
    let mut style = base;
    let mut plain_start = 0usize;
    let mut index = 0usize;

    // Flush accumulated plain text before a style change.
    macro_rules! flush {
        ($end:expr) => {
            if plain_start < $end {
                out.push(&input[plain_start..$end], style);
            }
        };
    }

    while index < bytes.len() {
        let rest = &input[index..];

        // Code spans suppress all other formatting, so they are handled first.
        if let Some(body_len) = code_span(rest) {
            flush!(index);
            let (marker, body) = split_code(rest, body_len);
            out.push(
                body,
                Style {
                    code: true,
                    ..style
                },
            );
            index += marker;
            plain_start = index;
            continue;
        }

        // Angle-bracket entities: mentions, channels, roles, emoji, timestamps.
        if bytes[index] == b'<' {
            if let Some((consumed, text, kind)) = entity(rest) {
                flush!(index);
                out.push(&text, Style { kind, ..style });
                index += consumed;
                plain_start = index;
                continue;
            }
        }

        // Bare URLs.
        if rest.starts_with("http://") || rest.starts_with("https://") {
            let end = rest.find(|c: char| c.is_whitespace()).unwrap_or(rest.len());
            flush!(index);
            out.push(
                &rest[..end],
                Style {
                    kind: Kind::Url,
                    ..style
                },
            );
            index += end;
            plain_start = index;
            continue;
        }

        // Paired formatting markers. Longest first so `**` beats `*` and
        // `__` beats `_`.
        let marker = [
            ("***", 3usize),
            ("**", 2),
            ("__", 2),
            ("~~", 2),
            ("||", 2),
            ("*", 1),
            ("_", 1),
        ]
        .into_iter()
        .find(|(token, _)| rest.starts_with(token));

        if let Some((token, len)) = marker {
            // Only treat it as a delimiter if it closes later on this line.
            let closes = rest[len..].contains(token);
            let active = match token {
                "***" => style.bold && style.italic,
                "**" => style.bold,
                "__" => style.underline,
                "~~" => style.strike,
                "||" => style.spoiler,
                _ => style.italic,
            };

            if closes || active {
                flush!(index);
                match token {
                    "***" => {
                        style.bold = !style.bold;
                        style.italic = !style.italic;
                    }
                    "**" => style.bold = !style.bold,
                    "__" => style.underline = !style.underline,
                    "~~" => style.strike = !style.strike,
                    "||" => style.spoiler = !style.spoiler,
                    _ => style.italic = !style.italic,
                }
                index += len;
                plain_start = index;
                continue;
            }
        }

        index += next_char_len(bytes, index);
    }

    flush!(input.len());
}

fn next_char_len(bytes: &[u8], index: usize) -> usize {
    let first = bytes[index];
    if first < 0x80 {
        1
    } else if first >> 5 == 0b110 {
        2
    } else if first >> 4 == 0b1110 {
        3
    } else {
        4
    }
}

/// If `rest` opens an inline code span, return the total byte length consumed.
///
/// Fenced blocks are handled in [`parse`] before this point, so only the
/// single-backtick form reaches here.
fn code_span(rest: &str) -> Option<usize> {
    if !rest.starts_with('`') {
        return None;
    }
    let close = rest[1..].find('`')? + 1;
    Some(close + 1)
}

/// Split an inline code span into (consumed, body).
fn split_code(rest: &str, total: usize) -> (usize, &str) {
    (total, &rest[1..total - 1])
}

/// Parse `<...>` entities. Returns (bytes consumed, display text, kind).
fn entity(rest: &str) -> Option<(usize, String, Kind)> {
    let close = rest.find('>')?;
    let body = &rest[1..close];
    let consumed = close + 1;

    // Custom emoji: <:name:id> or <a:name:id>
    if let Some(inner) = body.strip_prefix(':').or_else(|| body.strip_prefix("a:")) {
        let name = inner.split(':').next()?;
        if name.is_empty() {
            return None;
        }
        return Some((consumed, format!(":{name}:"), Kind::Emoji));
    }

    // Role: <@&id>
    if let Some(id) = body.strip_prefix("@&") {
        return id
            .chars()
            .all(|c| c.is_ascii_digit())
            .then(|| (consumed, "@role".to_string(), Kind::Role));
    }

    // User: <@id> or <@!id>
    if let Some(id) = body.strip_prefix('@') {
        let id = id.strip_prefix('!').unwrap_or(id);
        return id
            .chars()
            .all(|c| c.is_ascii_digit())
            .then(|| (consumed, "@user".to_string(), Kind::Mention));
    }

    // Channel: <#id>
    if let Some(id) = body.strip_prefix('#') {
        return id
            .chars()
            .all(|c| c.is_ascii_digit())
            .then(|| (consumed, "#channel".to_string(), Kind::Channel));
    }

    // Timestamp: <t:unix> or <t:unix:style>
    if let Some(spec) = body.strip_prefix("t:") {
        let seconds: i64 = spec.split(':').next()?.parse().ok()?;
        let rendered = chrono::DateTime::from_timestamp(seconds, 0)
            .map(|time| {
                time.with_timezone(&chrono::Local)
                    .format("%Y-%m-%d %H:%M")
                    .to_string()
            })
            .unwrap_or_else(|| "invalid time".to_string());
        return Some((consumed, rendered, Kind::Timestamp));
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn styles(input: &str) -> Vec<(String, Style)> {
        let parsed = parse(input);
        parsed
            .runs
            .iter()
            .map(|(range, style)| (parsed.text[range.clone()].to_string(), *style))
            .collect()
    }

    #[test]
    fn plain_text_has_no_runs() {
        let parsed = parse("hello world");
        assert_eq!(parsed.text, "hello world");
        assert!(parsed.runs.is_empty());
    }

    #[test]
    fn bold_and_italic() {
        let parsed = parse("a **b** c *d*");
        assert_eq!(parsed.text, "a b c d");
        let runs = styles("a **b** c *d*");
        assert!(runs.iter().any(|(t, s)| t == "b" && s.bold));
        assert!(runs.iter().any(|(t, s)| t == "d" && s.italic));
    }

    #[test]
    fn bold_italic_combined() {
        let runs = styles("***both***");
        assert!(runs.iter().any(|(t, s)| t == "both" && s.bold && s.italic));
    }

    #[test]
    fn underline_strike_spoiler() {
        assert!(styles("__u__").iter().any(|(t, s)| t == "u" && s.underline));
        assert!(styles("~~s~~").iter().any(|(t, s)| t == "s" && s.strike));
        assert!(styles("||x||").iter().any(|(t, s)| t == "x" && s.spoiler));
    }

    #[test]
    fn code_suppresses_formatting() {
        let parsed = parse("`**not bold**`");
        assert_eq!(parsed.text, "**not bold**");
        assert!(
            parsed
                .runs
                .iter()
                .all(|(_, style)| style.code && !style.bold)
        );
    }

    #[test]
    fn fenced_block_drops_language_tag() {
        let parsed = parse("```rust\nlet x = 1;```");
        assert_eq!(parsed.text, "let x = 1;");
    }

    #[test]
    fn unmatched_marker_is_literal() {
        // A lone asterisk is common in prose and must not eat the rest.
        let parsed = parse("2 * 3 = 6");
        assert_eq!(parsed.text, "2 * 3 = 6");
        assert!(parsed.runs.is_empty());
    }

    #[test]
    fn mentions_and_channels() {
        let parsed = parse("hi <@123> see <#456> and <@&789>");
        assert_eq!(parsed.text, "hi @user see #channel and @role");
        let kinds: Vec<_> = parsed.runs.iter().map(|(_, s)| s.kind).collect();
        assert!(kinds.contains(&Kind::Mention));
        assert!(kinds.contains(&Kind::Channel));
        assert!(kinds.contains(&Kind::Role));
    }

    #[test]
    fn custom_emoji_falls_back_to_name() {
        let parsed = parse("nice <:ferris:1234>");
        assert_eq!(parsed.text, "nice :ferris:");
        assert!(parsed.runs.iter().any(|(_, s)| s.kind == Kind::Emoji));
    }

    #[test]
    fn urls_are_classified() {
        let parsed = parse("see https://example.com/x now");
        assert!(parsed.text.contains("https://example.com/x"));
        assert!(parsed.runs.iter().any(|(_, s)| s.kind == Kind::Url));
    }

    #[test]
    fn blockquote_marks_the_line() {
        let parsed = parse("> quoted\nplain");
        assert_eq!(parsed.text, "quoted\nplain");
        assert!(parsed.runs.iter().any(|(_, s)| s.quote));
    }

    #[test]
    fn multibyte_text_does_not_panic_or_split() {
        let parsed = parse("**héllo** 🎉 wörld");
        assert!(parsed.text.contains('🎉'));
        assert!(parsed.text.contains("héllo"));
        for (range, _) in &parsed.runs {
            assert!(parsed.text.is_char_boundary(range.start));
            assert!(parsed.text.is_char_boundary(range.end));
        }
    }

    #[test]
    fn timestamps_render_as_dates() {
        let parsed = parse("at <t:1700000000:F>");
        assert!(!parsed.text.contains("<t:"));
        assert!(parsed.runs.iter().any(|(_, s)| s.kind == Kind::Timestamp));
    }
}
