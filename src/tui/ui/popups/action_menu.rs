use super::*;
use crate::tui::keybindings::KeyBindings;
use crate::tui::state::ActionItem;
use crate::tui::state::ServerPanelTab;

const KEY_SEQUENCE_HINT_MIN_WIDTH: u16 = 74;
const KEY_SEQUENCE_HINT_ROWS: usize = 4;
const KEY_SEQUENCE_HINT_COLUMN_GAP: usize = 4;

// ============================================================================
// Shared action-menu family
// ============================================================================
// Message, thread/post, server, channel, and member action menus (and their
// mute-duration/notification submenus) all render as the same centered popup:
// one row per action, selection marker + [shortcut] + label.

struct ActionMenuRow {
    shortcut: String,
    label: String,
    enabled: bool,
    disabled_reason: Option<String>,
}

/// Builds the menu rows for one scope from its action items and the
/// keybindings lookups for that scope.
fn action_menu_rows<K>(
    actions: &[ActionItem<K>],
    shortcut: impl Fn(&[ActionItem<K>], usize) -> String,
    label: impl Fn(&ActionItem<K>) -> String,
) -> Vec<ActionMenuRow> {
    actions
        .iter()
        .enumerate()
        .map(|(index, action)| ActionMenuRow {
            shortcut: shortcut(actions, index),
            label: label(action),
            enabled: action.is_enabled(),
            disabled_reason: action.disabled_reason().map(str::to_owned),
        })
        .collect()
}

fn action_menu_lines(rows: &[ActionMenuRow], selected: usize) -> Vec<Line<'static>> {
    let prefixes: Vec<String> = rows
        .iter()
        .map(|row| shortcut_label_prefix(&row.shortcut))
        .collect();
    let prefix_width = prefixes
        .iter()
        .map(|prefix| prefix.width())
        .max()
        .unwrap_or(0);
    rows.iter()
        .enumerate()
        .map(|(index, row)| {
            let is_selected = index == selected;
            let shortcut = padded_shortcut_prefix(&prefixes[index], prefix_width);
            let label = match (row.enabled, row.disabled_reason.as_deref()) {
                (false, Some(reason)) => format!("{} ({reason})", row.label),
                _ => row.label.clone(),
            };
            let style = selectable_popup_label_style(is_selected, row.enabled);
            selected_row_line(
                Line::from(vec![
                    selectable_popup_marker(is_selected),
                    selectable_popup_shortcut_span(shortcut),
                    Span::styled(label, style),
                ]),
                is_selected,
            )
        })
        .collect()
}

/// Rows for the submenus (mute durations, notification levels), which are
/// activated by their list position via the `[1]`..`[9]` indexed shortcuts.
fn indexed_action_menu_rows(labels: impl IntoIterator<Item = String>) -> Vec<ActionMenuRow> {
    labels
        .into_iter()
        .enumerate()
        .map(|(index, label)| ActionMenuRow {
            shortcut: KeyBindings::indexed_shortcut(index)
                .map(|shortcut| shortcut.to_string())
                .unwrap_or_default(),
            label,
            enabled: true,
            disabled_reason: None,
        })
        .collect()
}

fn render_action_menu(
    frame: &mut Frame,
    area: Rect,
    title: impl Into<String>,
    lines: Vec<Line<'static>>,
    scroll: usize,
) {
    let popup = action_menu_area(area, lines.len());
    render_selectable_popup_list(frame, popup, title, lines, scroll);
}

pub(in crate::tui::ui) fn action_menu_area(area: Rect, action_count: usize) -> Rect {
    centered_rect(area, 54, (action_count as u16).saturating_add(2))
}

fn shortcut_label_prefix(label: &str) -> String {
    if label.is_empty() {
        return "[]".to_owned();
    }
    format!("[{label}] ")
}

fn padded_shortcut_prefix(prefix: &str, width: usize) -> String {
    if prefix == "[]" {
        "[] ".to_owned()
    } else {
        format!("{prefix:<width$}")
    }
}

// ============================================================================
// Which Key sequence hint
// ============================================================================
// The bottom hint lists key bindings reachable from the active dashboard or
// popup prefix without replacing the modal that owns popup input.

