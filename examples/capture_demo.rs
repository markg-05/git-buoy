//! Capture the real demo fixture through Git Buoy's production collector,
//! state machine, and ratatui renderer.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use git_buoy::app::{App, AppSettings, InspectTarget, Mode, Msg};
use git_buoy::harbor::Condition;
use git_buoy::{git, ui};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier};

const CELL_WIDTH: f64 = 8.6;
const CELL_HEIGHT: f64 = 18.5;
const PADDING: f64 = 14.0;
const BACKGROUND: &str = "#0f1622";
const DEFAULT_FOREGROUND: &str = "#dcdfe4";

fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let repository = args
        .next()
        .map(PathBuf::from)
        .context("usage: capture_demo REPOSITORY OUTPUT_DIRECTORY")?;
    let output = args
        .next()
        .map(PathBuf::from)
        .context("usage: capture_demo REPOSITORY OUTPUT_DIRECTORY")?;
    if args.next().is_some() {
        bail!("usage: capture_demo REPOSITORY OUTPUT_DIRECTORY");
    }

    fs::create_dir_all(&output).with_context(|| format!("cannot create {}", output.display()))?;

    let static_frame = capture_static(&repository)?;
    write_svg(
        &output.join("demo.svg"),
        "Git Buoy ambient demo",
        &[static_frame],
        None,
    )?;

    let inspect_frame = capture_inspect(&repository)?;
    write_svg(
        &output.join("inspect.svg"),
        "Git Buoy changed-path inspection",
        &[inspect_frame],
        None,
    )?;

    let motion_frames = capture_motion(&repository)?;
    write_svg(
        &output.join("demo-motion.svg"),
        "Git Buoy live cargo transition and ambient paging",
        &motion_frames,
        Some(Duration::from_millis(125)),
    )?;
    Ok(())
}

fn demo_app(repository: &Path) -> Result<(App, Instant)> {
    let snapshot = git::collect(repository)?;
    verify_fixture(&snapshot)?;
    let started = Instant::now();
    let mut settings = AppSettings::default();
    settings.idle_after = Duration::from_secs(1);
    settings.cycle_interval = Duration::from_secs(1);
    settings.github_enabled = false;
    let mut app = App::with_settings(snapshot.name.clone(), settings).with_frames_per_second(8);
    observe(&mut app, snapshot.clone(), started);
    observe(&mut app, snapshot, started + Duration::from_secs(2));
    Ok((app, started))
}

fn verify_fixture(snapshot: &git::RepoSnapshot) -> Result<()> {
    if snapshot.workspaces.len() != 10 {
        bail!(
            "demo fixture has {} workspaces; expected 10",
            snapshot.workspaces.len()
        );
    }
    let harbor = git_buoy::harbor::to_harbor(snapshot);
    for (name, expected) in [
        ("main", Condition::Calm),
        ("demo/blocked", Condition::Blocked),
        ("demo/cargo-overflow", Condition::Sealed),
        ("demo/diverged", Condition::Diverged),
        ("demo/idle", Condition::Calm),
        ("demo/incoming", Condition::Incoming),
        ("demo/live-loading", Condition::Calm),
        ("demo/loading", Condition::Loading),
        ("demo/outbound", Condition::Outbound),
        ("demo/sealed", Condition::Sealed),
        ("demo/moored", Condition::Moored),
        ("demo/parked", Condition::Moored),
    ] {
        let actual = harbor
            .docks
            .iter()
            .find(|dock| dock.name == name)
            .with_context(|| format!("fixture is missing {name}"))?
            .condition;
        if actual != expected {
            bail!("{name} is {actual:?}; expected {expected:?}");
        }
    }
    let cargo = harbor
        .docks
        .iter()
        .find(|dock| dock.name == "demo/cargo-overflow")
        .and_then(|dock| dock.vessel.as_ref())
        .context("demo/cargo-overflow has no worktree")?;
    if cargo.cargo.len() < 90 {
        bail!(
            "cargo overflow fixture has only {} changed paths",
            cargo.cargo.len()
        );
    }
    Ok(())
}

fn observe(app: &mut App, snapshot: git::RepoSnapshot, observed_at: Instant) {
    app.update(Msg::Snapshot {
        result: Ok(snapshot),
        observed_at,
    });
}

fn capture_static(repository: &Path) -> Result<RenderedFrame> {
    let (mut app, _) = demo_app(repository)?;
    for _ in 0..7 {
        app.update(Msg::Tick);
    }
    render(&app, 124, 35)
}

fn capture_inspect(repository: &Path) -> Result<RenderedFrame> {
    let (mut app, _) = demo_app(repository)?;
    app.selected = dock_index(&app, "demo/loading")?;
    app.mode = Mode::Inspect;
    app.inspect_target = InspectTarget::Change(0);
    render(&app, 124, 25)
}

