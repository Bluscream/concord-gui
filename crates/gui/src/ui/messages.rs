//! Message list rendering.
//!
//! Layout follows the grouped-block model established in `model::message`:
//! the first row of a block carries an avatar, author name and timestamp; a
//! continuation row is indented to align with the block body and shows its
//! timestamp only in the gutter. That gutter alignment is most of what makes
//! a dense log scannable.

use gpui::{
    Div, FontStyle, FontWeight, HighlightStyle, StrikethroughStyle, StyledText, UnderlineStyle,
    prelude::*, px, rgb,
};

use crate::model::markdown::{self, Kind};

use crate::model::message::{MessageRow, format_bytes};
use crate::theme::{DARK, layout, space, text};
use crate::ui::chrome::{avatar, column, row};

/// Width reserved to the left of message bodies, so continuation rows align
/// with the avatar column above them.
const GUTTER: f32 = layout::AVATAR + space::MD;

/// Render the full message list, oldest first.
///
/// GPUI requires a stateful element (one with an id) for a scroll container,
/// so the list carries a stable id and scroll position survives re-renders.
pub fn message_list(rows: &[MessageRow]) -> impl IntoElement {
    let mut list = column()
        .id("message-list")
        .flex_1()
        .w_full()
        .px(px(space::LG))
        .py(px(space::MD))
        .overflow_y_scroll();

    if rows.is_empty() {
        return list.child(
            column().flex_1().items_center().justify_center().child(
                gpui::div()
                    .text_size(px(text::SM))
                    .text_color(rgb(DARK.text_subtle))
                    .child("No messages loaded"),
            ),
        );
    }

    for message in rows {
        list = list.child(message_block(message));
    }

    list
}

fn message_block(message: &MessageRow) -> Div {
    let mut block = column()
        .w_full()
        .when(!message.continues, |d| d.pt(px(space::MD)))
        .when(message.continues, |d| d.pt(px(2.)));

    if let Some((author, content)) = &message.reply_to {
        block = block.child(reply_context(author, content));
    }

    if message.continues {
        block.child(
            row()
                .w_full()
                .items_start()
                .child(
                    // Timestamp gutter, mirroring the avatar column width.
                    gpui::div()
                        .w(px(GUTTER))
                        .flex_none()
                        .text_size(px(text::XS))
                        .text_color(rgb(DARK.text_subtle))
                        .child(message.short_time()),
                )
                .child(message_body(message)),
        )
    } else {
        block
            .child(
                row()
                    .w_full()
                    .items_center()
                    .gap(px(space::MD))
                    .child(avatar(layout::AVATAR, &message.author))
                    .child(author_line(message)),
            )
            .child(
                row()
                    .w_full()
                    .items_start()
                    .child(gpui::div().w(px(GUTTER)).flex_none())
                    .child(message_body(message)),
            )
    }
}

fn author_line(message: &MessageRow) -> Div {
    let name_color = message.author_color.unwrap_or(DARK.text);

    let mut line = row().gap(px(space::SM)).child(
        gpui::div()
            .text_size(px(text::BASE))
            .text_color(rgb(name_color))
            .child(message.author.clone()),
    );

    if message.author_is_bot {
        line = line.child(
            gpui::div()
                .px(px(4.))
                .rounded(px(3.))
                .bg(rgb(DARK.accent))
                .text_size(px(text::XS))
                .text_color(rgb(DARK.on_accent))
                .child("BOT"),
        );
    }

    line = line.child(
        gpui::div()
            .text_size(px(text::XS))
            .text_color(rgb(DARK.text_subtle))
            .child(message.long_time()),
    );

    if message.pinned {
        line = line.child(
            gpui::div()
                .text_size(px(text::XS))
                .text_color(rgb(DARK.text_subtle))
                .child("pinned"),
        );
    }

    line
}

fn message_body(message: &MessageRow) -> Div {
    let mut body = column().flex_1().gap(px(space::XS));

    if !message.content.is_empty() {
        let mut source = message.content.clone();
        if message.edited {
            source.push_str("  (edited)");
        }
        body = body.child(rich_text(&source));
    }

    for attachment in &message.attachments {
        body = body.child(attachment_chip(
            &attachment.filename,
            attachment.size_bytes,
            attachment.is_image,
        ));
    }

    if message.embed_count > 0 {
        body = body.child(
            gpui::div()
                .text_size(px(text::XS))
                .text_color(rgb(DARK.text_subtle))
                .child(format!(
                    "{} embed{}",
                    message.embed_count,
                    if message.embed_count == 1 { "" } else { "s" }
                )),
        );
    }

    if !message.reactions.is_empty() {
        body = body.child(reaction_bar(&message.reactions));
    }

    body
}