pub(in crate::tui::ui) fn render_key_sequence_hint(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
) {
    if !state.is_key_sequence_active() {
        return;
    }

    let lines = key_sequence_hint_lines(state, area.height.saturating_sub(2) as usize);
    let popup = key_sequence_hint_area(area, &lines);
    let lines = truncate_popup_lines(lines, popup.width.saturating_sub(2).max(1) as usize);
    render_modal_paragraph(frame, popup, state.key_sequence_title(), lines);
}

pub(in crate::tui::ui) fn key_sequence_hint_area(area: Rect, lines: &[Line<'_>]) -> Rect {
    let content_width = lines.iter().map(key_sequence_line_width).max().unwrap_or(0);
    let desired_width = content_width.saturating_add(2).min(u16::MAX as usize) as u16;
    let width = KEY_SEQUENCE_HINT_MIN_WIDTH
        .max(desired_width)
        .min(area.width)
        .max(1);
    let line_count = lines.len() as u16;
    let height = line_count.saturating_add(2).min(area.height).max(1);
    Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height),
        width,
        height,
    }
}

pub(in crate::tui::ui) fn key_sequence_hint_area_for_state(
    area: Rect,
    state: &DashboardState,
) -> Rect {
    let lines = key_sequence_hint_lines(state, area.height.saturating_sub(2) as usize);
    key_sequence_hint_area(area, &lines)
}

// ============================================================================
// Server / channel / member action menus
// ============================================================================

pub(in crate::tui::ui) fn render_guild_action_menu(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
) {
    if !state.is_active_modal_popup(ActiveModalPopupKind::GuildActionMenu) {
        return;
    }
    let Some((title, lines)) = guild_action_menu_content(state) else {
        return;
    };
    render_action_menu(
        frame,
        area,
        title,
        lines,
        state
            .popup_list_scroll(SelectablePopupTarget::GuildActions)
            .expect("guild actions have selection state"),
    );
}

fn guild_action_menu_content(state: &DashboardState) -> Option<(&'static str, Vec<Line<'static>>)> {
    let selected = state.selected_guild_action_index().unwrap_or(0);
    if state.is_guild_action_mute_duration_phase() {
        let rows = indexed_action_menu_rows(
            state
                .selected_guild_mute_duration_items()
                .iter()
                .map(|item| item.label.to_owned()),
        );
        return Some(("Mute server", action_menu_lines(&rows, selected)));
    }
    let actions = state.selected_guild_action_items();
    if actions.is_empty() {
        return None;
    }
    let rows = action_menu_rows(
        &actions,
        |actions, index| {
            state
                .key_bindings()
                .guild_action_shortcut_label(actions, index)
        },
        |action| state.key_bindings().guild_action_label(action),
    );
    Some(("Server actions", action_menu_lines(&rows, selected)))
}

pub(in crate::tui::ui) fn render_channel_action_menu(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
) {
    if !state.is_active_modal_popup(ActiveModalPopupKind::ChannelActionMenu) {
        return;
    }
    let Some((title, lines)) = channel_action_menu_content(state) else {
        return;
    };
    render_action_menu(
        frame,
        area,
        title,
        lines,
        state
            .popup_list_scroll(SelectablePopupTarget::ChannelActions)
            .expect("channel actions have selection state"),
    );
}

fn channel_action_menu_content(
    state: &DashboardState,
) -> Option<(&'static str, Vec<Line<'static>>)> {
    let selected = state.selected_channel_action_index().unwrap_or(0);
    if state.is_channel_action_mute_duration_phase() {
        let rows = indexed_action_menu_rows(
            state
                .selected_channel_mute_duration_items()
                .iter()
                .map(|item| item.label.to_owned()),
        );
        return Some(("Mute channel", action_menu_lines(&rows, selected)));
    }
    if state.is_channel_action_stream_target_phase() {
        let rows = indexed_action_menu_rows(
            state
                .selected_stream_capture_targets()
                .iter()
                .map(|target| target.title.clone()),
        );
        return Some(("Share screen", action_menu_lines(&rows, selected)));
    }
    let actions = state.selected_channel_action_items();
    if actions.is_empty() {
        return None;
    }
    let rows = action_menu_rows(
        &actions,
        |actions, index| {
            state
                .key_bindings()
                .channel_action_shortcut_label(actions, index)
        },
        |action| state.key_bindings().channel_action_label(action),
    );
    Some((
        state.channel_action_menu_title(),
        action_menu_lines(&rows, selected),
    ))
}

