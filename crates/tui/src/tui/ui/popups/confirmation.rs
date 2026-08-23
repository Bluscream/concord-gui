use super::*;
use crate::tui::state::{ConfirmationButton, MessageConfirmationKind};

pub(in crate::tui::ui) fn render_long_message_confirmation(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
) {
    if !state.is_active_modal_popup(ActiveModalPopupKind::LongMessageConfirmation) {
        return;
    }

    let Some((character_count, character_limit)) = state.long_message_confirmation_counts() else {
        return;
    };
    let lines = long_message_confirmation_lines(
        character_count,
        character_limit,
        state.active_confirmation_button(),
    );
    let popup = long_message_confirmation_popup_area(area, lines.len());
    render_modal_paragraph(frame, popup, "Message is too long", lines);
}

pub(in crate::tui::ui) fn render_message_confirmation(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
) {
    if !state.is_active_modal_popup(ActiveModalPopupKind::MessageConfirmation) {
        return;
    }

    let Some((kind, author, content)) = state.message_confirmation_lines() else {
        return;
    };

    let lines = message_confirmation_lines(
        kind,
        &author,
        content.as_deref(),
        56,
        state.active_confirmation_button(),
    );
    let popup = message_confirmation_popup_area(area, lines.len());
    render_modal_paragraph(frame, popup, kind.title(), lines);
}

pub(in crate::tui::ui) fn render_quit_confirmation(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
) {
    if !state.is_active_modal_popup(ActiveModalPopupKind::QuitConfirmation) {
        return;
    }

    let lines = quit_confirmation_popup_lines(state.active_confirmation_button());
    let popup = quit_confirmation_popup_area(area);
    render_modal_paragraph(frame, popup, "Quit", lines);
}

pub(in crate::tui::ui) fn render_guild_leave_confirmation(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
) {
    if !state.is_active_modal_popup(ActiveModalPopupKind::GuildLeaveConfirmation) {
        return;
    }

    let Some(name) = state.guild_leave_confirmation_name() else {
        return;
    };

    let lines = guild_leave_confirmation_lines(&name, 56, state.active_confirmation_button());
    let popup = guild_leave_confirmation_popup_area(area, lines.len());
    render_modal_paragraph(frame, popup, "Leave server?", lines);
}

/// Creating or renaming a channel.
pub(in crate::tui::ui) fn render_channel_edit(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
) {
    if !state.is_active_modal_popup(ActiveModalPopupKind::ChannelEdit) {
        return;
    }
    let Some(edit) = state.channel_edit_state() else {
        return;
    };

    let (title, hint, kind_line) = match edit.purpose() {
        crate::tui::state::ChannelEditPurpose::Create { kind } => (
            "New channel",
            "tab changes the kind, enter creates, esc cancels",
            Some(format!("Kind: {}", kind.label())),
        ),
        crate::tui::state::ChannelEditPurpose::Edit { .. } => (
            "Channel settings",
            "tab moves between fields, space toggles, enter saves, esc cancels",
            None,
        ),
    };

    // Every field this channel has, with the focused one marked. A caret is
    // easy to lose in a column of near-identical rows.
    let mut lines: Vec<Line<'static>> = edit
        .fields()
        .iter()
        .enumerate()
        .map(|(index, field)| {
            let marker = if index == edit.focused() { ">" } else { " " };
            Line::from(Span::raw(format!(
                "{marker} {}: {}",
                field.label(),
                edit.value(*field)
            )))
        })
        .collect();
    lines.extend(kind_line.map(|line| Line::from(Span::raw(line))));
    lines.push(Line::from(Span::raw(String::new())));
    lines.push(Line::from(Span::styled(
        hint.to_owned(),
        theme::current().style(theme::HighlightGroup::Hint),
    )));

    let popup = channel_edit_popup_area(area);
    render_modal_paragraph(frame, popup, title, lines);
}

pub(in crate::tui::ui) fn channel_edit_popup_area(area: Rect) -> Rect {
    centered_rect(area, 60, 10)
}

/// Deleting a channel, which takes its whole history with it.
pub(in crate::tui::ui) fn render_channel_delete_confirmation(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
) {
    if !state.is_active_modal_popup(ActiveModalPopupKind::ChannelDelete) {
        return;
    }
    let Some(name) = state.channel_delete_name() else {
        return;
    };

    let lines = channel_delete_confirmation_lines(&name, state.active_confirmation_button());
    let popup = message_confirmation_popup_area(area, lines.len());
    render_modal_paragraph(frame, popup, "Delete channel?", lines);
}

