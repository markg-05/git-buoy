use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};

use crate::app::{App, SettingItem};

use super::theme::Theme;

const LABEL_WIDTH: usize = 20;

/// Draw session-local behavior as a harbor control logbook over the scene.
/// Values adjust in place, preserving the ambient context underneath.
pub fn draw_settings(frame: &mut Frame, screen: Rect, app: &App, theme: &Theme) {
    let lines = settings_lines(app, theme);
    let width = lines
        .iter()
        .map(Line::width)
        .max()
        .unwrap_or(0)
        .saturating_add(4) as u16;
    let height = lines.len() as u16 + 2;
    let area = centered(screen, width, height);
    let selected_line = setting_line_index(app.settings_selected);
    let inner_height = area.height.saturating_sub(2) as usize;
    let scroll = selected_line.saturating_sub(inner_height.saturating_sub(1));

    let block = Block::bordered()
        .title(" Harbor controls ")
        .border_style(Style::new().fg(theme.pier))
        .title_style(Style::new().fg(theme.text).add_modifier(Modifier::BOLD));

    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .block(block)
            .scroll((u16::try_from(scroll).unwrap_or(u16::MAX), 0)),
        area,
    );
}

fn settings_lines(app: &App, theme: &Theme) -> Vec<Line<'static>> {
    let mut lines = Vec::with_capacity(12);
    lines.push(section("Harbor watch", theme));
    for index in 0..3 {
        lines.push(setting_line(app, index, theme));
    }
    lines.push(section("Local surveys", theme));
    for index in 3..5 {
        lines.push(setting_line(app, index, theme));
    }
    lines.push(section("Remote signal", theme));
    for index in 5..SettingItem::ALL.len() {
        lines.push(setting_line(app, index, theme));
    }
    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        "  Session only · CLI flags set starting values",
        Style::new().fg(theme.dim),
    )));
    lines
}

fn section(title: &'static str, theme: &Theme) -> Line<'static> {
    Line::from(Span::styled(
        format!(" {title}"),
        Style::new().fg(theme.dim).add_modifier(Modifier::BOLD),
    ))
}

fn setting_line(app: &App, index: usize, theme: &Theme) -> Line<'static> {
    let item = SettingItem::ALL[index];
    let selected = index == app.settings_selected;
    let marker = if selected { "▶" } else { " " };
    let label_style = if selected {
        Style::new().fg(theme.text).add_modifier(Modifier::BOLD)
    } else {
        Style::new().fg(theme.text)
    };
    let value_style = if selected {
        Style::new().fg(theme.water).add_modifier(Modifier::BOLD)
    } else {
        Style::new().fg(theme.dim)
    };
    Line::from(vec![
        Span::styled(format!(" {marker} "), Style::new().fg(theme.water)),
        Span::styled(
            format!("{:<width$}", item.label(), width = LABEL_WIDTH),
            label_style,
        ),
        Span::styled(app.settings.value_label(item), value_style),
    ])
}

fn setting_line_index(selected: usize) -> usize {
    match selected {
        0..=2 => selected + 1,
        3..=4 => selected + 2,
        _ => selected + 3,
    }
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let w = width.min(area.width);
    let h = height.min(area.height);
    Rect {
        x: area.x + (area.width - w) / 2,
        y: area.y + (area.height - h) / 2,
        width: w,
        height: h,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_setting_remains_visible_in_short_terminals() {
        let screen = Rect::new(0, 0, 60, 6);
        let selected_line = setting_line_index(SettingItem::ALL.len() - 1);
        let inner_height = screen.height.saturating_sub(2) as usize;
        let scroll = selected_line.saturating_sub(inner_height.saturating_sub(1));

        assert!(scroll > 0);
        assert!(selected_line - scroll < inner_height);
    }
}
