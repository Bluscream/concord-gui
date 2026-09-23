use super::super::text::{RenderedText, TextHighlight, TextHighlightKind};

pub(super) fn add_literal_mention_highlights(
    rendered: &mut RenderedText,
    mention: &str,
    kind: TextHighlightKind,
) {
    let mut cursor = 0usize;
    while let Some(relative_start) = rendered.text[cursor..].find(mention) {
        let start = cursor.saturating_add(relative_start);
        let end = start.saturating_add(mention.len());
        if is_literal_mention_boundary(&rendered.text, start, end) {
            rendered.highlights.push(TextHighlight { start, end, kind });
        }
        cursor = end;
    }
}

pub(super) fn normalize_mention_highlights(highlights: &mut Vec<TextHighlight>) {
    let (mut mention_highlights, mut other_highlights): (Vec<_>, Vec<_>) = highlights
        .drain(..)
        .partition(|highlight| is_mention_highlight(highlight.kind));
    mention_highlights.sort_by_key(|highlight| (highlight.start, highlight.end));
    let mut normalized: Vec<TextHighlight> = Vec::new();
    for highlight in mention_highlights {
        let Some(last) = normalized.last_mut() else {
            normalized.push(highlight);
            continue;
        };
        if highlight.start <= last.end {
            last.end = last.end.max(highlight.end);
            // SelfMention always wins over OtherMention so that ranges that
            // happen to overlap (e.g. `@everyone @me` collisions) keep the
            // louder colour.
            if matches!(highlight.kind, TextHighlightKind::SelfMention) {
                last.kind = TextHighlightKind::SelfMention;
            }
        } else {
            normalized.push(highlight);
        }
    }
    normalized.append(&mut other_highlights);
    normalized.sort_by_key(|highlight| (highlight.start, highlight.end));
    *highlights = normalized;
}

fn is_mention_highlight(kind: TextHighlightKind) -> bool {
    matches!(
        kind,
        TextHighlightKind::SelfMention
            | TextHighlightKind::OtherMention
            | TextHighlightKind::RoleMention { .. }
    )
}

fn is_literal_mention_boundary(value: &str, start: usize, end: usize) -> bool {
    let before = value[..start].chars().next_back();
    let after = value[end..].chars().next();
    !before.is_some_and(is_literal_mention_word_char)
        && !after.is_some_and(is_literal_mention_word_char)
}

fn is_literal_mention_word_char(value: char) -> bool {
    value.is_ascii_alphanumeric() || value == '_'
}
