//! Message composer.
//!
//! GPUI 0.2.2 ships no text-input widget, so this implements the minimum
//! honestly needed to send a message: a focusable surface, a text buffer with
//! a cursor, and key handling. It is deliberately a *small* editor - no
//! selection, no clipboard, no multi-line - and the gaps are listed in
//! `docs/REWRITE.md` rather than papered over.

use gpui::{Div, KeyDownEvent, prelude::*, px, rgb};

use crate::theme::{DARK, layout, space, text};
use crate::ui::chrome::row;

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

    /// Apply a key event. Returns true when the message should be sent.
    pub fn handle_key(&mut self, event: &KeyDownEvent) -> bool {
        let keystroke = &event.keystroke;

        match keystroke.key.as_str() {
            "enter" if !keystroke.modifiers.shift => return !self.is_empty(),
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
                if !keystroke.modifiers.control && !keystroke.modifiers.platform {
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
}

/// Render the composer. `enabled` is false when no channel is open, in which
/// case it shows why rather than accepting input that could not be sent.
pub fn composer_view(composer: &Composer, focused: bool, enabled: bool, placeholder: &str) -> Div {
    let content: Div = if !enabled {
        gpui::div()
            .text_color(rgb(DARK.text_subtle))
            .child(placeholder.to_string())
    } else if composer.text().is_empty() {
        gpui::div()
            .text_color(rgb(DARK.text_subtle))
            .child(placeholder.to_string())
    } else {
        // Cursor is drawn by splitting the string at the caret; a real caret
        // element needs text measurement, which is a later refinement.
        let (before, after) = composer
            .text()
            .split_at(composer.cursor.min(composer.text().len()));
        row()
            .child(
                gpui::div()
                    .text_color(rgb(DARK.text))
                    .child(before.to_string()),
            )
            .when(focused, |d| {
                d.child(
                    gpui::div()
                        .w(px(1.))
                        .h(px(text::BASE + 4.))
                        .bg(rgb(DARK.accent)),
                )
            })
            .child(
                gpui::div()
                    .text_color(rgb(DARK.text))
                    .child(after.to_string()),
            )
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
                .bg(rgb(DARK.surface_hover))
                .border_1()
                .border_color(rgb(if focused && enabled {
                    DARK.accent
                } else {
                    DARK.border
                }))
                .text_size(px(text::BASE))
                .child(content),
        )
}
