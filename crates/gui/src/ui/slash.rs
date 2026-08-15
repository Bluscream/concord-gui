//! Slash-command autocomplete.
//!
//! Lists the core's builtin commands as the user types a leading `/`. Parsing
//! and dispatch stay in the core (`builtin_slash_commands`,
//! `parse_builtin_slash_command`), so the GUI and the TUI accept exactly the
//! same syntax rather than drifting apart.

use concord::discord::{BuiltinSlashCommandInfo, builtin_slash_commands};
use gpui::{Div, prelude::*, px, rgb};

use crate::theme::{active, layout, space, text};
use crate::ui::chrome::{column, row};

/// Autocomplete state, present only while the composer holds a `/` prefix.
pub struct SlashPicker {
    pub matches: Vec<&'static BuiltinSlashCommandInfo>,
    pub selected: usize,
}

impl SlashPicker {
    /// Build a picker for the composer's current content.
    ///
    /// Returns `None` unless the content is a lone command being typed: a `/`
    /// mid-message is ordinary text, and a completed command with arguments
    /// should show the composer rather than a menu.
    pub fn for_input(content: &str) -> Option<Self> {
        let rest = content.strip_prefix('/')?;
        if rest.contains(char::is_whitespace) || content.contains('\n') {
            return None;
        }

        let needle = rest.to_lowercase();
        let matches: Vec<_> = builtin_slash_commands()
            .iter()
            .filter(|command| command.name.starts_with(&needle))
            .collect();

        (!matches.is_empty()).then_some(Self {
            matches,
            selected: 0,
        })
    }

    pub fn move_selection(&mut self, delta: isize) {
        if self.matches.is_empty() {
            return;
        }
        let count = self.matches.len() as isize;
        self.selected = ((self.selected as isize + delta).rem_euclid(count)) as usize;
    }

    /// The replacement text for the highlighted command.
    pub fn completion(&self) -> Option<&'static str> {
        self.matches
            .get(self.selected)
            .map(|command| command.replacement)
    }
}

pub fn slash_view(picker: &SlashPicker) -> Div {
    let mut panel = column()
        .w_full()
        .rounded(px(layout::RADIUS))
        .bg(rgb(active().surface))
        .border_1()
        .border_color(rgb(active().border))
        .overflow_hidden();

    for (index, command) in picker.matches.iter().enumerate() {
        let selected = index == picker.selected;

        panel = panel.child(
            row()
                .w_full()
                .px(px(space::MD))
                .py(px(space::XS))
                .gap(px(space::SM))
                .when(selected, |d| d.bg(rgb(active().surface_active)))
                .child(
                    gpui::div()
                        .w(px(90.))
                        .text_size(px(text::SM))
                        .text_color(rgb(if selected {
                            active().text
                        } else {
                            active().text_muted
                        }))
                        .child(format!("/{}", command.name)),
                )
                .child(
                    gpui::div()
                        .flex_1()
                        .text_size(px(text::XS))
                        .text_color(rgb(active().text_subtle))
                        .child(command.description),
                ),
        );
    }

    panel
}
