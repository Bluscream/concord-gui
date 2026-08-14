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

use crate::theme::{active, layout, space, text};
use crate::ui::chrome::{column, row};

/// A single-line text buffer with a cursor.
#[derive(Default)]
pub struct Composer {
    text: String,
    /// Cursor position as a byte offset into `text`, always on a char boundary.
    cursor: usize,
}

impl Composer {
    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn is_empty(&self) -> bool {
        self.text.trim().is_empty()
    }

    /// Take the buffer's contents, resetting it.
    pub fn take(&mut self) -> String {
        self.cursor = 0;
        std::mem::take(&mut self.text)
    }

    /// Replace the buffer, putting the cursor at the end.
    pub fn set_text(&mut self, text: &str) {
        self.text = text.to_string();
        self.cursor = self.text.len();
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
    }

    pub fn insert(&mut self, input: &str) {
        self.text.insert_str(self.cursor, input);
        self.cursor += input.len();
    }

    pub fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let previous = self.text[..self.cursor]
            .char_indices()
            .next_back()
            .map(|(index, _)| index)
            .unwrap_or(0);
        self.text.replace_range(previous..self.cursor, "");
        self.cursor = previous;
    }

    pub fn delete(&mut self) {
        if self.cursor >= self.text.len() {
            return;
        }
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
        let keystroke = &event.keystroke;
        let control = keystroke.modifiers.control || keystroke.modifiers.platform;

        match keystroke.key.as_str() {
            // Shift-Enter inserts a newline; plain Enter sends.
            "enter" if keystroke.modifiers.shift => self.newline(),
            "enter" => return !self.is_empty(),
            "v" if control => {
                if let Some(text) = clipboard {
                    // Normalise line endings so pasted CRLF does not produce
                    // stray carriage returns in the sent message.
                    self.insert(&text.replace("\r\n", "\n").replace('\r', "\n"));
                }
            }
            "w" if control => self.delete_word(),
            "backspace" if control => self.delete_word(),
            "a" if control => self.move_home(),
            "e" if control => self.move_end(),
            "u" if control => {
                self.text.replace_range(..self.cursor, "");
                self.cursor = 0;
            }
            "backspace" => self.backspace(),
            "delete" => self.delete(),
            "left" => self.move_left(),
            "right" => self.move_right(),
            "home" => self.move_home(),
            "end" => self.move_end(),
            _ => {
                // `key_char` carries the text the platform produced, which
                // already accounts for layout and dead keys. Control chords
                // are filtered out so e.g. ctrl-a does not insert "a".
                if !control {
                    if let Some(input) = keystroke.key_char.as_ref() {
                        if !input.is_empty() && !input.chars().any(char::is_control) {
                            self.insert(input);
                        }
                    }
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
    let content: Div = if !enabled {
        gpui::div()
            .text_color(rgb(active().text_subtle))
            .child(placeholder.to_string())
    } else if composer.text().is_empty() {
        gpui::div()
            .text_color(rgb(active().text_subtle))
            .child(placeholder.to_string())
    } else {
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
                .text_size(px(text::BASE))
                .child(content),
        )
}
