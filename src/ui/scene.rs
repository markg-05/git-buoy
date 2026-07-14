use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::{App, Mode};
use crate::harbor::{
    CargoCounts, Clearance, Condition, Dock, DockEvent, DockKind, DockTransition,
    DockTransitionKind, EventKind, InspectionStatus, ReviewStatus, Vessel, VesselActivity,
};

use super::theme::Theme;

/// Terminals narrower than this drop the water art for a compact list.
const COMPACT_WIDTH: u16 = 46;
/// Column where a vessel's hull begins on its water line.
const VESSEL_X: usize = 5;
/// The pier post that anchors every water row to its dock.
const POST: &str = "  │ ";
/// A single busy dock shows at most this many rows of cargo; anything beyond
/// collapses into a trailing "…N". The exact totals always remain available
/// in inspect mode, and this keeps one flooded worktree from swamping the
/// whole harbor.
const MAX_CARGO_ROWS: usize = 6;

// Glyphs shared with the legend, kept here as the single source so the two
// never disagree about what the harbor draws.
pub(super) const VESSEL_HULL: &str = "▙▄▄▟";
pub(super) const MOORING_BUOY: char = '◍';
pub(super) const CARGO_STAGED: char = '▣';
pub(super) const CARGO_UNSTAGED: char = '▢';
pub(super) const CARGO_UNTRACKED: char = '+';
pub(super) const CARGO_CONFLICT: char = '✕';
pub(super) const ACTIVITY_WAKE: &str = "≈~";
pub(super) const EVENT_COMMIT: &str = "▣ committed";
pub(super) const EVENT_PUSH: &str = "▙▄▄▟→ pushed";
pub(super) const EVENT_MERGE: &str = "←▣ merged";
pub(super) const EVENT_UPDATE: &str = "↔ updated";
pub(super) const CLEARANCE_FLAG: &str = "PR#";
pub(super) const RELEASE_CONVOY: &str = "▙▄▄▟ ▙▄▄▟→";

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
    if app.harbor.docks.is_empty() && app.harbor.convoys.is_empty() {
        let empty = Paragraph::new("open water: no branches yet")
            .style(Style::new().fg(theme.dim))
            .centered();
        frame.render_widget(empty, area);
        return;
    }

    let frame_number = app.animation.frame();
    let mut area = area;
    if let Some(convoy) = app
        .harbor
        .convoys
        .iter()
        .find(|convoy| convoy.is_latest)
        .or_else(|| app.harbor.convoys.first())
    {
        let convoy_area = Rect::new(area.x, area.y, area.width, 1);
        frame.render_widget(
            Paragraph::new(convoy_line(convoy, frame_number, theme)),
            convoy_area,
        );
        area.y = area.y.saturating_add(1);
        area.height = area.height.saturating_sub(1);
        if area.height == 0 || app.harbor.docks.is_empty() {
            return;
        }
    }

    let compact = area.width < COMPACT_WIDTH;
    let width = area.width as usize;
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
                vec![compact_line(dock, selected, frame_number, theme)]
            } else {
                let mut block = vec![pier_line(dock, width, selected, frame_number, theme)];
                block.extend(water_lines(dock, width, index, frame_number, theme));
                block.push(Line::default());
                block
            }
        })
        .collect();

    let avail = area.height as usize;
    let heights: Vec<usize> = blocks.iter().map(Vec::len).collect();
    let mut lines: Vec<Line> = if inspecting {
        let top = scroll_top(&heights, avail, Some(app.selected));
        blocks.into_iter().flatten().skip(top).take(avail).collect()
    } else {
        let pages = ambient_pages(&heights, avail);
        if pages.len() == 1 {
            // A one-row viewport cannot show a dock and an overflow marker at
            // once. Keep the marker truthful and static rather than cycling an
            // unreadable view.
            let hidden = hidden_docks_below(&heights, avail);
            if hidden == 0 {
                blocks.into_iter().flatten().take(avail).collect()
            } else {
                let content_height = avail.saturating_sub(1);
                let hidden = hidden_docks_below(&heights, content_height);
                let mut lines: Vec<Line> =
                    blocks.into_iter().flatten().take(content_height).collect();
                lines.push(dock_position_line(0, hidden, width, theme));
                lines
            }
        } else {
            let page_index = ambient_page_index(
                app.page_cycle_frame(),
                pages.len(),
                app.settings.auto_cycle && !app.settings.reduced_motion,
                app.settings.page_hold_frames(),
            );
            let page = pages[page_index];
            let content_height = avail - 1;
            let mut lines: Vec<Line> = blocks
                .into_iter()
                .skip(page.start)
                .take(page.end - page.start)
                .flatten()
                .take(content_height)
                .collect();
            lines.push(dock_position_line(
                page.start,
                heights.len() - page.end,
                width,
                theme,
            ));
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AmbientPage {
    start: usize,
    end: usize,
}

/// Split docks into stable pages without cutting between ordinary dock rows.
/// One row is reserved for position information whenever cycling is needed.
/// A single dock taller than the remaining space is clipped on its own page so
/// its pier is still seen and the cycle can continue.
fn ambient_pages(heights: &[usize], avail: usize) -> Vec<AmbientPage> {
    if heights.is_empty() {
        return Vec::new();
    }
    if avail < 2 || hidden_docks_below(heights, avail) == 0 {
        return vec![AmbientPage {
            start: 0,
            end: heights.len(),
        }];
    }

    let capacity = avail - 1;
    let mut pages = Vec::new();
    let mut start = 0;
    while start < heights.len() {
        let mut end = start;
        let mut used = 0usize;
        while end < heights.len() {
            let height = heights[end];
            if end > start && used.saturating_add(height) > capacity {
                break;
            }
            end += 1;
            used = used.saturating_add(height);
            if used >= capacity {
                break;
            }
        }
        pages.push(AmbientPage { start, end });
        start = end;
    }
    pages
}

fn ambient_page_index(frame: u64, page_count: usize, auto_cycle: bool, hold_frames: u64) -> usize {
    if !auto_cycle || page_count <= 1 {
        return 0;
    }
    ((frame / hold_frames.max(1)) % page_count as u64) as usize
}

fn dock_position_line(above: usize, below: usize, width: usize, theme: &Theme) -> Line<'static> {
    let docks = |count| if count == 1 { "dock" } else { "docks" };
    let full = match (above, below) {
        (0, below) => format!("  … {below} more {} below", docks(below)),
        (above, 0) => format!("  … {above} {} above", docks(above)),
        (above, below) => format!("  … {above} above · {below} below"),
    };
    let compact = match (above, below) {
        (0, below) => format!("… ↓{below}"),
        (above, 0) => format!("… ↑{above}"),
        (above, below) => format!("… ↑{above} ↓{below}"),
    };
    let text = if full.chars().count() <= width {
        full
    } else {
        compact
    };
    Line::from(Span::styled(text, Style::new().fg(theme.dim)))
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
fn pier_line(
    dock: &Dock,
    width: usize,
    selected: bool,
    frame: u64,
    theme: &Theme,
) -> Line<'static> {
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
    let mut status_style = Style::new().fg(theme.condition(dock.condition));
    if lane_transition_is_emphasized(dock, frame) {
        status_style = status_style.add_modifier(Modifier::BOLD | Modifier::REVERSED);
    }
    Line::from(vec![
        Span::styled(post.to_string(), Style::new().fg(theme.pier)),
        Span::styled(name, name_style),
        Span::styled("─".repeat(fill), Style::new().fg(theme.pier)),
        Span::styled(status, status_style),
        Span::styled("──".to_string(), Style::new().fg(theme.pier)),
    ])
}

fn convoy_line(convoy: &crate::harbor::Convoy, frame: u64, theme: &Theme) -> Line<'static> {
    let travel = " ".repeat(((frame / 4) % 6) as usize);
    let prerelease = if convoy.is_prerelease {
        " · prerelease"
    } else {
        ""
    };
    Line::from(vec![
        Span::styled("  convoy ", Style::new().fg(theme.dim)),
        Span::styled(
            format!("{travel}{RELEASE_CONVOY}"),
            Style::new().fg(theme.condition(Condition::Outbound)),
        ),
        Span::styled(
            format!(" {}{}", convoy.tag, prerelease),
            Style::new().fg(theme.text),
        ),
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

    match rendered_vessel(dock, frame) {
        Some((vessel, motion)) => {
            let vessel_x = match motion {
                VesselMotion::Steady => vessel_x(dock, water_width, frame),
                VesselMotion::Arriving { elapsed, duration } => {
                    let base = vessel_x_base(water_width);
                    interpolate_position(vessel_x_furthest(water_width), base, elapsed, duration)
                }
                VesselMotion::Departing { elapsed, duration } => {
                    let base = vessel_x_base(water_width);
                    interpolate_position(base, vessel_x_furthest(water_width), elapsed, duration)
                }
            };
            // Open water leads to the vessel. A wake replaces its last cells
            // when motion is communicating recent or directional activity.
            let mut content: Vec<(char, Color)> = (0..vessel_x.min(water_width))
                .map(|x| (wave_char(x, dock_index, frame), theme.water))
                .collect();
            if shows_left_wake(dock, vessel) {
                overlay_left_wake(&mut content, frame, theme.water);
            }
            let color = theme.condition(dock.condition);
            for ch in vessel_hull(vessel, frame).chars() {
                content.push((ch, color));
            }
            content.push((' ', color));

            if dock.condition == Condition::Incoming {
                for ch in ACTIVITY_WAKE.chars() {
                    content.push((ch, theme.water));
                }
                content.push((' ', theme.water));
            }

            let mut cargo = cargo_cells(rendered_cargo_counts(dock, vessel, frame), theme);

            // Bound the very busy case: keep as much cargo as a few rows hold,
            // then note the remainder as "…N" rather than drawing thousands.
            let budget = water_width
                .saturating_mul(MAX_CARGO_ROWS)
                .saturating_sub(content.len());
            if cargo.len() > budget {
                let hidden = cargo[budget..]
                    .iter()
                    .filter(|(glyph, _)| *glyph != ' ')
                    .count();
                cargo.truncate(budget);
                while cargo.last().is_some_and(|(glyph, _)| *glyph == ' ') {
                    cargo.pop();
                }
                content.extend(cargo);
                for ch in format!("…{hidden}").chars() {
                    content.push((ch, theme.dim));
                }
            } else {
                content.extend(cargo);
            }
            for event in &dock.events {
                content.push((' ', theme.dim));
                content.push((' ', theme.dim));
                content.extend(event_cells(event, frame, theme));
            }
            for clearance in &dock.clearances {
                content.push((' ', theme.dim));
                content.push((' ', theme.dim));
                content.extend(clearance_cells(clearance, theme));
            }

            flow_water(content, water_width, dock_index, frame, theme)
        }
        None if !dock.clearances.is_empty() => {
            let mut content: Vec<(char, Color)> = (0..VESSEL_X.min(water_width))
                .map(|x| (wave_char(x, dock_index, frame), theme.water))
                .collect();
            for ch in VESSEL_HULL.chars() {
                content.push((ch, theme.condition(Condition::Awaiting)));
            }
            content.push((' ', theme.dim));
            for clearance in &dock.clearances {
                content.extend(clearance_cells(clearance, theme));
                content.push((' ', theme.dim));
            }
            flow_water(content, water_width, dock_index, frame, theme)
        }
        None => {
            let mut content: Vec<(char, Color)> = (0..VESSEL_X.min(water_width))
                .map(|x| (wave_char(x, dock_index, frame), theme.water))
                .collect();
            content.push((MOORING_BUOY, theme.condition(Condition::Moored)));
            flow_water(content, water_width, dock_index, frame, theme)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VesselMotion {
    Steady,
    Arriving { elapsed: u64, duration: u64 },
    Departing { elapsed: u64, duration: u64 },
}

fn rendered_vessel(dock: &Dock, frame: u64) -> Option<(&Vessel, VesselMotion)> {
    if let Some(transition) = dock.transition.as_ref()
        && let Some((elapsed, duration)) = transition_progress(transition, frame)
    {
        match &transition.kind {
            DockTransitionKind::VesselArriving => {
                return dock
                    .vessel
                    .as_ref()
                    .map(|vessel| (vessel, VesselMotion::Arriving { elapsed, duration }));
            }
            DockTransitionKind::VesselDeparting { vessel } => {
                return Some((vessel, VesselMotion::Departing { elapsed, duration }));
            }
            _ => {}
        }
    }
    dock.vessel
        .as_ref()
        .map(|vessel| (vessel, VesselMotion::Steady))
}

fn rendered_cargo_counts(dock: &Dock, vessel: &Vessel, frame: u64) -> CargoCounts {
    let target = vessel.cargo_counts();
    let Some(transition) = dock.transition.as_ref() else {
        return target;
    };
    let Some((elapsed, duration)) = transition_progress(transition, frame) else {
        return target;
    };
    let DockTransitionKind::Cargo { from } = transition.kind else {
        return target;
    };
    CargoCounts {
        staged: interpolate_count(from.staged, target.staged, elapsed, duration),
        unstaged: interpolate_count(from.unstaged, target.unstaged, elapsed, duration),
        untracked: interpolate_count(from.untracked, target.untracked, elapsed, duration),
        conflicted: interpolate_count(from.conflicted, target.conflicted, elapsed, duration),
    }
}

fn transition_progress(transition: &DockTransition, frame: u64) -> Option<(u64, u64)> {
    let duration = transition.duration_frames.max(1);
    let elapsed = frame.wrapping_sub(transition.started_frame);
    (elapsed < duration).then_some((elapsed, duration))
}

fn interpolate_count(from: usize, to: usize, elapsed: u64, duration: u64) -> usize {
    if from == to {
        return to;
    }
    let distance = from.abs_diff(to) as u128;
    let moved = distance
        .saturating_mul(elapsed as u128)
        .saturating_add((duration / 2) as u128)
        / duration as u128;
    let moved = usize::try_from(moved).unwrap_or(usize::MAX);
    if to > from {
        from.saturating_add(moved).min(to)
    } else {
        from.saturating_sub(moved).max(to)
    }
}

fn interpolate_position(from: usize, to: usize, elapsed: u64, duration: u64) -> usize {
    interpolate_count(from, to, elapsed, duration)
}

fn clearance_cells(clearance: &Clearance, theme: &Theme) -> Vec<(char, Color)> {
    let inspection = clearance.inspection_status();
    let inspection_glyph = match inspection {
        InspectionStatus::Passing => '✓',
        InspectionStatus::Failing => '!',
        InspectionStatus::Pending => '…',
        InspectionStatus::Unknown => '?',
    };
    let review_glyph = match clearance.review {
        ReviewStatus::Approved => '✓',
        ReviewStatus::ChangesRequested => '!',
        ReviewStatus::Required => '…',
        ReviewStatus::None => '?',
    };
    let color = match inspection {
        InspectionStatus::Failing => theme.condition(Condition::Blocked),
        InspectionStatus::Pending => theme.condition(Condition::Loading),
        InspectionStatus::Passing if clearance.review == ReviewStatus::Approved => {
            theme.condition(Condition::Calm)
        }
        _ => theme.condition(Condition::Awaiting),
    };
    format!(
        "{CLEARANCE_FLAG}{} {review_glyph}{inspection_glyph}",
        clearance.number
    )
    .chars()
    .map(|glyph| (glyph, color))
    .collect()
}

fn event_cells(event: &DockEvent, frame: u64, theme: &Theme) -> Vec<(char, Color)> {
    let phase = ((frame / 3) % 6) as usize;
    let (leading, marker, color) = match event.kind {
        EventKind::Commit => (phase % 2, EVENT_COMMIT, theme.condition(Condition::Sealed)),
        EventKind::Push => (phase, EVENT_PUSH, theme.condition(Condition::Outbound)),
        EventKind::Merge => (
            5usize.saturating_sub(phase),
            EVENT_MERGE,
            theme.condition(Condition::Calm),
        ),
        EventKind::Update => (phase % 2, EVENT_UPDATE, theme.text),
    };
    std::iter::repeat_n((' ', theme.water), leading)
        .chain(marker.chars().map(|glyph| (glyph, color)))
        .collect()
}

fn flow_water(
    content: Vec<(char, Color)>,
    water_width: usize,
    dock_index: usize,
    frame: u64,
    theme: &Theme,
) -> Vec<Line<'static>> {
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

fn vessel_x(dock: &Dock, water_width: usize, frame: u64) -> usize {
    let base = vessel_x_base(water_width);
    let furthest = vessel_x_furthest(water_width);
    let distance = furthest.saturating_sub(base);
    if distance == 0 {
        return base;
    }
    let phase = ((frame / 4) % (distance as u64 + 1)) as usize;
    match dock.condition {
        Condition::Outbound => base + phase,
        Condition::Incoming => furthest - phase,
        Condition::Diverged => base + ((frame / 6) as usize % 2).min(distance),
        _ => base,
    }
}

fn vessel_x_base(water_width: usize) -> usize {
    VESSEL_X.min(water_width.saturating_sub(1))
}

fn vessel_x_furthest(water_width: usize) -> usize {
    let base = vessel_x_base(water_width);
    water_width
        .saturating_sub(VESSEL_HULL.chars().count() + 1)
        .min(base.saturating_add(12))
}

fn shows_left_wake(dock: &Dock, vessel: &Vessel) -> bool {
    dock.condition == Condition::Outbound || vessel.activity == VesselActivity::Recent
}

fn overlay_left_wake(content: &mut [(char, Color)], frame: u64, color: Color) {
    let wake = if (frame / 3).is_multiple_of(2) {
        ACTIVITY_WAKE
    } else {
        "~≈"
    };
    let start = content.len().saturating_sub(wake.chars().count());
    for (cell, glyph) in content[start..].iter_mut().zip(wake.chars()) {
        *cell = (glyph, color);
    }
}

fn vessel_hull(vessel: &Vessel, frame: u64) -> &'static str {
    if vessel.activity == VesselActivity::Recent && (frame / 4).is_multiple_of(2) {
        "▜▄▄▛"
    } else {
        VESSEL_HULL
    }
}

fn cargo_cells(counts: CargoCounts, theme: &Theme) -> Vec<(char, Color)> {
    let mut cargo = Vec::new();
    push_cargo_group(
        &mut cargo,
        counts.staged,
        CARGO_STAGED,
        theme.condition(Condition::Sealed),
    );
    push_cargo_group(
        &mut cargo,
        counts.unstaged,
        CARGO_UNSTAGED,
        theme.condition(Condition::Loading),
    );
    push_cargo_group(&mut cargo, counts.untracked, CARGO_UNTRACKED, theme.text);
    push_cargo_group(
        &mut cargo,
        counts.conflicted,
        CARGO_CONFLICT,
        theme.condition(Condition::Blocked),
    );
    cargo
}

fn push_cargo_group(out: &mut Vec<(char, Color)>, count: usize, glyph: char, color: Color) {
    if count == 0 {
        return;
    }
    if !out.is_empty() {
        out.push((' ', color));
    }
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
fn compact_line(dock: &Dock, selected: bool, frame: u64, theme: &Theme) -> Line<'static> {
    let marker = if selected { "▶" } else { " " };
    let name_style = if selected {
        Style::new().fg(theme.text).add_modifier(Modifier::REVERSED)
    } else {
        Style::new().fg(theme.text)
    };
    let mut status_style = Style::new().fg(theme.condition(dock.condition));
    let mut detail_style = Style::new().fg(theme.dim);
    if transition_is_emphasized(dock, frame) {
        let emphasis = Modifier::BOLD | Modifier::REVERSED;
        status_style = status_style.add_modifier(emphasis);
        detail_style = detail_style.add_modifier(emphasis);
    }
    Line::from(vec![
        Span::styled(format!("{marker} "), Style::new().fg(theme.pier)),
        Span::styled("● ", status_style),
        Span::styled(dock.name.clone(), name_style),
        Span::styled(format!(" {}", status_text(dock)), detail_style),
    ])
}

fn lane_transition_is_emphasized(dock: &Dock, frame: u64) -> bool {
    dock.transition.as_ref().is_some_and(|transition| {
        matches!(
            transition.kind,
            DockTransitionKind::BecameBlocked | DockTransitionKind::BecameUnblocked
        ) && transition_emphasis(transition, frame)
    })
}

fn transition_is_emphasized(dock: &Dock, frame: u64) -> bool {
    dock.transition
        .as_ref()
        .is_some_and(|transition| transition_emphasis(transition, frame))
}

fn transition_emphasis(transition: &DockTransition, frame: u64) -> bool {
    let Some((elapsed, duration)) = transition_progress(transition, frame) else {
        return false;
    };
    ((elapsed.saturating_mul(4) / duration).min(3)).is_multiple_of(2)
}

fn kind_note(kind: DockKind) -> &'static str {
    match kind {
        DockKind::MainTerminal => " · main terminal",
        DockKind::DetachedWorktree => " · detached",
        DockKind::RemoteBranch => " · remote PR",
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
    if let Some(vessel) = dock.vessel.as_ref() {
        parts.push(format!("· {}", vessel.activity.label()));
    }
    if let Some(event) = dock.events.last() {
        parts.push(format!("· {}", event.kind.label()));
    }
    if let Some(clearance) = dock.clearances.first() {
        let review = match clearance.review {
            ReviewStatus::Approved => '✓',
            ReviewStatus::ChangesRequested => '!',
            ReviewStatus::Required => '…',
            ReviewStatus::None => '?',
        };
        let checks = match clearance.inspection_status() {
            InspectionStatus::Passing => '✓',
            InspectionStatus::Failing => '!',
            InspectionStatus::Pending => '…',
            InspectionStatus::Unknown => '?',
        };
        parts.push(format!(
            "· {CLEARANCE_FLAG}{} {review}{checks}",
            clearance.number
        ));
        if dock.clearances.len() > 1 {
            parts.push(format!("+{}", dock.clearances.len() - 1));
        }
    }
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
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use crate::harbor::{Condition, DockKind, Harbor, Vessel};

    use super::*;

    fn vessel_dock(vessel: Vessel) -> Dock {
        Dock {
            name: "x".to_string(),
            kind: DockKind::Branch,
            condition: Condition::Loading,
            vessel: Some(vessel),
            sync: None,
            detail: Vec::new(),
            events: Vec::new(),
            transition: None,
            clearances: Vec::new(),
        }
    }

    fn motion_dock(condition: Condition, activity: VesselActivity) -> Dock {
        Dock {
            condition,
            vessel: Some(Vessel {
                activity,
                ..Vessel::default()
            }),
            ..vessel_dock(Vessel::default())
        }
    }

    fn ambient_app(names: &[&str]) -> App {
        let mut app = App::new("test".to_string(), false);
        app.harbor = Harbor {
            name: "test".to_string(),
            convoys: Vec::new(),
            docks: names
                .iter()
                .map(|name| Dock {
                    name: (*name).to_string(),
                    kind: DockKind::Branch,
                    condition: Condition::Moored,
                    vessel: None,
                    sync: None,
                    detail: Vec::new(),
                    events: Vec::new(),
                    transition: None,
                    clearances: Vec::new(),
                })
                .collect(),
        };
        app.loaded = true;
        app
    }

    fn render_scene(app: &App, width: u16, height: u16) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| draw_scene(frame, frame.area(), app, &Theme::detect()))
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
    fn waves_are_deterministic_per_frame() {
        let row: String = (0..40).map(|x| wave_char(x, 0, 7)).collect();
        let again: String = (0..40).map(|x| wave_char(x, 0, 7)).collect();
        assert_eq!(row, again);
        let moved: String = (0..40).map(|x| wave_char(x, 0, 10)).collect();
        assert_ne!(row, moved, "the water should drift between frames");
    }

    #[test]
    fn branch_sync_controls_vessel_direction() {
        let outbound = motion_dock(Condition::Outbound, VesselActivity::Idle);
        let incoming = motion_dock(Condition::Incoming, VesselActivity::Idle);
        let calm = motion_dock(Condition::Calm, VesselActivity::Idle);

        assert!(vessel_x(&outbound, 40, 16) > vessel_x(&outbound, 40, 0));
        assert!(vessel_x(&incoming, 40, 16) < vessel_x(&incoming, 40, 0));
        assert_eq!(vessel_x(&calm, 40, 16), vessel_x(&calm, 40, 0));
    }

    #[test]
    fn recent_activity_animates_hull_and_wake() {
        let vessel = Vessel {
            activity: VesselActivity::Recent,
            ..Vessel::default()
        };
        assert_ne!(vessel_hull(&vessel, 0), vessel_hull(&vessel, 4));

        let mut water = vec![(' ', Color::Blue); 5];
        overlay_left_wake(&mut water, 0, Color::Blue);
        assert_eq!(water[3].0, '≈');
        assert_eq!(water[4].0, '~');
    }

    #[test]
    fn cargo_transition_interpolates_monotonically_to_current_counts() {
        let mut dock = vessel_dock(Vessel {
            staged: 10,
            unstaged: 1,
            ..Vessel::default()
        });
        dock.transition = Some(DockTransition {
            kind: DockTransitionKind::Cargo {
                from: CargoCounts {
                    staged: 2,
                    unstaged: 5,
                    ..CargoCounts::default()
                },
            },
            started_frame: 0,
            duration_frames: 8,
        });
        let vessel = dock.vessel.as_ref().unwrap();

        let start = rendered_cargo_counts(&dock, vessel, 0);
        let middle = rendered_cargo_counts(&dock, vessel, 4);
        let end = rendered_cargo_counts(&dock, vessel, 8);
        assert_eq!((start.staged, start.unstaged), (2, 5));
        assert_eq!((middle.staged, middle.unstaged), (6, 3));
        assert_eq!((end.staged, end.unstaged), (10, 1));
        assert!(start.staged <= middle.staged && middle.staged <= end.staged);
        assert!(start.unstaged >= middle.unstaged && middle.unstaged >= end.unstaged);
    }

    #[test]
    fn vessel_arrives_toward_pier_and_departure_resolves_to_buoy() {
        let mut arriving = vessel_dock(Vessel::default());
        arriving.condition = Condition::Local;
        arriving.transition = Some(DockTransition {
            kind: DockTransitionKind::VesselArriving,
            started_frame: 0,
            duration_frames: 8,
        });
        let position = |dock: &Dock, frame| {
            let (_, motion) = rendered_vessel(dock, frame).unwrap();
            match motion {
                VesselMotion::Arriving { elapsed, duration } => interpolate_position(
                    vessel_x_furthest(40),
                    vessel_x_base(40),
                    elapsed,
                    duration,
                ),
                VesselMotion::Steady => vessel_x(dock, 40, frame),
                VesselMotion::Departing { .. } => unreachable!(),
            }
        };
        assert!(position(&arriving, 0) > position(&arriving, 4));
        assert!(position(&arriving, 4) > position(&arriving, 8));

        let mut departing = vessel_dock(Vessel::default());
        departing.condition = Condition::Moored;
        departing.vessel = None;
        departing.transition = Some(DockTransition {
            kind: DockTransitionKind::VesselDeparting {
                vessel: Vessel::default(),
            },
            started_frame: 0,
            duration_frames: 8,
        });
        assert!(matches!(
            rendered_vessel(&departing, 0),
            Some((_, VesselMotion::Departing { .. }))
        ));
        assert!(rendered_vessel(&departing, 8).is_none());
        let settled: String = water_lines(&departing, 40, 0, 8, &Theme::detect())
            .into_iter()
            .flat_map(|line| line.spans)
            .map(|span| span.content.into_owned())
            .collect();
        assert!(settled.contains(MOORING_BUOY));
        assert!(!settled.contains(VESSEL_HULL));
    }

    #[test]
    fn lane_and_compact_cues_pulse_twice_then_settle() {
        let transition = DockTransition {
            kind: DockTransitionKind::BecameBlocked,
            started_frame: 0,
            duration_frames: 8,
        };
        assert!(transition_emphasis(&transition, 0));
        assert!(!transition_emphasis(&transition, 2));
        assert!(transition_emphasis(&transition, 4));
        assert!(!transition_emphasis(&transition, 6));
        assert!(!transition_emphasis(&transition, 8));

        let mut compact = vessel_dock(Vessel::default());
        compact.transition = Some(DockTransition {
            kind: DockTransitionKind::Cargo {
                from: CargoCounts::default(),
            },
            ..transition
        });
        let emphasized = compact_line(&compact, false, 0, &Theme::detect());
        assert!(
            emphasized.spans[1]
                .style
                .add_modifier
                .contains(Modifier::REVERSED)
        );
        let settled = compact_line(&compact, false, 8, &Theme::detect());
        assert!(
            !settled.spans[1]
                .style
                .add_modifier
                .contains(Modifier::REVERSED)
        );
    }

    #[test]
    fn transition_progress_handles_wrapping_frame_counter() {
        let transition = DockTransition {
            kind: DockTransitionKind::VesselArriving,
            started_frame: u64::MAX - 2,
            duration_frames: 8,
        };
        assert_eq!(transition_progress(&transition, 1), Some((4, 8)));
    }

    #[test]
    fn live_transition_markers_move_in_their_semantic_direction() {
        let theme = Theme::detect();
        let push = DockEvent {
            kind: EventKind::Push,
            summary: String::new(),
        };
        let merge = DockEvent {
            kind: EventKind::Merge,
            summary: String::new(),
        };
        let leading_spaces =
            |cells: Vec<(char, Color)>| cells.iter().take_while(|(glyph, _)| *glyph == ' ').count();

        assert!(
            leading_spaces(event_cells(&push, 12, &theme))
                > leading_spaces(event_cells(&push, 0, &theme))
        );
        assert!(
            leading_spaces(event_cells(&merge, 12, &theme))
                < leading_spaces(event_cells(&merge, 0, &theme))
        );
    }

    #[test]
    fn clearance_marker_reports_review_and_check_state() {
        let clearance = Clearance {
            number: 42,
            title: String::new(),
            url: String::new(),
            is_draft: false,
            review: ReviewStatus::Approved,
            landing: crate::harbor::LandingStatus::Blocked,
            inspections: vec![crate::harbor::Inspection {
                name: "test".to_string(),
                status: InspectionStatus::Failing,
                url: None,
            }],
        };
        let marker: String = clearance_cells(&clearance, &Theme::detect())
            .into_iter()
            .map(|(glyph, _)| glyph)
            .collect();

        assert_eq!(marker, "PR#42 ✓!");
    }

    #[test]
    fn latest_release_renders_as_a_convoy() {
        let convoy = crate::harbor::Convoy {
            tag: "v1.0.0".to_string(),
            name: "One".to_string(),
            is_latest: true,
            is_prerelease: false,
            published_at: None,
        };
        let rendered = convoy_line(&convoy, 0, &Theme::detect())
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();

        assert!(rendered.contains(RELEASE_CONVOY));
        assert!(rendered.contains("v1.0.0"));
    }

    #[test]
    fn light_cargo_stays_on_one_row() {
        let dock = vessel_dock(Vessel {
            staged: 1,
            unstaged: 2,
            untracked: 1,
            conflicted: 0,
            ..Vessel::default()
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
            ..Vessel::default()
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
    fn cargo_groups_use_distinct_glyphs_colors_and_spacing() {
        let theme = Theme::detect();
        let vessel = Vessel {
            staged: 1,
            unstaged: 2,
            untracked: 3,
            conflicted: 1,
            ..Vessel::default()
        };
        let cargo = cargo_cells(vessel.cargo_counts(), &theme);
        let glyphs: String = cargo.iter().map(|(glyph, _)| glyph).collect();

        assert_eq!(glyphs, "▣ ▢▢ +++ ✕");
        assert_eq!(cargo[0].1, theme.condition(Condition::Sealed));
        assert_eq!(cargo[2].1, theme.condition(Condition::Loading));
        assert_eq!(cargo[5].1, theme.text);
        assert_eq!(cargo[9].1, theme.condition(Condition::Blocked));
    }

    #[test]
    fn untracked_cargo_uses_a_visible_single_cell_glyph() {
        let symbol = Line::from(CARGO_UNTRACKED.to_string());

        assert_eq!(CARGO_UNTRACKED, '+');
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
            ..Vessel::default()
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
    fn cargo_overflow_count_ignores_group_separators() {
        let dock = vessel_dock(Vessel {
            staged: 1,
            untracked: 100,
            ..Vessel::default()
        });
        let rendered: String = water_lines(&dock, 20, 0, 0, &Theme::detect())
            .into_iter()
            .flat_map(|line| line.spans)
            .map(|span| span.content.into_owned())
            .collect();

        assert!(
            rendered.contains("…16"),
            "group spacing must not be reported as hidden cargo: {rendered}"
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
    fn ambient_pages_preserve_dock_boundaries_and_cover_every_dock() {
        assert_eq!(
            ambient_pages(&[3, 3, 3, 3, 3], 7),
            vec![
                AmbientPage { start: 0, end: 2 },
                AmbientPage { start: 2, end: 4 },
                AmbientPage { start: 4, end: 5 },
            ]
        );
        assert_eq!(
            ambient_pages(&[4, 2, 5, 1], 7),
            vec![
                AmbientPage { start: 0, end: 2 },
                AmbientPage { start: 2, end: 4 },
            ]
        );
    }

    #[test]
    fn ambient_pages_handle_exact_fit_tall_docks_and_tiny_views() {
        assert_eq!(
            ambient_pages(&[3, 3], 6),
            vec![AmbientPage { start: 0, end: 2 }]
        );
        assert_eq!(
            ambient_pages(&[10, 3, 3], 6),
            vec![
                AmbientPage { start: 0, end: 1 },
                AmbientPage { start: 1, end: 2 },
                AmbientPage { start: 2, end: 3 },
            ]
        );
        assert_eq!(
            ambient_pages(&[1, 1, 1], 1),
            vec![AmbientPage { start: 0, end: 3 }]
        );
    }

    #[test]
    fn ambient_page_index_holds_wraps_and_respects_auto_cycle() {
        assert_eq!(ambient_page_index(0, 3, true, 120), 0);
        assert_eq!(ambient_page_index(119, 3, true, 120), 0);
        assert_eq!(ambient_page_index(120, 3, true, 120), 1);
        assert_eq!(ambient_page_index(240, 3, true, 120), 2);
        assert_eq!(ambient_page_index(360, 3, true, 120), 0);
        assert_eq!(ambient_page_index(u64::MAX, 3, false, 120), 0);
    }

    #[test]
    fn dock_position_marker_reports_both_directions_and_compacts() {
        let theme = Theme::detect();
        let text = |above, below, width| {
            dock_position_line(above, below, width, &theme)
                .spans
                .into_iter()
                .map(|span| span.content.into_owned())
                .collect::<String>()
        };

        assert_eq!(text(0, 1, 80), "  … 1 more dock below");
        assert_eq!(text(0, 3, 80), "  … 3 more docks below");
        assert_eq!(text(3, 4, 80), "  … 3 above · 4 below");
        assert_eq!(text(1, 0, 80), "  … 1 dock above");
        assert_eq!(text(12, 34, 8), "… ↑12 ↓34");
    }

    #[test]
    fn ambient_render_advances_pages_and_reduced_motion_returns_to_first() {
        let mut app = ambient_app(&["dock-0", "dock-1", "dock-2", "dock-3", "dock-4"]);

        let first = render_scene(&app, 40, 4);
        assert!(first[0].contains("dock-0"));
        assert!(first[3].contains("2 more docks below"));

        for _ in 0..app.settings.page_hold_frames() {
            app.animation.tick();
        }
        let second = render_scene(&app, 40, 4);
        assert!(second[0].contains("dock-3"));
        assert!(second[2].contains("3 docks above"));

        app.settings.auto_cycle = false;
        let held = render_scene(&app, 40, 4);
        assert!(held[0].contains("dock-0"));
        assert!(held[3].contains("2 more docks below"));

        app.settings.auto_cycle = true;
        app.settings.reduced_motion = true;
        let reduced = render_scene(&app, 40, 4);
        assert!(reduced[0].contains("dock-0"));
        assert!(reduced[3].contains("2 more docks below"));
    }
}