pub(in crate::tui::ui) fn render_member_action_menu(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
) {
    if !state.is_active_modal_popup(ActiveModalPopupKind::MemberActionMenu) {
        return;
    }
    let actions = state.selected_member_action_items();
    if actions.is_empty() {
        return;
    }
    let selected = state.selected_member_action_index().unwrap_or(0);
    let rows = action_menu_rows(
        &actions,
        |actions, index| {
            state
                .key_bindings()
                .member_action_shortcut_label(actions, index)
        },
        |action| state.key_bindings().member_action_label(action),
    );
    render_action_menu(
        frame,
        area,
        "Member actions",
        action_menu_lines(&rows, selected),
        state
            .popup_list_scroll(SelectablePopupTarget::MemberActions)
            .expect("member actions have selection state"),
    );
}

#[cfg(test)]
pub(in crate::tui::ui) fn channel_action_menu_lines_for_test(
    state: &DashboardState,
) -> Vec<Line<'static>> {
    channel_action_menu_content(state)
        .map(|(_, lines)| lines)
        .unwrap_or_default()
}

fn key_sequence_hint_lines(state: &DashboardState, max_lines: usize) -> Vec<Line<'static>> {
    let lines = state
        .key_sequence_shortcuts()
        .into_iter()
        .map(|item| {
            let label = if item.has_children {
                format!("{} ›", item.label)
            } else {
                item.label
            };
            leader_shortcut_text_line(&item.key, &label, true)
        })
        .collect::<Vec<_>>();
    leader_shortcut_grid_lines(lines, max_lines)
}

fn leader_shortcut_grid_lines(lines: Vec<Line<'static>>, max_lines: usize) -> Vec<Line<'static>> {
    if lines.is_empty() {
        return lines;
    }
    let row_count = lines
        .len()
        .min(KEY_SEQUENCE_HINT_ROWS)
        .min(max_lines.max(1));
    let column_count = lines.len().div_ceil(row_count);
    let column_widths: Vec<usize> = (0..column_count)
        .map(|column| {
            (0..row_count)
                .filter_map(|row| lines.get(column * row_count + row))
                .map(key_sequence_line_width)
                .max()
                .unwrap_or(0)
        })
        .collect();

    (0..row_count)
        .map(|row| {
            let mut spans = Vec::new();
            for (column, width) in column_widths.iter().enumerate() {
                let Some(line) = lines.get(column * row_count + row) else {
                    continue;
                };
                let line_width = key_sequence_line_width(line);
                spans.extend(line.spans.iter().cloned());
                if column + 1 < column_count {
                    spans.push(Span::raw(" ".repeat(
                        width.saturating_sub(line_width) + KEY_SEQUENCE_HINT_COLUMN_GAP,
                    )));
                }
            }
            Line::from(spans)
        })
        .collect()
}

fn key_sequence_line_width(line: &Line<'_>) -> usize {
    line.spans.iter().map(|span| span.content.width()).sum()
}

fn leader_shortcut_text_line(key: &str, label: &str, enabled: bool) -> Line<'static> {
    let style = if enabled {
        Style::default()
    } else {
        theme::current().style(theme::HighlightGroup::Disabled)
    };
    Line::from(vec![
        Span::styled(
            format!("[{key}] "),
            theme::current().style(theme::HighlightGroup::Shortcut),
        ),
        Span::raw(" "),
        Span::styled(label.to_owned(), style),
    ])
}

// ============================================================================
// Message action menu
// ============================================================================

pub(in crate::tui::ui) fn render_message_action_menu(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
) {
    if !state.is_active_modal_popup(ActiveModalPopupKind::MessageActionMenu) {
        return;
    }

    let actions = state.selected_message_action_items();
    if actions.is_empty() {
        return;
    }
    let selected = state.selected_message_action_index().unwrap_or(0);
    let lines =
        message_action_menu_lines_with_key_bindings(&actions, selected, state.key_bindings());
    render_action_menu(
        frame,
        area,
        "Message actions",
        lines,
        state
            .popup_list_scroll(SelectablePopupTarget::MessageActions)
            .expect("message actions have selection state"),
    );
}

