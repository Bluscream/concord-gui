//! Forum channel view.
//!
//! A forum's children are threads, not messages, so opening one shows a list
//! of posts rather than a chat log. Selecting a post opens it as an ordinary
//! channel, which is what it is underneath.
//!
//! Discord returns each post's opening message alongside the thread, so the
//! preview here is real content rather than a title alone.

use gpui::{Div, prelude::*, px, rgb};

use crate::theme::{active, layout, scaled, space, text};
use crate::ui::chrome::{column, row};

/// One forum post, flattened for rendering.
pub struct ForumPost {
    pub channel_id: concord::discord::Id<concord::discord::marker::ChannelMarker>,
    pub title: String,
    /// Opening message, trimmed. Empty when Discord returned no first message.
    pub preview: String,
    pub author: String,
    pub message_count: u64,
    pub archived: bool,
}

/// State for the forum currently being browsed.
pub struct ForumView {
    pub channel_id: concord::discord::Id<concord::discord::marker::ChannelMarker>,
    pub name: String,
    pub posts: Vec<ForumPost>,
    pub loading: bool,
    /// True once Discord reports no further pages.
    pub complete: bool,
    /// Whether archived posts are being shown instead of active ones.
    pub showing_archived: bool,
    /// Offset for the next page, supplied by Discord.
    pub next_offset: usize,
    pub error: Option<String>,
}

impl ForumView {
    pub fn loading(
        channel_id: concord::discord::Id<concord::discord::marker::ChannelMarker>,
        name: String,
    ) -> Self {
        Self {
            channel_id,
            name,
            posts: Vec::new(),
            loading: true,
            complete: false,
            showing_archived: false,
            next_offset: 0,
            error: None,
        }
    }
}

/// Render the post list.
pub fn forum_view(
    view: &ForumView,
    on_open: impl Fn(usize, &mut gpui::App) + Clone + 'static,
    on_toggle_archived: impl Fn(&mut gpui::App) + Clone + 'static,
    on_load_more: impl Fn(&mut gpui::App) + Clone + 'static,
    on_new_post: impl Fn(&mut gpui::App) + Clone + 'static,
) -> Div {
    let mut panel = column().flex_1().h_full().bg(rgb(active().surface));

    panel = panel.child(
        row()
            .w_full()
            .h(px(layout::HEADER))
            .px(px(space::LG))
            .gap(px(space::SM))
            .bg(rgb(active().surface))
            .border_b_1()
            .border_color(rgb(active().border))
            .child(
                gpui::div()
                    .flex_1()
                    .text_size(px(scaled(text::BASE)))
                    .text_color(rgb(active().text))
                    .child(view.name.clone()),
            )
            .child(
                gpui::div()
                    .id("forum-archived")
                    .px(px(space::SM))
                    .py(px(space::XS))
                    .rounded(px(layout::RADIUS))
                    .cursor_pointer()
                    .text_size(px(scaled(text::XS)))
                    .bg(rgb(if view.showing_archived {
                        active().surface_active
                    } else {
                        active().surface_hover
                    }))
                    .text_color(rgb(active().text_muted))
                    .child(if view.showing_archived {
                        "Archived"
                    } else {
                        "Active"
                    })
                    .on_click(move |_event, _window, cx| on_toggle_archived(cx)),
            )
            .child(
                gpui::div()
                    .id("forum-new-post")
                    .px(px(space::SM))
                    .py(px(space::XS))
                    .rounded(px(layout::RADIUS))
                    .cursor_pointer()
                    .text_size(px(scaled(text::XS)))
                    .bg(rgb(active().accent))
                    .text_color(rgb(active().on_accent))
                    .child("New post")
                    .on_click(move |_event, _window, cx| on_new_post(cx)),
            ),
    );

    let mut list = column()
        .id("forum-posts")
        .flex_1()
        .w_full()
        .px(px(space::LG))
        .py(px(space::MD))
        .gap(px(space::SM))
        .overflow_y_scroll();

    if let Some(error) = &view.error {
        list = list.child(
            gpui::div()
                .text_size(px(scaled(text::SM)))
                .text_color(rgb(active().danger))
                .child(error.clone()),
        );
    } else if view.posts.is_empty() && !view.loading {
        list = list.child(
            gpui::div()
                .text_size(px(scaled(text::SM)))
                .text_color(rgb(active().text_subtle))
                .child(if view.showing_archived {
                    "No archived posts"
                } else {
                    "No posts yet"
                }),
        );
    }

    for (index, post) in view.posts.iter().enumerate() {
        let handler = on_open.clone();

        list = list.child(
            column()
                .id(("forum-post", index))
                .w_full()
                .p(px(space::MD))
                .gap(px(space::XS))
                .rounded(px(layout::RADIUS))
                .bg(rgb(active().surface_hover))
                .border_1()
                .border_color(rgb(active().border))
                .cursor_pointer()
                .hover(|style| style.bg(rgb(active().surface_active)))
                .when(post.archived, |d| d.opacity(0.6))
                .child(
                    row()
                        .w_full()
                        .gap(px(space::SM))
                        .child(
                            gpui::div()
                                .flex_1()
                                .text_size(px(scaled(text::BASE)))
                                .text_color(rgb(active().text))
                                .child(post.title.clone()),
                        )
                        .child(
                            gpui::div()
                                .text_size(px(scaled(text::XS)))
                                .text_color(rgb(active().text_subtle))
                                .child(format!("{} replies", post.message_count)),
                        ),
                )
                .when(!post.preview.is_empty(), |d| {
                    d.child(
                        gpui::div()
                            .text_size(px(scaled(text::SM)))
                            .text_color(rgb(active().text_muted))
                            .child(post.preview.clone()),
                    )
                })
                .child(
                    gpui::div()
                        .text_size(px(scaled(text::XS)))
                        .text_color(rgb(active().text_subtle))
                        .child(post.author.clone()),
                )
                .on_click(move |_event, _window, cx| handler(index, cx)),
        );
    }

    if view.loading {
        list = list.child(
            gpui::div()
                .text_size(px(scaled(text::XS)))
                .text_color(rgb(active().text_subtle))
                .child("Loading posts…"),
        );
    } else if !view.complete && !view.posts.is_empty() {
        // Paged explicitly rather than on scroll: a forum can be very large,
        // and fetching more only when asked keeps the request count honest.
        list = list.child(
            gpui::div()
                .id("forum-more")
                .px(px(space::MD))
                .py(px(space::SM))
                .rounded(px(layout::RADIUS))
                .cursor_pointer()
                .text_size(px(scaled(text::SM)))
                .text_color(rgb(active().accent))
                .hover(|style| style.bg(rgb(active().surface_hover)))
                .child("Load more")
                .on_click(move |_event, _window, cx| on_load_more(cx)),
        );
    }

    panel.child(list)
}