fn capture_motion(repository: &Path) -> Result<Vec<RenderedFrame>> {
    let (mut app, started) = demo_app(repository)?;
    let live_index = dock_index(&app, "demo/live-loading")?;
    let workspace = app.harbor.docks[live_index]
        .vessel
        .as_ref()
        .context("demo/live-loading has no worktree")?
        .workspace
        .clone();
    let transition_files: Vec<_> = (1..=12)
        .map(|number| workspace.join(format!("live-crate-{number:03}.txt")))
        .collect();
    for (number, path) in transition_files.iter().enumerate() {
        if path.exists() {
            fs::remove_file(path).with_context(|| format!("cannot clean {}", path.display()))?;
        }
        fs::write(path, format!("new live cargo {:03}\n", number + 1))?;
    }

    let changed = git::collect(repository)?;
    observe(&mut app, changed, started + Duration::from_secs(3));

    let result = (|| {
        let mut frames = Vec::new();
        for _ in 0..16 {
            frames.push(render(&app, 124, 29)?);
            app.update(Msg::Tick);
        }
        Ok(frames)
    })();
    for path in transition_files {
        fs::remove_file(&path).with_context(|| format!("cannot restore {}", path.display()))?;
    }
    result
}

fn dock_index(app: &App, name: &str) -> Result<usize> {
    app.harbor
        .docks
        .iter()
        .position(|dock| dock.name == name)
        .with_context(|| format!("fixture is missing {name}"))
}

#[derive(Clone)]
struct RenderedCell {
    symbol: String,
    foreground: String,
    background: Option<String>,
    bold: bool,
}

struct RenderedFrame {
    width: u16,
    height: u16,
    cells: Vec<RenderedCell>,
}

fn render(app: &App, width: u16, height: u16) -> Result<RenderedFrame> {
    let mut terminal = Terminal::new(TestBackend::new(width, height))?;
    terminal.draw(|frame| ui::draw(frame, app, &ui::Theme::detect()))?;
    Ok(frame_from_buffer(
        terminal.backend().buffer(),
        width,
        height,
    ))
}

fn frame_from_buffer(buffer: &Buffer, width: u16, height: u16) -> RenderedFrame {
    let mut cells = Vec::with_capacity(usize::from(width) * usize::from(height));
    for y in 0..height {
        for x in 0..width {
            let cell = buffer.cell((x, y)).expect("capture coordinates are valid");
            let reversed = cell.modifier.contains(Modifier::REVERSED);
            let normal_foreground = color(cell.fg, DEFAULT_FOREGROUND);
            let normal_background = match cell.bg {
                Color::Reset => None,
                value => Some(color(value, BACKGROUND)),
            };
            let (foreground, background) = if reversed {
                (
                    normal_background.unwrap_or_else(|| BACKGROUND.to_string()),
                    Some(normal_foreground),
                )
            } else {
                (normal_foreground, normal_background)
            };
            cells.push(RenderedCell {
                symbol: cell.symbol().to_string(),
                foreground,
                background,
                bold: cell.modifier.contains(Modifier::BOLD),
            });
        }
    }
    RenderedFrame {
        width,
        height,
        cells,
    }
}

fn color(color: Color, reset: &str) -> String {
    let rgb = match color {
        Color::Reset => return reset.to_string(),
        Color::Black => (0, 0, 0),
        Color::Red => (205, 49, 49),
        Color::Green => (13, 188, 121),
        Color::Yellow => (229, 229, 16),
        Color::Blue => (36, 114, 200),
        Color::Magenta => (188, 63, 188),
        Color::Cyan => (17, 168, 205),
        Color::Gray => (204, 204, 204),
        Color::DarkGray => (102, 102, 102),
        Color::LightRed => (241, 76, 76),
        Color::LightGreen => (35, 209, 139),
        Color::LightYellow => (245, 245, 67),
        Color::LightBlue => (59, 142, 234),
        Color::LightMagenta => (214, 112, 214),
        Color::LightCyan => (41, 184, 219),
        Color::White => (242, 242, 242),
        Color::Rgb(red, green, blue) => (red, green, blue),
        Color::Indexed(index) => indexed_rgb(index),
    };
    format!("#{:02x}{:02x}{:02x}", rgb.0, rgb.1, rgb.2)
}

fn indexed_rgb(index: u8) -> (u8, u8, u8) {
    const ANSI: [(u8, u8, u8); 16] = [
        (0, 0, 0),
        (128, 0, 0),
        (0, 128, 0),
        (128, 128, 0),
        (0, 0, 128),
        (128, 0, 128),
        (0, 128, 128),
        (192, 192, 192),
        (128, 128, 128),
        (255, 0, 0),
        (0, 255, 0),
        (255, 255, 0),
        (0, 0, 255),
        (255, 0, 255),
        (0, 255, 255),
        (255, 255, 255),
    ];
    if index < 16 {
        return ANSI[usize::from(index)];
    }
    if index >= 232 {
        let value = 8 + (index - 232) * 10;
        return (value, value, value);
    }
    let cube = index - 16;
    let channel = |value: u8| if value == 0 { 0 } else { 55 + value * 40 };
    (
        channel(cube / 36),
        channel((cube % 36) / 6),
        channel(cube % 6),
    )
}

