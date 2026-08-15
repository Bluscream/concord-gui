//! Slash-command autocomplete.
//!
//! Lists the core's builtin commands as the user types a leading `/`. Parsing
//! and dispatch stay in the core (`builtin_slash_commands`,
//! `parse_builtin_slash_command`), so the GUI and the TUI accept exactly the
//! same syntax rather than drifting apart.

use concord::discord::{ApplicationCommandInfo, BuiltinSlashCommandInfo, builtin_slash_commands};
use gpui::{Div, prelude::*, px, rgb};

use crate::theme::{active, layout, space, text};
use crate::ui::chrome::{column, row};

/// One offered command, from either source.
///
/// Builtins are handled locally by the core; application commands are defined
/// by a bot and sent to Discord. They share a picker because a user typing `/`
/// does not care which is which, but they dispatch differently.
pub enum Entry {
    Builtin(&'static BuiltinSlashCommandInfo),
    Application {
        name: String,
        description: String,
        /// The bot providing it, shown so two commands with the same name are
        /// distinguishable.
        application: Option<String>,
    },
}

impl Entry {
    pub fn name(&self) -> &str {
        match self {
            Entry::Builtin(command) => command.name,
            Entry::Application { name, .. } => name,
        }
    }

    fn description(&self) -> &str {
        match self {
            Entry::Builtin(command) => command.description,
            Entry::Application { description, .. } => description,
        }
    }

    fn source(&self) -> Option<&str> {
        match self {
            Entry::Builtin(_) => None,
            Entry::Application { application, .. } => application.as_deref(),
        }
    }

    /// Text to place in the composer when accepted.
    pub fn replacement(&self) -> String {
        match self {
            Entry::Builtin(command) => command.replacement.to_string(),
            Entry::Application { name, .. } => format!("/{name} "),
        }
    }
}

/// Autocomplete state, present only while the composer holds a `/` prefix.
pub struct SlashPicker {
    pub matches: Vec<Entry>,
    pub selected: usize,
}

impl SlashPicker {
    /// Build a picker for the composer's current content.
    ///
    /// Returns `None` unless the content is a lone command being typed: a `/`
    /// mid-message is ordinary text, and a completed command with arguments
    /// should show the composer rather than a menu.
    pub fn for_input(content: &str, application: &[ApplicationCommandInfo]) -> Option<Self> {
        let rest = content.strip_prefix('/')?;
        if rest.contains(char::is_whitespace) || content.contains('\n') {
            return None;
        }

        let needle = rest.to_lowercase();

        // Builtins first: they always work, where an application command
        // depends on a bot being present and responsive.
        let mut matches: Vec<Entry> = builtin_slash_commands()
            .iter()
            .filter(|command| command.name.starts_with(&needle))
            .map(Entry::Builtin)
            .collect();

        matches.extend(
            application
                .iter()
                .filter(|command| command.name.to_lowercase().starts_with(&needle))
                .map(|command| Entry::Application {
                    name: command.name.clone(),
                    description: command.description.clone(),
                    application: command.application_name.clone(),
                }),
        );

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
    pub fn completion(&self) -> Option<String> {
        self.matches
            .get(self.selected)
            .map(|command| command.replacement())
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
                        .child(format!("/{}", command.name())),
                )
                .child(
                    gpui::div()
                        .flex_1()
                        .text_size(px(text::XS))
                        .text_color(rgb(active().text_subtle))
                        .child(command.description().to_string()),
                )
                .children(command.source().map(|application| {
                    gpui::div()
                        .text_size(px(text::XS))
                        .text_color(rgb(active().accent))
                        .child(application.to_string())
                })),
        );
    }

    panel
}
