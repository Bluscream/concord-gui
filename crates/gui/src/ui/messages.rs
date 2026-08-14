//! Message list rendering.
//!
//! Layout follows the grouped-block model established in `model::message`:
//! the first row of a block carries an avatar, author name and timestamp; a
//! continuation row is indented to align with the block body and shows its
//! timestamp only in the gutter. That gutter alignment is most of what makes
//! a dense log scannable.

use gpui::{Div, prelude::*, px, rgb};

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
        // Markdown and mention rendering are not implemented yet; the raw
        // content is shown verbatim rather than partially parsed.
        let mut line = message.content.clone();
        if message.edited {
            line.push_str("  (edited)");
        }
        body = body.child(
            gpui::div()
                .text_size(px(text::BASE))
                .text_color(rgb(DARK.text))
                .child(line),
        );
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