fn write_svg(
    path: &Path,
    title: &str,
    frames: &[RenderedFrame],
    frame_duration: Option<Duration>,
) -> Result<()> {
    let first = frames.first().context("cannot write an empty capture")?;
    if frames
        .iter()
        .any(|frame| frame.width != first.width || frame.height != first.height)
    {
        bail!("all animation frames must use the same terminal size");
    }
    let width = f64::from(first.width) * CELL_WIDTH + PADDING * 2.0;
    let height = f64::from(first.height) * CELL_HEIGHT + PADDING * 2.0;
    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width:.0}\" height=\"{height:.0}\" viewBox=\"0 0 {width:.1} {height:.1}\" font-family=\"ui-monospace,SFMono-Regular,Menlo,Consolas,'DejaVu Sans Mono',monospace\" font-size=\"15\">\n<title>{}</title>\n<rect width=\"100%\" height=\"100%\" rx=\"10\" fill=\"{BACKGROUND}\"/>\n",
        xml_escape(title)
    );
    if let Some(duration) = frame_duration {
        let total = duration.as_secs_f64() * frames.len() as f64;
        let visible = 100.0 / frames.len() as f64;
        svg.push_str(&format!(
            "<style>\n.frame{{opacity:0;animation:frame-cycle {total:.3}s linear infinite}}\n@keyframes frame-cycle{{0%,{:.5}%{{opacity:1}}{visible:.5}%,100%{{opacity:0}}}}\n@media (prefers-reduced-motion:reduce){{.frame{{animation:none;opacity:0}}.frame-0{{opacity:1}}}}\n</style>\n",
            visible - 0.00001
        ));
    }
    for (index, frame) in frames.iter().enumerate() {
        if let Some(duration) = frame_duration {
            svg.push_str(&format!(
                "<g class=\"frame frame-{index}\" style=\"animation-delay:{:.3}s\">\n",
                duration.as_secs_f64() * index as f64
            ));
        } else {
            svg.push_str("<g>\n");
        }
        svg.push_str(&frame_svg(frame));
        svg.push_str("</g>\n");
    }
    svg.push_str("</svg>\n");
    fs::write(path, svg).with_context(|| format!("cannot write {}", path.display()))
}

fn frame_svg(frame: &RenderedFrame) -> String {
    let mut svg = String::new();
    for y in 0..frame.height {
        let row = usize::from(y) * usize::from(frame.width);
        let mut x = 0usize;
        while x < usize::from(frame.width) {
            let cell = &frame.cells[row + x];
            let Some(background) = &cell.background else {
                x += 1;
                continue;
            };
            let start = x;
            while x < usize::from(frame.width)
                && frame.cells[row + x].background.as_ref() == Some(background)
            {
                x += 1;
            }
            svg.push_str(&format!(
                "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{CELL_HEIGHT:.1}\" fill=\"{background}\"/>\n",
                PADDING + start as f64 * CELL_WIDTH,
                PADDING + f64::from(y) * CELL_HEIGHT,
                (x - start) as f64 * CELL_WIDTH,
            ));
        }

        x = 0;
        while x < usize::from(frame.width) {
            while x < usize::from(frame.width) && frame.cells[row + x].symbol.trim().is_empty() {
                x += 1;
            }
            if x >= usize::from(frame.width) {
                break;
            }
            let start = x;
            let style = (
                frame.cells[row + x].foreground.as_str(),
                frame.cells[row + x].bold,
            );
            let mut symbols = Vec::new();
            while x < usize::from(frame.width) {
                let cell = &frame.cells[row + x];
                if (cell.foreground.as_str(), cell.bold) != style {
                    break;
                }
                symbols.push(cell.symbol.as_str());
                x += 1;
            }
            let Some(last) = symbols.iter().rposition(|symbol| !symbol.trim().is_empty()) else {
                continue;
            };
            let text = symbols[..=last].concat();
            let cell_count = last + 1;
            svg.push_str(&format!(
                "<text x=\"{:.1}\" y=\"{:.1}\" fill=\"{}\"{} xml:space=\"preserve\" textLength=\"{:.1}\" lengthAdjust=\"spacingAndGlyphs\">{}</text>\n",
                PADDING + start as f64 * CELL_WIDTH,
                PADDING + f64::from(y) * CELL_HEIGHT + 13.9,
                style.0,
                if style.1 { " font-weight=\"700\"" } else { "" },
                cell_count as f64 * CELL_WIDTH,
                xml_escape(&text),
            ));
        }
    }
    svg
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
