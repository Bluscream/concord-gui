//! Server management overlay views.

use gpui::{prelude::*, px, rgb, Div};

use concord::t;

use crate::theme::{active, scaled, space, text};
use crate::ui::chrome::{column, row};
use crate::ui::overlay::{button, panel};

/// One row in the server-management panel.
pub struct ServerRow {
    pub primary: String,
    pub secondary: Option<String>,
    pub action: Option<String>,
    pub secondary_action: Option<String>,
    pub tertiary_action: Option<String>,
}

/// A guild's invites, emoji or audit log state.
pub struct ServerPanel<'a> {
    pub tabs: &'a [(String, bool)],
    pub rows: &'a [ServerRow],
    pub empty_label: &'a str,
    pub loading: bool,
    pub error: Option<&'a str>,
    pub add_label: Option<&'a str>,
}

pub struct ServerPanelActions {
    pub reload: Box<dyn Fn(&mut gpui::App)>,
    pub add: Box<dyn Fn(&mut gpui::App)>,
    pub close: Box<dyn Fn(&mut gpui::App)>,
}

pub fn server_management_view(
    panel_state: ServerPanel<'_>,
    on_tab: impl Fn(usize, &mut gpui::App) + Clone + 'static,
    on_row_action: impl Fn(usize, &mut gpui::App) + Clone + 'static,
    on_row_secondary: impl Fn(usize, &mut gpui::App) + Clone + 'static,
    on_row_tertiary: impl Fn(usize, &mut gpui::App) + Clone + 'static,
    actions: ServerPanelActions,
) -> Div {
    let on_reload = actions.reload;
    let on_add = actions.add;
    let on_close = actions.close;
    let mut tab_row = row()
        .w_full()
        .px(px(space::LG))
        .py(px(space::SM))
        .gap(px(space::XS));

    for (index, (label, selected)) in panel_state.tabs.iter().enumerate() {
        let pick = on_tab.clone();
        tab_row = tab_row.child(
            gpui::div()
                .id(("server-tab", index))
                .px(px(space::SM))
                .py(px(space::XS))
                .rounded(px(4.))
                .cursor_pointer()
                .text_size(px(scaled(text::SM)))
                .bg(rgb(if *selected {
                    active().accent
                } else {
                    active().surface
                }))
                .text_color(rgb(if *selected {
                    active().text
                } else {
                    active().text_muted
                }))
                .hover(|style| style.bg(rgb(active().surface_hover)))
                .on_click(move |_event, _window, cx| pick(index, cx))
                .child(label.clone()),
        );
    }

    let mut list = column()
        .id("server-rows")
        .max_h(px(360.))
        .overflow_y_scroll();

    let notice = if panel_state.loading {
        Some(t!("status-loading"))
    } else if let Some(error) = panel_state.error {
        Some(error.to_owned())
    } else if panel_state.rows.is_empty() {
        Some(panel_state.empty_label.to_owned())
    } else {
        None
    };

    if let Some(notice) = notice {
        list = list.child(
            gpui::div()
                .px(px(space::LG))
                .py(px(space::MD))
                .text_size(px(scaled(text::SM)))
                .text_color(rgb(if panel_state.error.is_some() {
                    active().danger
                } else {
                    active().text_subtle
                }))
                .child(notice),
        );
    }

    for (index, entry) in panel_state.rows.iter().enumerate() {
        let act = on_row_action.clone();
        let second = on_row_secondary.clone();
        let third = on_row_tertiary.clone();
        list = list.child(
            row()
                .id(("server-row", index))
                .w_full()
                .px(px(space::LG))
                .py(px(space::XS))
                .gap(px(space::SM))
                .items_center()
                .child(
                    column()
                        .flex_1()
                        .child(
                            gpui::div()
                                .text_size(px(scaled(text::SM)))
                                .text_color(rgb(active().text))
                                .child(entry.primary.clone()),
                        )
                        .children(entry.secondary.as_ref().map(|detail| {
                            gpui::div()
                                .text_size(px(scaled(text::XS)))
                                .text_color(rgb(active().text_subtle))
                                .child(detail.clone())
                        })),
                )
                .children(entry.tertiary_action.as_ref().map(|label| {
                    gpui::div()
                        .id(("server-row-third", index))
                        .px(px(space::SM))
                        .py(px(space::XS))
                        .rounded(px(4.))
                        .cursor_pointer()
                        .text_size(px(scaled(text::XS)))
                        .text_color(rgb(active().text_muted))
                        .hover(|style| style.bg(rgb(active().surface_hover)))
                        .on_click(move |_event, _window, cx| third(index, cx))
                        .child(label.clone())
                }))
                .children(entry.secondary_action.as_ref().map(|label| {
                    gpui::div()
                        .id(("server-row-second", index))
                        .px(px(space::SM))
                        .py(px(space::XS))
                        .rounded(px(4.))
                        .cursor_pointer()
                        .text_size(px(scaled(text::XS)))
                        .text_color(rgb(active().text_muted))
                        .hover(|style| style.bg(rgb(active().surface_hover)))
                        .on_click(move |_event, _window, cx| second(index, cx))
                        .child(label.clone())
                }))
                .children(entry.action.as_ref().map(|label| {
                    gpui::div()
                        .id(("server-row-action", index))
                        .px(px(space::SM))
                        .py(px(space::XS))
                        .rounded(px(4.))
                        .cursor_pointer()
                        .text_size(px(scaled(text::XS)))
                        .text_color(rgb(active().danger))
                        .hover(|style| style.bg(rgb(active().surface_hover)))
                        .on_click(move |_event, _window, cx| act(index, cx))
                        .child(label.clone())
                })),
        );
    }

    panel(&t!("label-server-management"), 520.)
        .child(tab_row)
        .child(list)
        .child(
            row()
                .w_full()
                .px(px(space::LG))
                .py(px(space::MD))
                .gap(px(space::SM))
                .justify_end()
                .children(
                    panel_state
                        .add_label
                        .map(|label| button("server-add", label, false, on_add)),
                )
                .child(button(
                    "server-reload",
                    &t!("action-reload"),
                    false,
                    on_reload,
                ))
                .child(button("server-close", &t!("action-close"), true, on_close)),
        )
}

