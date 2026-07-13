//! Rendering. Consumes the harbor scene and application state; owns all
//! ratatui specifics so nothing above this layer touches the terminal.

mod inspect;
mod legend;
mod scene;
mod theme;

pub use theme::Theme;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};

use crate::app::{App, Mode};

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

    // The legend floats above everything else so it can be summoned in either
    // mode without disturbing the scene underneath.
    if app.show_legend {
        legend::draw_legend(frame, frame.area(), theme);
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
                let wanted = inspect::content_width(dock).saturating_add(2) as u16;
                let panel_width = wanted.clamp(MIN_INSPECT_WIDTH, area.width.saturating_sub(2));
                let panel = Rect {
                    x: area.x + area.width - panel_width,
                    y: area.y,
                    width: panel_width,
                    height: area.height,
                };
                frame.render_widget(Clear, panel);
                inspect::draw_inspect(frame, panel, app, theme);
            }
        }
        // Too narrow to float: precision wins over ambience, panel takes over.
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
    let hints = if app.show_legend {
        " l/esc close legend"
    } else {
        match app.mode {
            Mode::Ambient => " i inspect · l legend · m motion · q quit",
            Mode::Inspect => " tab/j/k select · l legend · esc back · q quit",
        }
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
