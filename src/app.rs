use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent};

use crate::git::{RepoSnapshot, TipAction, UpstreamAction};
use crate::harbor::{
    self, Animation, Clearance, Condition, Convoy, Dock, DockEvent, DockKind, DockTransition,
    DockTransitionKind, EventKind, Harbor, Inspection, InspectionStatus, LandingStatus,
    ReviewStatus, Vessel, VesselActivity,
};
use crate::hosting::{CheckState, HostingSnapshot, MergeState, ReviewState};

const DEFAULT_IDLE_AFTER: Duration = Duration::from_secs(30);
const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(2);
const DEFAULT_GITHUB_POLL_INTERVAL: Duration = Duration::from_secs(30);
const DEFAULT_CYCLE_INTERVAL: Duration = Duration::from_secs(10);
const DEFAULT_FRAMES_PER_SECOND: u8 = 12;
const EVENT_LIFETIME: Duration = Duration::from_secs(12);
const VISUAL_TRANSITION_DURATION: Duration = Duration::from_millis(750);

const CYCLE_INTERVALS: [Duration; 4] = [
    Duration::from_secs(5),
    Duration::from_secs(10),
    Duration::from_secs(20),
    Duration::from_secs(30),
];
const POLL_INTERVALS: [Duration; 5] = [
    Duration::from_millis(500),
    Duration::from_secs(1),
    Duration::from_secs(2),
    Duration::from_secs(5),
    Duration::from_secs(10),
];
const IDLE_INTERVALS: [Duration; 5] = [
    Duration::from_secs(10),
    Duration::from_secs(30),
    Duration::from_secs(60),
    Duration::from_secs(120),
    Duration::from_secs(300),
];
const GITHUB_POLL_INTERVALS: [Duration; 5] = [
    Duration::from_secs(10),
    Duration::from_secs(30),
    Duration::from_secs(60),
    Duration::from_secs(120),
    Duration::from_secs(300),
];

