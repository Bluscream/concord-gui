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
    /// Heading level, 1 to 3. Zero when this is not a heading.
    pub heading: u8,
    /// Discord's `-#` small print.
    pub subtext: bool,
    pub kind: Kind,
}

/// Semantic classification, which drives colour rather than weight.
///
/// Entity variants carry their snowflake so a click handler can act on the
/// target - jumping to a channel, opening a profile - without re-parsing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Kind {
    #[default]
    Text,
    /// `<@id>` - a user mention.
    Mention(u64),
    /// `<#id>` - a channel link.
    Channel(u64),
    /// `<@&id>` - a role mention.
    Role(u64),
    /// `<:name:id>` - a custom emoji. Animated emoji need a different CDN
    /// form, so the flag travels with the id.
    Emoji {
        id: u64,
        animated: bool,
    },
    Url,
    /// `[text](url)` - a link whose text is not its address. The address is
    /// held in [`Parsed::links`], because the run's own text is the label.
    Link(u32),
    /// `</name:id>` - a slash command.
    Command(u64),
    /// `<t:unix:style>` - a rendered timestamp.
    Timestamp,
}

/// Supplies display names for mention targets.
///
/// Parsing is kept separate from resolution so the parser stays pure and
/// testable; the projection layer supplies a resolver backed by guild state.
pub trait Mentions {
    fn user(&self, id: u64) -> Option<String>;
    fn channel(&self, id: u64) -> Option<String>;
    fn role(&self, id: u64) -> Option<String>;
    /// Custom emoji name, when the guild is known.
    fn emoji(&self, id: u64) -> Option<String>;
}

/// A resolver that knows nothing, used where guild state is unavailable.
///
/// Unresolved mentions render with the snowflake rather than a fake name, so
/// an unknown target is visibly unknown instead of silently wrong.
///
/// Only the tests construct it: every render path has guild state to hand, and
/// `parse` is the resolver-free entry point they use.
#[cfg(test)]
pub struct Unresolved;

#[cfg(test)]
impl Mentions for Unresolved {
    fn user(&self, _id: u64) -> Option<String> {
        None
    }
    fn channel(&self, _id: u64) -> Option<String> {
        None
    }
    fn role(&self, _id: u64) -> Option<String> {
        None
    }
    fn emoji(&self, _id: u64) -> Option<String> {
        None
    }
}

