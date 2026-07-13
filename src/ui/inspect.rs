use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Wrap};

use crate::app::App;
use crate::harbor::Dock;

use super::theme::Theme;

/// Width the panel needs to show its longest detail line and its title without
/// wrapping. The overlay is sized to this so paths and commit messages stay on
/// one line whenever the terminal has room.
pub fn content_width(dock: &Dock) -> usize {
    let label_width = dock
        .detail
        .iter()
        .map(|(label, _)| label.chars().count())
        .max()
        .unwrap_or(0);
    let widest_row = dock
        .detail
        .iter()
        // Mirrors the " {label}  {value}" layout drawn below.
        .map(|(_, value)| 1 + label_width + 2 + value.chars().count())
        .max()
        .unwrap_or(0);
    widest_row.max(dock.name.chars().count() + 2)
}

/// The inspect panel: exact Git facts behind the selected dock, stated
/// plainly. The metaphor stops at this border.
pub fn draw_inspect(frame: &mut Frame, area: Rect, app: &App, theme: &Theme) {
    let Some(dock) = app.harbor.docks.get(app.selected) else {
        return;
    };
    let block = Block::bordered()
        .title(format!(" {} ", dock.name))
        .border_style(Style::new().fg(theme.pier))
        .title_style(Style::new().fg(theme.text));

    let label_width = dock
        .detail
        .iter()
        .map(|(label, _)| label.chars().count())
        .max()
        .unwrap_or(0);
    let lines: Vec<Line> = dock
        .detail
        .iter()
        .map(|(label, value)| {
            Line::from(vec![
                Span::styled(
                    format!(" {label:label_width$}  "),
                    Style::new().fg(theme.dim),
                ),
                Span::styled(value.clone(), Style::new().fg(theme.text)),
            ])
        })
        .collect();

    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}
