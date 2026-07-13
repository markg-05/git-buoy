use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::{App, Mode};
use crate::harbor::{Condition, Dock, DockKind};

use super::theme::Theme;

/// Terminals narrower than this drop the water art for a compact list.
const COMPACT_WIDTH: u16 = 46;
/// Column where a vessel's hull begins on its water line.
const VESSEL_X: usize = 5;
/// The pier post that anchors every water row to its dock.
const POST: &str = "  │ ";
/// A single busy dock shows at most this many rows of cargo; anything beyond
/// collapses into a trailing "+N". The exact totals always remain available
/// in inspect mode, and this keeps one flooded worktree from swamping the
/// whole harbor.
const MAX_CARGO_ROWS: usize = 6;

// Glyphs shared with the legend, kept here as the single source so the two
// never disagree about what the harbor draws.
pub(super) const VESSEL_HULL: &str = "▙▄▄▟";
pub(super) const MOORING_BUOY: char = '◍';
pub(super) const CARGO_STAGED: char = '▣';
pub(super) const CARGO_UNSTAGED: char = '▢';
pub(super) const CARGO_UNTRACKED: char = '○';
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
    let width = area.width as usize;
    let frame_number = app.animation.frame();
    let inspecting = app.mode == Mode::Inspect;

    // Each dock renders to a block of one or more lines. Blocks vary in height
    // because a vessel's cargo can wrap, so the visible window is computed over
    // the flattened lines rather than a fixed rows-per-dock.
    let blocks: Vec<Vec<Line>> = app
        .harbor
        .docks
        .iter()
        .enumerate()
        .map(|(index, dock)| {
            let selected = inspecting && index == app.selected;
            if compact {
                vec![compact_line(dock, selected, theme)]
            } else {
                let mut block = vec![pier_line(dock, width, selected, theme)];
                block.extend(water_lines(dock, width, index, frame_number, theme));
                block.push(Line::default());
                block
            }
        })
        .collect();

    let avail = area.height as usize;
    let heights: Vec<usize> = blocks.iter().map(Vec::len).collect();
    let top = scroll_top(&heights, avail, inspecting.then_some(app.selected));

    let mut lines: Vec<Line> = if inspecting {
        blocks.into_iter().flatten().skip(top).take(avail).collect()
    } else {
        // Ambient mode has no selection to scroll into view, so reserve the
        // last row for an explicit count when whole docks would otherwise be
        // silently omitted below the viewport.
        let hidden = hidden_docks_below(&heights, avail);
        if hidden == 0 {
            blocks.into_iter().flatten().take(avail).collect()
        } else {
            let content_height = avail.saturating_sub(1);
            let hidden = hidden_docks_below(&heights, content_height);
            let mut lines: Vec<Line> = blocks.into_iter().flatten().take(content_height).collect();
            lines.push(more_docks_line(hidden, theme));
            lines
        }
    };
    lines.truncate(avail);
    frame.render_widget(Paragraph::new(lines), area);
}

/// Count docks whose first line begins below the visible portion of the
/// flattened scene. A partially visible dock is already represented and is
/// therefore not included in the count.
fn hidden_docks_below(heights: &[usize], visible_lines: usize) -> usize {
    let mut start = 0;
    heights
        .iter()
        .filter(|height| {
            let hidden = start >= visible_lines;
            start += **height;
            hidden
        })
        .count()
}

fn more_docks_line(hidden: usize, theme: &Theme) -> Line<'static> {
    let noun = if hidden == 1 { "dock" } else { "docks" };
    Line::from(Span::styled(
        format!("  … {hidden} more {noun} below"),
        Style::new().fg(theme.dim),
    ))
}

