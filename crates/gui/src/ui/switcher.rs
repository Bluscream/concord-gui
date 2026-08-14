//! Quick switcher.
//!
//! Type to jump to any channel, thread or DM across every guild. This is the
//! navigation path a keyboard user actually lives in, and its absence was the
//! largest gap against the TUI.
//!
//! Scoring reuses the TUI's matcher rather than a second implementation, so
//! the same query ranks the same way in both clients.

use concord::discord::{Id, marker};
use gpui::{Div, prelude::*, px, rgb};

use crate::theme::{active, layout, space, text};
use crate::ui::chrome::{column, row};
use crate::ui::workspace::ChannelKind;

/// One switcher candidate.
pub struct Candidate {
    pub channel_id: Id<marker::ChannelMarker>,
    /// Guild the channel belongs to, so selecting it can switch guild too.
    pub guild_id: Option<Id<marker::GuildMarker>>,
    pub name: String,
    /// Guild name, or "Direct Messages", shown as context.
    pub context: String,
    pub kind: ChannelKind,
    pub unread: bool,
}

#[derive(Default)]
pub struct Switcher {
    pub query: crate::ui::composer::Composer,
    /// Candidates matching the query, best first.
    pub results: Vec<Candidate>,
    pub selected: usize,
}

impl Switcher {
    /// Rank candidates against the query.
    ///
    /// An empty query lists unread channels first: opening the switcher with
    /// nothing typed is usually "where do I need to look", not "show me
    /// everything".
    pub fn rank(&mut self, mut all: Vec<Candidate>) {
        let query = self.query.text().trim().to_string();

        if query.is_empty() {
            all.sort_by_key(|candidate| (!candidate.unread, candidate.name.to_lowercase()));
            all.truncate(50);
            self.results = all;
        } else {
            let mut scored: Vec<_> = all
                .into_iter()
                .filter_map(|candidate| {
                    // Matched against "name context" so "gen rost" finds
                    // #general in RostFaden.
                    let haystack = format!("{} {}", candidate.name, candidate.context);
                    concord::tui::fuzzy::fuzzy_text_score(&haystack, &query)
                        .map(|score| (score, candidate))
                })
                .collect();

            scored.sort_by(|(a, _), (b, _)| a.cmp(b));
            self.results = scored
                .into_iter()
                .map(|(_, candidate)| candidate)
                .take(50)
                .collect();
        }

        self.selected = self.selected.min(self.results.len().saturating_sub(1));
    }

    pub fn move_selection(&mut self, delta: isize) {
        if self.results.is_empty() {
            return;
        }
        let count = self.results.len() as isize;
        self.selected = ((self.selected as isize + delta).rem_euclid(count)) as usize;
    }

    pub fn selection(&self) -> Option<&Candidate> {
        self.results.get(self.selected)
    }
}

pub fn switcher_view(switcher: &Switcher) -> Div {
    let mut panel = column()
        .w(px(560.))
        .rounded(px(layout::RADIUS_LG))
        .bg(rgb(active().surface))
        .border_1()
        .border_color(rgb(active().border))
        .overflow_hidden();

    panel = panel.child(
        row()
            .w_full()
            .h(px(44.))
            .px(px(space::LG))
            .border_b_1()
            .border_color(rgb(active().border))
            .text_size(px(text::BASE))
            .child(if switcher.query.text().is_empty() {
                gpui::div()
                    .text_color(rgb(active().text_subtle))
                    .child("Jump to a channel or conversation")
            } else {
                gpui::div()
                    .text_color(rgb(active().text))
                    .child(switcher.query.text().to_string())
            }),
    );

    if switcher.results.is_empty() {
        return panel.child(
            gpui::div()
                .px(px(space::LG))
                .py(px(space::MD))
                .text_size(px(text::SM))
                .text_color(rgb(active().text_subtle))
                .child("No matches"),
        );
    }

    let mut list = column()
        .id("switcher-results")
        .max_h(px(360.))
        .overflow_y_scroll();

    for (index, candidate) in switcher.results.iter().enumerate() {
        let selected = index == switcher.selected;

        list = list.child(
            row()
                .w_full()
                .px(px(space::LG))
                .py(px(space::SM))
                .gap(px(space::SM))
                .when(selected, |d| d.bg(rgb(active().surface_active)))
                .child(
                    gpui::div()
                        .w(px(14.))
                        .text_color(rgb(active().text_subtle))
                        .child(candidate.kind.glyph()),
                )
                .child(
                    gpui::div()
                        .flex_1()
                        .text_size(px(text::SM))
                        .text_color(rgb(if candidate.unread || selected {
                            active().text
                        } else {
                            active().text_muted
                        }))
                        .child(candidate.name.clone()),
                )
                .child(
                    gpui::div()
                        .text_size(px(text::XS))
                        .text_color(rgb(active().text_subtle))
                        .child(candidate.context.clone()),
                ),
        );
    }

    panel.child(list)
}
