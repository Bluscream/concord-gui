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
use concord::discord::custom_emoji_image_url;

use crate::model::message::{MessageRow, format_bytes};
use crate::theme::{active, layout, space, text};
use crate::ui::chrome::{avatar_with_url, column, row};

/// Width reserved to the left of message bodies, so continuation rows align
/// with the avatar column above them.
const GUTTER: f32 = layout::AVATAR + space::MD;

/// Inline custom emoji are sized to the line rather than to the text height,
/// so a line containing one does not grow taller than its neighbours.
const EMOJI_SIZE: f32 = 20.;

impl MessageAction {
    /// A stable small number, used only to build unique element ids.
    fn slot(self) -> usize {
        match self {
            MessageAction::Reply => 0,
            MessageAction::React => 1,
            MessageAction::Edit => 2,
            MessageAction::Delete => 3,
            MessageAction::ToggleReaction(_) => 4,
            MessageAction::RevealSpoiler => 5,
            MessageAction::OpenProfile => 6,
            MessageAction::LoadOlder => 7,
        }
    }
}

/// An action requested from a message's hover toolbar or its reaction bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessageAction {
    Reply,
    /// Open the emoji picker for this message.
    React,
    Edit,
    Delete,
    /// Open the author's profile.
    OpenProfile,
    /// Fetch the page of messages before the oldest one loaded.
    LoadOlder,
    /// Toggle an existing reaction, identified by its index in the row's
    /// reaction list. Carrying the index avoids threading emoji identity
    /// through the callback.
    ToggleReaction(usize),
    /// Reveal a spoiler that was hidden in this message.
    RevealSpoiler,
}

/// Render the full message list, oldest first.
///
/// GPUI requires a stateful element (one with an id) for a scroll container,
/// so the list carries a stable id and scroll position survives re-renders.
pub fn message_list(
    rows: &[MessageRow],
    show_avatars: bool,
    circular_avatars: bool,
    hour24: bool,
    show_emoji: bool,
    on_action: impl Fn(usize, MessageAction, &mut gpui::App) + Clone + 'static,
) -> impl IntoElement {
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
                    .text_color(rgb(active().text_subtle))
                    .child("No messages loaded"),
            ),
        );
    }

    // Scrollback is fetched on request rather than on scroll: a channel can
    // have years of history, and paging only when asked keeps the request
    // count predictable.
    if !rows.is_empty() {
        let handler = on_action.clone();
        list = list.child(
            gpui::div()
                .id("load-older")
                .w_full()
                .py(px(space::SM))
                .flex()
                .justify_center()
                .cursor_pointer()
                .text_size(px(text::XS))
                .text_color(rgb(active().accent))
                .hover(|style| style.bg(rgb(active().surface_hover)))
                .child("Load earlier messages")
                .on_click(move |_event, _window, cx| handler(0, MessageAction::LoadOlder, cx)),
        );
    }

    for (index, message) in rows.iter().enumerate() {
        list = list.child(message_row(
            index,
            message,
            show_avatars,
            circular_avatars,
            hour24,
            show_emoji,
            on_action.clone(),
        ));
    }

    list
}

/// A message plus its hover toolbar.
///
/// The toolbar is absolutely positioned and only visible on hover, so it never
/// reflows the log - a toolbar that pushed text around on mouse-over would
/// make the list feel unstable while scanning it.
fn message_row(
    index: usize,
    message: &MessageRow,
    show_avatars: bool,
    circular_avatars: bool,
    hour24: bool,
    show_emoji: bool,
    on_action: impl Fn(usize, MessageAction, &mut gpui::App) + Clone + 'static,
) -> impl IntoElement {
    let own = message.own;

    gpui::div()
        .id(("message", index))
        .relative()
        .w_full()
        .group("message")
        .hover(|style| style.bg(rgb(active().surface_hover)))
        .child(message_block(
            index,
            message,
            show_avatars,
            circular_avatars,
            hour24,
            show_emoji,
            on_action.clone(),
        ))
        .child(
            gpui::div()
                .absolute()
                .top(px(-8.))
                .right(px(space::MD))
                .invisible()
                .group_hover("message", |style| style.visible())
                .child(action_bar(index, own, on_action)),
        )
}

fn action_bar(
    index: usize,
    own: bool,
    on_action: impl Fn(usize, MessageAction, &mut gpui::App) + Clone + 'static,
) -> Div {
    let mut bar = row()
        .gap(px(2.))
        .p(px(2.))
        .rounded(px(layout::RADIUS))
        .bg(rgb(active().surface))
        .border_1()
        .border_color(rgb(active().border));

    let button = |label: &'static str, action: MessageAction, danger: bool| {
        let handler = on_action.clone();
        gpui::div()
            .id(("action", index * 8 + action.slot()))
            .px(px(6.))
            .py(px(2.))
            .rounded(px(3.))
            .cursor_pointer()
            .text_size(px(text::XS))
            .text_color(rgb(if danger {
                active().danger
            } else {
                active().text_muted
            }))
            .hover(|style| style.bg(rgb(active().surface_hover)))
            .child(label)
            .on_click(move |_event, _window, cx| handler(index, action, cx))
    };

    bar = bar
        .child(button("reply", MessageAction::Reply, false))
        .child(button("react", MessageAction::React, false));

    // Edit and delete are only offered on the user's own messages; showing
    // them otherwise would invite a request the server will reject.
    if own {
        bar = bar
            .child(button("edit", MessageAction::Edit, false))
            .child(button("delete", MessageAction::Delete, true));
    }

    bar
}