fn channel_delete_confirmation_lines(name: &str, active: ConfirmationButton) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(Span::raw(
            "Deleting a channel takes its whole history with it, and Discord has no undo."
                .to_owned(),
        )),
        Line::from(Span::styled(
            format!("Channel: #{name}"),
            theme::current().style(theme::HighlightGroup::Error),
        )),
        Line::from(Span::raw(String::new())),
    ];
    lines.extend(confirmation_button_lines_with_labels(
        active, "delete", "cancel",
    ));
    lines
}

pub(in crate::tui::ui) fn channel_delete_confirmation_popup_area_for_state(
    area: Rect,
    state: &DashboardState,
) -> Option<Rect> {
    let name = state.channel_delete_name()?;
    let lines = channel_delete_confirmation_lines(&name, state.active_confirmation_button());
    Some(message_confirmation_popup_area(area, lines.len()))
}

pub(in crate::tui::ui) fn render_risk_warning(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
) {
    if !state.is_active_modal_popup(ActiveModalPopupKind::RiskWarning) {
        return;
    }

    let Some(warning) = state.risk_warning() else {
        return;
    };

    let lines = risk_warning_lines(
        &warning.explanation(),
        warning.dont_ask(),
        RISK_WARNING_WIDTH,
        state.active_confirmation_button(),
    );
    let popup = risk_warning_popup_area(area, lines.len());
    render_modal_paragraph(frame, popup, "Are you sure?", lines);
}

pub(in crate::tui::ui) fn render_thread_delete_confirmation(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
) {
    if !state.is_active_modal_popup(ActiveModalPopupKind::ThreadDeleteConfirmation) {
        return;
    }

    let Some((name, noun)) = state.thread_delete_confirmation_target() else {
        return;
    };

    let lines =
        thread_delete_confirmation_lines(&name, noun, 56, state.active_confirmation_button());
    let popup = thread_delete_confirmation_popup_area(area, lines.len());
    render_modal_paragraph(frame, popup, format!("Delete {noun}?"), lines);
}

pub(in crate::tui::ui) fn render_notification_inbox_mark_all_confirmation(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
) {
    if !state.notification_inbox_is_confirming_mark_all() {
        return;
    }

    let lines = notification_inbox_mark_all_confirmation_lines(state.active_confirmation_button());
    let popup = message_confirmation_popup_area(area, lines.len());
    render_modal_paragraph(frame, popup, "Mark read?", lines);
}

pub(in crate::tui::ui) fn message_confirmation_popup_area(area: Rect, line_count: usize) -> Rect {
    centered_rect(area, 60, (line_count as u16).saturating_add(2))
}

pub(in crate::tui::ui) fn long_message_confirmation_popup_area(
    area: Rect,
    line_count: usize,
) -> Rect {
    centered_rect(area, 64, (line_count as u16).saturating_add(2))
}

pub(in crate::tui::ui) fn long_message_confirmation_popup_area_for_state(
    area: Rect,
    state: &DashboardState,
) -> Option<Rect> {
    let (character_count, character_limit) = state.long_message_confirmation_counts()?;
    let lines = long_message_confirmation_lines(
        character_count,
        character_limit,
        state.active_confirmation_button(),
    );
    Some(long_message_confirmation_popup_area(area, lines.len()))
}

pub(in crate::tui::ui) fn message_confirmation_popup_area_for_state(
    area: Rect,
    state: &DashboardState,
) -> Option<Rect> {
    let (kind, author, content) = state.message_confirmation_lines()?;
    let lines = message_confirmation_lines(
        kind,
        &author,
        content.as_deref(),
        56,
        state.active_confirmation_button(),
    );
    Some(message_confirmation_popup_area(area, lines.len()))
}

pub(in crate::tui::ui) fn quit_confirmation_popup_area(area: Rect) -> Rect {
    centered_rect(
        area,
        44,
        (quit_confirmation_popup_lines(ConfirmationButton::default()).len() as u16)
            .saturating_add(2),
    )
}

pub(in crate::tui::ui) fn guild_leave_confirmation_popup_area(
    area: Rect,
    line_count: usize,
) -> Rect {
    centered_rect(area, 60, (line_count as u16).saturating_add(2))
}

