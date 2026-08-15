use super::*;
use crate::tui::state::JoinServerState;

/// Height of the prompt before an invite resolves: field plus its hint.
const PROMPT_LINES: usize = 3;

pub(in crate::tui::ui) fn render_join_server(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
) {
    if !state.is_active_modal_popup(ActiveModalPopupKind::JoinServer) {
        return;
    }
    let Some(join) = state.join_server_state() else {
        return;
    };

    let lines = join_server_lines(join);
    let popup = join_server_popup_area(area, lines.len());
    render_modal_paragraph(frame, popup, "Join a server", lines);
}

fn join_server_lines(join: &JoinServerState) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    match join.preview() {
        // Resolved: show where the invite leads, so joining is a decision
        // rather than a leap. An invite code says nothing on its own.
        Some(preview) => {
            lines.push(Line::from(vec![Span::styled(
                preview.guild_name.clone(),
                theme::current().style(theme::HighlightGroup::Strong),
            )]));

            if let (Some(members), Some(online)) = (preview.member_count, preview.online_count) {
                lines.push(Line::from(Span::styled(
                    format!("{members} members, {online} online"),
                    theme::current().style(theme::HighlightGroup::Muted),
                )));
            }

            for (label, value) in [
                ("Channel", preview.channel_name.clone()),
                ("Invited by", preview.inviter.clone()),
            ] {
                if let Some(value) = value {
                    lines.push(Line::from(Span::styled(
                        format!("{label}: {value}"),
                        theme::current().style(theme::HighlightGroup::Description),
                    )));
                }
            }

            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                if join.is_joinable() {
                    "Enter join | Esc cancel".to_owned()
                } else {
                    "You are already in this server - Esc close".to_owned()
                },
                theme::current().style(theme::HighlightGroup::Hint),
            )));
        }
        None => {
            lines.push(Line::from(vec![
                Span::styled(
                    "> ",
                    theme::current().style(theme::HighlightGroup::Decoration),
                ),
                Span::styled(
                    join.input().value().to_owned(),
                    theme::current().style(theme::HighlightGroup::Normal),
                ),
            ]));
            lines.push(Line::from(""));

            let hint = if let Some(error) = join.error() {
                Span::styled(
                    error.to_owned(),
                    theme::current().style(theme::HighlightGroup::Error),
                )
            } else if join.is_resolving() {
                // Said explicitly, or a slow lookup looks like a dropped key.
                Span::styled(
                    "Looking up invite...".to_owned(),
                    theme::current().style(theme::HighlightGroup::Loading),
                )
            } else {
                Span::styled(
                    "Paste an invite link or code - Enter look up | Esc cancel".to_owned(),
                    theme::current().style(theme::HighlightGroup::Hint),
                )
            };
            lines.push(Line::from(hint));
        }
    }

    lines
}

pub(in crate::tui::ui) fn join_server_popup_area(frame_area: Rect, lines: usize) -> Rect {
    // Sized to its content: the prompt is two lines and a preview is five or
    // six, and a fixed height would leave one of them mostly empty.
    centered_rect(
        frame_area,
        60,
        (lines.max(PROMPT_LINES) as u16).saturating_add(2),
    )
}