#[cfg(test)]
pub(in crate::tui::ui) fn message_action_menu_lines(
    actions: &[MessageActionItem],
    selected: usize,
) -> Vec<Line<'static>> {
    message_action_menu_lines_with_key_bindings(
        actions,
        selected,
        &crate::tui::keybindings::KeyBindings::default(),
    )
}

#[cfg(test)]
pub(in crate::tui::ui) fn message_action_menu_lines_with_keymap_options(
    actions: &[MessageActionItem],
    selected: usize,
    keymap_options: &crate::config::KeymapOptions,
) -> Vec<Line<'static>> {
    let key_bindings = crate::tui::keybindings::KeyBindings::try_from_options(keymap_options)
        .expect("test keymap options should parse");
    message_action_menu_lines_with_key_bindings(actions, selected, &key_bindings)
}

fn message_action_menu_lines_with_key_bindings(
    actions: &[MessageActionItem],
    selected: usize,
    key_bindings: &KeyBindings,
) -> Vec<Line<'static>> {
    let rows = action_menu_rows(
        actions,
        |actions, index| key_bindings.message_action_shortcut_label(actions, index),
        |action| key_bindings.message_action_label(action),
    );
    action_menu_lines(&rows, selected)
}

// ============================================================================
// Thread / forum-post action menu
// ============================================================================

pub(in crate::tui::ui) fn render_thread_action_menu(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
) {
    if !state.is_active_modal_popup(ActiveModalPopupKind::ThreadActionMenu) {
        return;
    }

    let selected = state.selected_thread_action_index().unwrap_or(0);
    let noun = state.thread_action_menu_noun();
    let (title, lines) = if state.is_thread_action_mute_duration_phase() {
        let rows = indexed_action_menu_rows(
            state
                .selected_thread_mute_duration_items()
                .iter()
                .map(|item| item.label.to_owned()),
        );
        (format!("Mute {noun}"), action_menu_lines(&rows, selected))
    } else if state.is_thread_action_notification_phase() {
        let items = state.selected_thread_notification_items();
        if items.is_empty() {
            return;
        }
        let rows = indexed_action_menu_rows(items.into_iter().map(|item| item.label));
        (
            "Notification settings".to_owned(),
            action_menu_lines(&rows, selected),
        )
    } else {
        let items = state.selected_thread_action_items();
        if items.is_empty() {
            return;
        }
        let lines = thread_action_menu_lines(&items, selected, state.key_bindings());
        // Title-case the noun: "Post actions" / "Thread actions".
        let title = format!("{}{} actions", noun[..1].to_uppercase(), &noun[1..]);
        (title, lines)
    };
    render_action_menu(
        frame,
        area,
        title,
        lines,
        state
            .popup_list_scroll(SelectablePopupTarget::ThreadActions)
            .expect("thread actions have selection state"),
    );
}

fn thread_action_menu_lines(
    actions: &[ThreadActionItem],
    selected: usize,
    key_bindings: &KeyBindings,
) -> Vec<Line<'static>> {
    let rows = action_menu_rows(
        actions,
        |actions, index| key_bindings.thread_action_shortcut_label(actions, index),
        |action| key_bindings.thread_action_label(action),
    );
    action_menu_lines(&rows, selected)
}

/// The sticker picker: the open guild's own stickers.
///
/// Names rather than images: a terminal cannot show a sticker inline without
/// the image protocol, and a name is what makes one selectable anyway.
pub(in crate::tui::ui) fn render_sticker_picker(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
) {
    if !state.is_active_modal_popup(ActiveModalPopupKind::StickerPicker) {
        return;
    }

    let items = state.sticker_picker_items();
    let selected = state.selected_sticker_index().unwrap_or(0);

    let lines = if items.is_empty() {
        // Only the guild's own stickers are sendable without Nitro, so a
        // guild with none has nothing to offer here.
        vec![Line::from(Span::styled(
            "This server has no stickers".to_owned(),
            theme::current().style(theme::HighlightGroup::Hint),
        ))]
    } else {
        let rows = indexed_action_menu_rows(items.iter().map(|sticker| sticker.name.clone()));
        action_menu_lines(&rows, selected)
    };

    let staged = state.pending_sticker_count();
    let title = if staged == 0 {
        "Stickers".to_owned()
    } else {
        // The count matters: Discord caps a message at three, and the picker
        // is where someone finds out they are at the limit.
        format!("Stickers ({staged} staged)")
    };

    render_action_menu(
        frame,
        area,
        title,
        lines,
        state
            .popup_list_scroll(SelectablePopupTarget::Stickers)
            .expect("the sticker picker has selection state"),
    );
}

