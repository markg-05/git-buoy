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

    /// Start with pull requests, reviews, checks, and releases enabled.
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

    let mut collector = spawn_collector(root.clone());
    let mut hosting_collector = spawn_hosting_collector(root);

    let mut app = App::new(name, args.reduced_motion)
        .with_frames_per_second(args.fps)
        .with_poll_interval(Duration::from_secs_f64(args.poll_interval.max(0.2)))
        .with_idle_after(Duration::from_secs_f64(args.idle_after.max(1.0)))
        .with_github(
            args.github,
            Duration::from_secs_f64(args.github_poll_interval.max(5.0)),
        );
    let theme = ui::Theme::detect();
    let tick = Duration::from_millis(1000 / u64::from(args.fps.clamp(1, 30)));

    let terminal = ratatui::init();
    let result = run(
        terminal,
        &mut app,
        &theme,
        &mut collector,
        &mut hosting_collector,
        tick,
    );
    ratatui::restore();
    result
}

fn spawn_hosting_collector(root: PathBuf) -> Collector<Result<hosting::HostingSnapshot, String>> {
    spawn_worker(move || hosting::collect_github(&root).map_err(|error| error.to_string()))
}

fn spawn_collector(root: PathBuf) -> Collector<(Result<git::RepoSnapshot, String>, Instant)> {
    spawn_worker(move || {
        let snapshot = git::collect(&root).map_err(|error| error.to_string());
        (snapshot, Instant::now())
    })
}

/// A request-driven worker keeps repository and hosting reads off the render
/// path while allowing session settings to change their cadence immediately.
struct Collector<T> {
    requests: mpsc::Sender<()>,
    results: mpsc::Receiver<T>,
    in_flight: bool,
    last_started: Option<Instant>,
}

impl<T> Collector<T> {
    fn request_if_due(&mut self, interval: Duration) {
        if self.in_flight
            || self
                .last_started
                .is_some_and(|started| started.elapsed() < interval)
        {
            return;
        }
        if self.requests.send(()).is_ok() {
            self.in_flight = true;
            self.last_started = Some(Instant::now());
        }
    }

    fn try_recv(&mut self) -> Option<T> {
        let result = self.results.try_recv().ok()?;
        self.in_flight = false;
        Some(result)
    }

    fn reset_schedule(&mut self) {
        self.last_started = None;
    }
}

fn spawn_worker<T, F>(collect: F) -> Collector<T>
where
    T: Send + 'static,
    F: Fn() -> T + Send + 'static,
{
    let (request_sender, request_receiver) = mpsc::channel();
    let (result_sender, result_receiver) = mpsc::channel();
    thread::spawn(move || {
        while request_receiver.recv().is_ok() {
            if result_sender.send(collect()).is_err() {
                return;
            }
        }
    });
    Collector {
        requests: request_sender,
        results: result_receiver,
        in_flight: false,
        last_started: None,
    }
}

fn run(
    mut terminal: ratatui::DefaultTerminal,
    app: &mut App,
    theme: &ui::Theme,
    collector: &mut Collector<(Result<git::RepoSnapshot, String>, Instant)>,
    hosting_collector: &mut Collector<Result<hosting::HostingSnapshot, String>>,
    tick: Duration,
) -> Result<()> {
    let mut last_tick = Instant::now();
    let mut github_was_enabled = false;
    loop {
        collector.request_if_due(app.settings.poll_interval);
        while let Some((result, observed_at)) = collector.try_recv() {
            app.update(Msg::Snapshot {
                result,
                observed_at,
            });
        }

        if app.settings.github_enabled {
            if !github_was_enabled {
                hosting_collector.reset_schedule();
            }
            hosting_collector.request_if_due(app.settings.github_poll_interval);
            while let Some(result) = hosting_collector.try_recv() {
                app.update(Msg::Hosting(result));
            }
        } else {
            // Drain an observation that completed just as the user disabled
            // the adapter; App intentionally ignores it while disabled.
            while let Some(result) = hosting_collector.try_recv() {
                app.update(Msg::Hosting(result));
            }
        }
        github_was_enabled = app.settings.github_enabled;

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
