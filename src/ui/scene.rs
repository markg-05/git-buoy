use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::{App, Mode};
use crate::harbor::{Condition, Dock, DockKind};

use super::theme::Theme;

/// Terminals narrower than this drop the water art for a compact list.
const COMPACT_WIDTH: u16 = 46;
/// Rows per dock in the full scene: pier, water, gap.
const ROWS_PER_DOCK: usize = 3;
/// Column where a vessel's hull begins on its water line.
const VESSEL_X: usize = 5;

// Glyphs shared with the legend, kept here as the single source so the two
// never disagree about what the harbor draws.
pub(super) const VESSEL_HULL: &str = "▙▄▄▟";
pub(super) const MOORING_BUOY: char = '◍';
pub(super) const CARGO_STAGED: char = '▣';
pub(super) const CARGO_UNSTAGED: char = '▢';
pub(super) const CARGO_UNTRACKED: char = '·';
pub(super) const CARGO_CONFLICT: char = '✕';

pub fn draw_scene(frame: &mut Frame, area: Rect, app: &App, theme: &Theme) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    if !app.loaded {
        let waiting = Paragraph::new("surveying the harbor...")
            .style(Style::new().fg(theme.dim))
            .centered();
        frame.render_widget(waiting, area);
        return;
    }
    if app.harbor.docks.is_empty() {
        let empty = Paragraph::new("open water: no branches yet")
            .style(Style::new().fg(theme.dim))
            .centered();
        frame.render_widget(empty, area);
        return;
    }

    let compact = area.width < COMPACT_WIDTH;
    let rows_per_dock = if compact { 1 } else { ROWS_PER_DOCK };
    let visible = (area.height as usize / rows_per_dock).max(1);
    // Keep the selection on screen; otherwise show from the top.
    let first = match app.mode {
        Mode::Inspect if app.selected >= visible => app.selected + 1 - visible,
        _ => 0,
    };

    let width = area.width as usize;
    let frame_number = app.animation.frame();
    let mut lines: Vec<Line> = Vec::new();
    for (index, dock) in app
        .harbor
        .docks
        .iter()
        .enumerate()
        .skip(first)
        .take(visible)
    {
        let selected = app.mode == Mode::Inspect && index == app.selected;
        if compact {
            lines.push(compact_line(dock, selected, theme));
        } else {
            lines.push(pier_line(dock, width, selected, theme));
            lines.push(water_line(dock, width, index, frame_number, theme));
            lines.push(Line::default());
        }
    }
    frame.render_widget(Paragraph::new(lines), area);
}

/// The pier: a horizontal walkway carrying the dock's name and condition.
///
/// `──┬─ main · main terminal ────────────── ↑2 sealed ──`
fn pier_line(dock: &Dock, width: usize, selected: bool, theme: &Theme) -> Line<'static> {
    let post = if selected {
        "─▶┬─"
    } else {
        "──┬─"
    };
    let name = format!(" {}{} ", dock.name, kind_note(dock.kind));
    let status = format!(" {} ", status_text(dock));

    let used = post.chars().count() + name.chars().count() + status.chars().count() + 2;
    let fill = width.saturating_sub(used);

    let name_style = if selected {
        Style::new().fg(theme.text).add_modifier(Modifier::REVERSED)
    } else {
        Style::new().fg(theme.text).add_modifier(Modifier::BOLD)
    };
    Line::from(vec![
        Span::styled(post.to_string(), Style::new().fg(theme.pier)),
        Span::styled(name, name_style),
        Span::styled("─".repeat(fill), Style::new().fg(theme.pier)),
        Span::styled(status, Style::new().fg(theme.condition(dock.condition))),
        Span::styled("──".to_string(), Style::new().fg(theme.pier)),
    ])
}