pub(in crate::tui::ui) fn guild_leave_confirmation_popup_area_for_state(
    area: Rect,
    state: &DashboardState,
) -> Option<Rect> {
    let name = state.guild_leave_confirmation_name()?;
    let lines = guild_leave_confirmation_lines(&name, 56, state.active_confirmation_button());
    Some(guild_leave_confirmation_popup_area(area, lines.len()))
}

pub(in crate::tui::ui) fn risk_warning_popup_area(area: Rect, line_count: usize) -> Rect {
    centered_rect(
        area,
        RISK_WARNING_WIDTH as u16 + 4,
        (line_count as u16).saturating_add(2),
    )
}

pub(in crate::tui::ui) fn risk_warning_popup_area_for_state(
    area: Rect,
    state: &DashboardState,
) -> Option<Rect> {
    let warning = state.risk_warning()?;
    let lines = risk_warning_lines(
        &warning.explanation(),
        warning.dont_ask(),
        RISK_WARNING_WIDTH,
        state.active_confirmation_button(),
    );
    Some(risk_warning_popup_area(area, lines.len()))
}

pub(in crate::tui::ui) fn thread_delete_confirmation_popup_area(
    area: Rect,
    line_count: usize,
) -> Rect {
    centered_rect(area, 60, (line_count as u16).saturating_add(2))
}

pub(in crate::tui::ui) fn thread_delete_confirmation_popup_area_for_state(
    area: Rect,
    state: &DashboardState,
) -> Option<Rect> {
    let (name, noun) = state.thread_delete_confirmation_target()?;
    let lines =
        thread_delete_confirmation_lines(&name, noun, 56, state.active_confirmation_button());
    Some(thread_delete_confirmation_popup_area(area, lines.len()))
}

#[cfg(test)]
pub(in crate::tui::ui) fn message_delete_confirmation_lines(
    author: &str,
    content: Option<&str>,
    width: usize,
) -> Vec<Line<'static>> {
    message_confirmation_lines(
        MessageConfirmationKind::Delete,
        author,
        content,
        width,
        ConfirmationButton::default(),
    )
}

#[cfg(test)]
pub(in crate::tui::ui) fn message_pin_confirmation_lines(
    pinned: bool,
    author: &str,
    content: Option<&str>,
    width: usize,
) -> Vec<Line<'static>> {
    message_confirmation_lines(
        MessageConfirmationKind::Pin { pinned },
        author,
        content,
        width,
        ConfirmationButton::default(),
    )
}

#[cfg(test)]
pub(in crate::tui::ui) fn quit_confirmation_lines() -> Vec<Line<'static>> {
    quit_confirmation_popup_lines(ConfirmationButton::default())
}

#[cfg(test)]
pub(in crate::tui::ui) fn message_remove_embeds_confirmation_lines(
    author: &str,
    content: Option<&str>,
    width: usize,
) -> Vec<Line<'static>> {
    message_confirmation_lines(
        MessageConfirmationKind::RemoveEmbeds,
        author,
        content,
        width,
        ConfirmationButton::default(),
    )
}

fn quit_confirmation_popup_lines(active: ConfirmationButton) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(Span::raw("Quit Concord?")),
        Line::from(Span::raw(String::new())),
    ];
    lines.extend(confirmation_button_lines(active));
    lines
}

fn guild_leave_confirmation_lines(
    name: &str,
    width: usize,
    active: ConfirmationButton,
) -> Vec<Line<'static>> {
    let name = truncate_display_width(name, width.max(1).saturating_sub(2));
    let mut lines = vec![
        Line::from(Span::raw("Leave the current server?")),
        Line::from(Span::styled(
            format!("Server: {name}"),
            theme::current().style(theme::HighlightGroup::Error),
        )),
        Line::from(Span::raw(String::new())),
    ];
    lines.extend(confirmation_button_lines(active));
    lines
}

/// Wide enough that an explanation reads as prose rather than a column of
/// fragments - the point of the warning is that it gets read.
const RISK_WARNING_WIDTH: usize = 64;

