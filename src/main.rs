use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use clap::Parser;
use crossterm::event::{self, Event, KeyEventKind};

use git_buoy::app::{App, Msg};
use git_buoy::{git, hosting, ui};

/// A living terminal harbor for understanding parallel software work at a
/// glance.
#[derive(Parser, Debug)]
#[command(version, about)]
struct Args {
    /// Path to a Git repository, or any directory inside one.
    #[arg(default_value = ".")]
    path: PathBuf,

    /// Render a static scene instead of animating the water.
    #[arg(long)]
    reduced_motion: bool,

    /// Ambient animation rate in frames per second (1-30).
    #[arg(long, default_value_t = 12)]
    fps: u8,

    /// Seconds between repository surveys.
    #[arg(long, default_value_t = 2.0)]
    poll_interval: f64,

    /// Seconds without observable repository changes before a vessel is idle.
    #[arg(long, default_value_t = 30.0)]
    idle_after: f64,

    /// Observe pull requests, reviews, checks, and releases through GitHub CLI.
    #[arg(long)]
    github: bool,

    /// Seconds between optional GitHub surveys.
    #[arg(long, default_value_t = 30.0)]
    github_poll_interval: f64,
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Fail with a plain message before touching the terminal.
    let root = git::discover_root(&args.path)?;
    let name = root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "repository".to_string());

    let receiver = spawn_collector(
        root.clone(),
        Duration::from_secs_f64(args.poll_interval.max(0.2)),
    );
    let hosting_receiver = args.github.then(|| {
        spawn_hosting_collector(
            root,
            Duration::from_secs_f64(args.github_poll_interval.max(5.0)),
        )
    });

    let mut app = App::new(name, args.reduced_motion)
        .with_idle_after(Duration::from_secs_f64(args.idle_after.max(1.0)));
    let theme = ui::Theme::detect();
    let tick = Duration::from_millis(1000 / u64::from(args.fps.clamp(1, 30)));

    let terminal = ratatui::init();
    let result = run(
        terminal,
        &mut app,
        &theme,
        &receiver,
        hosting_receiver.as_ref(),
        tick,
    );
    ratatui::restore();
    result
}

fn spawn_hosting_collector(
    root: PathBuf,
    interval: Duration,
) -> mpsc::Receiver<Result<hosting::HostingSnapshot, String>> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        loop {
            let snapshot = hosting::collect_github(&root).map_err(|error| error.to_string());
            if sender.send(snapshot).is_err() {
                return;
            }
            thread::sleep(interval);
        }
    });
    receiver
}

/// Survey the repository on an interval, off the render path so animation
/// never waits on repository reads. The thread ends when the UI drops the
/// receiver.
fn spawn_collector(
    root: PathBuf,
    interval: Duration,
) -> mpsc::Receiver<(Result<git::RepoSnapshot, String>, Instant)> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        loop {
            let snapshot = git::collect(&root).map_err(|e| e.to_string());
            if sender.send((snapshot, Instant::now())).is_err() {
                return;
            }
            thread::sleep(interval);
        }
    });
    receiver
}

fn run(
    mut terminal: ratatui::DefaultTerminal,
    app: &mut App,
    theme: &ui::Theme,
    receiver: &mpsc::Receiver<(Result<git::RepoSnapshot, String>, Instant)>,
    hosting_receiver: Option<&mpsc::Receiver<Result<hosting::HostingSnapshot, String>>>,
    tick: Duration,
) -> Result<()> {
    let mut last_tick = Instant::now();
    loop {
        while let Ok((result, observed_at)) = receiver.try_recv() {
            app.update(Msg::Snapshot {
                result,
                observed_at,
            });
        }
        if let Some(receiver) = hosting_receiver {
            while let Ok(result) = receiver.try_recv() {
                app.update(Msg::Hosting(result));
            }
        }

        terminal
            .draw(|frame| ui::draw(frame, app, theme))
            .context("failed to draw the harbor")?;

        if app.should_quit {
            return Ok(());
        }

        let timeout = tick.saturating_sub(last_tick.elapsed());
        if event::poll(timeout).context("failed to poll terminal events")? {
            match event::read().context("failed to read terminal event")? {
                // Windows delivers key release events too; act on presses only.
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    app.update(Msg::Key(key));
                }
                _ => {}
            }
        }
        if last_tick.elapsed() >= tick {
            app.update(Msg::Tick);
            last_tick = Instant::now();
        }
    }
}
