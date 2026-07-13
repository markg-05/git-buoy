use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent};

use crate::git::{RepoSnapshot, TipAction};
use crate::harbor::{self, Animation, DockEvent, EventKind, Harbor, VesselActivity};

const DEFAULT_IDLE_AFTER: Duration = Duration::from_secs(30);
const EVENT_LIFETIME: Duration = Duration::from_secs(12);

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
}

/// Application state: the current scene plus mode, selection, and clock.
/// `update` is a pure state transition over [`Msg`], so behavior is testable
/// without a terminal.
pub struct App {
    pub harbor: Harbor,
    pub mode: Mode,
    pub selected: usize,
    pub inspect_target: InspectTarget,
    pub reduced_motion: bool,
    pub animation: Animation,
    pub should_quit: bool,
    /// Most recent collector failure, shown until a survey succeeds again.
    pub error: Option<String>,
    /// False until the first snapshot arrives.
    pub loaded: bool,
    /// Whether the legend overlay is currently shown.
    pub show_legend: bool,
    activity: ActivityTracker,
    transitions: TransitionTracker,
}

impl App {
    pub fn new(name: String, reduced_motion: bool) -> Self {
        Self {
            harbor: Harbor {
                name,
                docks: Vec::new(),
            },
            mode: Mode::Ambient,
            selected: 0,
            inspect_target: InspectTarget::Dock,
            reduced_motion,
            animation: Animation::default(),
            should_quit: false,
            error: None,
            loaded: false,
            show_legend: false,
            activity: ActivityTracker::new(DEFAULT_IDLE_AFTER),
            transitions: TransitionTracker::default(),
        }
    }

    pub fn with_idle_after(mut self, idle_after: Duration) -> Self {
        self.activity.idle_after = idle_after;
        self
    }

    pub fn update(&mut self, msg: Msg) {
        match msg {
            Msg::Tick => {
                if !self.reduced_motion {
                    self.animation.tick();
                }
            }
            Msg::Snapshot {
                result: Ok(snapshot),
                observed_at,
            } => {
                let activities = self.activity.observe(&snapshot, observed_at);
                let events = self.transitions.observe(&snapshot, observed_at);
                let mut harbor = harbor::to_harbor_with_activity(&snapshot, |workspace| {
                    activities
                        .get(&workspace.path)
                        .copied()
                        .unwrap_or(VesselActivity::Observing)
                });
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
            Msg::Key(key) => self.handle_key(key),
        }
    }

    fn handle_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('l') | KeyCode::Char('?') => self.show_legend = !self.show_legend,
            // Escape peels back one layer at a time: legend, then inspect,
            // then quit.
            KeyCode::Esc if self.show_legend => self.show_legend = false,
            KeyCode::Esc | KeyCode::Left | KeyCode::Char('h') => self.step_out(),
            KeyCode::Char('m') => self.reduced_motion = !self.reduced_motion,
            KeyCode::Char('i') => self.enter_inspect(),
            KeyCode::Enter | KeyCode::Right => self.step_in(),
            KeyCode::Tab => self.select_next_dock(),
            KeyCode::BackTab => self.select_previous_dock(),
            KeyCode::Down | KeyCode::Char('j') => self.select_next(),
            KeyCode::Up | KeyCode::Char('k') => self.select_previous(),
            _ => {}
        }
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
            target => target,
        };
    }

    fn step_out(&mut self) {
        if self.mode == Mode::Ambient {
            self.should_quit = true;
            return;
        }
        self.inspect_target = match self.inspect_target {
            InspectTarget::Change(_) => InspectTarget::Vessel,
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
            self.select_next_dock();
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
            self.select_previous_dock();
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

#[derive(Debug, Default)]
struct TransitionTracker {
    branches: HashMap<String, BranchObservation>,
    active: Vec<ActiveDockEvent>,
}

#[derive(Debug)]
struct BranchObservation {
    tip_id: Option<String>,
    ahead: Option<usize>,
}

#[derive(Debug, Clone)]
struct ActiveDockEvent {
    branch: String,
    event: DockEvent,
    expires_at: Instant,
}

impl TransitionTracker {
    fn observe(&mut self, snapshot: &RepoSnapshot, observed_at: Instant) -> Vec<ActiveDockEvent> {
        self.active.retain(|event| event.expires_at > observed_at);
        self.branches
            .retain(|name, _| snapshot.branches.iter().any(|branch| &branch.name == name));

        for branch in &snapshot.branches {
            let current_tip = branch.tip.as_ref().map(|tip| tip.id.clone());
            let current_ahead = branch.sync.as_ref().map(|sync| sync.ahead);
            if let Some(previous) = self.branches.get(&branch.name) {
                if previous.tip_id != current_tip
                    && let Some(tip) = &branch.tip
                {
                    let kind = if tip.parent_count > 1 || tip.action == TipAction::Merge {
                        Some(EventKind::Merge)
                    } else if tip.action == TipAction::Commit {
                        Some(EventKind::Commit)
                    } else {
                        None
                    };
                    if let Some(kind) = kind {
                        self.active.push(ActiveDockEvent {
                            branch: branch.name.clone(),
                            event: DockEvent {
                                kind,
                                summary: tip.summary.clone(),
                            },
                            expires_at: observed_at + EVENT_LIFETIME,
                        });
                    }
                }

                if previous.tip_id == current_tip
                    && let (Some(before), Some(after)) = (previous.ahead, current_ahead)
                    && before > after
                {
                    let count = before - after;
                    let noun = if count == 1 { "commit" } else { "commits" };
                    self.active.push(ActiveDockEvent {
                        branch: branch.name.clone(),
                        event: DockEvent {
                            kind: EventKind::Push,
                            summary: format!("{count} {noun} sent upstream"),
                        },
                        expires_at: observed_at + EVENT_LIFETIME,
                    });
                }
            }

            self.branches.insert(
                branch.name.clone(),
                BranchObservation {
                    tip_id: current_tip,
                    ahead: current_ahead,
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
        BranchInfo, ChangeCounts, ChangeFile, ChangeKind, HeadState, RepoSnapshot, SyncState,
        TipInfo, Workspace,
    };

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

    fn activity(app: &App) -> VesselActivity {
        app.harbor.docks[0].vessel.as_ref().unwrap().activity
    }

    fn transition_snapshot(
        id: &str,
        action: TipAction,
        parent_count: usize,
        ahead: usize,
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
    fn commit_and_merge_events_are_live_not_historical() {
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
            transition_snapshot("merge", TipAction::Merge, 2, 2),
            start + Duration::from_secs(15),
        );
        assert!(app.harbor.docks[0].events.is_empty());
    }

    #[test]
    fn unchanged_tip_with_lower_ahead_count_is_a_push() {
        let start = Instant::now();
        let mut app = App::new("test".to_string(), false);
        observe(
            &mut app,
            transition_snapshot("same", TipAction::Commit, 1, 2),
            start,
        );
        observe(
            &mut app,
            transition_snapshot("same", TipAction::Commit, 1, 0),
            start + Duration::from_secs(1),
        );

        assert_eq!(app.harbor.docks[0].events[0].kind, EventKind::Push);
        assert_eq!(
            app.harbor.docks[0].events[0].summary,
            "2 commits sent upstream"
        );
    }
}