/// A parsed message body: display text plus the ranges that carry styling.
#[derive(Debug, Default)]
pub struct Parsed {
    pub text: String,
    pub runs: Vec<(Range<usize>, Style)>,
    /// Addresses for [`Kind::Link`] runs, whose own text is a label rather
    /// than the address. Indexed by the number the run carries.
    pub links: Vec<String>,
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
#[cfg(test)]
pub fn parse(input: &str) -> Parsed {
    parse_with(input, &Unresolved)
}

/// Parse, resolving mention targets to display names.
pub fn parse_with(input: &str, mentions: &dyn Mentions) -> Parsed {
    let mut out = Parsed::default();
    let mut rest = input;

    while let Some(start) = rest.find("```") {
        let (before, remainder) = rest.split_at(start);
        // An unterminated fence is literal text, not a block to end-of-message.
        let Some(end) = remainder[3..].find("```") else {
            break;
        };

        parse_lines(before, mentions, &mut out);

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

    parse_lines(rest, mentions, &mut out);
    out
}

/// Line-oriented parsing for everything outside a fenced block.
fn parse_lines(input: &str, mentions: &dyn Mentions, out: &mut Parsed) {
    if input.is_empty() {
        return;
    }

    // Once `>>>` has been seen every remaining line is quoted, which is what
    // it means on Discord: it is a block quote, not a line quote.
    let mut quote_rest = false;

    for (index, line) in input.split('\n').enumerate() {
        if index > 0 {
            out.text.push('\n');
        }

        let (line, style) = block_style(line, &mut quote_rest);

        // Lists rewrite the line, so the rewritten text has to outlive the
        // borrow of the original.
        let rewritten = list_marker(line);
        let line = rewritten.as_deref().unwrap_or(line);

        parse_inline(line, style, mentions, out);
    }
}

/// Strip a line's block marker and turn it into the style it implies.
///
/// Discord decides these per line and by prefix only - a `#` in the middle of
/// a line is a hash, not a heading - so this runs once per line rather than
/// being folded into the inline scanner.
fn block_style<'a>(line: &'a str, quote_rest: &mut bool) -> (&'a str, Style) {
    let mut style = Style {
        quote: *quote_rest,
        ..Style::default()
    };

    if *quote_rest {
        return (line, style);
    }

    // `>>> ` quotes everything from here down.
    if let Some(rest) = line.strip_prefix(">>> ") {
        *quote_rest = true;
        style.quote = true;
        return (rest, style);
    }
    if let Some(rest) = line.strip_prefix("> ") {
        style.quote = true;
        return (rest, style);
    }

    // Subtext before headings: `-#` starts with the same character as a
    // bullet, and `###` with the same as a heading, so the longest marker
    // has to be tried first.
    if let Some(rest) = line.strip_prefix("-# ") {
        style.subtext = true;
        return (rest, style);
    }
    for (marker, level) in [("### ", 3), ("## ", 2), ("# ", 1)] {
        if let Some(rest) = line.strip_prefix(marker) {
            style.heading = level;
            return (rest, style);
        }
    }

    (line, style)
}

/// Replace a list marker with the bullet or number it draws as.
///
/// Kept as text rather than as a style flag because the output is a flat
/// string with styled ranges: a bullet that lived only in the style would
/// have nowhere to be drawn, and the indent would be lost with it.
///
/// Returns the rewritten line, or `None` when this is not a list item.
fn list_marker(line: &str) -> Option<String> {
    let indent = line.len() - line.trim_start().len();
    let body = line.trim_start();

    // Discord nests by twos; deeper indents keep nesting.
    let depth = indent / 2;
    let pad = "    ".repeat(depth);

    for marker in ["- ", "* "] {
        if let Some(rest) = body.strip_prefix(marker) {
            // Alternating glyphs, as Discord does, so a nested list is
            // readable without counting the indentation.
            let bullet = if depth.is_multiple_of(2) {
                "\u{2022}"
            } else {
                "\u{25E6}"
            };
            return Some(format!("{pad}{bullet} {rest}"));
        }
    }

    // `12. item`, keeping the author's own numbering rather than renumbering:
    // a list that starts at 3 was probably meant to.
    let digits: String = body.chars().take_while(char::is_ascii_digit).collect();
    if !digits.is_empty()
        && let Some(rest) = body[digits.len()..].strip_prefix(". ")
    {
        return Some(format!("{pad}{digits}. {rest}"));
    }

    None
}

/// Scan a single line, tracking active delimiters.
fn parse_inline(input: &str, base: Style, mentions: &dyn Mentions, out: &mut Parsed) {
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

        // Masked link: [label](https://...). Before the bare-URL scan, or the
        // address inside the brackets is picked up as a link of its own and
        // the label is left as literal square brackets.
        if bytes[index] == b'['
            && let Some((consumed, label, url)) = masked_link(rest)
        {
            flush!(index);
            let slot = out.links.len() as u32;
            out.links.push(url);
            out.push(
                &label,
                Style {
                    kind: Kind::Link(slot),
                    ..style
                },
            );
            index += consumed;
            plain_start = index;
            continue;
        }

        // `<https://...>` suppresses the embed on Discord. The brackets are
        // syntax, not part of the address, so they are not shown.
        if bytes[index] == b'<'
            && let Some(end) = rest.find('>')
            && let inner = &rest[1..end]
            && (inner.starts_with("http://") || inner.starts_with("https://"))
        {
            flush!(index);
            out.push(
                inner,
                Style {
                    kind: Kind::Url,
                    ..style
                },
            );
            index += end + 1;
            plain_start = index;
            continue;
        }

        // Angle-bracket entities: mentions, channels, roles, emoji, timestamps.
        if bytes[index] == b'<'
            && let Some((consumed, text, kind)) = entity(rest, mentions)
        {
            flush!(index);
            out.push(&text, Style { kind, ..style });
            index += consumed;
            plain_start = index;
            continue;
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
fn entity(rest: &str, mentions: &dyn Mentions) -> Option<(usize, String, Kind)> {
    let close = rest.find('>')?;
    let body = &rest[1..close];
    let consumed = close + 1;

    // Custom emoji: <:name:id> or <a:name:id>
    let animated = body.starts_with("a:");
    if let Some(inner) = body.strip_prefix(':').or_else(|| body.strip_prefix("a:")) {
        let mut parts = inner.split(':');
        let name = parts.next()?;
        if name.is_empty() {
            return None;
        }
        let id: u64 = parts.next()?.parse().ok()?;
        let name = mentions.emoji(id).unwrap_or_else(|| name.to_string());
        return Some((consumed, format!(":{name}:"), Kind::Emoji { id, animated }));
    }

    // Role: <@&id>
    if let Some(raw) = body.strip_prefix("@&") {
        let id: u64 = raw.parse().ok()?;
        let name = mentions.role(id).unwrap_or_else(|| id.to_string());
        return Some((consumed, format!("@{name}"), Kind::Role(id)));
    }

    // User: <@id> or <@!id>
    if let Some(raw) = body.strip_prefix('@') {
        let raw = raw.strip_prefix('!').unwrap_or(raw);
        let id: u64 = raw.parse().ok()?;
        let name = mentions.user(id).unwrap_or_else(|| id.to_string());
        return Some((consumed, format!("@{name}"), Kind::Mention(id)));
    }

    // Channel: <#id>
    if let Some(raw) = body.strip_prefix('#') {
        let id: u64 = raw.parse().ok()?;
        let name = mentions.channel(id).unwrap_or_else(|| id.to_string());
        return Some((consumed, format!("#{name}"), Kind::Channel(id)));
    }

    // Slash command: </name:id>
    if let Some(rest) = body.strip_prefix('/') {
        let (name, raw) = rest.rsplit_once(':')?;
        let id: u64 = raw.parse().ok()?;
        return Some((consumed, format!("/{name}"), Kind::Command(id)));
    }

    // Timestamp: <t:unix> or <t:unix:style>
    if let Some(spec) = body.strip_prefix("t:") {
        let mut parts = spec.split(':');
        let seconds: i64 = parts.next()?.parse().ok()?;
        let style = parts.next().and_then(|s| s.chars().next()).unwrap_or('f');
        return Some((consumed, render_timestamp(seconds, style), Kind::Timestamp));
    }

    None
}

/// Split `[label](url)` into its two halves.
///
/// Returns how much of the input it consumed, so the scanner can step past
/// the whole construct rather than re-reading the address as text.
fn masked_link(input: &str) -> Option<(usize, String, String)> {
    let close = input.find("](")?;
    let label = &input[1..close];
    // A label may not contain a bracket of its own; Discord does not nest
    // these, and accepting one would swallow the rest of the line.
    if label.contains('[') || label.contains(']') {
        return None;
    }

    let rest = &input[close + 2..];
    let end = rest.find(')')?;
    let url = &rest[..end];
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return None;
    }

    Some((close + 2 + end + 1, label.to_string(), url.to_string()))
}

/// Format a timestamp the way Discord's style letter asks for.
///
/// All seven, because rendering them identically is worse than not supporting
/// them: the author picked `R` to say "an hour ago" and got an absolute date,
/// with nothing to show the request had been ignored.
fn render_timestamp(seconds: i64, style: char) -> String {
    let Some(time) = chrono::DateTime::from_timestamp(seconds, 0) else {
        return "invalid time".to_string();
    };
    let local = time.with_timezone(&chrono::Local);

    match style {
        // Short and long time.
        't' => local.format("%H:%M").to_string(),
        'T' => local.format("%H:%M:%S").to_string(),
        // Short and long date.
        'd' => local.format("%d/%m/%Y").to_string(),
        'D' => local.format("%-d %B %Y").to_string(),
        // Long date with time, and the same with the weekday.
        'F' => local.format("%A, %-d %B %Y %H:%M").to_string(),
        'R' => relative_time(chrono::Utc::now().signed_duration_since(time)),
        // `f` is Discord's default, and the fallback for a letter this build
        // does not know - which is better than showing nothing.
        _ => local.format("%-d %B %Y %H:%M").to_string(),
    }
}

/// "in 3 hours", "2 days ago" - the `R` style.
fn relative_time(elapsed: chrono::Duration) -> String {
    let seconds = elapsed.num_seconds();
    let ahead = seconds < 0;
    let magnitude = seconds.abs();

    let (count, unit) = match magnitude {
        0..60 => (magnitude, "second"),
        60..3_600 => (magnitude / 60, "minute"),
        3_600..86_400 => (magnitude / 3_600, "hour"),
        86_400..2_592_000 => (magnitude / 86_400, "day"),
        2_592_000..31_536_000 => (magnitude / 2_592_000, "month"),
        _ => (magnitude / 31_536_000, "year"),
    };

    let plural = if count == 1 { "" } else { "s" };
    if ahead {
        format!("in {count} {unit}{plural}")
    } else {
        format!("{count} {unit}{plural} ago")
    }
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

    /// A resolver with fixed answers, for tests.
    struct FakeMentions;

    impl Mentions for FakeMentions {
        fn user(&self, id: u64) -> Option<String> {
            (id == 123).then(|| "ferris".to_string())
        }
        fn channel(&self, id: u64) -> Option<String> {
            (id == 456).then(|| "general".to_string())
        }
        fn role(&self, id: u64) -> Option<String> {
            (id == 789).then(|| "Maintainer".to_string())
        }
        fn emoji(&self, _id: u64) -> Option<String> {
            None
        }
    }

    #[test]
    fn mentions_resolve_to_display_names() {
        let parsed = parse_with("hi <@123> see <#456> and <@&789>", &FakeMentions);
        assert_eq!(parsed.text, "hi @ferris see #general and @Maintainer");

        let kinds: Vec<_> = parsed.runs.iter().map(|(_, s)| s.kind).collect();
        assert!(kinds.contains(&Kind::Mention(123)));
        assert!(kinds.contains(&Kind::Channel(456)));
        assert!(kinds.contains(&Kind::Role(789)));
    }

    #[test]
    fn unresolved_mentions_show_the_id_not_a_fake_name() {
        // An unknown target must be visibly unknown rather than silently wrong.
        let parsed = parse("hi <@999>");
        assert_eq!(parsed.text, "hi @999");
    }

    #[test]
    fn custom_emoji_falls_back_to_name() {
        let parsed = parse("nice <:ferris:1234>");
        assert_eq!(parsed.text, "nice :ferris:");
        assert!(
            parsed
                .runs
                .iter()
                .any(|(_, s)| matches!(s.kind, Kind::Emoji { .. }))
        );
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

#[cfg(test)]
mod discord_syntax_tests {
    use super::*;

    #[test]
    fn headings_take_their_level_and_lose_their_hashes() {
        for (input, level) in [("# one", 1), ("## two", 2), ("### three", 3)] {
            let parsed = parse(input);
            assert!(!parsed.text.contains('#'), "{input} kept its marker");
            assert!(
                parsed.runs.iter().any(|(_, s)| s.heading == level),
                "{input} did not become a level {level} heading"
            );
        }

        // A hash that is not at the start of a line is a hash.
        let parsed = parse("not a # heading");
        assert!(parsed.text.contains('#'));
        assert!(parsed.runs.iter().all(|(_, s)| s.heading == 0));
    }

    #[test]
    fn subtext_is_not_mistaken_for_a_bullet() {
        // `-#` and `-` start with the same character, so the order the
        // markers are tried in is the whole of this.
        let parsed = parse("-# small print");
        assert_eq!(parsed.text, "small print");
        assert!(parsed.runs.iter().any(|(_, s)| s.subtext));

        let parsed = parse("- a bullet");
        assert!(parsed.text.starts_with('\u{2022}'), "got {:?}", parsed.text);
        assert!(parsed.runs.iter().all(|(_, s)| !s.subtext));
    }

    #[test]
    fn lists_keep_their_nesting_and_their_numbering() {
        let parsed = parse("- one\n  - two\n1. first\n7. seventh");
        let lines: Vec<&str> = parsed.text.lines().collect();

        assert!(lines[0].starts_with('\u{2022}'));
        // Nested, so indented and drawn with the other glyph.
        assert!(lines[1].starts_with("    \u{25E6}"), "got {:?}", lines[1]);
        // The author's own numbers: a list starting at 7 meant to.
        assert!(lines[2].starts_with("1."), "got {:?}", lines[2]);
        assert!(lines[3].starts_with("7."), "got {:?}", lines[3]);
    }

    #[test]
    fn a_block_quote_swallows_the_rest_of_the_message() {
        // `>>>` is not a line prefix like `>`; everything after it is quoted,
        // and treating it as one would leave the tail unquoted.
        let parsed = parse("plain\n>>> quoted\nstill quoted");
        let quoted: Vec<bool> = parsed
            .text
            .lines()
            .map(|line| {
                let start = parsed.text.find(line).unwrap_or(0);
                parsed
                    .runs
                    .iter()
                    .any(|(range, style)| range.contains(&start) && style.quote)
            })
            .collect();
        assert_eq!(quoted, vec![false, true, true], "got {parsed:?}");
    }

    #[test]
    fn a_masked_link_shows_its_label_and_keeps_its_address() {
        let parsed = parse("see [the repo](https://github.com/bluscream/concord) now");
        assert_eq!(parsed.text, "see the repo now");
        assert_eq!(parsed.links, vec!["https://github.com/bluscream/concord"]);
        assert!(
            parsed
                .runs
                .iter()
                .any(|(_, s)| matches!(s.kind, Kind::Link(0)))
        );

        // Not a link: the brackets stay as written rather than eating the line.
        let parsed = parse("[not a link] (nope)");
        assert!(parsed.text.contains('['));
        assert!(parsed.links.is_empty());
    }

    #[test]
    fn angle_brackets_are_stripped_from_a_suppressed_link() {
        let parsed = parse("see <https://example.com/x>");
        assert_eq!(parsed.text, "see https://example.com/x");
        assert!(parsed.runs.iter().any(|(_, s)| s.kind == Kind::Url));
    }

    #[test]
    fn a_slash_command_reads_as_its_name() {
        let parsed = parse("try </settings:12345>");
        assert_eq!(parsed.text, "try /settings");
        assert!(
            parsed
                .runs
                .iter()
                .any(|(_, s)| s.kind == Kind::Command(12345))
        );
    }

    #[test]
    fn every_timestamp_style_renders_differently() {
        // Rendering them identically is worse than not supporting them: the
        // author picked `R` to say "an hour ago" and got an absolute date,
        // with nothing to show the request had been ignored.
        let rendered: Vec<String> = ['t', 'T', 'd', 'D', 'f', 'F', 'R']
            .into_iter()
            .map(|style| parse(&format!("<t:1700000000:{style}>")).text)
            .collect();

        for (index, one) in rendered.iter().enumerate() {
            assert!(!one.contains("<t:"), "style {index} was left unparsed");
            for other in &rendered[index + 1..] {
                assert_ne!(one, other, "two styles rendered the same: {rendered:?}");
            }
        }
    }

    #[test]
    fn relative_times_read_forwards_and_backwards() {
        use chrono::Duration;
        assert_eq!(relative_time(Duration::seconds(30)), "30 seconds ago");
        assert_eq!(relative_time(Duration::seconds(-30)), "in 30 seconds");
        assert_eq!(relative_time(Duration::hours(1)), "1 hour ago");
        assert_eq!(relative_time(Duration::days(3)), "3 days ago");
    }
}