/// Pick the first flattened line to show so the selected dock stays on screen.
/// Without a selection (ambient mode) the harbor is anchored at the top.
fn scroll_top(heights: &[usize], avail: usize, selected: Option<usize>) -> usize {
    let total: usize = heights.iter().sum();
    let max_top = total.saturating_sub(avail);
    let Some(sel) = selected else {
        return 0;
    };
    let start: usize = heights[..sel.min(heights.len())].iter().sum();
    let end = start + heights.get(sel).copied().unwrap_or(0);

    let mut top = 0;
    if end > avail {
        top = end - avail;
    }
    // A dock taller than the window pins to its own top rather than its bottom.
    if start < top {
        top = start;
    }
    top.min(max_top)
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

/// The water beneath a dock: open sea with a vessel and its cargo, or a
/// mooring buoy. Cargo is drawn in full and wraps onto extra rows when it
/// runs past the terminal edge, so a busy vessel reads as visibly loaded.
fn water_lines(
    dock: &Dock,
    width: usize,
    dock_index: usize,
    frame: u64,
    theme: &Theme,
) -> Vec<Line<'static>> {
    let water_width = width.saturating_sub(POST.chars().count()).max(1);

    // A stretch of open water leads up to the vessel.
    let mut content: Vec<(char, Color)> = (0..VESSEL_X.min(water_width))
        .map(|x| (wave_char(x, dock_index, frame), theme.water))
        .collect();

    match &dock.vessel {
        Some(vessel) => {
            let color = theme.condition(dock.condition);
            for ch in VESSEL_HULL.chars() {
                content.push((ch, color));
            }
            content.push((' ', color));

            let mut cargo = Vec::new();
            push_cargo(&mut cargo, vessel.staged, CARGO_STAGED, color);
            push_cargo(&mut cargo, vessel.unstaged, CARGO_UNSTAGED, color);
            push_cargo(&mut cargo, vessel.untracked, CARGO_UNTRACKED, color);
            push_cargo(&mut cargo, vessel.conflicted, CARGO_CONFLICT, color);

            // Bound the very busy case: keep as much cargo as a few rows hold,
            // then note the remainder as "+N" rather than drawing thousands.
            let budget = water_width
                .saturating_mul(MAX_CARGO_ROWS)
                .saturating_sub(content.len());
            if cargo.len() > budget {
                let hidden = cargo.len() - budget;
                cargo.truncate(budget);
                content.extend(cargo);
                for ch in format!("+{hidden}").chars() {
                    content.push((ch, theme.dim));
                }
            } else {
                content.extend(cargo);
            }
        }
        None => content.push((MOORING_BUOY, theme.condition(Condition::Moored))),
    }

    // Flow the content across as many rows as it needs, filling the final row
    // out to the edge with open water.
    let rows = content.len().div_ceil(water_width).max(1);
    (0..rows)
        .map(|row| {
            let start = row * water_width;
            let end = (start + water_width).min(content.len());
            let mut cells = content[start..end].to_vec();
            for col in cells.len()..water_width {
                cells.push((wave_char(col, dock_index, frame), theme.water));
            }
            cells_to_line(cells, theme)
        })
        .collect()
}

fn push_cargo(out: &mut Vec<(char, Color)>, count: usize, glyph: char, color: Color) {
    out.extend(std::iter::repeat_n((glyph, color), count));
}

