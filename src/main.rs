use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use clap::Parser;
use crossterm::event::{self, Event, KeyEventKind};

use git_buoy::app::{App, AppSettings, Msg};
use git_buoy::config::SettingsConfig;
use git_buoy::{config, git, hosting, ui};

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

    /// Override the saved seconds between repository surveys.
    #[arg(long)]
    poll_interval: Option<f64>,

    /// Override the saved seconds before an unchanged vessel is idle.
    #[arg(long)]
    idle_after: Option<f64>,

    /// Start with pull requests, reviews, checks, and releases enabled.
    #[arg(long)]
    github: bool,

    /// Override the saved seconds between optional GitHub surveys.
    #[arg(long)]
    github_poll_interval: Option<f64>,
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Fail with a plain message before touching the terminal.
    let root = git::discover_root(&args.path)?;
    let name = root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "repository".to_string());

    let mut persistence = SettingsPersistence::load();
    let settings = initial_settings(&args, &persistence.saved);

    let mut collector = spawn_collector(root.clone());
    let mut hosting_collector = spawn_hosting_collector(root);

    let mut app = App::with_settings(name, settings).with_frames_per_second(args.fps);
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
        &mut persistence,
    );
    ratatui::restore();
    if let Some(error) = persistence.last_error {
        eprintln!("warning: settings were not saved: {error}");
    }
    result
}

fn initial_settings(args: &Args, saved: &SettingsConfig) -> AppSettings {
    let mut settings = AppSettings::default();
    saved.apply_to(&mut settings);
    if args.reduced_motion {
        settings.reduced_motion = true;
    }
    if let Some(seconds) = args.poll_interval {
        settings.poll_interval = Duration::from_secs_f64(seconds.max(0.2));
    }
    if let Some(seconds) = args.idle_after {
        settings.idle_after = Duration::from_secs_f64(seconds.max(1.0));
    }
    if args.github {
        settings.github_enabled = true;
    }
    if let Some(seconds) = args.github_poll_interval {
        settings.github_poll_interval = Duration::from_secs_f64(seconds.max(5.0));
    }
    settings
}

struct SettingsPersistence {
    path: Option<PathBuf>,
    saved: SettingsConfig,
    last_error: Option<String>,
}

impl SettingsPersistence {
    fn load() -> Self {
        let path = config::default_path();
        let saved = path
            .as_deref()
            .map(config::load)
            .transpose()
            .unwrap_or_else(|error| {
                eprintln!("warning: saved settings were ignored: {error}");
                Some(SettingsConfig::default())
            })
            .unwrap_or_default();
        Self {
            path,
            saved,
            last_error: None,
        }
    }

    fn record_changes(&mut self, before: &AppSettings, after: &AppSettings) {
        if before == after {
            return;
        }
        self.saved.record_changes(before, after);
        let Some(path) = &self.path else {
            self.last_error = Some(
                "no configuration directory is available; set GIT_BUOY_CONFIG to a file path"
                    .to_string(),
            );
            return;
        };
        match config::save(path, &self.saved) {
            Ok(()) => self.last_error = None,
            Err(error) => self.last_error = Some(error.to_string()),
        }
    }
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
    persistence: &mut SettingsPersistence,
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
                    let previous_settings = app.settings.clone();
                    app.update(Msg::Key(key));
                    persistence.record_changes(&previous_settings, &app.settings);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_preferences_supply_values_when_flags_are_absent() {
        let mut saved_settings = AppSettings::default();
        saved_settings.reduced_motion = true;
        saved_settings.poll_interval = Duration::from_secs(5);
        saved_settings.github_enabled = true;
        let saved = SettingsConfig::from_settings(&saved_settings);
        let args = Args::try_parse_from(["git-buoy"]).unwrap();

        let settings = initial_settings(&args, &saved);

        assert!(settings.reduced_motion);
        assert_eq!(settings.poll_interval, Duration::from_secs(5));
        assert!(settings.github_enabled);
    }

    #[test]
    fn explicit_flags_override_saved_preferences_for_the_run() {
        let saved = SettingsConfig::default();
        let args = Args::try_parse_from([
            "git-buoy",
            "--reduced-motion",
            "--poll-interval",
            "1",
            "--idle-after",
            "60",
            "--github",
            "--github-poll-interval",
            "120",
        ])
        .unwrap();

        let settings = initial_settings(&args, &saved);

        assert!(settings.reduced_motion);
        assert_eq!(settings.poll_interval, Duration::from_secs(1));
        assert_eq!(settings.idle_after, Duration::from_secs(60));
        assert!(settings.github_enabled);
        assert_eq!(settings.github_poll_interval, Duration::from_secs(120));
    }

    #[test]
    fn changed_runtime_preferences_are_saved_immediately() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        let mut persistence = SettingsPersistence {
            path: Some(path.clone()),
            saved: SettingsConfig::default(),
            last_error: None,
        };
        let before = AppSettings::default();
        let mut after = before.clone();
        after.reduced_motion = true;

        persistence.record_changes(&before, &after);

        assert!(persistence.last_error.is_none());
        let saved = config::load(&path).unwrap();
        let mut restored = AppSettings::default();
        saved.apply_to(&mut restored);
        assert!(restored.reduced_motion);
    }
}