/// One server from Discord's public list.
pub struct DiscoveryRow {
    pub name: String,
    pub summary: String,
    pub joinable: bool,
}

/// Discord's public server list, under the invite box.
pub fn discovery_results(
    rows: &[DiscoveryRow],
    searching: bool,
    on_join: impl Fn(usize, &mut gpui::App) + Clone + 'static,
) -> Div {
    let mut list = column().w_full().max_h(px(240.));

    if searching {
        return list.child(
            gpui::div()
                .px(px(space::LG))
                .py(px(space::MD))
                .text_size(px(scaled(text::SM)))
                .text_color(rgb(active().text_subtle))
                .child(t!("status-searching-discovery")),
        );
    }

    for (index, entry) in rows.iter().enumerate() {
        let join = on_join.clone();
        list = list.child(
            row()
                .id(("discovery-row", index))
                .w_full()
                .px(px(space::LG))
                .py(px(space::XS))
                .gap(px(space::SM))
                .items_center()
                .child(
                    column()
                        .flex_1()
                        .child(
                            gpui::div()
                                .text_size(px(scaled(text::SM)))
                                .text_color(rgb(active().text))
                                .child(entry.name.clone()),
                        )
                        .child(
                            gpui::div()
                                .text_size(px(scaled(text::XS)))
                                .text_color(rgb(active().text_subtle))
                                .child(entry.summary.clone()),
                        ),
                )
                .when(entry.joinable, |r| {
                    r.child(
                        gpui::div()
                            .id(("discovery-join", index))
                            .px(px(space::SM))
                            .py(px(space::XS))
                            .rounded(px(4.))
                            .cursor_pointer()
                            .text_size(px(scaled(text::XS)))
                            .text_color(rgb(active().text_muted))
                            .hover(|style| style.bg(rgb(active().surface_hover)))
                            .on_click(move |_event, _window, cx| join(index, cx))
                            .child(t!("action-join")),
                    )
                }),
        );
    }
    list
}