/// Turn a row of coloured cells into a line, prefixed with the pier post and
/// merging neighbouring cells that share a colour into one span.
fn cells_to_line(cells: Vec<(char, Color)>, theme: &Theme) -> Line<'static> {
    let mut spans = vec![Span::styled(POST.to_string(), Style::new().fg(theme.pier))];
    let mut run = String::new();
    let mut run_color: Option<Color> = None;
    for (ch, color) in cells {
        if run_color != Some(color) {
            if let Some(previous) = run_color {
                spans.push(Span::styled(
                    std::mem::take(&mut run),
                    Style::new().fg(previous),
                ));
            }
            run_color = Some(color);
        }
        run.push(ch);
    }
    if let Some(previous) = run_color {
        spans.push(Span::styled(run, Style::new().fg(previous)));
    }
    Line::from(spans)
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
    use crate::harbor::{Condition, DockKind, Vessel};

    use super::*;

    fn vessel_dock(vessel: Vessel) -> Dock {
        Dock {
            name: "x".to_string(),
            kind: DockKind::Branch,
            condition: Condition::Loading,
            vessel: Some(vessel),
            sync: None,
            detail: Vec::new(),
        }
    }

    #[test]
    fn waves_are_deterministic_per_frame() {
        let row: String = (0..40).map(|x| wave_char(x, 0, 7)).collect();
        let again: String = (0..40).map(|x| wave_char(x, 0, 7)).collect();
        assert_eq!(row, again);
        let moved: String = (0..40).map(|x| wave_char(x, 0, 10)).collect();
        assert_ne!(row, moved, "the water should drift between frames");
    }

    #[test]
    fn light_cargo_stays_on_one_row() {
        let dock = vessel_dock(Vessel {
            staged: 1,
            unstaged: 2,
            untracked: 1,
            conflicted: 0,
        });
        assert_eq!(water_lines(&dock, 80, 0, 0, &Theme::detect()).len(), 1);
    }

    #[test]
    fn cargo_wraps_only_after_crossing_the_full_water_width() {
        // At width 20, the water area is 16 cells. Five approach cells, the
        // four-cell hull, and its separator leave exactly six cargo cells.
        let exact = vessel_dock(Vessel {
            staged: 6,
            ..Vessel::default()
        });
        let overflow = vessel_dock(Vessel {
            staged: 7,
            ..Vessel::default()
        });

        assert_eq!(water_lines(&exact, 20, 0, 0, &Theme::detect()).len(), 1);
        assert_eq!(water_lines(&overflow, 20, 0, 0, &Theme::detect()).len(), 2);
    }

    #[test]
    fn wrapped_cargo_preserves_category_order_across_rows() {
        let dock = vessel_dock(Vessel {
            staged: 8,
            unstaged: 8,
            untracked: 8,
            conflicted: 8,
        });
        let lines = water_lines(&dock, 20, 0, 0, &Theme::detect());
        assert_eq!(lines.len(), 3);

        let rendered: String = lines
            .into_iter()
            .flat_map(|line| line.spans)
            .map(|span| span.content.into_owned())
            .collect();
        let cargo: String = rendered
            .chars()
            .filter(|ch| {
                [
                    CARGO_STAGED,
                    CARGO_UNSTAGED,
                    CARGO_UNTRACKED,
                    CARGO_CONFLICT,
                ]
                .contains(ch)
            })
            .collect();
        assert_eq!(
            cargo,
            format!(
                "{}{}{}{}",
                CARGO_STAGED.to_string().repeat(8),
                CARGO_UNSTAGED.to_string().repeat(8),
                CARGO_UNTRACKED.to_string().repeat(8),
                CARGO_CONFLICT.to_string().repeat(8)
            )
        );
    }

    #[test]
    fn untracked_cargo_uses_a_visible_single_cell_glyph() {
        let symbol = Line::from(CARGO_UNTRACKED.to_string());

        assert_eq!(CARGO_UNTRACKED, '○');
        assert_eq!(
            symbol.width(),
            1,
            "cargo counts must preserve one cell each"
        );
    }

    #[test]
    fn heavy_cargo_wraps_but_stays_bounded() {
        let dock = vessel_dock(Vessel {
            staged: 0,
            unstaged: 500,
            untracked: 0,
            conflicted: 0,
        });
        let lines = water_lines(&dock, 40, 0, 0, &Theme::detect());
        assert!(lines.len() > 1, "500 changes should wrap onto extra rows");
        assert!(
            lines.len() <= MAX_CARGO_ROWS + 1,
            "a flooded dock must stay bounded, got {} rows",
            lines.len()
        );
    }

    #[test]
    fn scroll_keeps_the_selected_dock_in_view() {
        let heights = vec![3, 3, 3, 3, 3]; // 15 lines total
        assert_eq!(scroll_top(&heights, 6, None), 0);
        assert_eq!(scroll_top(&heights, 6, Some(0)), 0);
        // The last dock occupies lines 12..15; a 6-line window scrolls to 9.
        assert_eq!(scroll_top(&heights, 6, Some(4)), 9);
        // A dock taller than the window pins to its own top.
        assert_eq!(scroll_top(&[10], 4, Some(0)), 0);
    }

    #[test]
    fn hidden_dock_count_ignores_partially_visible_docks() {
        let heights = vec![4, 3, 2];

        assert_eq!(hidden_docks_below(&heights, 9), 0);
        assert_eq!(hidden_docks_below(&heights, 5), 1);
        assert_eq!(hidden_docks_below(&heights, 4), 2);
        assert_eq!(hidden_docks_below(&heights, 0), 3);
    }

    #[test]
    fn overflow_marker_uses_singular_and_plural_dock_counts() {
        let theme = Theme::detect();
        let text = |count| {
            more_docks_line(count, &theme)
                .spans
                .into_iter()
                .map(|span| span.content.into_owned())
                .collect::<String>()
        };

        assert_eq!(text(1), "  … 1 more dock below");
        assert_eq!(text(3), "  … 3 more docks below");
    }
}
