use std::ops::Range;

use super::text_cursor::{
    clamp_cursor_index, next_char_boundary, next_word_boundary, previous_char_boundary,
    previous_word_boundary, vertical_cursor_target,
};

#[derive(Clone, Default, Eq, PartialEq)]
pub struct TextInputState {
    value: String,
    cursor_byte_index: usize,
    /// Whether this holds a credential. Set once at creation; it makes the
    /// value render as bullets and keeps it out of `{:?}`.
    masked: bool,
}

// Hand-written rather than derived: the derive would print a password in full
// from any `{:?}` of a popup, which is what a debug log or a failing test
// assertion does.
impl std::fmt::Debug for TextInputState {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TextInputState")
            .field(
                "value",
                if self.masked {
                    &"[redacted]"
                } else {
                    &self.value
                },
            )
            .field("cursor_byte_index", &self.cursor_byte_index)
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TextEditAction {
    DeletePreviousChar,
    DeletePreviousWord,
    DeleteToLineStart,
    DeleteToLineEnd,
    MoveCursorUp,
    MoveCursorDown,
    MoveCursorWordLeft,
    MoveCursorLeft,
    MoveCursorWordRight,
    MoveCursorRight,
    MoveCursorHome,
    MoveCursorEnd,
}

impl TextInputState {
    /// An input whose contents are a credential.
    pub fn masked() -> Self {
        Self {
            masked: true,
            ..Self::default()
        }
    }