/// The role picker: every role in the guild, with what the member has.
pub(in crate::tui::ui) fn render_role_picker(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
) {
    if !state.is_active_modal_popup(ActiveModalPopupKind::RolePicker) {
        return;
    }

    let items = state.role_picker_items();
    let selected = state.selected_role_index().unwrap_or(0);

    let lines = if items.is_empty() {
        vec![Line::from(Span::styled(
            "This server has no assignable roles".to_owned(),
            theme::current().style(theme::HighlightGroup::Hint),
        ))]
    } else {
        let rows = indexed_action_menu_rows(items.iter().map(|item| {
            // The marker carries the state, so a glance down the column shows
            // what the member has without reading every line.
            let marker = if item.assigned { "[x]" } else { "[ ]" };
            match item.disabled_reason {
                Some(reason) => format!("{marker} {} - {reason}", item.name),
                None => format!("{marker} {}", item.name),
            }
        }));
        action_menu_lines(&rows, selected)
    };

    render_action_menu(
        frame,
        area,
        "Roles (space toggle, ctrl-s save)".to_owned(),
        lines,
        state
            .popup_list_scroll(SelectablePopupTarget::Roles)
            .expect("the role picker has selection state"),
    );
}

/// A guild's ban list, with the reason each ban was given.
/// A guild's invites, emoji or audit log.
///
/// One popup with three lists rather than three popups, the same as the GUI's
/// panel. The title says which tab is showing and how to change it, because a
/// tab strip drawn in a terminal is easy to miss.
pub(in crate::tui::ui) fn render_server_management(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
) {
    if !state.is_active_modal_popup(ActiveModalPopupKind::ServerManagement) {
        return;
    }
    let Some(panel) = state.server_management_state() else {
        return;
    };

    let selected = state.selected_server_row().unwrap_or(0);
    let tab = panel.tab();

    let lines = if let Some(error) = panel.error() {
        vec![Line::from(Span::styled(
            error.to_owned(),
            theme::current().style(theme::HighlightGroup::Error),
        ))]
    } else if panel.is_loading() {
        // Distinct from an empty list, which for an audit log especially would
        // be a misleading thing to believe.
        vec![Line::from(Span::styled(
            "Loading...".to_owned(),
            theme::current().style(theme::HighlightGroup::Loading),
        ))]
    } else {
        let rows: Vec<String> = match tab {
            ServerPanelTab::Invites => panel
                .invites()
                .iter()
                .map(|invite| {
                    let uses = match invite.max_uses {
                        Some(max) => format!("{}/{max}", invite.uses),
                        // Discord writes "no limit" as 0; "3/0" would read as
                        // already spent.
                        None => format!("{} uses", invite.uses),
                    };
                    let expiry = match invite.max_age_seconds {
                        Some(seconds) => format!("{}m", seconds / 60),
                        None => "never expires".to_owned(),
                    };
                    format!("discord.gg/{} - {uses} - {expiry}", invite.code)
                })
                .collect(),
            ServerPanelTab::Settings => panel
                .settings()
                .iter()
                .map(|(label, value)| format!("{label}: {value}"))
                .collect(),
            ServerPanelTab::Roles => panel
                .roles()
                .iter()
                .map(|role| {
                    let mut line = role.name.clone();
                    if role.hoist {
                        line.push_str(" - shown separately");
                    }
                    // The count is what people actually want to know about a
                    // role they are about to change.
                    let granted = crate::discord::permissions_catalogue::ALL
                        .iter()
                        .filter(|permission| permission.is_set(role.permissions))
                        .count();
                    line.push_str(&format!(" ({granted} permissions)"));
                    line
                })
                .collect(),
            ServerPanelTab::Emoji => panel
                .emojis()
                .iter()
                .map(|emoji| {
                    let mut line = format!(":{}:", emoji.name);
                    if emoji.animated {
                        line.push_str(" - animated");
                    }
                    // Worth saying: a role-restricted emoji is unusable for
                    // most members, who would otherwise wonder why.
                    if emoji.role_restricted {
                        line.push_str(" - role-restricted");
                    }
                    line
                })
                .collect(),
            ServerPanelTab::Sounds => panel
                .sounds()
                .iter()
                .map(|sound| {
                    let mut line = match &sound.emoji_name {
                        Some(emoji) => format!("{emoji} {}", sound.name),
                        None => sound.name.clone(),
                    };
                    if !sound.available {
                        line.push_str(" - unavailable");
                    }
                    line
                })
                .collect(),
            ServerPanelTab::AutoMod => panel
                .automod()
                .iter()
                .map(|rule| {
                    format!(
                        "[{}] {} - {}",
                        if rule.enabled { "on" } else { "off" },
                        rule.name,
                        rule.summary()
                    )
                })
                .collect(),
            ServerPanelTab::AuditLog => panel
                .audit_log()
                .iter()
                .map(|entry| {
                    let actor = entry.actor.clone().unwrap_or_else(|| "someone".to_owned());
                    let mut line = match &entry.target {
                        Some(target) => format!("{actor} {} {target}", entry.action.label()),
                        None => format!("{actor} {}", entry.action.label()),
                    };
                    if let Some(reason) = &entry.reason {
                        line.push_str(&format!(" ({reason})"));
                    }
                    line
                })
                .collect(),
            // Built by the state rather than here: the values come from three
            // separate fetches, and each has an "unknown" that is not "off".
            ServerPanelTab::Events => state
                .scheduled_events()
                .iter()
                .map(|event| format!("{} - {}", event.name, event.summary()))
                .collect(),
            ServerPanelTab::Members => state
                .member_rows()
                .iter()
                .map(|member| format!("{} - {}", member.name, member.summary()))
                .collect(),
            ServerPanelTab::Templates => state
                .guild_templates()
                .iter()
                .map(|template| {
                    format!(
                        "{} - {} - {}",
                        template.name,
                        template.url(),
                        template.summary()
                    )
                })
                .collect(),
            ServerPanelTab::Membership => state
                .membership_rows()
                .into_iter()
                .map(|(label, value)| format!("{label}: {value}"))
                .collect(),
        };

        if rows.is_empty() {
            let empty = match tab {
                ServerPanelTab::Settings => "No settings",
                ServerPanelTab::Roles => "No roles",
                ServerPanelTab::Invites => "No invites",
                ServerPanelTab::Emoji => "No custom emoji",
                ServerPanelTab::Sounds => "No sounds in this server",
                ServerPanelTab::AutoMod => "No AutoMod rules",
                ServerPanelTab::AuditLog => "Nothing recorded",
                ServerPanelTab::Membership => "Loading",
                ServerPanelTab::Events => "No scheduled events",
                ServerPanelTab::Templates => "No templates",
                // Members arrive over the gateway as the client learns about
                // them, so an empty list here usually means "not yet" rather
                // than "none".
                ServerPanelTab::Members => "No members loaded yet",
            };
            vec![Line::from(Span::styled(
                empty.to_owned(),
                theme::current().style(theme::HighlightGroup::Hint),
            ))]
        } else {
            action_menu_lines(&indexed_action_menu_rows(rows), selected)
        }
    };

    // While renaming, the field is what the popup is for, so it replaces the
    // list rather than sitting under it in a menu that no longer responds.
    let (lines, hint) = match panel.renaming() {
        Some((edit, input)) => match edit {
            crate::tui::state::EmojiEdit::Rename(_) => (
                vec![Line::from(Span::raw(format!("Name: {}", input.value())))],
                "enter to rename, esc to cancel",
            ),
            crate::tui::state::EmojiEdit::GuildIcon => (
                vec![Line::from(Span::raw(format!("Icon: {}", input.value())))],
                "path to a PNG, JPEG, GIF or WebP - enter sets it, esc cancels",
            ),
            crate::tui::state::EmojiEdit::GuildName => (
                vec![Line::from(Span::raw(format!("Name: {}", input.value())))],
                "enter renames the server, esc cancels",
            ),
            crate::tui::state::EmojiEdit::NewRole => (
                vec![Line::from(Span::raw(format!("Name: {}", input.value())))],
                "enter creates the role, esc cancels",
            ),
            crate::tui::state::EmojiEdit::EditEvent(_) | crate::tui::state::EmojiEdit::NewEvent => {
                let problem = crate::discord::parse_new_event(input.value())
                    .and_then(|event| event.problem())
                    .map(crate::discord::NewEventProblem::message);
                (
                    vec![
                        Line::from(Span::raw(format!("Event: {}", input.value()))),
                        Line::from(Span::raw(
                            problem.unwrap_or_else(|| "enter creates it".to_owned()),
                        )),
                    ],
                    "name | start | end | where - times as 2026-09-01T19:00:00Z",
                )
            }
            crate::tui::state::EmojiEdit::WelcomeDescription => (
                vec![Line::from(Span::raw(format!(
                    "Description: {}",
                    input.value()
                )))],
                "enter sets it, empty clears it, esc cancels",
            ),
            crate::tui::state::EmojiEdit::WidgetChannel => (
                vec![Line::from(Span::raw(format!("Channel: {}", input.value())))],
                "channel name - enter sets it, empty means no invite, esc cancels",
            ),
            crate::tui::state::EmojiEdit::NewTemplate => (
                vec![Line::from(Span::raw(format!("Name: {}", input.value())))],
                "enter creates the template, esc cancels",
            ),
            crate::tui::state::EmojiEdit::AddImage => (
                vec![Line::from(Span::raw(format!("Image: {}", input.value())))],
                "path to a PNG, JPEG, GIF or WebP - enter to add, esc to cancel",
            ),
        },
        // The audit log has no row action - history is a record, not something
        // to be edited from the client that reads it - so the hint changes.
        None => (
            lines,
            // The query goes in the hint rather than in the list: a row for it
            // would shift every index below it, and the selection is what
            // decides which member enter acts on.
            match tab {
                ServerPanelTab::Invites => "tab to switch, r to reload, enter to revoke",
                ServerPanelTab::Settings => "tab to switch, enter to change, a for the icon",
                ServerPanelTab::Roles => "tab, N new, p permissions, K/J move, enter delete",
                ServerPanelTab::Emoji => "tab, r reload, a add, n rename, enter delete",
                ServerPanelTab::Sounds => "tab, r reload, n rename, enter delete",
                ServerPanelTab::AutoMod => "tab, r reload, enter on/off, d delete",
                ServerPanelTab::Membership => {
                    "tab, r reload, enter toggles or cycles, e edits, P prunes"
                }
                ServerPanelTab::Events => "tab, r reload, enter marks interested, N new, d cancels",
                ServerPanelTab::Templates => "tab, r reload, enter syncs, N new, d deletes",
                ServerPanelTab::Members => "tab, / search, enter bans",
                ServerPanelTab::AuditLog => "tab to switch, r to reload",
            },
        ),
    };

    render_action_menu(
        frame,
        area,
        // The query goes in the title rather than as a row: a row for it would
        // shift every index below, and the selection is what decides which
        // member enter acts on.
        match state.member_search_text() {
            Some(query) => format!("{} matching \"{query}\" (esc ends the search)", tab.label()),
            None => format!("{} ({hint})", tab.label()),
        },
        lines,
        state
            .popup_list_scroll(SelectablePopupTarget::ServerManagement)
            .expect("the server panel has selection state"),
    );
}

