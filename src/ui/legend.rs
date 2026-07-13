use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};

use crate::harbor::Condition;

use super::scene::{
    ACTIVITY_WAKE, CARGO_CONFLICT, CARGO_STAGED, CARGO_UNSTAGED, CARGO_UNTRACKED, CLEARANCE_FLAG,
    EVENT_COMMIT, EVENT_MERGE, EVENT_PUSH, MOORING_BUOY, RELEASE_CONVOY, VESSEL_HULL,
};
use super::theme::Theme;

/// Width of the label column so descriptions line up under each other.
const LABEL_WIDTH: usize = 9;

/// Draw the legend as a centered overlay above the current scene. It reads
/// the same condition list, descriptions, and glyphs the harbor draws with,
/// so it can never describe a state the scene renders differently.
pub fn draw_legend(frame: &mut Frame, screen: Rect, theme: &Theme, scroll: usize) {
    let lines = legend_lines(theme);
    let width = lines
        .iter()
        .map(Line::width)
        .max()
        .unwrap_or(0)
        .saturating_add(4) as u16; // borders plus a column of breathing room
    let height = lines.len() as u16 + 2;
    let area = centered(screen, width, height);

    let block = Block::bordered()
        .title(" Legend · j/k scroll ")
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

fn legend_lines(theme: &Theme) -> Vec<Line<'static>> {
    let mut lines = vec![section("Dock conditions", theme)];

    for condition in Condition::ALL {
        let color = theme.condition(condition);
        lines.push(Line::from(vec![
            Span::styled("  ● ", Style::new().fg(color)),
            Span::styled(
                format!("{:<width$}", condition.label(), width = LABEL_WIDTH),
                Style::new().fg(color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                condition.description().to_string(),
                Style::new().fg(theme.text),
            ),
        ]));
    }

    lines.push(Line::default());
    lines.push(section("Cargo aboard a vessel", theme));
    lines.push(cargo_line(
        CARGO_STAGED,
        "staged files",
        theme.condition(Condition::Sealed),
        theme,
    ));
    lines.push(cargo_line(
        CARGO_UNSTAGED,
        "unstaged files",
        theme.condition(Condition::Loading),
        theme,
    ));
    lines.push(cargo_line(
        CARGO_UNTRACKED,
        "untracked files",
        theme.text,
        theme,
    ));
    lines.push(cargo_line(
        CARGO_CONFLICT,
        "conflicted files",
        theme.condition(Condition::Blocked),
        theme,
    ));

    lines.push(Line::default());
    lines.push(section("Symbols", theme));
    lines.push(symbol_line(
        VESSEL_HULL.to_string(),
        "work checked out at this dock",
        theme.text,
        theme,
    ));
    lines.push(symbol_line(
        MOORING_BUOY.to_string(),
        "a branch with no worktree",
        theme.condition(Condition::Moored),
        theme,
    ));
    lines.push(symbol_line(
        "↑ ↓".to_string(),
        "commits ahead of / behind upstream",
        theme.condition(Condition::Outbound),
        theme,
    ));
    lines.push(symbol_line(
        ACTIVITY_WAKE.to_string(),
        "wake from recent or directional activity",
        theme.water,
        theme,
    ));
    lines.push(symbol_line(
        format!("{CLEARANCE_FLAG}42 ✓!"),
        "pull request: review passed, check failed",
        theme.condition(Condition::Blocked),
        theme,
    ));
    lines.push(symbol_line(
        RELEASE_CONVOY.to_string(),
        "latest published release convoy",
        theme.condition(Condition::Outbound),
        theme,
    ));

    lines.push(Line::default());
    lines.push(section("Live transitions", theme));
    lines.push(symbol_line(
        EVENT_COMMIT.to_string(),
        "a commit observed during this run",
        theme.condition(Condition::Sealed),
        theme,
    ));
    lines.push(symbol_line(
        EVENT_PUSH.to_string(),
        "ahead commits sent upstream",
        theme.condition(Condition::Outbound),
        theme,
    ));
    lines.push(symbol_line(
        EVENT_MERGE.to_string(),
        "a merge commit arriving at a dock",
        theme.condition(Condition::Calm),
        theme,
    ));
    lines.push(Line::from(Span::styled(
        "  Cargo, vessel, and lane changes animate once, then settle.",
        Style::new().fg(theme.dim),
    )));

    lines
}

fn section(title: &str, theme: &Theme) -> Line<'static> {
    Line::from(Span::styled(
        format!(" {title}"),
        Style::new().fg(theme.dim).add_modifier(Modifier::BOLD),
    ))
}

fn cargo_line(
    glyph: char,
    label: &'static str,
    glyph_color: ratatui::style::Color,
    theme: &Theme,
) -> Line<'static> {
    symbol_line(glyph.to_string(), label, glyph_color, theme)
}

fn symbol_line(
    glyph: String,
    label: &'static str,
    glyph_color: ratatui::style::Color,
    theme: &Theme,
) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("  {glyph:<4} "), Style::new().fg(glyph_color)),
        Span::styled(label, Style::new().fg(theme.text)),
    ])
}

/// A rectangle of `width` x `height` centered in `area`, clamped to fit.
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
