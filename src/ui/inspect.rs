use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Wrap};

use crate::app::{App, InspectTarget};
use crate::harbor::{Dock, Vessel};

use super::Theme;

/// Width the panel needs to show its longest line and title without wrapping.
pub fn content_width(dock: &Dock, target: InspectTarget) -> usize {
    detail_lines(dock, target, None)
        .iter()
        .map(Line::width)
        .max()
        .unwrap_or(0)
        .max(Line::from(format!(" {} ", title(dock, target))).width())
}

fn detail_lines(dock: &Dock, target: InspectTarget, theme: Option<&Theme>) -> Vec<Line<'static>> {
    match target {
        InspectTarget::Dock => labeled_lines(&dock.detail, theme),
        InspectTarget::Vessel => dock
            .vessel
            .as_ref()
            .map_or_else(Vec::new, |vessel| vessel_lines(vessel, theme)),
        InspectTarget::Change(selected) => dock
            .vessel
            .as_ref()
            .map_or_else(Vec::new, |vessel| change_lines(vessel, selected, theme)),
    }
}

fn labeled_lines(detail: &[(&'static str, String)], theme: Option<&Theme>) -> Vec<Line<'static>> {
    let label_width = detail
        .iter()
        .map(|(label, _)| Line::from(*label).width())
        .max()
        .unwrap_or(0);

    detail
        .iter()
        .map(|(label, value)| {
            let padding = label_width.saturating_sub(Line::from(*label).width());
            let label_style = theme.map_or_else(Style::new, |theme| Style::new().fg(theme.dim));
            let value_style = theme.map_or_else(Style::new, |theme| Style::new().fg(theme.text));
            Line::from(vec![
                Span::styled(format!(" {label}{}  ", " ".repeat(padding)), label_style),
                Span::styled(value.clone(), value_style),
            ])
        })
        .collect()
}

fn vessel_lines(vessel: &Vessel, theme: Option<&Theme>) -> Vec<Line<'static>> {
    labeled_lines(
        &[
            ("workspace", vessel.workspace.display().to_string()),
            ("activity", vessel.activity.label().to_string()),
            ("staged", vessel.staged.to_string()),
            ("unstaged", vessel.unstaged.to_string()),
            ("untracked", vessel.untracked.to_string()),
            ("conflicted", vessel.conflicted.to_string()),
        ],
        theme,
    )
}

fn change_lines(vessel: &Vessel, selected: usize, theme: Option<&Theme>) -> Vec<Line<'static>> {
    let kind_width = vessel
        .cargo
        .iter()
        .map(|item| item.kind.label().len())
        .max()
        .unwrap_or(0);
    vessel
        .cargo
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let is_selected = index == selected;
            let marker_style = theme.map_or_else(Style::new, |theme| Style::new().fg(theme.pier));
            let kind_style = theme.map_or_else(Style::new, |theme| Style::new().fg(theme.dim));
            let mut path_style = theme.map_or_else(Style::new, |theme| Style::new().fg(theme.text));
            if is_selected {
                path_style = path_style.add_modifier(Modifier::REVERSED);
            }
            Line::from(vec![
                Span::styled(if is_selected { " ▶ " } else { "   " }, marker_style),
                Span::styled(format!("{:<kind_width$}  ", item.kind.label()), kind_style),
                Span::styled(item.path.display().to_string(), path_style),
            ])
        })
        .collect()
}

fn title(dock: &Dock, target: InspectTarget) -> String {
    match target {
        InspectTarget::Dock => dock.name.clone(),
        InspectTarget::Vessel => format!("{} / vessel", dock.name),
        InspectTarget::Change(_) => format!("{} / changed files", dock.name),
    }
}

/// Exact Git facts behind the selected dock, vessel, or changed path.
pub fn draw_inspect(frame: &mut Frame, area: Rect, app: &App, theme: &Theme) {
    let Some(dock) = app.harbor.docks.get(app.selected) else {
        return;
    };
    let block = Block::bordered()
        .title(format!(" {} ", title(dock, app.inspect_target)))
        .border_style(Style::new().fg(theme.pier))
        .title_style(Style::new().fg(theme.text));
    let lines = detail_lines(dock, app.inspect_target, Some(theme));
    let visible = area.height.saturating_sub(2) as usize;
    let scroll = match app.inspect_target {
        InspectTarget::Change(selected) if selected >= visible && visible > 0 => {
            selected + 1 - visible
        }
        _ => 0,
    };

    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((u16::try_from(scroll).unwrap_or(u16::MAX), 0));
    frame.render_widget(paragraph, area);
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::harbor::{CargoItem, CargoKind, Condition, DockKind, VesselActivity};

    use super::*;

    fn dock() -> Dock {
        Dock {
            name: "功能".to_string(),
            kind: DockKind::Branch,
            condition: Condition::Local,
            vessel: Some(Vessel {
                workspace: PathBuf::from("/tmp/港口"),
                activity: VesselActivity::Recent,
                cargo: vec![CargoItem {
                    path: PathBuf::from("src/航道.rs"),
                    kind: CargoKind::Unstaged,
                }],
                ..Vessel::default()
            }),
            sync: None,
            detail: vec![
                ("路径", "/tmp/港口".to_string()),
                ("last commit", "ready".to_string()),
            ],
        }
    }

    #[test]
    fn content_width_counts_terminal_cells_and_shared_label_padding() {
        let dock = dock();
        assert_eq!(content_width(&dock, InspectTarget::Dock), 23);
        let lines = detail_lines(&dock, InspectTarget::Dock, None);
        assert_eq!(lines[0].width(), 23);
        assert_eq!(lines[1].width(), 19);
    }

    #[test]
    fn changed_file_view_contains_exact_category_and_path() {
        let dock = dock();
        let lines = detail_lines(&dock, InspectTarget::Change(0), None);
        let rendered = lines[0]
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();

        assert!(rendered.contains("unstaged"));
        assert!(rendered.contains("src/航道.rs"));
    }
}
