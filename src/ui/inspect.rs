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
    detail_lines(dock, None)
        .iter()
        .map(Line::width)
        .max()
        .unwrap_or(0)
        .max(Line::from(format!(" {} ", dock.name)).width())
}

/// Build the rows once for both measurement and rendering. Ratatui's line
/// width uses terminal cells rather than Unicode scalar counts, so wide paths,
/// branch names, and commit summaries are padded and measured consistently.
fn detail_lines<'a>(dock: &'a Dock, theme: Option<&Theme>) -> Vec<Line<'a>> {
    let label_width = dock
        .detail
        .iter()
        .map(|(label, _)| Line::from(*label).width())
        .max()
        .unwrap_or(0);

    dock.detail
        .iter()
        .map(|(label, value)| {
            let padding = label_width.saturating_sub(Line::from(*label).width());
            let label_style = theme.map_or_else(Style::new, |theme| Style::new().fg(theme.dim));
            let value_style = theme.map_or_else(Style::new, |theme| Style::new().fg(theme.text));
            Line::from(vec![
                Span::styled(format!(" {label}{}  ", " ".repeat(padding)), label_style),
                Span::styled(value.as_str(), value_style),
            ])
        })
        .collect()
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

    let lines = detail_lines(dock, Some(theme));

    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

#[cfg(test)]
mod tests {
    use crate::harbor::{Condition, DockKind};

    use super::*;

    #[test]
    fn content_width_counts_terminal_cells_and_shared_label_padding() {
        let dock = Dock {
            name: "功能".to_string(),
            kind: DockKind::Branch,
            condition: Condition::Calm,
            vessel: None,
            sync: None,
            detail: vec![
                ("路径", "/tmp/港口".to_string()),
                ("last commit", "ready".to_string()),
            ],
        };

        // "路径" and "港口" each occupy four terminal cells. The longest
        // rendered row is: leading space + 11-cell label column + two spaces
        // + the 9-cell value.
        assert_eq!(content_width(&dock), 23);
        let lines = detail_lines(&dock, None);
        assert_eq!(lines[0].width(), 23);
        assert_eq!(lines[1].width(), 19);
    }
}