    /// What to draw. Bullets for a masked input, so a password is not put on
    /// screen in a terminal someone else may be looking at.
    pub fn display_value(&self) -> String {
        if self.masked {
            "•".repeat(self.value.chars().count())
        } else {
            self.value.clone()
        }
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    pub fn cursor_byte_index(&self) -> usize {
        clamp_cursor_index(&self.value, self.cursor_byte_index)
    }

    pub fn set_value(&mut self, value: String) {
        self.cursor_byte_index = value.len();
        self.value = value;
    }

    pub fn clear(&mut self) {
        self.value.clear();
        self.cursor_byte_index = 0;
    }

    pub fn insert_char(&mut self, value: char) {
        let cursor = self.cursor_byte_index();
        self.value.insert(cursor, value);
        self.cursor_byte_index = cursor + value.len_utf8();
    }

    pub fn insert_str(&mut self, value: &str) {
        if value.is_empty() {
            return;
        }
        let cursor = self.cursor_byte_index();
        self.value.insert_str(cursor, value);
        self.cursor_byte_index = cursor + value.len();
    }

    pub fn replace_range(&mut self, range: Range<usize>, replacement: &str) -> bool {
        if range.start > range.end
            || range.end > self.value.len()
            || !self.value.is_char_boundary(range.start)
            || !self.value.is_char_boundary(range.end)
        {
            return false;
        }
        self.value.replace_range(range.clone(), replacement);
        self.cursor_byte_index = range.start + replacement.len();
        true
    }

    pub fn delete_previous_grapheme(&mut self) -> bool {
        let end = self.cursor_byte_index();
        if end == 0 {
            return false;
        }
        let start = previous_char_boundary(&self.value, end);
        self.replace_range(start..end, "")
    }

    pub fn delete_previous_word(&mut self) -> bool {
        let end = self.cursor_byte_index();
        if end == 0 {
            return false;
        }
        let start = previous_word_boundary(&self.value, end);
        self.replace_range(start..end, "")
    }

    pub fn delete_to_line_start(&mut self) -> bool {
        let end = self.cursor_byte_index();
        let start = self.value[..end].rfind('\n').map_or(0, |index| index + 1);
        if start == end {
            return false;
        }
        self.replace_range(start..end, "")
    }

    pub fn delete_to_line_end(&mut self) -> bool {
        let start = self.cursor_byte_index();
        let end = self.value[start..]
            .find('\n')
            .map_or(self.value.len(), |offset| start + offset);
        if start == end {
            return false;
        }
        self.replace_range(start..end, "")
    }

    pub fn apply_edit_action(&mut self, action: TextEditAction) -> bool {
        match action {
            TextEditAction::DeletePreviousChar => self.delete_previous_grapheme(),
            TextEditAction::DeletePreviousWord => self.delete_previous_word(),
            TextEditAction::DeleteToLineStart => self.delete_to_line_start(),
            TextEditAction::DeleteToLineEnd => self.delete_to_line_end(),
            TextEditAction::MoveCursorUp => {
                self.move_up();
                false
            }
            TextEditAction::MoveCursorDown => {
                self.move_down();
                false
            }
            TextEditAction::MoveCursorWordLeft => {
                self.move_word_left();
                false
            }
            TextEditAction::MoveCursorLeft => {
                self.move_left();
                false
            }
            TextEditAction::MoveCursorWordRight => {
                self.move_word_right();
                false
            }
            TextEditAction::MoveCursorRight => {
                self.move_right();
                false
            }
            TextEditAction::MoveCursorHome => {
                self.move_home();
                false
            }
            TextEditAction::MoveCursorEnd => {
                self.move_end();
                false
            }
        }
    }

    pub fn move_left(&mut self) {
        let cursor = self.cursor_byte_index();
        self.cursor_byte_index = previous_char_boundary(&self.value, cursor);
    }

    pub fn move_right(&mut self) {
        let cursor = self.cursor_byte_index();
        self.cursor_byte_index = next_char_boundary(&self.value, cursor);
    }

    pub fn move_word_left(&mut self) {
        let cursor = self.cursor_byte_index();
        self.cursor_byte_index = previous_word_boundary(&self.value, cursor);
    }

    pub fn move_word_right(&mut self) {
        let cursor = self.cursor_byte_index();
        self.cursor_byte_index = next_word_boundary(&self.value, cursor);
    }

    pub fn move_up(&mut self) {
        if let Some(target) = vertical_cursor_target(&self.value, self.cursor_byte_index(), -1) {
            self.cursor_byte_index = target;
        }
    }

    pub fn move_down(&mut self) {
        if let Some(target) = vertical_cursor_target(&self.value, self.cursor_byte_index(), 1) {
            self.cursor_byte_index = target;
        }
    }

    pub fn move_home(&mut self) {
        self.cursor_byte_index = 0;
    }

    pub fn move_end(&mut self) {
        self.cursor_byte_index = self.value.len();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_edits_at_utf8_cursor_boundaries() {
        let mut input = TextInputState::default();
        input.insert_str("가🇰🇷나");
        input.move_left();
        input.delete_previous_grapheme();

        assert_eq!(input.value(), "가나");
        assert_eq!(input.cursor_byte_index(), "가".len());
    }

    #[test]
    fn vertical_movement_moves_across_lines() {
        let mut input = TextInputState::default();
        input.set_value("hello\nworld".to_owned());

        // Cursor starts at the end of "world" (column 5).
        input.move_up();
        assert_eq!(input.cursor_byte_index(), "hello".len());
        input.move_down();
        assert_eq!(input.cursor_byte_index(), "hello\nworld".len());

        // No line above the first one, so up is a no-op there.
        input.move_up();
        input.move_up();
        assert_eq!(input.cursor_byte_index(), "hello".len());
    }

    #[test]
    fn replace_rejects_invalid_boundaries() {
        let mut input = TextInputState::default();
        input.insert_str("가나");

        assert!(!input.replace_range(1..2, "x"));
        assert_eq!(input.value(), "가나");
    }
}

#[cfg(test)]
mod masking_tests {
    use super::*;

    fn typed(value: &str, masked: bool) -> TextInputState {
        let mut input = if masked {
            TextInputState::masked()
        } else {
            TextInputState::default()
        };
        input.set_value(value.to_owned());
        input
    }

    #[test]
    fn a_masked_input_never_prints_its_value() {
        // The real risk is `{:?}` on a whole popup, which a debug log or a
        // failing assertion does without anyone thinking about it.
        let input = typed("hunter2", true);

        assert!(!format!("{input:?}").contains("hunter2"));
        assert!(!input.display_value().contains("hunter2"));
    }

    #[test]
    fn an_ordinary_input_is_unaffected() {
        // Masking every input would hide the emoji and channel names that the
        // other prompts exist to show.
        let input = typed("general", false);

        assert!(format!("{input:?}").contains("general"));
        assert_eq!(input.display_value(), "general");
    }

    #[test]
    fn the_masked_value_is_still_readable_for_the_request() {
        assert_eq!(typed("hunter2", true).value(), "hunter2");
    }

    #[test]
    fn bullets_count_characters_not_bytes() {
        // A multi-byte password would otherwise show more bullets than it has
        // characters, which reads as typing that did not land where it did.
        assert_eq!(typed("héllo", true).display_value().chars().count(), 5);
    }
}