/// The two complementary experiences: a passive overview and keyboard-driven
/// access to exact repository details.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Ambient,
    Inspect,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum InspectTarget {
    #[default]
    Dock,
    Vessel,
    Change(usize),
    PullRequest(usize),
    Check {
        pull_request: usize,
        check: usize,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingItem {
    Motion,
    AutoCycle,
    CycleInterval,
    SettingHelp,
    PollInterval,
    IdleAfter,
    GithubEnabled,
    GithubPollInterval,
}

impl SettingItem {
    pub const ALL: [Self; 8] = [
        Self::Motion,
        Self::AutoCycle,
        Self::CycleInterval,
        Self::SettingHelp,
        Self::PollInterval,
        Self::IdleAfter,
        Self::GithubEnabled,
        Self::GithubPollInterval,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Motion => "Motion",
            Self::AutoCycle => "Overflow pages",
            Self::CycleInterval => "Page interval",
            Self::SettingHelp => "Setting help",
            Self::PollInterval => "Repository survey",
            Self::IdleAfter => "Workspace idle",
            Self::GithubEnabled => "GitHub observer",
            Self::GithubPollInterval => "GitHub survey",
        }
    }

    pub fn help(self) -> &'static str {
        match self {
            Self::Motion => {
                "Stops visual animation while keeping every condition, arrow, and activity label visible."
            }
            Self::AutoCycle => {
                "Advances only when docks overflow the available height. Reduced motion pauses it."
            }
            Self::CycleInterval => {
                "Controls how long each overflowing dock page remains visible before the next one."
            }
            Self::SettingHelp => {
                "Shows this logbook note for the selected control. Very short terminals omit the note."
            }
            Self::PollInterval => {
                "Controls how often local branches, worktrees, changes, and synchronization are surveyed."
            }
            Self::IdleAfter => {
                "Sets how long a workspace can remain unchanged before its vessel is labeled idle."
            }
            Self::GithubEnabled => {
                "Surveys pull requests, reviews, checks, and releases through the authenticated gh CLI."
            }
            Self::GithubPollInterval => {
                "Controls the remote survey cadence while the GitHub observer is enabled."
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppSettings {
    pub reduced_motion: bool,
    pub auto_cycle: bool,
    pub cycle_interval: Duration,
    pub setting_help: bool,
    pub poll_interval: Duration,
    pub idle_after: Duration,
    pub github_enabled: bool,
    pub github_poll_interval: Duration,
    frames_per_second: u8,
}

impl AppSettings {
    pub fn value_label(&self, item: SettingItem) -> String {
        match item {
            SettingItem::Motion => if self.reduced_motion {
                "reduced"
            } else {
                "full"
            }
            .to_string(),
            SettingItem::AutoCycle => {
                if !self.auto_cycle {
                    "hold first page".to_string()
                } else if self.reduced_motion {
                    "cycle (paused)".to_string()
                } else {
                    "cycle".to_string()
                }
            }
            SettingItem::CycleInterval => duration_label(self.cycle_interval, "every"),
            SettingItem::SettingHelp => if self.setting_help { "on" } else { "off" }.to_string(),
            SettingItem::PollInterval => duration_label(self.poll_interval, "every"),
            SettingItem::IdleAfter => duration_label(self.idle_after, "after"),
            SettingItem::GithubEnabled => {
                if self.github_enabled { "on" } else { "off" }.to_string()
            }
            SettingItem::GithubPollInterval => duration_label(self.github_poll_interval, "every"),
        }
    }

    pub fn page_hold_frames(&self) -> u64 {
        self.cycle_interval
            .as_secs()
            .saturating_mul(u64::from(self.frames_per_second))
            .max(1)
    }
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            reduced_motion: false,
            auto_cycle: true,
            cycle_interval: DEFAULT_CYCLE_INTERVAL,
            setting_help: true,
            poll_interval: DEFAULT_POLL_INTERVAL,
            idle_after: DEFAULT_IDLE_AFTER,
            github_enabled: false,
            github_poll_interval: DEFAULT_GITHUB_POLL_INTERVAL,
            frames_per_second: DEFAULT_FRAMES_PER_SECOND,
        }
    }
}

fn duration_label(duration: Duration, prefix: &str) -> String {
    let millis = duration.as_millis();
    if millis < 1_000 {
        return format!("{prefix} {} ms", millis);
    }
    let seconds = duration.as_secs();
    if seconds < 60 {
        format!("{prefix} {seconds} s")
    } else {
        format!("{prefix} {} min", seconds / 60)
    }
}

fn transition_duration_frames(frames_per_second: u8) -> u64 {
    let frame_millis =
        u64::from(frames_per_second).saturating_mul(VISUAL_TRANSITION_DURATION.as_millis() as u64);
    frame_millis.div_ceil(1_000).max(1)
}

/// Everything that can happen to the application.
#[derive(Debug)]
pub enum Msg {
    /// One animation frame elapsed.
    Tick,
    Key(KeyEvent),
    /// A fresh repository survey arrived from the collector thread.
    Snapshot {
        result: Result<RepoSnapshot, String>,
        observed_at: Instant,
    },
    /// A fresh optional remote-hosting survey arrived.
    Hosting(Result<HostingSnapshot, String>),
}

/// Application state: the current scene plus mode, selection, and clock.
/// `update` is a pure state transition over [`Msg`], so behavior is testable
/// without a terminal.
pub struct App {
    pub harbor: Harbor,
    pub mode: Mode,
    pub selected: usize,
    pub inspect_target: InspectTarget,
    pub settings: AppSettings,
    pub animation: Animation,
    pub should_quit: bool,
    /// Most recent collector failure, shown until a survey succeeds again.
    pub error: Option<String>,
    /// False until the first snapshot arrives.
    pub loaded: bool,
    /// Whether the legend overlay is currently shown.
    pub show_legend: bool,
    pub legend_scroll: usize,
    /// Whether the session-local harbor controls are shown.
    pub show_settings: bool,
    pub settings_selected: usize,
    /// Most recent optional hosting failure, independent from local Git.
    pub hosting_error: Option<String>,
    activity: ActivityTracker,
    repository_events: RepositoryEventTracker,
    visual_transitions: VisualTransitionTracker,
    hosting: Option<HostingSnapshot>,
    cycle_origin_frame: u64,
}

impl App {
    pub fn new(name: String, reduced_motion: bool) -> Self {
        let settings = AppSettings {
            reduced_motion,
            ..AppSettings::default()
        };
        Self::with_settings(name, settings)
    }

    pub fn with_settings(name: String, settings: AppSettings) -> Self {
        let idle_after = settings.idle_after;
        Self {
            harbor: Harbor {
                name,
                docks: Vec::new(),
                convoys: Vec::new(),
            },
            mode: Mode::Ambient,
            selected: 0,
            inspect_target: InspectTarget::Dock,
            settings,
            animation: Animation::default(),
            should_quit: false,
            error: None,
            loaded: false,
            show_legend: false,
            legend_scroll: 0,
            show_settings: false,
            settings_selected: 0,
            hosting_error: None,
            activity: ActivityTracker::new(idle_after),
            repository_events: RepositoryEventTracker::default(),
            visual_transitions: VisualTransitionTracker::default(),
            hosting: None,
            cycle_origin_frame: 0,
        }
    }

    pub fn with_idle_after(mut self, idle_after: Duration) -> Self {
        self.settings.idle_after = idle_after;
        self.activity.idle_after = idle_after;
        self
    }

    pub fn with_poll_interval(mut self, poll_interval: Duration) -> Self {
        self.settings.poll_interval = poll_interval;
        self
    }

    pub fn with_github(mut self, enabled: bool, poll_interval: Duration) -> Self {
        self.settings.github_enabled = enabled;
        self.settings.github_poll_interval = poll_interval;
        self
    }

    pub fn with_frames_per_second(mut self, frames_per_second: u8) -> Self {
        self.settings.frames_per_second = frames_per_second.clamp(1, 30);
        self
    }

    pub fn page_cycle_frame(&self) -> u64 {
        self.animation.frame().wrapping_sub(self.cycle_origin_frame)
    }

    pub fn update(&mut self, msg: Msg) {
        match msg {
            Msg::Tick => {
                if !self.settings.reduced_motion {
                    self.animation.tick();
                    self.visual_transitions
                        .expire(&mut self.harbor, self.animation.frame());
                }
            }
            Msg::Snapshot {
                result: Ok(snapshot),
                observed_at,
            } => {
                let activities = self.activity.observe(&snapshot, observed_at);
                let events = self.repository_events.observe(&snapshot, observed_at);
                let mut harbor = harbor::to_harbor_with_activity(&snapshot, |workspace| {
                    activities
                        .get(&workspace.path)
                        .copied()
                        .unwrap_or(VesselActivity::Observing)
                });
                self.visual_transitions.observe(
                    &mut harbor,
                    self.animation.frame(),
                    transition_duration_frames(self.settings.frames_per_second),
                    self.settings.reduced_motion,
                );
                for active in events {
                    if let Some(dock) = harbor
                        .docks
                        .iter_mut()
                        .find(|dock| dock.name == active.branch)
                    {
                        dock.detail.push((
                            "recent event",
                            format!("{} — {}", active.event.kind.label(), active.event.summary),
                        ));
                        dock.events.push(active.event);
                    }
                }
                if self.settings.github_enabled
                    && let Some(hosting) = &self.hosting
                {
                    apply_hosting(&mut harbor, hosting);
                }
                self.harbor = harbor;
                self.loaded = true;
                self.error = None;
                self.clamp_selection();
            }
            Msg::Snapshot {
                result: Err(message),
                ..
            } => {
                self.error = Some(message);
            }
            Msg::Hosting(Ok(snapshot)) => {
                if !self.settings.github_enabled {
                    return;
                }
                apply_hosting(&mut self.harbor, &snapshot);
                self.hosting = Some(snapshot);
                self.hosting_error = None;
                self.clamp_selection();
            }
            Msg::Hosting(Err(message)) => {
                if self.settings.github_enabled {
                    self.hosting_error = Some(message);
                }
            }
            Msg::Key(key) => self.handle_key(key),
        }
    }

    fn handle_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Esc if self.show_settings => self.show_settings = false,
            KeyCode::Char('s') if self.show_settings => self.show_settings = false,
            KeyCode::Down | KeyCode::Char('j') if self.show_settings => {
                self.settings_selected = (self.settings_selected + 1) % SettingItem::ALL.len();
            }
            KeyCode::Up | KeyCode::Char('k') if self.show_settings => {
                self.settings_selected =
                    (self.settings_selected + SettingItem::ALL.len() - 1) % SettingItem::ALL.len();
            }
            KeyCode::Left | KeyCode::Char('h') if self.show_settings => {
                self.adjust_setting(false);
            }
            KeyCode::Right | KeyCode::Char('l') | KeyCode::Enter | KeyCode::Char(' ')
                if self.show_settings =>
            {
                self.adjust_setting(true);
            }
            _ if self.show_settings => {}
            KeyCode::Char('s') => {
                self.show_settings = true;
                self.show_legend = false;
            }
            KeyCode::Char('l') | KeyCode::Char('?') => {
                self.show_legend = !self.show_legend;
                self.show_settings = false;
            }
            // Escape peels back one layer at a time: legend, then inspect,
            // then quit.
            KeyCode::Esc if self.show_legend => self.show_legend = false,
            KeyCode::Down | KeyCode::Char('j') if self.show_legend => {
                self.legend_scroll = self.legend_scroll.saturating_add(1);
            }
            KeyCode::Up | KeyCode::Char('k') if self.show_legend => {
                self.legend_scroll = self.legend_scroll.saturating_sub(1);
            }
            _ if self.show_legend => {}
            KeyCode::Esc | KeyCode::Left | KeyCode::Char('h') => self.step_out(),
            KeyCode::Char('m') => {
                self.toggle_motion();
            }
            KeyCode::Char('i') => self.enter_inspect(),
            KeyCode::Char('p') => self.inspect_pull_request(),
            KeyCode::Enter | KeyCode::Right => self.step_in(),
            KeyCode::Tab => self.select_next_dock(),
            KeyCode::BackTab => self.select_previous_dock(),
            KeyCode::Down | KeyCode::Char('j') => self.select_next(),
            KeyCode::Up | KeyCode::Char('k') => self.select_previous(),
            _ => {}
        }
    }

    fn adjust_setting(&mut self, forward: bool) {
        let item = SettingItem::ALL[self.settings_selected];
        match item {
            SettingItem::Motion => {
                self.toggle_motion();
            }
            SettingItem::AutoCycle => {
                self.settings.auto_cycle = !self.settings.auto_cycle;
                if self.settings.auto_cycle {
                    self.reset_page_cycle();
                }
            }
            SettingItem::CycleInterval => {
                self.settings.cycle_interval =
                    step_duration(self.settings.cycle_interval, &CYCLE_INTERVALS, forward);
                self.reset_page_cycle();
            }
            SettingItem::SettingHelp => self.settings.setting_help = !self.settings.setting_help,
            SettingItem::PollInterval => {
                self.settings.poll_interval =
                    step_duration(self.settings.poll_interval, &POLL_INTERVALS, forward);
            }
            SettingItem::IdleAfter => {
                self.settings.idle_after =
                    step_duration(self.settings.idle_after, &IDLE_INTERVALS, forward);
                self.activity.idle_after = self.settings.idle_after;
            }
            SettingItem::GithubEnabled => {
                self.settings.github_enabled = !self.settings.github_enabled;
                if !self.settings.github_enabled {
                    clear_hosting(&mut self.harbor);
                    self.hosting = None;
                    self.hosting_error = None;
                    self.clamp_selection();
                }
            }
            SettingItem::GithubPollInterval => {
                self.settings.github_poll_interval = step_duration(
                    self.settings.github_poll_interval,
                    &GITHUB_POLL_INTERVALS,
                    forward,
                );
            }
        }
    }

    fn toggle_motion(&mut self) {
        self.settings.reduced_motion = !self.settings.reduced_motion;
        if self.settings.reduced_motion {
            self.visual_transitions.clear(&mut self.harbor);
        } else {
            self.reset_page_cycle();
        }
    }

    fn reset_page_cycle(&mut self) {
        self.cycle_origin_frame = self.animation.frame();
    }

    fn enter_inspect(&mut self) {
        if !self.harbor.docks.is_empty() {
            self.mode = Mode::Inspect;
            self.inspect_target = InspectTarget::Dock;
        }
    }

    fn step_in(&mut self) {
        if self.mode == Mode::Ambient {
            self.enter_inspect();
            return;
        }
        let Some(dock) = self.harbor.docks.get(self.selected) else {
            return;
        };
        self.inspect_target = match self.inspect_target {
            InspectTarget::Dock if dock.vessel.is_some() => InspectTarget::Vessel,
            InspectTarget::Vessel if dock.vessel.as_ref().is_some_and(|v| !v.cargo.is_empty()) => {
                InspectTarget::Change(0)
            }
            InspectTarget::PullRequest(pull_request)
                if dock
                    .clearances
                    .get(pull_request)
                    .is_some_and(|clearance| !clearance.inspections.is_empty()) =>
            {
                InspectTarget::Check {
                    pull_request,
                    check: 0,
                }
            }
            target => target,
        };
    }

    fn inspect_pull_request(&mut self) {
        if self.mode == Mode::Ambient {
            self.enter_inspect();
        }
        if self
            .harbor
            .docks
            .get(self.selected)
            .is_some_and(|dock| !dock.clearances.is_empty())
        {
            self.inspect_target = InspectTarget::PullRequest(0);
        }
    }

    fn step_out(&mut self) {
        if self.mode == Mode::Ambient {
            self.should_quit = true;
            return;
        }
        self.inspect_target = match self.inspect_target {
            InspectTarget::Change(_) => InspectTarget::Vessel,
            InspectTarget::Check { pull_request, .. } => InspectTarget::PullRequest(pull_request),
            InspectTarget::PullRequest(_) => InspectTarget::Dock,
            InspectTarget::Vessel => InspectTarget::Dock,
            InspectTarget::Dock => {
                self.mode = Mode::Ambient;
                InspectTarget::Dock
            }
        };
    }

    fn select_next(&mut self) {
        if self.mode == Mode::Ambient {
            // The first navigation key only opens inspect mode on the
            // current dock; movement starts with the next press.
            self.enter_inspect();
        } else if let InspectTarget::Change(selected) = self.inspect_target {
            if let Some(count) = self.selected_cargo_count().filter(|count| *count > 0) {
                self.inspect_target = InspectTarget::Change((selected + 1) % count);
            }
        } else {
            match self.inspect_target {
                InspectTarget::PullRequest(selected) => {
                    if let Some(count) = self.selected_clearance_count().filter(|count| *count > 0)
                    {
                        self.inspect_target = InspectTarget::PullRequest((selected + 1) % count);
                    }
                }
                InspectTarget::Check {
                    pull_request,
                    check,
                } => {
                    if let Some(count) = self
                        .selected_inspection_count(pull_request)
                        .filter(|count| *count > 0)
                    {
                        self.inspect_target = InspectTarget::Check {
                            pull_request,
                            check: (check + 1) % count,
                        };
                    }
                }
                _ => self.select_next_dock(),
            }
        }
    }

    fn select_previous(&mut self) {
        if self.mode == Mode::Ambient {
            self.enter_inspect();
        } else if let InspectTarget::Change(selected) = self.inspect_target {
            if let Some(count) = self.selected_cargo_count().filter(|count| *count > 0) {
                self.inspect_target = InspectTarget::Change((selected + count - 1) % count);
            }
        } else {
            match self.inspect_target {
                InspectTarget::PullRequest(selected) => {
                    if let Some(count) = self.selected_clearance_count().filter(|count| *count > 0)
                    {
                        self.inspect_target =
                            InspectTarget::PullRequest((selected + count - 1) % count);
                    }
                }
                InspectTarget::Check {
                    pull_request,
                    check,
                } => {
                    if let Some(count) = self
                        .selected_inspection_count(pull_request)
                        .filter(|count| *count > 0)
                    {
                        self.inspect_target = InspectTarget::Check {
                            pull_request,
                            check: (check + count - 1) % count,
                        };
                    }
                }
                _ => self.select_previous_dock(),
            }
        }
    }

    fn select_next_dock(&mut self) {
        if self.mode == Mode::Ambient {
            self.enter_inspect();
        } else if !self.harbor.docks.is_empty() {
            self.selected = (self.selected + 1) % self.harbor.docks.len();
            self.clamp_inspect_target();
        }
    }

    fn select_previous_dock(&mut self) {
        if self.mode == Mode::Ambient {
            self.enter_inspect();
        } else if !self.harbor.docks.is_empty() {
            let count = self.harbor.docks.len();
            self.selected = (self.selected + count - 1) % count;
            self.clamp_inspect_target();
        }
    }

    fn selected_cargo_count(&self) -> Option<usize> {
        self.harbor
            .docks
            .get(self.selected)?
            .vessel
            .as_ref()
            .map(|vessel| vessel.cargo.len())
    }

    fn selected_clearance_count(&self) -> Option<usize> {
        self.harbor
            .docks
            .get(self.selected)
            .map(|dock| dock.clearances.len())
    }

    fn selected_inspection_count(&self, pull_request: usize) -> Option<usize> {
        self.harbor
            .docks
            .get(self.selected)?
            .clearances
            .get(pull_request)
            .map(|clearance| clearance.inspections.len())
    }

    fn clamp_inspect_target(&mut self) {
        let Some(dock) = self.harbor.docks.get(self.selected) else {
            self.inspect_target = InspectTarget::Dock;
            return;
        };
        let Some(vessel) = dock.vessel.as_ref() else {
            self.inspect_target = InspectTarget::Dock;
            return;
        };
        if let InspectTarget::Change(selected) = self.inspect_target {
            self.inspect_target = if vessel.cargo.is_empty() {
                InspectTarget::Vessel
            } else {
                InspectTarget::Change(selected.min(vessel.cargo.len() - 1))
            };
        }
        match self.inspect_target {
            InspectTarget::PullRequest(selected) => {
                self.inspect_target = if dock.clearances.is_empty() {
                    InspectTarget::Dock
                } else {
                    InspectTarget::PullRequest(selected.min(dock.clearances.len() - 1))
                };
            }
            InspectTarget::Check {
                pull_request,
                check,
            } => {
                let Some(clearance) = dock.clearances.get(pull_request) else {
                    self.inspect_target = InspectTarget::Dock;
                    return;
                };
                self.inspect_target = if clearance.inspections.is_empty() {
                    InspectTarget::PullRequest(pull_request)
                } else {
                    InspectTarget::Check {
                        pull_request,
                        check: check.min(clearance.inspections.len() - 1),
                    }
                };
            }
            _ => {}
        }
    }

    fn clamp_selection(&mut self) {
        if self.harbor.docks.is_empty() {
            self.selected = 0;
            self.mode = Mode::Ambient;
            self.inspect_target = InspectTarget::Dock;
        } else if self.selected >= self.harbor.docks.len() {
            self.selected = self.harbor.docks.len() - 1;
        }
        self.clamp_inspect_target();
    }
}