/// The permission grid.
pub(in crate::tui::ui) fn render_permission_grid(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
) {
    if !state.is_active_modal_popup(ActiveModalPopupKind::PermissionGrid) {
        return;
    }
    let Some(grid) = state.permission_grid_state() else {
        return;
    };

    let selected = state.selected_permission_index().unwrap_or(0);
    let rows: Vec<String> = crate::discord::permissions_catalogue::ALL
        .iter()
        .enumerate()
        .map(|(index, permission)| format!("{} {}", grid.setting(index).marker(), permission.label))
        .collect();

    let title = if grid.is_dirty() {
        // Said in the title because escape discards, and a grid of 53
        // switches is easy to walk away from by accident.
        format!("{} - unsaved (enter saves)", grid.scope_name())
    } else {
        format!("{} (space toggles)", grid.scope_name())
    };

    render_action_menu(
        frame,
        area,
        title,
        action_menu_lines(&indexed_action_menu_rows(rows), selected),
        state
            .popup_list_scroll(SelectablePopupTarget::Permissions)
            .expect("the permission grid has selection state"),
    );
}

/// The soundboard.
pub(in crate::tui::ui) fn render_soundboard(frame: &mut Frame, area: Rect, state: &DashboardState) {
    if !state.is_active_modal_popup(ActiveModalPopupKind::Soundboard) {
        return;
    }
    let Some(board) = state.soundboard_state() else {
        return;
    };

    let selected = state.selected_sound_index().unwrap_or(0);
    let lines = if let Some(error) = board.error() {
        vec![Line::from(Span::styled(
            error.to_owned(),
            theme::current().style(theme::HighlightGroup::Error),
        ))]
    } else if board.is_loading() {
        vec![Line::from(Span::styled(
            "Loading sounds...".to_owned(),
            theme::current().style(theme::HighlightGroup::Loading),
        ))]
    } else if board.len() == 0 {
        vec![Line::from(Span::styled(
            "No sounds".to_owned(),
            theme::current().style(theme::HighlightGroup::Hint),
        ))]
    } else {
        let rows = indexed_action_menu_rows(board.sounds().map(|sound| {
            let mut label = match &sound.emoji_name {
                Some(emoji) => format!("{emoji} {}", sound.name),
                None => sound.name.clone(),
            };
            // Shown and refused rather than hidden, so a guild that lost its
            // boosts does not look like it lost its sounds.
            if !sound.available {
                label.push_str(" - unavailable");
            }
            label
        }));
        action_menu_lines(&rows, selected)
    };

    render_action_menu(
        frame,
        area,
        "Soundboard (enter to play)".to_owned(),
        lines,
        state
            .popup_list_scroll(SelectablePopupTarget::Soundboard)
            .expect("the soundboard has selection state"),
    );
}

