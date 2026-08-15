//! Message composer.
//!
//! GPUI 0.2.2 ships no text-input widget, so this implements one: a focusable
//! surface, a text buffer with a cursor, key handling, clipboard support and
//! multi-line editing.
//!
//! Still absent: selection (and therefore cut/copy of a range), and undo. The
//! buffer is a plain `String` with a byte cursor, which is adequate for
//! message composition but is not a general-purpose editor.

use gpui::{Div, KeyDownEvent, prelude::*, px, rgb};

use crate::theme::{active, layout, scaled, space, text};
use crate::ui::chrome::{column, row};

/// What a key press asked the host to do with the clipboard.
///
/// The composer cannot reach the clipboard without pulling GPUI context into
/// it, so it reports the intent and the caller performs it. That keeps the
/// buffer unit-testable.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ClipboardIntent {
    #[default]
    None,
    Copy,
    Cut,
}

/// A single-line text buffer with a cursor.
#[derive(Default)]
pub struct Composer {
    text: String,
    /// Cursor position as a byte offset into `text`, always on a char boundary.
    cursor: usize,
    /// Selection anchor. `Some` while a selection is active; the selected
    /// range runs between it and the cursor in either direction.
    anchor: Option<usize>,
    /// Undo snapshots, oldest first.
    undo: Vec<Snapshot>,
    /// Snapshots undone but not yet re-applied.
    redo: Vec<Snapshot>,
    /// Whether the last edit was a plain insert, so a run of typing coalesces
    /// into one undo step instead of one per character.
    coalescing: bool,
    /// Clipboard action requested by the last key press.
    pending_clipboard: ClipboardIntent,
}

#[derive(Clone)]
struct Snapshot {
    text: String,
    cursor: usize,
}

impl Composer {
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The selected byte range, normalised so start <= end.
    pub fn selection(&self) -> Option<std::ops::Range<usize>> {
        let anchor = self.anchor?;
        if anchor == self.cursor {
            return None;
        }
        Some(anchor.min(self.cursor)..anchor.max(self.cursor))
    }

    pub fn selected_text(&self) -> Option<&str> {
        self.selection().map(|range| &self.text[range])
    }