/// The water: drifting waves with a vessel or mooring buoy at the dock.
///
/// `  │  ~  ▙▄▄▟ ▣▣▢    ≈      ~        ≈   `
fn water_line(
    dock: &Dock,
    width: usize,
    dock_index: usize,
    frame: u64,
    theme: &Theme,
) -> Line<'static> {
    let post = "  │ ";
    let water_width = width.saturating_sub(post.chars().count());

    let (craft, craft_color) = match &dock.vessel {
        Some(vessel) => {
            let mut hull = format!("{VESSEL_HULL} ");
            hull.push_str(&cargo_glyphs(vessel.staged, CARGO_STAGED));
            hull.push_str(&cargo_glyphs(vessel.unstaged, CARGO_UNSTAGED));
            hull.push_str(&cargo_glyphs(vessel.untracked, CARGO_UNTRACKED));
            hull.push_str(&cargo_glyphs(vessel.conflicted, CARGO_CONFLICT));
            (hull.trim_end().to_string(), theme.condition(dock.condition))
        }
        None => (MOORING_BUOY.to_string(), theme.condition(Condition::Moored)),
    };

    let craft_len = craft.chars().count();
    let craft_x = VESSEL_X.min(water_width);
    let after_craft = water_width.saturating_sub(craft_x + craft_len);

    Line::from(vec![
        Span::styled(post.to_string(), Style::new().fg(theme.pier)),
        Span::styled(
            waves(0, craft_x, dock_index, frame),
            Style::new().fg(theme.water),
        ),
        Span::styled(craft, Style::new().fg(craft_color)),
        Span::styled(
            waves(craft_x + craft_len, after_craft, dock_index, frame),
            Style::new().fg(theme.water),
        ),
    ])
}

/// Narrow-terminal fallback: one accurate line per dock, no art.
fn compact_line(dock: &Dock, selected: bool, theme: &Theme) -> Line<'static> {
    let marker = if selected { "▶" } else { " " };
    let name_style = if selected {
        Style::new().fg(theme.text).add_modifier(Modifier::REVERSED)
    } else {
        Style::new().fg(theme.text)
    };
    Line::from(vec![
        Span::styled(format!("{marker} "), Style::new().fg(theme.pier)),
        Span::styled("● ", Style::new().fg(theme.condition(dock.condition))),
        Span::styled(dock.name.clone(), name_style),
        Span::styled(
            format!(" {}", status_text(dock)),
            Style::new().fg(theme.dim),
        ),
    ])
}

fn kind_note(kind: DockKind) -> &'static str {
    match kind {
        DockKind::MainTerminal => " · main terminal",
        DockKind::DetachedWorktree => " · detached",
        DockKind::Branch => "",
    }
}

fn status_text(dock: &Dock) -> String {
    let mut parts = Vec::new();
    if let Some((ahead, behind)) = dock.sync {
        if ahead > 0 {
            parts.push(format!("↑{ahead}"));
        }
        if behind > 0 {
            parts.push(format!("↓{behind}"));
        }
    }
    parts.push(dock.condition.label().to_string());
    parts.join(" ")
}

/// Up to four glyphs per cargo category keeps busy docks readable.
fn cargo_glyphs(count: usize, glyph: char) -> String {
    std::iter::repeat_n(glyph, count.min(4)).collect()
}

/// A stretch of water starting at column `start`, `len` cells long.
fn waves(start: usize, len: usize, dock_index: usize, frame: u64) -> String {
    (start..start + len)
        .map(|x| wave_char(x, dock_index, frame))
        .collect()
}

/// Deterministic wave field: sparse crests drifting slowly left. Every glyph
/// is a pure function of position and frame, so a paused frame is a valid
/// static scene (reduced motion) and tests can assert exact output.
fn wave_char(x: usize, dock_index: usize, frame: u64) -> char {
    let drift = (x as i64 - (frame / 3) as i64).rem_euclid(97) as u64;
    let value = drift.wrapping_mul(31).wrapping_add(dock_index as u64 * 17) % 23;
    match value {
        0 | 11 => '~',
        5 => '≈',
        _ => ' ',
    }
}

#[cfg(test)]
mod tests {
    use super::wave_char;

    #[test]
    fn waves_are_deterministic_per_frame() {
        let row: String = (0..40).map(|x| wave_char(x, 0, 7)).collect();
        let again: String = (0..40).map(|x| wave_char(x, 0, 7)).collect();
        assert_eq!(row, again);
        let moved: String = (0..40).map(|x| wave_char(x, 0, 10)).collect();
        assert_ne!(row, moved, "the water should drift between frames");
    }
}