pub(in crate::tui::ui) fn render_ban_list(frame: &mut Frame, area: Rect, state: &DashboardState) {
    if !state.is_active_modal_popup(ActiveModalPopupKind::BanList) {
        return;
    }
    let Some(bans) = state.ban_list_state() else {
        return;
    };

    let selected = state.selected_ban_index().unwrap_or(0);

    // While typing ids, the field is what the popup is for: it replaces the
    // list rather than sitting under one that no longer responds.
    if let Some(text) = state.bulk_ban_text() {
        let count = state.bulk_ban_count();
        render_action_menu(
            frame,
            area,
            "Ban by user id".to_owned(),
            vec![
                Line::from(Span::raw(format!("Ids: {text}"))),
                Line::from(Span::styled(
                    // The count is the check: a mistyped separator shows as a
                    // number that does not match what was pasted.
                    format!("{count} ids - enter bans them, esc cancels"),
                    theme::current().style(theme::HighlightGroup::Hint),
                )),
            ],
            0,
        );
        return;
    }

    let lines = if let Some(error) = bans.error() {
        vec![Line::from(Span::styled(
            error.to_owned(),
            theme::current().style(theme::HighlightGroup::Error),
        ))]
    } else if bans.is_loading() {
        // Distinct from an empty list: a slow fetch must not read as "nobody
        // is banned", which would be a dangerous thing to believe.
        vec![Line::from(Span::styled(
            "Loading bans...".to_owned(),
            theme::current().style(theme::HighlightGroup::Loading),
        ))]
    } else if bans.bans().is_empty() {
        vec![Line::from(Span::styled(
            "Nobody is banned from this server".to_owned(),
            theme::current().style(theme::HighlightGroup::Hint),
        ))]
    } else {
        let rows = indexed_action_menu_rows(bans.bans().iter().map(|ban| match &ban.reason {
            Some(reason) => format!("{} - {reason}", ban.username),
            None => ban.username.clone(),
        }));
        action_menu_lines(&rows, selected)
    };

    render_action_menu(
        frame,
        area,
        "Bans (enter unbans, B bans by id)".to_owned(),
        lines,
        state
            .popup_list_scroll(SelectablePopupTarget::Bans)
            .expect("the ban list has selection state"),
    );
}