/// Render markdown as a single styled text element.
///
/// One element rather than a row of styled spans, so wrapping happens at word
/// boundaries across style changes instead of at segment boundaries.
fn rich_text(source: &str) -> impl IntoElement {
    let parsed = markdown::parse(source);

    let highlights = parsed.runs.iter().map(|(range, style)| {
        let mut highlight = HighlightStyle::default();

        if style.bold {
            highlight.font_weight = Some(FontWeight::BOLD);
        }
        if style.italic {
            highlight.font_style = Some(FontStyle::Italic);
        }
        if style.underline {
            highlight.underline = Some(UnderlineStyle {
                thickness: px(1.),
                ..Default::default()
            });
        }
        if style.strike {
            highlight.strikethrough = Some(StrikethroughStyle {
                thickness: px(1.),
                ..Default::default()
            });
        }

        // Colour is driven by semantic kind, then by the remaining modifiers.
        highlight.color = Some(
            match style.kind {
                Kind::Mention | Kind::Role => rgb(DARK.accent),
                Kind::Channel | Kind::Url => rgb(DARK.accent_hover),
                Kind::Emoji | Kind::Timestamp => rgb(DARK.text_muted),
                Kind::Text => {
                    if style.code {
                        rgb(DARK.warning)
                    } else if style.quote {
                        rgb(DARK.text_muted)
                    } else {
                        rgb(DARK.text)
                    }
                }
            }
            .into(),
        );

        // Spoilers are hidden by painting text on its own background. Click to
        // reveal needs per-run hit testing, which StyledText does not expose.
        if style.spoiler {
            highlight.background_color = Some(rgb(DARK.surface_active).into());
            highlight.color = Some(rgb(DARK.surface_active).into());
        }

        (range.clone(), highlight)
    });

    gpui::div()
        .text_size(px(text::BASE))
        .text_color(rgb(DARK.text))
        .child(StyledText::new(parsed.text.clone()).with_highlights(highlights.collect::<Vec<_>>()))
}

fn reply_context(author: &str, content: &str) -> Div {
    // Replies are truncated to a single line: the full message is one click
    // away in the log itself.
    let preview: String = content.chars().take(120).collect();

    row()
        .w_full()
        .gap(px(space::SM))
        .pl(px(GUTTER))
        .text_size(px(text::XS))
        .text_color(rgb(DARK.text_subtle))
        .child(gpui::div().child("\u{21b3}"))
        .child(
            gpui::div()
                .text_color(rgb(DARK.text_muted))
                .child(author.to_string()),
        )
        .child(gpui::div().child(preview))
}

fn attachment_chip(filename: &str, size: u64, is_image: bool) -> Div {
    row()
        .gap(px(space::SM))
        .px(px(space::SM))
        .py(px(space::XS))
        .rounded(px(layout::RADIUS))
        .bg(rgb(DARK.surface_hover))
        .border_1()
        .border_color(rgb(DARK.border))
        .text_size(px(text::SM))
        .child(
            gpui::div()
                .text_color(rgb(DARK.text_subtle))
                .child(if is_image { "IMG" } else { "FILE" }),
        )
        .child(
            gpui::div()
                .text_color(rgb(DARK.text))
                .child(filename.to_string()),
        )
        .child(
            gpui::div()
                .text_size(px(text::XS))
                .text_color(rgb(DARK.text_subtle))
                .child(format_bytes(size)),
        )
}

fn reaction_bar(reactions: &[(String, u64, bool)]) -> Div {
    let mut bar = row().gap(px(space::XS)).flex_wrap();

    for (glyph, count, mine) in reactions {
        bar = bar.child(
            row()
                .gap(px(space::XS))
                .px(px(6.))
                .py(px(2.))
                .rounded(px(layout::RADIUS))
                .bg(rgb(if *mine {
                    DARK.surface_active
                } else {
                    DARK.surface_hover
                }))
                .when(*mine, |d| d.border_1().border_color(rgb(DARK.accent)))
                .text_size(px(text::XS))
                .child(gpui::div().child(glyph.clone()))
                .child(
                    gpui::div()
                        .text_color(rgb(DARK.text_muted))
                        .child(count.to_string()),
                ),
        );
    }

    bar
}
