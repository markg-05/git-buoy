use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};

use crate::app::{App, SettingItem};

use super::theme::Theme;

const LABEL_WIDTH: usize = 20;
const PANEL_WIDTH: u16 = 68;
const HELP_PANEL_HEIGHT: u16 = 19;

/// Draw session-local behavior as a harbor control logbook over the scene.
/// Values adjust in place, preserving the ambient context underneath.
pub fn draw_settings(frame: &mut Frame, screen: Rect, app: &App, theme: &Theme) {
    let width = PANEL_WIDTH.min(screen.width);
    let help_width = width.saturating_sub(6) as usize;
    let lines = settings_lines(app, theme, help_width, screen.height >= HELP_PANEL_HEIGHT);
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

fn settings_lines(
    app: &App,
    theme: &Theme,
    help_width: usize,
    show_help_region: bool,
) -> Vec<Line<'static>> {
    let mut lines = Vec::with_capacity(17);
    lines.push(section("Harbor watch", theme));
    for index in 0..4 {
        lines.push(setting_line(app, index, theme));
    }
    lines.push(section("Local surveys", theme));
    for index in 4..6 {
        lines.push(setting_line(app, index, theme));
    }
    lines.push(section("Remote signal", theme));
    for index in 6..SettingItem::ALL.len() {
        lines.push(setting_line(app, index, theme));
    }
    lines.push(Line::default());
    if show_help_region {
        if app.settings.setting_help {
            let item = SettingItem::ALL[app.settings_selected];
            let [first, second] = wrap_help(item.help(), help_width);
            lines.push(Line::from(vec![
                Span::styled(" Logbook note · ", Style::new().fg(theme.dim)),
                Span::styled(
                    item.label(),
                    Style::new().fg(theme.water).add_modifier(Modifier::BOLD),
                ),
            ]));
            lines.push(Line::from(Span::styled(
                format!("  {first}"),
                Style::new().fg(theme.text),
            )));
            lines.push(Line::from(Span::styled(
                format!("  {second}"),
                Style::new().fg(theme.text),
            )));
        } else {
            lines.extend([Line::default(), Line::default(), Line::default()]);
        }
        lines.push(Line::default());
    }
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
        0..=3 => selected + 1,
        4..=5 => selected + 2,
        _ => selected + 3,
    }
}

fn wrap_help(text: &str, width: usize) -> [String; 2] {
    let width = width.max(8);
    let mut lines = [String::new(), String::new()];
    let mut current = 0;
    let mut truncated = false;

    for word in text.split_whitespace() {
        let separator = usize::from(!lines[current].is_empty());
        if lines[current].chars().count() + separator + word.chars().count() <= width {
            if separator == 1 {
                lines[current].push(' ');
            }
            lines[current].push_str(word);
        } else if current == 0 {
            current = 1;
            lines[current].push_str(word);
        } else {
            truncated = true;
            break;
        }
    }

    if truncated {
        while lines[1].chars().count() >= width {
            lines[1].pop();
        }
        lines[1].push('…');
    }
    lines
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

    #[test]
    fn help_wraps_to_a_stable_two_line_region() {
        let wrapped = wrap_help(SettingItem::GithubEnabled.help(), 38);
        assert!(!wrapped[0].is_empty());
        assert!(!wrapped[1].is_empty());
        assert!(wrapped.iter().all(|line| line.chars().count() <= 38));
    }
}