fn message_block(
    index: usize,
    message: &MessageRow,
    show_avatars: bool,
    circular_avatars: bool,
    hour24: bool,
    show_emoji: bool,
    on_action: impl Fn(usize, MessageAction, &mut gpui::App) + Clone + 'static,
) -> Div {
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
                        .text_color(rgb(active().text_subtle))
                        .child(message.short_time(hour24)),
                )
                .child(message_body(index, message, show_emoji, on_action)),
        )
    } else {
        block
            .child(
                row()
                    .w_full()
                    .items_center()
                    .gap(px(space::MD))
                    .when(show_avatars, |header| {
                        header.child(
                            gpui::div()
                                .id(("author-avatar", index))
                                .cursor_pointer()
                                .on_click({
                                    let handler = on_action.clone();
                                    move |_event, _window, cx| {
                                        handler(index, MessageAction::OpenProfile, cx)
                                    }
                                })
                                .child(avatar_with_url(
                                    layout::AVATAR,
                                    &message.author,
                                    message.author_avatar.as_deref(),
                                    circular_avatars,
                                )),
                        )
                    })
                    .child(author_line(message, hour24)),
            )
            .child(
                row()
                    .w_full()
                    .items_start()
                    .child(gpui::div().w(px(GUTTER)).flex_none())
                    .child(message_body(index, message, show_emoji, on_action)),
            )
    }
}

fn author_line(message: &MessageRow, hour24: bool) -> Div {
    let name_color = message.author_color.unwrap_or(active().text);

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
                .bg(rgb(active().accent))
                .text_size(px(text::XS))
                .text_color(rgb(active().on_accent))
                .child("BOT"),
        );
    }

    line = line.child(
        gpui::div()
            .text_size(px(text::XS))
            .text_color(rgb(active().text_subtle))
            .child(message.long_time(hour24)),
    );

    if message.pinned {
        line = line.child(
            gpui::div()
                .text_size(px(text::XS))
                .text_color(rgb(active().text_subtle))
                .child("pinned"),
        );
    }

    line
}

fn message_body(
    index: usize,
    message: &MessageRow,
    show_emoji: bool,
    on_action: impl Fn(usize, MessageAction, &mut gpui::App) + Clone + 'static,
) -> Div {
    let mut body = column().flex_1().gap(px(space::XS));

    if !message.body.text.is_empty() {
        let has_spoiler = message.body.runs.iter().any(|(_, style)| style.spoiler);
        let handler = on_action.clone();

        body = body.child(
            gpui::div()
                .id(("body", index))
                .when(has_spoiler && !message.spoiler_revealed, |d| {
                    d.cursor_pointer().on_click(move |_event, _window, cx| {
                        handler(index, MessageAction::RevealSpoiler, cx)
                    })
                })
                .child(rich_body(
                    &message.body,
                    message.spoiler_revealed,
                    show_emoji,
                )),
        );

        if message.edited {
            body = body.child(
                gpui::div()
                    .text_size(px(text::XS))
                    .text_color(rgb(active().text_subtle))
                    .child("edited"),
            );
        }
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
                .text_color(rgb(active().text_subtle))
                .child(format!(
                    "{} embed{}",
                    message.embed_count,
                    if message.embed_count == 1 { "" } else { "s" }
                )),
        );
    }

    if !message.reactions.is_empty() {
        body = body.child(reaction_bar(index, &message.reactions, on_action));
    }

    body
}

/// Render markdown as a single styled text element.
///
/// One element rather than a row of styled spans, so wrapping happens at word
/// boundaries across style changes instead of at segment boundaries.
/// Split a parsed body at custom-emoji runs.
///
/// Emoji are the only thing that cannot live inside a `StyledText`, so the
/// body is cut around them and reassembled as a wrapping row. Text between
/// emoji is still handed to `StyledText` whole, so it wraps at word
/// boundaries as before; only the emoji themselves are atomic.
///
/// A message with no custom emoji produces exactly one segment, which is the
/// common case and is rendered identically to before.
fn segments(parsed: &markdown::Parsed) -> Vec<Segment> {
    let mut emoji: Vec<(std::ops::Range<usize>, u64, bool)> = parsed
        .runs
        .iter()
        .filter_map(|(range, style)| match style.kind {
            Kind::Emoji { id, animated } => Some((range.clone(), id, animated)),
            _ => None,
        })
        .collect();
    emoji.sort_by_key(|(range, _, _)| range.start);

    if emoji.is_empty() {
        return vec![Segment::Text(0..parsed.text.len())];
    }

    let mut out = Vec::new();
    let mut cursor = 0usize;

    for (range, id, animated) in emoji {
        if range.start > cursor {
            out.push(Segment::Text(cursor..range.start));
        }
        out.push(Segment::Emoji { id, animated });
        cursor = range.end;
    }

    if cursor < parsed.text.len() {
        out.push(Segment::Text(cursor..parsed.text.len()));
    }

    out
}