fn step_duration(current: Duration, choices: &[Duration], forward: bool) -> Duration {
    let closest = choices
        .iter()
        .enumerate()
        .min_by_key(|(_, choice)| current.abs_diff(**choice))
        .map_or(0, |(index, _)| index);
    let next = if forward {
        (closest + 1) % choices.len()
    } else {
        (closest + choices.len() - 1) % choices.len()
    };
    choices[next]
}

fn clear_hosting(harbor: &mut Harbor) {
    harbor
        .docks
        .retain(|dock| dock.kind != DockKind::RemoteBranch);
    for dock in &mut harbor.docks {
        dock.clearances.clear();
        dock.detail.retain(|(label, _)| *label != "pull requests");
    }
    harbor.convoys.clear();
}

fn apply_hosting(harbor: &mut Harbor, snapshot: &HostingSnapshot) {
    clear_hosting(harbor);

    for pull_request in &snapshot.pull_requests {
        let clearance = Clearance {
            number: pull_request.number,
            title: pull_request.title.clone(),
            url: pull_request.url.clone(),
            is_draft: pull_request.is_draft,
            review: match pull_request.review {
                ReviewState::Approved => ReviewStatus::Approved,
                ReviewState::ChangesRequested => ReviewStatus::ChangesRequested,
                ReviewState::Required => ReviewStatus::Required,
                ReviewState::None => ReviewStatus::None,
            },
            landing: match pull_request.merge {
                MergeState::Ready => LandingStatus::Ready,
                MergeState::Blocked => LandingStatus::Blocked,
                MergeState::Unknown => LandingStatus::Unknown,
            },
            inspections: pull_request
                .checks
                .iter()
                .map(|check| Inspection {
                    name: check.name.clone(),
                    status: match check.state {
                        CheckState::Passing => InspectionStatus::Passing,
                        CheckState::Failing => InspectionStatus::Failing,
                        CheckState::Pending => InspectionStatus::Pending,
                        CheckState::Unknown => InspectionStatus::Unknown,
                    },
                    url: check.url.clone(),
                })
                .collect(),
        };
        if let Some(dock) = harbor
            .docks
            .iter_mut()
            .find(|dock| dock.name == pull_request.head_branch)
        {
            dock.clearances.push(clearance);
        } else {
            harbor.docks.push(Dock {
                name: pull_request.head_branch.clone(),
                kind: DockKind::RemoteBranch,
                condition: Condition::Awaiting,
                vessel: None,
                sync: None,
                detail: vec![
                    ("branch", pull_request.head_branch.clone()),
                    ("workspace", "remote only".to_string()),
                ],
                events: Vec::new(),
                transition: None,
                clearances: vec![clearance],
            });
        }
    }

    for dock in &mut harbor.docks {
        dock.clearances.sort_by_key(|clearance| clearance.number);
        if !dock.clearances.is_empty() {
            dock.detail
                .push(("pull requests", dock.clearances.len().to_string()));
        }
    }
    harbor.docks.sort_by(|a, b| {
        let rank = |dock: &Dock| match dock.kind {
            DockKind::MainTerminal => 0,
            DockKind::RemoteBranch => 3,
            _ if dock.vessel.is_some() => 1,
            _ => 2,
        };
        rank(a).cmp(&rank(b)).then_with(|| a.name.cmp(&b.name))
    });
    harbor.convoys = snapshot
        .releases
        .iter()
        .map(|release| Convoy {
            tag: release.tag.clone(),
            name: release.name.clone(),
            is_latest: release.is_latest,
            is_prerelease: release.is_prerelease,
            published_at: release.published_at.clone(),
        })
        .collect();
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum DockIdentity {
    Branch(String),
    Workspace(PathBuf),
}

#[derive(Debug, Clone)]
struct DockVisualState {
    condition: Condition,
    vessel: Option<Vessel>,
}

impl From<&Dock> for DockVisualState {
    fn from(dock: &Dock) -> Self {
        Self {
            condition: dock.condition,
            // Visual diffs need counts and a departure hull, not a second copy
            // of every changed path retained by the inspect model.
            vessel: dock.vessel.as_ref().map(|vessel| Vessel {
                cargo: Vec::new(),
                ..vessel.clone()
            }),
        }
    }
}

#[derive(Debug, Default)]
struct VisualTransitionTracker {
    initialized: bool,
    previous: HashMap<DockIdentity, DockVisualState>,
    active: HashMap<DockIdentity, DockTransition>,
}

impl VisualTransitionTracker {
    fn observe(
        &mut self,
        harbor: &mut Harbor,
        frame: u64,
        duration_frames: u64,
        reduced_motion: bool,
    ) {
        let current: HashMap<_, _> = harbor
            .docks
            .iter()
            .filter_map(|dock| dock_identity(dock).map(|key| (key, DockVisualState::from(dock))))
            .collect();

        self.active.retain(|key, transition| {
            current.contains_key(key) && transition_is_active(transition, frame)
        });

        if reduced_motion {
            self.active.clear();
        } else if self.initialized {
            for dock in &harbor.docks {
                let Some(key) = dock_identity(dock) else {
                    continue;
                };
                let Some(next) = current.get(&key) else {
                    continue;
                };
                let previous = self.previous.get(&key).or_else(|| {
                    next.vessel.as_ref().and_then(|vessel| {
                        self.previous
                            .get(&DockIdentity::Workspace(vessel.workspace.clone()))
                    })
                });
                let kind = match previous {
                    Some(previous) => transition_kind(previous, next),
                    None => next
                        .vessel
                        .as_ref()
                        .map(|_| DockTransitionKind::VesselArriving),
                };
                if let Some(kind) = kind {
                    self.active.insert(
                        key,
                        DockTransition {
                            kind,
                            started_frame: frame,
                            duration_frames,
                        },
                    );
                }
            }
        }

        for dock in &mut harbor.docks {
            dock.transition = dock_identity(dock)
                .and_then(|key| self.active.get(&key))
                .cloned();
        }
        self.previous = current;
        self.initialized = true;
    }

    fn expire(&mut self, harbor: &mut Harbor, frame: u64) {
        self.active
            .retain(|_, transition| transition_is_active(transition, frame));
        for dock in &mut harbor.docks {
            if dock
                .transition
                .as_ref()
                .is_some_and(|transition| !transition_is_active(transition, frame))
            {
                dock.transition = None;
            }
        }
    }

    fn clear(&mut self, harbor: &mut Harbor) {
        self.active.clear();
        for dock in &mut harbor.docks {
            dock.transition = None;
        }
    }
}

fn dock_identity(dock: &Dock) -> Option<DockIdentity> {
    match dock.kind {
        DockKind::RemoteBranch => None,
        DockKind::DetachedWorktree => dock
            .vessel
            .as_ref()
            .map(|vessel| DockIdentity::Workspace(vessel.workspace.clone())),
        DockKind::Branch if dock.name == "(no commits yet)" => dock
            .vessel
            .as_ref()
            .map(|vessel| DockIdentity::Workspace(vessel.workspace.clone())),
        DockKind::MainTerminal | DockKind::Branch => Some(DockIdentity::Branch(dock.name.clone())),
    }
}

fn transition_kind(
    previous: &DockVisualState,
    next: &DockVisualState,
) -> Option<DockTransitionKind> {
    let was_blocked = previous.condition == Condition::Blocked;
    let is_blocked = next.condition == Condition::Blocked;
    if was_blocked != is_blocked {
        return Some(if is_blocked {
            DockTransitionKind::BecameBlocked
        } else {
            DockTransitionKind::BecameUnblocked
        });
    }

    match (&previous.vessel, &next.vessel) {
        (None, Some(_)) => Some(DockTransitionKind::VesselArriving),
        (Some(vessel), None) => Some(DockTransitionKind::VesselDeparting {
            vessel: vessel.clone(),
        }),
        (Some(previous), Some(next)) => {
            let from = previous.cargo_counts();
            (from != next.cargo_counts()).then_some(DockTransitionKind::Cargo { from })
        }
        (None, None) => None,
    }
}

fn transition_is_active(transition: &DockTransition, frame: u64) -> bool {
    frame.wrapping_sub(transition.started_frame) < transition.duration_frames
}

#[derive(Debug, Default)]
struct RepositoryEventTracker {
    branches: HashMap<String, BranchObservation>,
    active: Vec<ActiveDockEvent>,
}

#[derive(Debug)]
struct BranchObservation {
    tip_id: Option<String>,
    upstream: Option<UpstreamObservation>,
}

#[derive(Debug)]
struct UpstreamObservation {
    name: String,
    tip_id: String,
    ahead: usize,
}

#[derive(Debug, Clone)]
struct ActiveDockEvent {
    branch: String,
    event: DockEvent,
    expires_at: Instant,
}

impl RepositoryEventTracker {
    fn observe(&mut self, snapshot: &RepoSnapshot, observed_at: Instant) -> Vec<ActiveDockEvent> {
        self.active.retain(|event| event.expires_at > observed_at);
        self.branches
            .retain(|name, _| snapshot.branches.iter().any(|branch| &branch.name == name));

        for branch in &snapshot.branches {
            let current_tip = branch.tip.as_ref().map(|tip| tip.id.clone());
            let current_upstream = branch.sync.as_ref().map(|sync| UpstreamObservation {
                name: sync.upstream.clone(),
                tip_id: sync.tip_id.clone(),
                ahead: sync.ahead,
            });
            if let Some(previous) = self.branches.get(&branch.name) {
                if previous.tip_id != current_tip
                    && let Some(tip) = &branch.tip
                {
                    let kind = if tip.parent_count > 1 && tip.action != TipAction::Other {
                        EventKind::Merge
                    } else if tip.action == TipAction::Commit {
                        EventKind::Commit
                    } else {
                        EventKind::Update
                    };
                    self.active.push(ActiveDockEvent {
                        branch: branch.name.clone(),
                        event: DockEvent {
                            kind,
                            summary: tip.summary.clone(),
                        },
                        expires_at: observed_at + EVENT_LIFETIME,
                    });
                }

                if previous.tip_id == current_tip
                    && let (Some(before), Some(after), Some(sync)) = (
                        previous.upstream.as_ref(),
                        current_upstream.as_ref(),
                        branch.sync.as_ref(),
                    )
                    && before.name == after.name
                    && before.tip_id != after.tip_id
                {
                    let (kind, summary) =
                        if sync.action == UpstreamAction::Push && before.ahead > after.ahead {
                            let count = before.ahead - after.ahead;
                            let noun = if count == 1 { "commit" } else { "commits" };
                            (EventKind::Push, format!("{count} {noun} sent upstream"))
                        } else {
                            (EventKind::Update, format!("{} reference moved", after.name))
                        };
                    self.active.push(ActiveDockEvent {
                        branch: branch.name.clone(),
                        event: DockEvent { kind, summary },
                        expires_at: observed_at + EVENT_LIFETIME,
                    });
                }
            }

            self.branches.insert(
                branch.name.clone(),
                BranchObservation {
                    tip_id: current_tip,
                    upstream: current_upstream,
                },
            );
        }
        self.active.clone()
    }
}

#[derive(Debug)]
struct ActivityTracker {
    idle_after: Duration,
    workspaces: HashMap<PathBuf, WorkspaceObservation>,
}

#[derive(Debug)]
struct WorkspaceObservation {
    token: u64,
    last_change: Instant,
    has_changed: bool,
}

impl ActivityTracker {
    fn new(idle_after: Duration) -> Self {
        Self {
            idle_after,
            workspaces: HashMap::new(),
        }
    }

    fn observe(
        &mut self,
        snapshot: &RepoSnapshot,
        observed_at: Instant,
    ) -> HashMap<PathBuf, VesselActivity> {
        self.workspaces.retain(|path, _| {
            snapshot
                .workspaces
                .iter()
                .any(|workspace| &workspace.path == path)
        });

        snapshot
            .workspaces
            .iter()
            .map(|workspace| {
                let observation =
                    self.workspaces
                        .entry(workspace.path.clone())
                        .or_insert(WorkspaceObservation {
                            token: workspace.activity_token,
                            last_change: observed_at,
                            has_changed: false,
                        });
                if observation.token != workspace.activity_token {
                    observation.token = workspace.activity_token;
                    observation.last_change = observed_at;
                    observation.has_changed = true;
                }
                let age = observed_at.saturating_duration_since(observation.last_change);
                let activity = if observation.has_changed && age < self.idle_after {
                    VesselActivity::Recent
                } else if age >= self.idle_after {
                    VesselActivity::Idle
                } else {
                    VesselActivity::Observing
                };
                (workspace.path.clone(), activity)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyEvent, KeyModifiers};

    use std::path::PathBuf;

    use crate::git::{
        BranchInfo, ChangeCounts, ChangeFile, ChangeKind, HeadState, Operation, RepoSnapshot,
        SyncState, TipInfo, UpstreamAction, Workspace,
    };
    use crate::hosting::{Check, PullRequest, Release};

    use super::*;

    fn key(code: KeyCode) -> Msg {
        Msg::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn snapshot_msg(result: Result<RepoSnapshot, String>) -> Msg {
        Msg::Snapshot {
            result,
            observed_at: Instant::now(),
        }
    }

    fn snapshot_with_branches(names: &[&str]) -> RepoSnapshot {
        RepoSnapshot {
            name: "test".to_string(),
            default_branch: names.first().map(|n| n.to_string()),
            branches: names
                .iter()
                .map(|n| BranchInfo {
                    name: n.to_string(),
                    sync: None,
                    last_commit: None,
                    tip: None,
                })
                .collect(),
            workspaces: Vec::new(),
        }
    }

    fn activity_snapshot(token: u64) -> RepoSnapshot {
        RepoSnapshot {
            name: "test".to_string(),
            default_branch: None,
            branches: vec![BranchInfo {
                name: "topic".to_string(),
                sync: None,
                last_commit: None,
                tip: None,
            }],
            workspaces: vec![Workspace {
                path: PathBuf::from("/tmp/activity-test"),
                is_main: true,
                head: HeadState::Branch("topic".to_string()),
                changes: ChangeCounts {
                    unstaged: 1,
                    ..ChangeCounts::default()
                },
                change_files: vec![ChangeFile {
                    path: PathBuf::from("src/main.rs"),
                    kind: ChangeKind::Unstaged,
                }],
                activity_token: token,
                operation: None,
            }],
        }
    }

    fn workspace_snapshot(
        changes: ChangeCounts,
        operation: Option<Operation>,
        occupied: bool,
        token: u64,
    ) -> RepoSnapshot {
        RepoSnapshot {
            name: "test".to_string(),
            default_branch: None,
            branches: vec![BranchInfo {
                name: "topic".to_string(),
                sync: None,
                last_commit: None,
                tip: None,
            }],
            workspaces: occupied
                .then(|| Workspace {
                    path: PathBuf::from("/tmp/transition-test"),
                    is_main: true,
                    head: HeadState::Branch("topic".to_string()),
                    changes,
                    change_files: Vec::new(),
                    activity_token: token,
                    operation,
                })
                .into_iter()
                .collect(),
        }
    }

    fn visual_transition(app: &App) -> Option<&DockTransition> {
        app.harbor.docks[0].transition.as_ref()
    }

    fn activity(app: &App) -> VesselActivity {
        app.harbor.docks[0].vessel.as_ref().unwrap().activity
    }

    fn transition_snapshot(
        id: &str,
        action: TipAction,
        parent_count: usize,
        ahead: usize,
    ) -> RepoSnapshot {
        transition_snapshot_with_upstream(
            id,
            action,
            parent_count,
            ahead,
            &format!("upstream-{ahead}"),
            UpstreamAction::Other,
        )
    }

    fn transition_snapshot_with_upstream(
        id: &str,
        action: TipAction,
        parent_count: usize,
        ahead: usize,
        upstream_tip: &str,
        upstream_action: UpstreamAction,
    ) -> RepoSnapshot {
        RepoSnapshot {
            name: "test".to_string(),
            default_branch: Some("main".to_string()),
            branches: vec![BranchInfo {
                name: "main".to_string(),
                sync: Some(SyncState {
                    upstream: "origin/main".to_string(),
                    ahead,
                    behind: 0,
                    tip_id: upstream_tip.to_string(),
                    action: upstream_action,
                }),
                last_commit: Some(id.to_string()),
                tip: Some(TipInfo {
                    id: id.to_string(),
                    summary: format!("summary {id}"),
                    parent_count,
                    action,
                }),
            }],
            workspaces: Vec::new(),
        }
    }

    fn observe(app: &mut App, snapshot: RepoSnapshot, at: Instant) {
        app.update(Msg::Snapshot {
            result: Ok(snapshot),
            observed_at: at,
        });
    }

    fn hosting_snapshot() -> HostingSnapshot {
        HostingSnapshot {
            pull_requests: vec![
                PullRequest {
                    number: 7,
                    title: "Local clearance".to_string(),
                    head_branch: "topic".to_string(),
                    url: "https://example/pr/7".to_string(),
                    is_draft: false,
                    review: ReviewState::Approved,
                    merge: MergeState::Ready,
                    checks: vec![Check {
                        name: "test".to_string(),
                        state: CheckState::Passing,
                        url: Some("https://example/check".to_string()),
                    }],
                },
                PullRequest {
                    number: 8,
                    title: "Remote clearance".to_string(),
                    head_branch: "remote-topic".to_string(),
                    url: "https://example/pr/8".to_string(),
                    is_draft: true,
                    review: ReviewState::Required,
                    merge: MergeState::Blocked,
                    checks: Vec::new(),
                },
            ],
            releases: vec![Release {
                tag: "v1.0.0".to_string(),
                name: "One".to_string(),
                is_latest: true,
                is_prerelease: false,
                published_at: Some("2026-07-13T00:00:00Z".to_string()),
            }],
        }
    }

    #[test]
    fn tick_is_ignored_under_reduced_motion() {
        let mut app = App::new("test".to_string(), true);
        app.update(Msg::Tick);
        assert_eq!(app.animation.frame(), 0);
        app.update(key(KeyCode::Char('m')));
        app.update(Msg::Tick);
        assert_eq!(app.animation.frame(), 1);
    }

    #[test]
    fn selection_wraps_and_survives_shrinking_snapshots() {
        let mut app = App::new("test".to_string(), false);
        app.update(snapshot_msg(Ok(snapshot_with_branches(&["a", "b", "c"]))));
        app.update(key(KeyCode::Tab)); // enters inspect on dock 0
        app.update(key(KeyCode::Tab));
        app.update(key(KeyCode::Tab));
        assert_eq!(app.mode, Mode::Inspect);
        assert_eq!(app.selected, 2);
        // A branch disappears; the selection must stay in bounds.
        app.update(snapshot_msg(Ok(snapshot_with_branches(&["a"]))));
        assert_eq!(app.selected, 0);
    }

    #[test]
    fn escape_leaves_inspect_before_quitting() {
        let mut app = App::new("test".to_string(), false);
        app.update(snapshot_msg(Ok(snapshot_with_branches(&["a"]))));
        app.update(key(KeyCode::Char('i')));
        app.update(key(KeyCode::Esc));
        assert_eq!(app.mode, Mode::Ambient);
        assert!(!app.should_quit);
        app.update(key(KeyCode::Esc));
        assert!(app.should_quit);
    }

    #[test]
    fn legend_toggles_and_escape_closes_it_first() {
        let mut app = App::new("test".to_string(), false);
        app.update(snapshot_msg(Ok(snapshot_with_branches(&["a"]))));
        app.update(key(KeyCode::Char('i')));

        app.update(key(KeyCode::Char('l')));
        assert!(app.show_legend);
        app.update(key(KeyCode::Char('j')));
        assert_eq!(app.legend_scroll, 1);
        app.update(key(KeyCode::Char('k')));
        assert_eq!(app.legend_scroll, 0);
        // Escape dismisses the legend without leaving inspect mode.
        app.update(key(KeyCode::Esc));
        assert!(!app.show_legend);
        assert_eq!(app.mode, Mode::Inspect);
        assert!(!app.should_quit);

        // '?' is an alias for the same toggle.
        app.update(key(KeyCode::Char('?')));
        assert!(app.show_legend);
        app.update(key(KeyCode::Char('l')));
        assert!(!app.show_legend);
    }

    #[test]
    fn settings_overlay_owns_navigation_and_closes_without_changing_mode() {
        let mut app = App::new("test".to_string(), false);
        app.update(snapshot_msg(Ok(snapshot_with_branches(&["a", "b"]))));

        app.update(key(KeyCode::Char('s')));
        assert!(app.show_settings);
        assert!(!app.show_legend);
        app.update(key(KeyCode::Char('j')));
        assert_eq!(app.settings_selected, 1);
        app.update(key(KeyCode::Right));
        assert!(!app.settings.auto_cycle);
        assert_eq!(app.selected, 0);
        assert_eq!(app.mode, Mode::Ambient);

        for _ in 0..300 {
            app.update(Msg::Tick);
        }
        app.update(key(KeyCode::Right));
        assert!(app.settings.auto_cycle);
        assert_eq!(app.page_cycle_frame(), 0);

        // Legend and dock navigation do not leak through the controls layer.
        app.update(key(KeyCode::Char('?')));
        app.update(key(KeyCode::Tab));
        assert!(!app.show_legend);
        assert_eq!(app.mode, Mode::Ambient);

        app.update(key(KeyCode::Esc));
        assert!(!app.show_settings);
        assert!(!app.should_quit);
    }

    #[test]
    fn idle_threshold_setting_updates_activity_tracker_immediately() {
        let start = Instant::now();
        let mut app = App::new("test".to_string(), false);
        observe(&mut app, activity_snapshot(1), start);

        app.update(key(KeyCode::Char('s')));
        for _ in 0..5 {
            app.update(key(KeyCode::Char('j')));
        }
        app.update(key(KeyCode::Left));
        assert_eq!(app.settings.idle_after, Duration::from_secs(10));

        observe(
            &mut app,
            activity_snapshot(1),
            start + Duration::from_secs(15),
        );
        assert_eq!(activity(&app), VesselActivity::Idle);
    }

    #[test]
    fn setting_help_is_on_by_default_and_can_be_toggled() {
        let mut app = App::new("test".to_string(), false);
        assert!(app.settings.setting_help);

        app.update(key(KeyCode::Char('s')));
        for _ in 0..3 {
            app.update(key(KeyCode::Char('j')));
        }
        app.update(key(KeyCode::Right));
        assert!(!app.settings.setting_help);
        app.update(key(KeyCode::Left));
        assert!(app.settings.setting_help);
    }

    #[test]
    fn workspace_activity_moves_from_observing_to_recent_to_idle() {
        let start = Instant::now();
        let mut app = App::new("test".to_string(), false).with_idle_after(Duration::from_secs(30));

        app.update(Msg::Snapshot {
            result: Ok(activity_snapshot(1)),
            observed_at: start,
        });
        assert_eq!(activity(&app), VesselActivity::Observing);

        app.update(Msg::Snapshot {
            result: Ok(activity_snapshot(2)),
            observed_at: start + Duration::from_secs(5),
        });
        assert_eq!(activity(&app), VesselActivity::Recent);

        app.update(Msg::Snapshot {
            result: Ok(activity_snapshot(2)),
            observed_at: start + Duration::from_secs(36),
        });
        assert_eq!(activity(&app), VesselActivity::Idle);
    }

    #[test]
    fn visual_transition_duration_tracks_configured_frame_rate() {
        assert_eq!(transition_duration_frames(1), 1);
        assert_eq!(transition_duration_frames(12), 9);
        assert_eq!(transition_duration_frames(30), 23);
    }

    #[test]
    fn initial_survey_is_quiet_then_cargo_changes_animate() {
        let start = Instant::now();
        let mut app = App::new("test".to_string(), false);
        observe(
            &mut app,
            workspace_snapshot(
                ChangeCounts {
                    unstaged: 1,
                    ..ChangeCounts::default()
                },
                None,
                true,
                1,
            ),
            start,
        );
        assert!(visual_transition(&app).is_none());

        observe(
            &mut app,
            workspace_snapshot(
                ChangeCounts {
                    staged: 1,
                    ..ChangeCounts::default()
                },
                None,
                true,
                2,
            ),
            start + Duration::from_secs(1),
        );
        assert!(matches!(
            visual_transition(&app).map(|transition| &transition.kind),
            Some(DockTransitionKind::Cargo {
                from: crate::harbor::CargoCounts { unstaged: 1, .. }
            })
        ));
    }

    #[test]
    fn blocked_change_outranks_cargo_and_vessel_outranks_cargo() {
        let start = Instant::now();
        let mut app = App::new("test".to_string(), false);
        observe(
            &mut app,
            workspace_snapshot(ChangeCounts::default(), None, true, 1),
            start,
        );
        observe(
            &mut app,
            workspace_snapshot(
                ChangeCounts {
                    unstaged: 2,
                    conflicted: 1,
                    ..ChangeCounts::default()
                },
                Some(Operation::Merge),
                true,
                2,
            ),
            start + Duration::from_secs(1),
        );
        assert!(matches!(
            visual_transition(&app).map(|transition| &transition.kind),
            Some(DockTransitionKind::BecameBlocked)
        ));

        let mut arrival = App::new("test".to_string(), false);
        observe(
            &mut arrival,
            workspace_snapshot(ChangeCounts::default(), None, false, 1),
            start,
        );
        observe(
            &mut arrival,
            workspace_snapshot(
                ChangeCounts {
                    unstaged: 3,
                    ..ChangeCounts::default()
                },
                None,
                true,
                2,
            ),
            start + Duration::from_secs(1),
        );
        assert!(matches!(
            visual_transition(&arrival).map(|transition| &transition.kind),
            Some(DockTransitionKind::VesselArriving)
        ));
    }

    #[test]
    fn vessel_departure_keeps_source_vessel_until_cue_finishes() {
        let start = Instant::now();
        let mut app = App::new("test".to_string(), false);
        observe(
            &mut app,
            workspace_snapshot(
                ChangeCounts {
                    staged: 2,
                    ..ChangeCounts::default()
                },
                None,
                true,
                1,
            ),
            start,
        );
        observe(
            &mut app,
            workspace_snapshot(ChangeCounts::default(), None, false, 2),
            start + Duration::from_secs(1),
        );
        assert!(app.harbor.docks[0].vessel.is_none());
        assert!(matches!(
            visual_transition(&app).map(|transition| &transition.kind),
            Some(DockTransitionKind::VesselDeparting { vessel }) if vessel.staged == 2
        ));

        for _ in 0..transition_duration_frames(DEFAULT_FRAMES_PER_SECOND) {
            app.update(Msg::Tick);
        }
        assert!(visual_transition(&app).is_none());
    }

    #[test]
    fn detached_and_unborn_workspace_identity_does_not_create_false_arrival() {
        let start = Instant::now();
        let headless = |head| RepoSnapshot {
            name: "test".to_string(),
            default_branch: None,
            branches: Vec::new(),
            workspaces: vec![Workspace {
                path: PathBuf::from("/tmp/transition-test"),
                is_main: true,
                head,
                changes: ChangeCounts::default(),
                change_files: Vec::new(),
                activity_token: 1,
                operation: None,
            }],
        };

        let mut detached = App::new("test".to_string(), false);
        observe(
            &mut detached,
            headless(HeadState::Detached("aaaaaaa".to_string())),
            start,
        );
        observe(
            &mut detached,
            headless(HeadState::Detached("bbbbbbb".to_string())),
            start + Duration::from_secs(1),
        );
        assert!(visual_transition(&detached).is_none());

        let mut unborn = App::new("test".to_string(), false);
        observe(&mut unborn, headless(HeadState::Unborn), start);
        observe(
            &mut unborn,
            workspace_snapshot(ChangeCounts::default(), None, true, 2),
            start + Duration::from_secs(1),
        );
        assert!(visual_transition(&unborn).is_none());
    }

    #[test]
    fn deleted_dock_disappears_without_a_ghost_transition() {
        let start = Instant::now();
        let mut app = App::new("test".to_string(), false);
        observe(
            &mut app,
            workspace_snapshot(ChangeCounts::default(), None, true, 1),
            start,
        );
        observe(
            &mut app,
            RepoSnapshot {
                name: "test".to_string(),
                default_branch: None,
                branches: Vec::new(),
                workspaces: Vec::new(),
            },
            start + Duration::from_secs(1),
        );
        assert!(app.harbor.docks.is_empty());
        assert!(app.visual_transitions.active.is_empty());
    }

    #[test]
    fn active_cue_survives_fast_poll_and_new_change_replaces_it() {
        let start = Instant::now();
        let mut app = App::new("test".to_string(), false);
        let changes = |unstaged| ChangeCounts {
            unstaged,
            ..ChangeCounts::default()
        };
        observe(
            &mut app,
            workspace_snapshot(changes(1), None, true, 1),
            start,
        );
        observe(
            &mut app,
            workspace_snapshot(changes(2), None, true, 2),
            start + Duration::from_millis(100),
        );
        let original_start = visual_transition(&app).unwrap().started_frame;
        app.update(Msg::Tick);
        observe(
            &mut app,
            workspace_snapshot(changes(2), None, true, 2),
            start + Duration::from_millis(200),
        );
        assert_eq!(
            visual_transition(&app).unwrap().started_frame,
            original_start
        );

        observe(
            &mut app,
            workspace_snapshot(changes(3), None, true, 3),
            start + Duration::from_millis(300),
        );
        let replacement = visual_transition(&app).unwrap();
        assert_eq!(replacement.started_frame, app.animation.frame());
        assert!(matches!(
            replacement.kind,
            DockTransitionKind::Cargo {
                from: crate::harbor::CargoCounts { unstaged: 2, .. }
            }
        ));
    }

    #[test]
    fn reduced_motion_clears_cues_and_does_not_replay_them() {
        let start = Instant::now();
        let mut app = App::new("test".to_string(), false);
        observe(
            &mut app,
            workspace_snapshot(ChangeCounts::default(), None, true, 1),
            start,
        );
        observe(
            &mut app,
            workspace_snapshot(
                ChangeCounts {
                    unstaged: 1,
                    ..ChangeCounts::default()
                },
                None,
                true,
                2,
            ),
            start + Duration::from_secs(1),
        );
        assert!(visual_transition(&app).is_some());

        app.update(key(KeyCode::Char('m')));
        assert!(app.settings.reduced_motion);
        assert!(visual_transition(&app).is_none());
        app.update(key(KeyCode::Char('m')));
        assert!(!app.settings.reduced_motion);
        assert!(visual_transition(&app).is_none());

        let mut reduced = App::new("test".to_string(), true);
        observe(
            &mut reduced,
            workspace_snapshot(ChangeCounts::default(), None, true, 1),
            start,
        );
        observe(
            &mut reduced,
            workspace_snapshot(
                ChangeCounts {
                    unstaged: 1,
                    ..ChangeCounts::default()
                },
                None,
                true,
                2,
            ),
            start + Duration::from_secs(1),
        );
        assert!(visual_transition(&reduced).is_none());
    }

    #[test]
    fn inspect_drills_into_vessel_and_changed_files_then_steps_back() {
        let mut app = App::new("test".to_string(), false);
        app.update(snapshot_msg(Ok(activity_snapshot(1))));

        app.update(key(KeyCode::Char('i')));
        assert_eq!(app.inspect_target, InspectTarget::Dock);
        app.update(key(KeyCode::Enter));
        assert_eq!(app.inspect_target, InspectTarget::Vessel);
        app.update(key(KeyCode::Enter));
        assert_eq!(app.inspect_target, InspectTarget::Change(0));

        app.update(key(KeyCode::Esc));
        assert_eq!(app.inspect_target, InspectTarget::Vessel);
        app.update(key(KeyCode::Esc));
        assert_eq!(app.inspect_target, InspectTarget::Dock);
        app.update(key(KeyCode::Esc));
        assert_eq!(app.mode, Mode::Ambient);
    }

    #[test]
    fn commit_merge_and_update_events_are_live_not_historical() {
        let start = Instant::now();
        let mut app = App::new("test".to_string(), false);
        observe(
            &mut app,
            transition_snapshot("one", TipAction::Other, 1, 0),
            start,
        );
        assert!(app.harbor.docks[0].events.is_empty());

        observe(
            &mut app,
            transition_snapshot("two", TipAction::Commit, 1, 1),
            start + Duration::from_secs(1),
        );
        assert_eq!(app.harbor.docks[0].events[0].kind, EventKind::Commit);

        observe(
            &mut app,
            transition_snapshot("merge", TipAction::Merge, 2, 2),
            start + Duration::from_secs(2),
        );
        assert_eq!(
            app.harbor.docks[0].events.last().unwrap().kind,
            EventKind::Merge
        );

        observe(
            &mut app,
            transition_snapshot("fast-forward", TipAction::Merge, 1, 3),
            start + Duration::from_secs(3),
        );
        assert_eq!(
            app.harbor.docks[0].events.last().unwrap().kind,
            EventKind::Update
        );

        observe(
            &mut app,
            transition_snapshot("fast-forward", TipAction::Merge, 1, 3),
            start + Duration::from_secs(16),
        );
        assert!(app.harbor.docks[0].events.is_empty());
    }

    #[test]
    fn repeated_polling_does_not_replay_an_active_event() {
        let start = Instant::now();
        let mut app = App::new("test".to_string(), false);
        observe(
            &mut app,
            transition_snapshot("one", TipAction::Other, 1, 0),
            start,
        );
        let committed = transition_snapshot("two", TipAction::Commit, 1, 1);
        observe(&mut app, committed.clone(), start + Duration::from_secs(1));
        observe(&mut app, committed, start + Duration::from_secs(2));

        assert_eq!(app.harbor.docks[0].events.len(), 1);
    }

    #[test]
    fn push_requires_upstream_reflog_evidence() {
        let start = Instant::now();
        let mut app = App::new("test".to_string(), false);
        observe(
            &mut app,
            transition_snapshot_with_upstream(
                "same",
                TipAction::Commit,
                1,
                2,
                "before",
                UpstreamAction::Other,
            ),
            start,
        );
        observe(
            &mut app,
            transition_snapshot_with_upstream(
                "same",
                TipAction::Commit,
                1,
                0,
                "after",
                UpstreamAction::Push,
            ),
            start + Duration::from_secs(1),
        );

        assert_eq!(app.harbor.docks[0].events[0].kind, EventKind::Push);
        assert_eq!(
            app.harbor.docks[0].events[0].summary,
            "2 commits sent upstream"
        );
    }

    #[test]
    fn ambiguous_upstream_movement_is_neutral_even_when_ahead_decreases() {
        let start = Instant::now();
        let mut app = App::new("test".to_string(), false);
        observe(
            &mut app,
            transition_snapshot_with_upstream(
                "same",
                TipAction::Commit,
                1,
                2,
                "before",
                UpstreamAction::Other,
            ),
            start,
        );
        observe(
            &mut app,
            transition_snapshot_with_upstream(
                "same",
                TipAction::Commit,
                1,
                0,
                "after",
                UpstreamAction::Other,
            ),
            start + Duration::from_secs(1),
        );

        assert_eq!(app.harbor.docks[0].events.len(), 1);
        assert_eq!(app.harbor.docks[0].events[0].kind, EventKind::Update);
        assert_eq!(
            app.harbor.docks[0].events[0].summary,
            "origin/main reference moved"
        );
    }

    #[test]
    fn hosting_data_attaches_prs_creates_remote_docks_and_supports_check_inspection() {
        let mut app =
            App::new("test".to_string(), false).with_github(true, DEFAULT_GITHUB_POLL_INTERVAL);
        app.update(snapshot_msg(Ok(snapshot_with_branches(&["topic"]))));
        app.update(Msg::Hosting(Ok(hosting_snapshot())));

        let local = app
            .harbor
            .docks
            .iter()
            .position(|dock| dock.name == "topic")
            .unwrap();
        let remote = app
            .harbor
            .docks
            .iter()
            .find(|dock| dock.name == "remote-topic")
            .unwrap();
        assert_eq!(app.harbor.docks[local].clearances[0].number, 7);
        assert_eq!(remote.kind, DockKind::RemoteBranch);
        assert_eq!(remote.condition, Condition::Awaiting);
        assert_eq!(app.harbor.convoys[0].tag, "v1.0.0");

        app.selected = local;
        app.update(key(KeyCode::Char('i')));
        app.update(key(KeyCode::Char('p')));
        assert_eq!(app.inspect_target, InspectTarget::PullRequest(0));
        app.update(key(KeyCode::Enter));
        assert_eq!(
            app.inspect_target,
            InspectTarget::Check {
                pull_request: 0,
                check: 0
            }
        );
    }

    #[test]
    fn disabling_github_clears_remote_state_and_ignores_late_results() {
        let mut app =
            App::new("test".to_string(), false).with_github(true, DEFAULT_GITHUB_POLL_INTERVAL);
        app.update(snapshot_msg(Ok(snapshot_with_branches(&["topic"]))));
        app.update(Msg::Hosting(Ok(hosting_snapshot())));
        assert_eq!(app.harbor.docks.len(), 2);
        assert!(!app.harbor.convoys.is_empty());

        app.update(key(KeyCode::Char('s')));
        for _ in 0..6 {
            app.update(key(KeyCode::Char('j')));
        }
        app.update(key(KeyCode::Right));

        assert!(!app.settings.github_enabled);
        assert_eq!(app.harbor.docks.len(), 1);
        assert!(app.harbor.docks[0].clearances.is_empty());
        assert!(app.harbor.convoys.is_empty());

        app.update(Msg::Hosting(Ok(hosting_snapshot())));
        assert_eq!(app.harbor.docks.len(), 1);
        assert!(app.harbor.convoys.is_empty());
    }
}