    /// Record the current buffer so the next edit can be undone.
    ///
    /// `coalesce` groups consecutive inserts: without it every keystroke
    /// becomes its own undo step, which makes undo useless for typing.
    fn checkpoint(&mut self, coalesce: bool) {
        if coalesce && self.coalescing {
            return;
        }
        self.undo.push(Snapshot {
            text: self.text.clone(),
            cursor: self.cursor,
        });
        // A bounded history: message composition does not need more, and this
        // keeps a long session from growing without limit.
        if self.undo.len() > 128 {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.coalescing = coalesce;
    }

    pub fn undo(&mut self) {
        let Some(previous) = self.undo.pop() else {
            return;
        };
        self.redo.push(Snapshot {
            text: self.text.clone(),
            cursor: self.cursor,
        });
        self.text = previous.text;
        self.cursor = previous.cursor.min(self.text.len());
        self.anchor = None;
        self.coalescing = false;
    }

    pub fn redo(&mut self) {
        let Some(next) = self.redo.pop() else {
            return;
        };
        self.undo.push(Snapshot {
            text: self.text.clone(),
            cursor: self.cursor,
        });
        self.text = next.text;
        self.cursor = next.cursor.min(self.text.len());
        self.anchor = None;
        self.coalescing = false;
    }

    pub fn select_all(&mut self) {
        self.anchor = Some(0);
        self.cursor = self.text.len();
    }

    pub fn clear_selection(&mut self) {
        self.anchor = None;
    }

    /// Take the clipboard action requested by the last key press.
    pub fn take_clipboard_intent(&mut self) -> ClipboardIntent {
        std::mem::replace(&mut self.pending_clipboard, ClipboardIntent::None)
    }

    /// Remove the selection after a cut has been performed.
    pub fn cut_selection(&mut self) {
        self.delete_selection();
    }

    /// Begin or extend a selection before a cursor move.
    fn anchor_for_extend(&mut self, extend: bool) {
        if extend {
            if self.anchor.is_none() {
                self.anchor = Some(self.cursor);
            }
        } else {
            self.anchor = None;
        }
    }

    /// Remove the selection, if any. Returns true when something was deleted.
    fn delete_selection(&mut self) -> bool {
        let Some(range) = self.selection() else {
            return false;
        };
        self.checkpoint(false);
        self.text.replace_range(range.clone(), "");
        self.cursor = range.start;
        self.anchor = None;
        true
    }

    pub fn is_empty(&self) -> bool {
        self.text.trim().is_empty()
    }

    /// Take the buffer's contents, resetting it.
    pub fn take(&mut self) -> String {
        self.cursor = 0;
        self.anchor = None;
        self.undo.clear();
        self.redo.clear();
        self.coalescing = false;
        std::mem::take(&mut self.text)
    }

    /// Replace the buffer, putting the cursor at the end.
    pub fn set_text(&mut self, text: &str) {
        self.checkpoint(false);
        self.text = text.to_string();
        self.cursor = self.text.len();
        self.anchor = None;
    }

    pub fn clear(&mut self) {
        self.checkpoint(false);
        self.text.clear();
        self.cursor = 0;
        self.anchor = None;
    }

    pub fn insert(&mut self, input: &str) {
        // Typed text replaces a selection, as every editor does.
        self.delete_selection();
        self.checkpoint(true);
        self.text.insert_str(self.cursor, input);
        self.cursor += input.len();
    }

    pub fn backspace(&mut self) {
        if self.delete_selection() {
            return;
        }
        if self.cursor == 0 {
            return;
        }
        self.checkpoint(false);
        let previous = self.text[..self.cursor]
            .char_indices()
            .next_back()
            .map(|(index, _)| index)
            .unwrap_or(0);
        self.text.replace_range(previous..self.cursor, "");
        self.cursor = previous;
    }

    pub fn delete(&mut self) {
        if self.delete_selection() {
            return;
        }
        if self.cursor >= self.text.len() {
            return;
        }
        self.checkpoint(false);
        let next = self.text[self.cursor..]
            .char_indices()
            .nth(1)
            .map(|(index, _)| self.cursor + index)
            .unwrap_or(self.text.len());
        self.text.replace_range(self.cursor..next, "");
    }

    pub fn move_left(&mut self) {
        self.cursor = self.text[..self.cursor]
            .char_indices()
            .next_back()
            .map(|(index, _)| index)
            .unwrap_or(0);
    }

    pub fn move_right(&mut self) {
        self.cursor = self.text[self.cursor..]
            .char_indices()
            .nth(1)
            .map(|(index, _)| self.cursor + index)
            .unwrap_or(self.text.len());
    }

    pub fn move_home(&mut self) {
        self.cursor = 0;
    }

    pub fn move_end(&mut self) {
        self.cursor = self.text.len();
    }

    /// Insert a newline at the cursor.
    pub fn newline(&mut self) {
        self.insert("\n");
    }

    /// Delete the word before the cursor, for ctrl-w / ctrl-backspace.
    pub fn delete_word(&mut self) {
        if self.delete_selection() {
            return;
        }
        self.checkpoint(false);
        let before = &self.text[..self.cursor];
        let trimmed = before.trim_end_matches(char::is_whitespace);
        let start = trimmed
            .rfind(char::is_whitespace)
            .map(|index| index + 1)
            .unwrap_or(0);
        self.text.replace_range(start..self.cursor, "");
        self.cursor = start;
    }

    /// Apply a key event. Returns true when the message should be sent.
    ///
    /// `clipboard` supplies pasted text; it is passed in rather than read here
    /// so the buffer stays free of GPUI context and remains unit-testable.
    pub fn handle_key_with_clipboard(
        &mut self,
        event: &KeyDownEvent,
        clipboard: Option<String>,
    ) -> bool {
        self.pending_clipboard = ClipboardIntent::None;
        let keystroke = &event.keystroke;
        let control = keystroke.modifiers.control || keystroke.modifiers.platform;

        let shift = keystroke.modifiers.shift;

        match keystroke.key.as_str() {
            // Escape drops a selection without touching the text, which is
            // what every other text field does. Only when there is one, so
            // escape still reaches whatever else is listening otherwise.
            "escape" if self.selection().is_some() => self.clear_selection(),
            // Shift-Enter inserts a newline; plain Enter sends.
            "enter" if shift => self.newline(),
            "enter" => return !self.is_empty(),
            // Select-all is the conventional GUI meaning of ctrl-a. The
            // emacs line-start binding moves to ctrl-home, since a chat
            // composer is a GUI field first.
            "a" if control => self.select_all(),
            "c" if control && self.selection().is_some() => {
                self.pending_clipboard = ClipboardIntent::Copy;
            }
            "x" if control && self.selection().is_some() => {
                self.pending_clipboard = ClipboardIntent::Cut;
            }
            "z" if control && shift => self.redo(),
            "z" if control => self.undo(),
            "y" if control => self.redo(),
            "v" if control => {
                if let Some(text) = clipboard {
                    // Paste replaces a selection.
                    self.delete_selection();
                    // Normalise line endings so pasted CRLF does not produce
                    // stray carriage returns in the sent message.
                    self.insert(&text.replace("\r\n", "\n").replace('\r', "\n"));
                }
            }
            "w" if control => self.delete_word(),
            "backspace" if control => self.delete_word(),
            "e" if control => {
                self.anchor_for_extend(shift);
                self.move_end();
            }
            "u" if control => {
                self.text.replace_range(..self.cursor, "");
                self.cursor = 0;
            }
            "backspace" => self.backspace(),
            "delete" => self.delete(),
            "left" => {
                self.anchor_for_extend(shift);
                self.move_left();
            }
            "right" => {
                self.anchor_for_extend(shift);
                self.move_right();
            }
            "home" => {
                self.anchor_for_extend(shift);
                self.move_home();
            }
            "end" => {
                self.anchor_for_extend(shift);
                self.move_end();
            }
            _ => {
                // `key_char` carries the text the platform produced, which
                // already accounts for layout and dead keys. Control chords
                // are filtered out so e.g. ctrl-a does not insert "a".
                if !control
                    && let Some(input) = keystroke.key_char.as_ref()
                    && !input.is_empty()
                    && !input.chars().any(char::is_control)
                {
                    self.insert(input);
                }
            }
        }

        false
    }

    /// Convenience for callers with no clipboard access.
    pub fn handle_key(&mut self, event: &KeyDownEvent) -> bool {
        self.handle_key_with_clipboard(event, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buffer(text: &str) -> Composer {
        let mut composer = Composer::default();
        composer.set_text(text);
        composer
    }

    #[test]
    fn set_text_puts_cursor_at_the_end() {
        let composer = buffer("hello");
        assert_eq!(composer.cursor, 5);
    }

    #[test]
    fn backspace_removes_a_whole_char_not_a_byte() {
        let mut composer = buffer("héllo");
        composer.backspace();
        assert_eq!(composer.text(), "héll");

        // A multi-byte char must not be split.
        let mut composer = buffer("caf\u{e9}");
        composer.backspace();
        assert_eq!(composer.text(), "caf");
    }

    #[test]
    fn delete_word_stops_at_whitespace() {
        let mut composer = buffer("hello brave world");
        composer.delete_word();
        assert_eq!(composer.text(), "hello brave ");

        composer.delete_word();
        assert_eq!(composer.text(), "hello ");
    }

    #[test]
    fn delete_word_on_empty_buffer_is_a_noop() {
        let mut composer = buffer("");
        composer.delete_word();
        assert_eq!(composer.text(), "");
        assert_eq!(composer.cursor, 0);
    }

    #[test]
    fn cursor_movement_clamps_at_both_ends() {
        let mut composer = buffer("ab");
        composer.move_home();
        composer.move_left();
        assert_eq!(composer.cursor, 0);

        composer.move_end();
        composer.move_right();
        assert_eq!(composer.cursor, 2);
    }

    #[test]
    fn newline_inserts_at_the_cursor() {
        let mut composer = buffer("ab");
        composer.move_home();
        composer.newline();
        assert_eq!(composer.text(), "\nab");
    }

    #[test]
    fn take_resets_the_buffer() {
        let mut composer = buffer("draft");
        assert_eq!(composer.take(), "draft");
        assert!(composer.text().is_empty());
        assert_eq!(composer.cursor, 0);
    }

    #[test]
    fn whitespace_only_counts_as_empty() {
        assert!(buffer("   \n ").is_empty());
        assert!(!buffer("x").is_empty());
    }
}

/// Render the composer. `enabled` is false when no channel is open, in which
/// case it shows why rather than accepting input that could not be sent.
pub fn composer_view(composer: &Composer, focused: bool, enabled: bool, placeholder: &str) -> Div {
    // Disabled and empty look the same on purpose: both show the placeholder,
    // which in the disabled case is the explanation of why.
    let content: Div = if !enabled || composer.text().is_empty() {
        gpui::div()
            .text_color(rgb(active().text_subtle))
            .child(placeholder.to_string())
    } else {
        // A selection is drawn as a tinted middle span; without a visible
        // range, select-all and shift-arrow would be invisible operations.
        if let Some(range) = composer.selection() {
            let text = composer.text();
            return row().flex_wrap().children([
                gpui::div()
                    .text_color(rgb(active().text))
                    .child(text[..range.start].to_string())
                    .into_any_element(),
                gpui::div()
                    .bg(rgb(active().accent))
                    .text_color(rgb(active().on_accent))
                    .child(text[range.clone()].to_string())
                    .into_any_element(),
                gpui::div()
                    .text_color(rgb(active().text))
                    .child(text[range.end..].to_string())
                    .into_any_element(),
            ]);
        }

        // The caret is drawn by splitting the buffer at the cursor. A measured
        // caret needs text metrics GPUI does not expose here; splitting keeps
        // it correct for the common case of typing at the end.
        let (before, after) = composer
            .text()
            .split_at(composer.cursor.min(composer.text().len()));

        // Only the line containing the caret is split; the rest render whole,
        // so a pasted multi-line block keeps its line breaks.
        let before_head = before.rsplit_once('\n');
        let after_tail = after.split_once('\n');

        let mut column_view = column().w_full();

        if let Some((head, _)) = before_head {
            column_view = column_view.child(
                gpui::div()
                    .text_color(rgb(active().text))
                    .child(head.to_string()),
            );
        }

        let caret_before = before_head.map(|(_, tail)| tail).unwrap_or(before);
        let caret_after = after_tail.map(|(head, _)| head).unwrap_or(after);

        column_view = column_view.child(
            row()
                .child(
                    gpui::div()
                        .text_color(rgb(active().text))
                        .child(caret_before.to_string()),
                )
                .when(focused, |d| {
                    d.child(
                        gpui::div()
                            .w(px(1.))
                            .h(px(text::BASE + 4.))
                            .bg(rgb(active().accent)),
                    )
                })
                .child(
                    gpui::div()
                        .text_color(rgb(active().text))
                        .child(caret_after.to_string()),
                ),
        );

        if let Some((_, tail)) = after_tail {
            column_view = column_view.child(
                gpui::div()
                    .text_color(rgb(active().text))
                    .child(tail.to_string()),
            );
        }

        column_view
    };

    gpui::div()
        .w_full()
        .px(px(space::LG))
        .pb(px(space::LG))
        .child(
            row()
                .w_full()
                .min_h(px(42.))
                .px(px(space::MD))
                .py(px(space::SM))
                .rounded(px(layout::RADIUS_LG))
                .bg(rgb(active().surface_hover))
                .border_1()
                .border_color(rgb(if focused && enabled {
                    active().accent
                } else {
                    active().border
                }))
                .text_size(px(scaled(text::BASE)))
                .child(content),
        )
}

#[cfg(test)]
mod selection_tests {
    use super::*;

    fn buffer(text: &str) -> Composer {
        let mut composer = Composer::default();
        composer.set_text(text);
        composer
    }

    #[test]
    fn select_all_covers_the_whole_buffer() {
        let mut composer = buffer("hello");
        composer.select_all();
        assert_eq!(composer.selected_text(), Some("hello"));
    }

    #[test]
    fn typing_replaces_a_selection() {
        let mut composer = buffer("hello");
        composer.select_all();
        composer.insert("bye");
        assert_eq!(composer.text(), "bye");
        assert!(composer.selection().is_none());
    }

    #[test]
    fn backspace_deletes_a_selection_rather_than_one_char() {
        let mut composer = buffer("hello");
        composer.select_all();
        composer.backspace();
        assert_eq!(composer.text(), "");
    }

    #[test]
    fn a_collapsed_selection_is_no_selection() {
        let mut composer = buffer("hi");
        composer.anchor_for_extend(true);
        // Anchor set but cursor unmoved: nothing is actually selected.
        assert!(composer.selection().is_none());
    }

    #[test]
    fn undo_restores_the_previous_text() {
        let mut composer = buffer("draft");
        composer.insert(" more");
        assert_eq!(composer.text(), "draft more");

        composer.undo();
        assert_eq!(composer.text(), "draft");
    }

    #[test]
    fn typing_coalesces_into_one_undo_step() {
        let mut composer = Composer::default();
        for ch in ["a", "b", "c"] {
            composer.insert(ch);
        }
        assert_eq!(composer.text(), "abc");

        // One undo should clear the whole run, not just the last character;
        // otherwise undo is unusable while typing.
        composer.undo();
        assert_eq!(composer.text(), "");
    }

    #[test]
    fn deletion_is_a_separate_undo_step_from_typing() {
        let mut composer = Composer::default();
        composer.insert("word");
        composer.backspace();
        assert_eq!(composer.text(), "wor");

        composer.undo();
        assert_eq!(composer.text(), "word", "the deletion undoes on its own");
    }

    #[test]
    fn redo_reapplies_an_undone_edit() {
        let mut composer = buffer("a");
        composer.insert("b");
        composer.undo();
        assert_eq!(composer.text(), "a");

        composer.redo();
        assert_eq!(composer.text(), "ab");
    }

    #[test]
    fn a_new_edit_discards_the_redo_stack() {
        let mut composer = buffer("a");
        composer.insert("b");
        composer.undo();
        composer.insert("c");

        composer.redo();
        assert_eq!(
            composer.text(),
            "ac",
            "redo must not resurrect a lost branch"
        );
    }

    #[test]
    fn undo_history_does_not_grow_without_bound() {
        let mut composer = Composer::default();
        for index in 0..300 {
            // Alternating insert and delete defeats coalescing, so each is a
            // distinct step.
            composer.insert(&index.to_string());
            composer.backspace();
        }
        assert!(composer.undo.len() <= 128);
    }

    #[test]
    fn taking_the_buffer_clears_its_history() {
        let mut composer = buffer("sent");
        composer.insert("!");
        let _ = composer.take();

        // Undo after send must not resurrect the sent message.
        composer.undo();
        assert_eq!(composer.text(), "");
    }

    #[test]
    fn selection_survives_multibyte_text() {
        let mut composer = buffer("héllo");
        composer.select_all();
        assert_eq!(composer.selected_text(), Some("héllo"));

        composer.insert("x");
        assert_eq!(composer.text(), "x");
    }
}