enum Segment {
    Text(std::ops::Range<usize>),
    Emoji { id: u64, animated: bool },
}

/// Render a body, drawing custom emoji as images.
fn rich_body(parsed: &markdown::Parsed, reveal_spoilers: bool, show_emoji: bool) -> Div {
    let parts = segments(parsed);

    // Fast path: no custom emoji, so no need to wrap in a row.
    if !show_emoji || parts.len() == 1 {
        return gpui::div().child(rich_text(parsed, reveal_spoilers));
    }

    let mut wrapper = row().flex_wrap().items_center().gap(px(2.));

    for part in parts {
        match part {
            Segment::Text(range) => {
                let slice = sub_parsed(parsed, range);
                if !slice.text.trim().is_empty() {
                    wrapper = wrapper.child(rich_text(&slice, reveal_spoilers));
                }
            }
            Segment::Emoji { id, animated } => {
                wrapper = wrapper.child(
                    gpui::img(gpui::SharedUri::from(custom_emoji_image_url(id, animated)))
                        .w(px(EMOJI_SIZE))
                        .h(px(EMOJI_SIZE)),
                );
            }
        }
    }

    gpui::div().child(wrapper)
}

/// Extract a sub-range of a parsed body, rebasing its style runs.
fn sub_parsed(parsed: &markdown::Parsed, range: std::ops::Range<usize>) -> markdown::Parsed {
    let runs = parsed
        .runs
        .iter()
        .filter_map(|(run, style)| {
            let start = run.start.max(range.start);
            let end = run.end.min(range.end);
            (start < end).then(|| ((start - range.start)..(end - range.start), *style))
        })
        .collect();

    markdown::Parsed {
        text: parsed.text[range].to_string(),
        runs,
    }
}

fn rich_text(parsed: &markdown::Parsed, reveal_spoilers: bool) -> impl IntoElement {
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
                Kind::Mention(_) | Kind::Role(_) => rgb(active().accent),
                Kind::Channel(_) | Kind::Url => rgb(active().accent_hover),
                Kind::Emoji { .. } | Kind::Timestamp => rgb(active().text_muted),
                Kind::Text => {
                    if style.code {
                        rgb(active().warning)
                    } else if style.quote {
                        rgb(active().text_muted)
                    } else {
                        rgb(active().text)
                    }
                }
            }
            .into(),
        );

        // Hidden spoilers paint the text in its own background colour. Once
        // revealed they keep the block tint, so it stays clear which part of
        // the message was spoilered.
        if style.spoiler {
            highlight.background_color = Some(rgb(active().surface_active).into());
            if !reveal_spoilers {
                highlight.color = Some(rgb(active().surface_active).into());
            }
        }

        (range.clone(), highlight)
    });

    gpui::div()
        .text_size(px(text::BASE))
        .text_color(rgb(active().text))
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
        .text_color(rgb(active().text_subtle))
        .child(gpui::div().child("\u{21b3}"))
        .child(
            gpui::div()
                .text_color(rgb(active().text_muted))
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
        .bg(rgb(active().surface_hover))
        .border_1()
        .border_color(rgb(active().border))
        .text_size(px(text::SM))
        .child(
            gpui::div()
                .text_color(rgb(active().text_subtle))
                .child(if is_image { "IMG" } else { "FILE" }),
        )
        .child(
            gpui::div()
                .text_color(rgb(active().text))
                .child(filename.to_string()),
        )
        .child(
            gpui::div()
                .text_size(px(text::XS))
                .text_color(rgb(active().text_subtle))
                .child(format_bytes(size)),
        )
}

fn reaction_bar(
    message_index: usize,
    reactions: &[(String, u64, bool)],
    on_action: impl Fn(usize, MessageAction, &mut gpui::App) + Clone + 'static,
) -> Div {
    let mut bar = row().gap(px(space::XS)).flex_wrap();

    for (reaction_index, (glyph, count, mine)) in reactions.iter().enumerate() {
        let handler = on_action.clone();
        bar = bar.child(
            row()
                .id(("reaction", message_index * 32 + reaction_index))
                .cursor_pointer()
                .on_click(move |_event, _window, cx| {
                    handler(
                        message_index,
                        MessageAction::ToggleReaction(reaction_index),
                        cx,
                    )
                })
                .gap(px(space::XS))
                .px(px(6.))
                .py(px(2.))
                .rounded(px(layout::RADIUS))
                .bg(rgb(if *mine {
                    active().surface_active
                } else {
                    active().surface_hover
                }))
                .when(*mine, |d| d.border_1().border_color(rgb(active().accent)))
                .text_size(px(text::XS))
                .child(gpui::div().child(glyph.clone()))
                .child(
                    gpui::div()
                        .text_color(rgb(active().text_muted))
                        .child(count.to_string()),
                ),
        );
    }

    bar
}
