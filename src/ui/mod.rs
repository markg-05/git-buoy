//! Rendering. Consumes the harbor scene and application state; owns all
//! ratatui specifics so nothing above this layer touches the terminal.

mod inspect;
mod legend;
mod scene;
mod settings;
mod theme;

pub use theme::Theme;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};

use crate::app::{App, InspectTarget, Mode};

/// At or above this width the inspect panel floats over the full-width scene;
/// below it the panel takes the whole body, since there is no room for both.
const SPLIT_WIDTH: u16 = 76;
/// The inspect overlay never shrinks below this, even for a bare dock.
const MIN_INSPECT_WIDTH: u16 = 34;

pub fn draw(frame: &mut Frame, app: &App, theme: &Theme) {
    let [header, body, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(frame.area());

    draw_header(frame, header, app, theme);
    draw_body(frame, body, app, theme);
    draw_footer(frame, footer, app, theme);

    // Controls and legend float above either experience without disturbing
    // the harbor underneath. The state machine keeps them mutually exclusive.
    if app.show_settings {
        settings::draw_settings(frame, frame.area(), app, theme);
    } else if app.show_legend {
        legend::draw_legend(frame, frame.area(), theme, app.legend_scroll);
    }
}

fn draw_body(frame: &mut Frame, area: Rect, app: &App, theme: &Theme) {
    match app.mode {
        Mode::Ambient => scene::draw_scene(frame, area, app, theme),
        // The scene keeps its full width and the panel floats over it, sized
        // to its own content rather than squeezed into a fixed column.
        Mode::Inspect if area.width >= SPLIT_WIDTH => {
            scene::draw_scene(frame, area, app, theme);
            if let Some(dock) = app.harbor.docks.get(app.selected) {
                let panel = inspect_panel_area(area, dock, app.inspect_target);
                frame.render_widget(Clear, panel);
                inspect::draw_inspect(frame, panel, app, theme);
            }
        }
        // Too narrow to float: precision wins over ambience, panel takes over.
        Mode::Inspect => inspect::draw_inspect(frame, area, app, theme),
    }
}

fn inspect_panel_area(area: Rect, dock: &crate::harbor::Dock, target: InspectTarget) -> Rect {
    let wanted =
        u16::try_from(inspect::content_width(dock, target).saturating_add(2)).unwrap_or(u16::MAX);
    let panel_width = wanted.clamp(MIN_INSPECT_WIDTH, area.width.saturating_sub(2));
    Rect {
        x: area.x + area.width - panel_width,
        y: area.y,
        width: panel_width,
        height: area.height,
    }
}

fn draw_header(frame: &mut Frame, area: Rect, app: &App, theme: &Theme) {
    let title = Line::from(vec![
        Span::styled(" ≋ ", Style::new().fg(theme.water)),
        Span::styled(app.harbor.name.clone(), Style::new().fg(theme.text)),
        Span::styled(" harbor", Style::new().fg(theme.dim)),
    ]);
    frame.render_widget(Paragraph::new(title), area);

    let mode = match (app.show_settings, app.show_legend, app.mode) {
        (true, _, _) => "settings ",
        (_, true, _) => "legend ",
        (_, _, Mode::Ambient) => "ambient ",
        (_, _, Mode::Inspect) => "inspect ",
    };
    let mode_label = Paragraph::new(Span::styled(mode, Style::new().fg(theme.dim))).right_aligned();
    frame.render_widget(mode_label, area);
}

fn draw_footer(frame: &mut Frame, area: Rect, app: &App, theme: &Theme) {
    if let Some(error) = &app.error {
        let warning = Paragraph::new(Span::styled(
            format!(" survey failed: {error}"),
            Style::new().fg(theme.condition(crate::harbor::Condition::Blocked)),
        ));
        frame.render_widget(warning, area);
        return;
    }
    if let Some(error) = &app.hosting_error {
        let warning = Paragraph::new(Span::styled(
            format!(" github survey failed: {error}"),
            Style::new().fg(theme.condition(crate::harbor::Condition::Blocked)),
        ));
        frame.render_widget(warning, area);
        return;
    }
    let hints = if app.show_settings {
        " j/k select · h/l or ←/→ adjust · s/esc close · session only"
    } else if app.show_legend {
        " j/k scroll · l/esc close legend"
    } else {
        match app.mode {
            Mode::Ambient => " i inspect · s settings · l legend · m motion · q quit",
            Mode::Inspect => match app.inspect_target {
                InspectTarget::Dock => {
                    " tab dock · enter vessel · p pull request · l legend · esc back · q quit"
                }
                InspectTarget::Vessel => {
                    " tab dock · enter files · p pull request · l legend · esc back · q quit"
                }
                InspectTarget::Change(_) => {
                    " j/k file · p pull request · tab dock · esc back · q quit"
                }
                InspectTarget::PullRequest(_) => {
                    " j/k PR · enter checks · tab dock · esc back · q quit"
                }
                InspectTarget::Check { .. } => {
                    " j/k check · p pull request · tab dock · esc back · q quit"
                }
            },
        }
    };
    frame.render_widget(
        Paragraph::new(Span::styled(hints, Style::new().fg(theme.dim))),
        area,
    );
    if app.settings.reduced_motion && !app.show_settings {
        let badge = Paragraph::new(Span::styled("reduced motion ", Style::new().fg(theme.dim)))
            .right_aligned();
        frame.render_widget(badge, area);
    }
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use crate::harbor::{Condition, Dock, DockKind, Harbor, Vessel};

    use super::*;

    fn inspect_app(detail: Vec<(&'static str, String)>) -> App {
        let mut app = App::new("test".to_string(), true);
        app.harbor = Harbor {
            name: "test".to_string(),
            convoys: Vec::new(),
            docks: vec![Dock {
                name: "feature/cargo".to_string(),
                kind: DockKind::Branch,
                condition: Condition::Loading,
                vessel: Some(Vessel {
                    staged: 2,
                    unstaged: 40,
                    untracked: 1,
                    conflicted: 0,
                    ..Vessel::default()
                }),
                sync: None,
                detail,
                events: Vec::new(),
                clearances: Vec::new(),
            }],
        };
        app.mode = Mode::Inspect;
        app.loaded = true;
        app
    }

    fn render(app: &App, width: u16, height: u16) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| draw(frame, app, &Theme::detect()))
            .unwrap();
        let buffer = terminal.backend().buffer();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer.cell((x, y)).unwrap().symbol())
                    .collect::<String>()
            })
            .collect()
    }

    #[test]
    fn wide_inspect_panel_is_content_sized_and_right_aligned() {
        let workspace = "/Users/example/Documents/repositories/harbor/feature-cargo";
        let commit = "Keep the complete commit summary visible";
        let app = inspect_app(vec![
            ("workspace", workspace.to_string()),
            ("last commit", commit.to_string()),
        ]);
        let body = Rect::new(0, 1, 120, 10);
        let panel = inspect_panel_area(body, &app.harbor.docks[0], app.inspect_target);

        assert_eq!(panel.right(), body.right());
        assert_eq!(
            panel.width as usize,
            inspect::content_width(&app.harbor.docks[0], app.inspect_target) + 2
        );

        let lines = render(&app, 120, 12);
        assert!(lines[2].contains(workspace), "rendered row: {:?}", lines[2]);
        assert!(lines[3].contains(commit), "rendered row: {:?}", lines[3]);
        assert_eq!(lines[2].matches(workspace).count(), 1);
        assert_eq!(lines[3].matches(commit).count(), 1);
    }

    #[test]
    fn wide_inspect_draws_harbor_under_a_floating_panel() {
        let app = inspect_app(vec![("branch", "feature/cargo".to_string())]);
        let lines = render(&app, SPLIT_WIDTH, 8);
        let panel = inspect_panel_area(
            Rect::new(0, 1, SPLIT_WIDTH, 6),
            &app.harbor.docks[0],
            app.inspect_target,
        );

        assert!(lines[1].starts_with("─▶┬─ feature/cargo"));
        assert!(lines[2].contains(scene::VESSEL_HULL));
        assert_eq!(lines[1].chars().nth(panel.x as usize), Some('┌'));
    }

    #[test]
    fn narrow_inspect_replaces_the_harbor() {
        let app = inspect_app(vec![("workspace", "/tmp/repo".to_string())]);
        let lines = render(&app, SPLIT_WIDTH - 1, 8);

        assert!(
            lines[1].starts_with("┌ feature/cargo "),
            "rendered border: {:?}",
            lines[1]
        );
        assert!(lines.iter().all(|line| !line.contains(scene::VESSEL_HULL)));
        assert!(lines[2].contains("/tmp/repo"));
    }

    #[test]
    fn settings_float_over_the_harbor_with_selected_value() {
        let mut app = inspect_app(vec![("workspace", "/tmp/repo".to_string())]);
        app.show_settings = true;
        app.settings_selected = 1;

        let lines = render(&app, 72, 18);
        assert!(lines.iter().any(|line| line.contains("Harbor controls")));
        assert!(lines.iter().any(|line| line.contains("Overflow pages")));
        assert!(lines.iter().any(|line| line.contains("cycle")));
        assert!(lines.iter().any(|line| line.contains("Session only")));
        assert!(lines[0].ends_with("settings "));
    }

    #[test]
    fn short_settings_panel_scrolls_to_the_selected_control() {
        let mut app = inspect_app(Vec::new());
        app.show_settings = true;
        app.settings_selected = crate::app::SettingItem::ALL.len() - 1;

        let lines = render(&app, 40, 7);
        assert!(lines.iter().any(|line| line.contains("GitHub survey")));
        assert!(lines.iter().any(|line| line.contains('▶')));
    }
}
