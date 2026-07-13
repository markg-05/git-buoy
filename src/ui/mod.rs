//! Rendering. Consumes the harbor scene and application state; owns all
//! ratatui specifics so nothing above this layer touches the terminal.

mod inspect;
mod scene;
mod theme;

pub use theme::Theme;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::{App, Mode};

/// Minimum width for showing the inspect panel beside the scene instead of
/// in place of it.
const SPLIT_WIDTH: u16 = 76;
const INSPECT_WIDTH: u16 = 42;

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
}

fn draw_body(frame: &mut Frame, area: Rect, app: &App, theme: &Theme) {
    match app.mode {
        Mode::Ambient => scene::draw_scene(frame, area, app, theme),
        Mode::Inspect if area.width >= SPLIT_WIDTH => {
            let [scene_area, inspect_area] =
                Layout::horizontal([Constraint::Min(0), Constraint::Length(INSPECT_WIDTH)])
                    .areas(area);
            scene::draw_scene(frame, scene_area, app, theme);
            inspect::draw_inspect(frame, inspect_area, app, theme);
        }
        // Too narrow for a split: precision wins over ambience.
        Mode::Inspect => inspect::draw_inspect(frame, area, app, theme),
    }
}

fn draw_header(frame: &mut Frame, area: Rect, app: &App, theme: &Theme) {
    let title = Line::from(vec![
        Span::styled(" ≋ ", Style::new().fg(theme.water)),
        Span::styled(app.harbor.name.clone(), Style::new().fg(theme.text)),
        Span::styled(" harbor", Style::new().fg(theme.dim)),
    ]);
    frame.render_widget(Paragraph::new(title), area);

    let mode = match app.mode {
        Mode::Ambient => "ambient ",
        Mode::Inspect => "inspect ",
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
    let hints = match app.mode {
        Mode::Ambient => " i inspect · m motion · q quit",
        Mode::Inspect => " tab/j/k select · esc back · m motion · q quit",
    };
    frame.render_widget(
        Paragraph::new(Span::styled(hints, Style::new().fg(theme.dim))),
        area,
    );
    if app.reduced_motion {
        let badge = Paragraph::new(Span::styled("reduced motion ", Style::new().fg(theme.dim)))
            .right_aligned();
        frame.render_widget(badge, area);
    }
}
