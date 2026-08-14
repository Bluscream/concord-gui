//! Emoji picker.
//!
//! Unicode only. Custom guild emoji require uploading an image reference and
//! render as `:name:` here, so offering them in the picker would produce
//! reactions that cannot be sent - they are excluded rather than shown broken.
//!
//! The set is a curated shortlist rather than the full Unicode emoji tables:
//! a complete picker needs search, skin-tone variants and a font that renders
//! every codepoint, none of which are in place. What is here works.

use gpui::{Div, prelude::*, px, rgb};

use crate::theme::{DARK, layout, space, text};
use crate::ui::chrome::{column, row};

/// Picker state, anchored to the message being reacted to.
pub struct EmojiPicker<Id> {
    pub target: Id,
    /// Index of the highlighted entry, for keyboard selection.
    pub cursor: usize,
}

/// Grouped shortlist. Groups exist so the grid is scannable rather than an
/// undifferentiated wall of glyphs.
pub const GROUPS: &[(&str, &[&str])] = &[
    (
        "Reactions",
        &["👍", "👎", "❤️", "🎉", "🔥", "👀", "✅", "❌"],
    ),
    (
        "Faces",
        &["😀", "😂", "🙂", "😅", "😍", "🤔", "😐", "😴", "😭", "😡"],
    ),
    ("Hands", &["👋", "🙏", "💪", "🤝", "👏", "🫡"]),
    ("Other", &["🚀", "💡", "🐛", "📌", "⚠️", "🦀", "☕", "🎯"]),
];

/// Every glyph in display order, which is what keyboard navigation indexes.
pub fn flat() -> Vec<&'static str> {
    GROUPS
        .iter()
        .flat_map(|(_, glyphs)| glyphs.iter().copied())
        .collect()
}

/// Render the picker as a floating panel.
pub fn picker_view(
    cursor: usize,
    on_pick: impl Fn(&'static str, &mut gpui::App) + Clone + 'static,
) -> Div {
    let mut panel = column()
        .w(px(320.))
        .p(px(space::SM))
        .gap(px(space::XS))
        .rounded(px(layout::RADIUS_LG))
        .bg(rgb(DARK.surface))
        .border_1()
        .border_color(rgb(DARK.border));

    panel = panel.child(
        gpui::div()
            .text_size(px(text::XS))
            .text_color(rgb(DARK.text_subtle))
            .child("Enter to pick  ·  arrows to move  ·  Esc to close"),
    );

    let mut offset = 0usize;

    for (title, glyphs) in GROUPS {
        panel = panel.child(
            gpui::div()
                .pt(px(space::XS))
                .text_size(px(text::XS))
                .text_color(rgb(DARK.text_subtle))
                .child(*title),
        );

        let mut grid = row().flex_wrap().gap(px(space::XS));

        for (index, glyph) in glyphs.iter().enumerate() {
            let position = offset + index;
            let selected = position == cursor;
            let handler = on_pick.clone();

            grid = grid.child(
                gpui::div()
                    .id(("emoji", position))
                    .w(px(28.))
                    .h(px(28.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(layout::RADIUS))
                    .cursor_pointer()
                    .text_size(px(text::LG))
                    .when(selected, |d| d.bg(rgb(DARK.surface_active)))
                    .hover(|style| style.bg(rgb(DARK.surface_hover)))
                    .child(*glyph)
                    .on_click(move |_event, _window, cx| handler(glyph, cx)),
            );
        }

        panel = panel.child(grid);
        offset += glyphs.len();
    }

    panel
}
