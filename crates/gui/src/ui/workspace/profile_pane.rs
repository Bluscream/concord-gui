use gpui::{prelude::*, px, rgb, Context, IntoElement};

use concord::t;

use crate::theme::{active, layout, scaled, space, text};
use crate::ui::chrome::{column, row};
use crate::ui::profile::{profile_view, ProfileView};
use crate::ui::workspace::Workspace;

impl Workspace {
    pub(super) fn profile_pane_impl(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let Some((user_id, view)) = &self.profile else {
            return gpui::div();
        };
        let user_id = *user_id;
        let moderation = self.moderation_controls(user_id, cx);
        let friendship = self.friend_controls(user_id, cx);

        let close_listener = cx.listener(|this, _, _, cx| {
            this.close_popup();
            cx.notify();
        });

        let container = gpui::div()
            .w(px(layout::MEMBERS))
            .h_full();

        match view {
            Some(view) => container
                .child(profile_view(
                    view,
                    self.options.display.circular_avatars,
                    Some(close_listener),
                ))
                .children(friendship)
                .children(moderation),
            None => container.child(profile_view(
                &ProfileView {
                    display_name: user_id.get().to_string(),
                    handle: None,
                    avatar: None,
                    pronouns: None,
                    bio: None,
                    activities: Vec::new(),
                    roles: Vec::new(),
                    mutual_guilds: Vec::new(),
                    loaded: false,
                },
                self.options.display.circular_avatars,
                Some(close_listener),
            )),
        }
    }

    pub(super) fn search_pane_impl(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(search) = &self.search else {
            return gpui::div();
        };

        let mut pane = column()
            .w(px(layout::MEMBERS + 80.))
            .h_full()
            .bg(rgb(active().surface_sunken))
            .border_l_1()
            .border_color(rgb(active().border));

        pane = pane.child(
            row()
                .w_full()
                .h(px(layout::HEADER))
                .px(px(space::MD))
                .border_b_1()
                .border_color(rgb(active().border))
                .text_size(px(scaled(text::SM)))
                .text_color(rgb(active().text))
                .child(t!("label-search")),
        );

        pane = pane.child(
            gpui::div().w_full().p(px(space::SM)).child(
                row()
                    .w_full()
                    .min_h(px(32.))
                    .px(px(space::SM))
                    .rounded(px(layout::RADIUS))
                    .bg(rgb(active().surface))
                    .border_1()
                    .border_color(rgb(active().accent))
                    .text_size(px(scaled(text::SM)))
                    .child(if search.input.text().is_empty() {
                        gpui::div()
                            .text_color(rgb(active().text_subtle))
                            .child(t!("status-type-and-press-enter"))
                    } else {
                        gpui::div()
                            .text_color(rgb(active().text))
                            .child(search.input.text().to_string())
                    }),
            ),
        );

        let status = if search.running {
            Some("Searching…".to_string())
        } else if let Some(error) = &search.error {
            Some(error.clone())
        } else {
            search
                .total
                .map(|total| format!("{total} result{}", if total == 1 { "" } else { "s" }))
        };

        if let Some(status) = status {
            pane = pane.child(
                gpui::div()
                    .px(px(space::MD))
                    .pb(px(space::XS))
                    .text_size(px(scaled(text::XS)))
                    .text_color(rgb(active().text_subtle))
                    .child(status),
            );
        }

        let mut results = column()
            .id("search-results")
            .flex_1()
            .w_full()
            .overflow_y_scroll();

        for (index, result) in search.results.iter().enumerate() {
            let preview: String = result.content.chars().take(160).collect();
            let (channel_id, message_id) = (result.channel_id, result.message_id);

            results = results.child(
                column()
                    .id(("search-result", index))
                    .w_full()
                    .px(px(space::MD))
                    .py(px(space::SM))
                    .gap(px(2.))
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(active().surface_hover)))
                    .on_click(cx.listener(move |this, _event, _window, cx| {
                        this.jump_to(channel_id, message_id);
                        cx.notify();
                    }))
                    .child(
                        gpui::div()
                            .text_size(px(scaled(text::XS)))
                            .text_color(rgb(active().accent))
                            .child(result.author.clone()),
                    )
                    .child(
                        gpui::div()
                            .text_size(px(scaled(text::SM)))
                            .text_color(rgb(active().text_muted))
                            .child(preview),
                    ),
            );
        }

        pane.child(results)
    }
}
