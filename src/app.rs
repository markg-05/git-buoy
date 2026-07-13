use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent};

use crate::git::RepoSnapshot;
use crate::harbor::{self, Animation, Harbor, VesselActivity};

const DEFAULT_IDLE_AFTER: Duration = Duration::from_secs(30);

/// The two complementary experiences: a passive overview and keyboard-driven
/// access to exact repository details.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Ambient,
    Inspect,
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
            reduced_motion,
            animation: Animation::default(),
            should_quit: false,
            error: None,
            loaded: false,
            show_legend: false,
            activity: ActivityTracker::new(DEFAULT_IDLE_AFTER),
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
                self.harbor = harbor::to_harbor_with_activity(&snapshot, |workspace| {
                    activities
                        .get(&workspace.path)
                        .copied()
                        .unwrap_or(VesselActivity::Observing)
                });
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
            KeyCode::Esc => match self.mode {
                Mode::Inspect => self.mode = Mode::Ambient,
                Mode::Ambient => self.should_quit = true,
            },
            KeyCode::Char('m') => self.reduced_motion = !self.reduced_motion,
            KeyCode::Char('i') | KeyCode::Enter => self.enter_inspect(),
            KeyCode::Tab | KeyCode::Down | KeyCode::Char('j') => self.select_next(),
            KeyCode::BackTab | KeyCode::Up | KeyCode::Char('k') => self.select_previous(),
            _ => {}
        }
    }

    fn enter_inspect(&mut self) {
        if !self.harbor.docks.is_empty() {
            self.mode = Mode::Inspect;
        }
    }

    fn select_next(&mut self) {
        if self.mode == Mode::Ambient {
            // The first navigation key only opens inspect mode on the
            // current dock; movement starts with the next press.
            self.enter_inspect();
        } else {
            self.selected = (self.selected + 1) % self.harbor.docks.len();
        }
    }

    fn select_previous(&mut self) {
        if self.mode == Mode::Ambient {
            self.enter_inspect();
        } else {
            let count = self.harbor.docks.len();
            self.selected = (self.selected + count - 1) % count;
        }
    }

    fn clamp_selection(&mut self) {
        if self.harbor.docks.is_empty() {
            self.selected = 0;
            self.mode = Mode::Ambient;
        } else if self.selected >= self.harbor.docks.len() {
            self.selected = self.harbor.docks.len() - 1;
        }
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

    use crate::git::{BranchInfo, ChangeCounts, HeadState, RepoSnapshot, Workspace};

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
            }],
            workspaces: vec![Workspace {
                path: PathBuf::from("/tmp/activity-test"),
                is_main: true,
                head: HeadState::Branch("topic".to_string()),
                changes: ChangeCounts::default(),
                activity_token: token,
                operation: None,
            }],
        }
    }

    fn activity(app: &App) -> VesselActivity {
        app.harbor.docks[0].vessel.unwrap().activity
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
}