fn risk_warning_lines(
    explanation: &str,
    dont_ask: bool,
    width: usize,
    active: ConfirmationButton,
) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = wrap_text_lines(explanation, width.max(1))
        .into_iter()
        .map(|line| Line::from(Span::raw(line)))
        .collect();
    lines.push(Line::from(Span::raw(String::new())));
    // The marker carries the state, the way the role picker's does, so the
    // choice is readable without colour.
    lines.push(Line::from(Span::raw(format!(
        "[{}] d  do not ask again",
        if dont_ask { "x" } else { " " }
    ))));
    lines.push(Line::from(Span::raw(String::new())));
    lines.extend(confirmation_button_lines_with_labels(
        active, "continue", "cancel",
    ));
    lines
}

fn thread_delete_confirmation_lines(
    name: &str,
    noun: &str,
    width: usize,
    active: ConfirmationButton,
) -> Vec<Line<'static>> {
    let name = truncate_display_width(name, width.max(1).saturating_sub(2));
    let label = capitalize_first(noun);
    let mut lines = vec![
        Line::from(Span::raw(format!("Permanently delete this {noun}?"))),
        Line::from(Span::styled(
            format!("{label}: {name}"),
            theme::current().style(theme::HighlightGroup::Error),
        )),
        Line::from(Span::raw(String::new())),
    ];
    lines.extend(confirmation_button_lines(active));
    lines
}

fn confirmation_button_lines(active: ConfirmationButton) -> Vec<Line<'static>> {
    confirmation_button_lines_with_labels(active, "confirm", "cancel")
}

fn confirmation_button_lines_with_labels(
    active: ConfirmationButton,
    confirm_label: &'static str,
    cancel_label: &'static str,
) -> Vec<Line<'static>> {
    vec![
        popup_button_line("y", confirm_label, active == ConfirmationButton::Confirm),
        popup_button_line("n", cancel_label, active == ConfirmationButton::Cancel),
    ]
}

fn long_message_confirmation_lines(
    character_count: usize,
    character_limit: usize,
    active: ConfirmationButton,
) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(Span::styled(
            format!("{character_count} / {character_limit} characters"),
            theme::current().style(theme::HighlightGroup::Error),
        )),
        Line::from(Span::raw("This message is too long to send as text.")),
        Line::from(Span::raw("Send the full text as message.txt instead?")),
        Line::from(Span::raw(String::new())),
    ];
    lines.extend(confirmation_button_lines_with_labels(
        active,
        "send as file",
        "cancel",
    ));
    lines
}

#[cfg(test)]
pub(in crate::tui::ui) fn long_message_confirmation_lines_for_test(
    character_count: usize,
    character_limit: usize,
) -> Vec<Line<'static>> {
    long_message_confirmation_lines(
        character_count,
        character_limit,
        ConfirmationButton::default(),
    )
}

fn notification_inbox_mark_all_confirmation_lines(
    active: ConfirmationButton,
) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(Span::raw("Mark all unread channels as read?")),
        Line::from(Span::raw(String::new())),
    ];
    lines.extend(confirmation_button_lines(active));
    lines
}

/// Uppercase the first ASCII letter so a noun like "post" renders as "Post:" in
/// the confirmation body. The nouns are known ASCII words, so this is sufficient.
fn capitalize_first(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
        None => String::new(),
    }
}

fn message_confirmation_lines(
    kind: MessageConfirmationKind,
    author: &str,
    content: Option<&str>,
    width: usize,
    active: ConfirmationButton,
) -> Vec<Line<'static>> {
    confirmation_lines(kind.prompt(), author, content, width, active)
}

fn confirmation_lines(
    prompt: String,
    author: &str,
    content: Option<&str>,
    width: usize,
    active: ConfirmationButton,
) -> Vec<Line<'static>> {
    let width = width.max(1);
    let excerpt = content
        .map(str::trim)
        .filter(|content| !content.is_empty())
        .map(|content| content.split_whitespace().collect::<Vec<_>>().join(" "))
        .unwrap_or_else(|| "[no text content]".to_owned());
    let excerpt = truncate_display_width(&excerpt, width.saturating_sub(2));
    let mut lines = vec![
        Line::from(Span::raw(prompt)),
        Line::from(Span::styled(
            format!("From: {author}"),
            theme::current().style(theme::HighlightGroup::MessageSecondary),
        )),
        Line::from(Span::styled(
            format!("\"{excerpt}\""),
            theme::current().style(theme::HighlightGroup::Error),
        )),
        Line::from(Span::raw(String::new())),
    ];
    lines.extend(confirmation_button_lines(active));
    lines
}
