use crate::discord::ApplicationCommandIdentity;
use crate::tui::text_input::TextInputState;

use super::completions::{EmojiCompletion, MentionCompletion};

#[derive(Clone, Debug)]
pub(in crate::tui::state) struct ComposerInputSnapshot {
    pub(super) input: TextInputState,
    pub(super) mention_completions: Vec<MentionCompletion>,
    pub(super) emoji_completions: Vec<EmojiCompletion>,
    pub(super) selected_command_identity: Option<ApplicationCommandIdentity>,
}

impl ComposerInputSnapshot {
    fn from_value(value: String) -> Self {
        let mut input = TextInputState::default();
        input.set_value(value);
        Self {
            input,
            mention_completions: Vec::new(),
            emoji_completions: Vec::new(),
            selected_command_identity: None,
        }
    }

    pub(in crate::tui::state) fn value(&self) -> &str {
        self.input.value()
    }

    pub(in crate::tui::state) fn into_translation_plan(
        self,
        request_id: u64,
    ) -> Result<ComposerTranslationPlan, &'static str> {
        if self.input.value().trim_start().starts_with('/') {
            return Err("slash command drafts cannot be translated");
        }

        ComposerTranslationPlan::new(self, request_id)
    }

    fn matches_translation_source(&self, other: &Self) -> bool {
        self.input.value() == other.input.value()
            && self.mention_completions == other.mention_completions
            && self.emoji_completions == other.emoji_completions
            && self.selected_command_identity == other.selected_command_identity
    }
}

#[derive(Debug)]
pub(in crate::tui::state) struct ComposerTranslationPlan {
    source: ComposerInputSnapshot,
    provider_text: String,
    tokens: Vec<ComposerTranslationToken>,
}

#[derive(Debug)]
struct ComposerTranslationToken {
    placeholder: String,
    visible_text: String,
    kind: ComposerTranslationTokenKind,
}

#[derive(Debug)]
enum ComposerTranslationTokenKind {
    Mention(MentionCompletion),
    Emoji(EmojiCompletion),
}

#[derive(Debug)]
struct ComposerTranslationSourceRange {
    byte_start: usize,
    byte_end: usize,
    kind: ComposerTranslationTokenKind,
}

impl ComposerTranslationPlan {
    fn new(source: ComposerInputSnapshot, request_id: u64) -> Result<Self, &'static str> {
        let input = source.input.value();
        let mut ranges = source
            .mention_completions
            .iter()
            .copied()
            .map(|completion| ComposerTranslationSourceRange {
                byte_start: completion.byte_start,
                byte_end: completion.byte_end,
                kind: ComposerTranslationTokenKind::Mention(completion),
            })
            .chain(source.emoji_completions.iter().cloned().map(|completion| {
                ComposerTranslationSourceRange {
                    byte_start: completion.byte_start,
                    byte_end: completion.byte_end,
                    kind: ComposerTranslationTokenKind::Emoji(completion),
                }
            }))
            .collect::<Vec<_>>();
        ranges.sort_by_key(|range| range.byte_start);

        let mut provider_text = String::with_capacity(input.len());
        let mut tokens = Vec::with_capacity(ranges.len());
        let mut source_cursor = 0;
        for (index, range) in ranges.into_iter().enumerate() {
            if range.byte_start < source_cursor
                || range.byte_start >= range.byte_end
                || range.byte_end > input.len()
                || !input.is_char_boundary(range.byte_start)
                || !input.is_char_boundary(range.byte_end)
            {
                return Err("composer contains invalid semantic ranges");
            }

            let placeholder = format!("\u{e000}CONCORD_{request_id}_{index}\u{e001}");
            if input.contains(&placeholder) {
                return Err("composer contains a reserved translation placeholder");
            }
            provider_text.push_str(&input[source_cursor..range.byte_start]);
            provider_text.push_str(&placeholder);
            tokens.push(ComposerTranslationToken {
                placeholder,
                visible_text: input[range.byte_start..range.byte_end].to_owned(),
                kind: range.kind,
            });
            source_cursor = range.byte_end;
        }
        provider_text.push_str(&input[source_cursor..]);

        Ok(Self {
            source,
            provider_text,
            tokens,
        })
    }

    pub(in crate::tui::state) fn source_text(&self) -> &str {
        self.source.value()
    }

    pub(in crate::tui::state) fn provider_text(&self) -> &str {
        &self.provider_text
    }

    pub(in crate::tui::state) fn matches_source(&self, current: &ComposerInputSnapshot) -> bool {
        self.source.matches_translation_source(current)
    }

    pub(in crate::tui::state) fn translated_snapshot(
        &self,
        translated_text: &str,
    ) -> Result<ComposerInputSnapshot, &'static str> {
        let mut occurrences = Vec::with_capacity(self.tokens.len());
        for token in &self.tokens {
            let mut matches = translated_text.match_indices(&token.placeholder);
            let Some((byte_start, _)) = matches.next() else {
                return Err("translation provider changed a protected mention or emoji");
            };
            if matches.next().is_some() {
                return Err("translation provider duplicated a protected mention or emoji");
            }
            occurrences.push((byte_start, token));
        }
        occurrences.sort_by_key(|(byte_start, _)| *byte_start);

        let mut restored = String::with_capacity(translated_text.len());
        let mut mention_completions = Vec::new();
        let mut emoji_completions = Vec::new();
        let mut translated_cursor = 0;
        for (byte_start, token) in occurrences {
            if byte_start < translated_cursor {
                return Err("translation provider returned overlapping protected values");
            }
            let byte_end = byte_start.saturating_add(token.placeholder.len());
            restored.push_str(&translated_text[translated_cursor..byte_start]);
            let restored_start = restored.len();
            restored.push_str(&token.visible_text);
            let restored_end = restored.len();
            match &token.kind {
                ComposerTranslationTokenKind::Mention(completion) => {
                    mention_completions.push(MentionCompletion {
                        byte_start: restored_start,
                        byte_end: restored_end,
                        target: completion.target,
                    });
                }
                ComposerTranslationTokenKind::Emoji(completion) => {
                    emoji_completions.push(EmojiCompletion {
                        byte_start: restored_start,
                        byte_end: restored_end,
                        replacement: completion.replacement.clone(),
                        custom_image_url: completion.custom_image_url.clone(),
                    });
                }
            }
            translated_cursor = byte_end;
        }
        restored.push_str(&translated_text[translated_cursor..]);

        let mut snapshot = ComposerInputSnapshot::from_value(restored);
        snapshot.mention_completions = mention_completions;
        snapshot.emoji_completions = emoji_completions;
        Ok(snapshot)
    }
}
