use super::*;

const MAX_STREAM_INFO_WIDTH: u16 = 36;

pub(in crate::tui::ui) fn render_stream_info(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
) {
    let lines = stream_info_lines(state);
    let popup = stream_info_area(area, &lines);
    if popup.is_empty() {
        return;
    }

    clear_area(frame, popup);
    frame.render_widget(
        Paragraph::new(lines)
            .style(theme::current().style(theme::HighlightGroup::Normal))
            .block(panel_block(" Streams ", false)),
        popup,
    );
}

pub(in crate::tui::ui) fn stream_info_area(area: Rect, lines: &[Line<'_>]) -> Rect {
    if lines.is_empty() || area.width < 3 || area.height < 3 {
        return Rect::default();
    }

    let content_width = lines.iter().map(Line::width).max().unwrap_or(1) as u16;
    let width = content_width
        .saturating_add(2)
        .min(MAX_STREAM_INFO_WIDTH)
        .min(area.width);
    let height = (lines.len() as u16).saturating_add(2).min(area.height);
    Rect {
        x: area.x + area.width.saturating_sub(width),
        y: area.y + area.height.saturating_sub(height),
        width,
        height,
    }
}

pub(in crate::tui::ui) fn stream_info_lines(state: &DashboardState) -> Vec<Line<'static>> {
    let sections = state.stream_info_sections();
    let mut lines = Vec::new();

    for (section_index, section) in sections.iter().enumerate() {
        if section_index > 0 {
            lines.push(Line::styled(
                "---",
                theme::current().style(theme::HighlightGroup::Muted),
            ));
        }
        lines.push(Line::styled(
            section.label,
            theme::current().style(theme::HighlightGroup::Info),
        ));

        let status = if section.paused { "PAUSED" } else { "LIVE" };
        lines.push(Line::from(vec![
            Span::styled("● ", theme::current().style(theme::HighlightGroup::Success)),
            Span::raw(section.broadcaster.clone()),
            Span::styled(
                format!("  {status}"),
                theme::current().style(theme::HighlightGroup::Success),
            ),
        ]));
        for viewer in &section.viewers {
            lines.push(Line::from(format!("  {viewer}")));
        }
    }

    lines
}
