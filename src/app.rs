use crossterm::event::{KeyCode, KeyEvent};

use crate::git::RepoSnapshot;
use crate::harbor::{self, Animation, Harbor};

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
    Snapshot(Result<RepoSnapshot, String>),
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
        }
    }

    pub fn update(&mut self, msg: Msg) {
        match msg {
            Msg::Tick => {
                if !self.reduced_motion {
                    self.animation.tick();
                }
            }
            Msg::Snapshot(Ok(snapshot)) => {
                self.harbor = harbor::to_harbor(&snapshot);
                self.loaded = true;
                self.error = None;
                self.clamp_selection();
            }
            Msg::Snapshot(Err(message)) => {
                self.error = Some(message);
            }
            Msg::Key(key) => self.handle_key(key),
        }
    }

    fn handle_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
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

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyEvent, KeyModifiers};

    use crate::git::{BranchInfo, RepoSnapshot};

    use super::*;

    fn key(code: KeyCode) -> Msg {
        Msg::Key(KeyEvent::new(code, KeyModifiers::NONE))
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
        app.update(Msg::Snapshot(Ok(snapshot_with_branches(&["a", "b", "c"]))));
        app.update(key(KeyCode::Tab)); // enters inspect on dock 0
        app.update(key(KeyCode::Tab));
        app.update(key(KeyCode::Tab));
        assert_eq!(app.mode, Mode::Inspect);
        assert_eq!(app.selected, 2);
        // A branch disappears; the selection must stay in bounds.
        app.update(Msg::Snapshot(Ok(snapshot_with_branches(&["a"]))));
        assert_eq!(app.selected, 0);
    }

    #[test]
    fn escape_leaves_inspect_before_quitting() {
        let mut app = App::new("test".to_string(), false);
        app.update(Msg::Snapshot(Ok(snapshot_with_branches(&["a"]))));
        app.update(key(KeyCode::Char('i')));
        app.update(key(KeyCode::Esc));
        assert_eq!(app.mode, Mode::Ambient);
        assert!(!app.should_quit);
        app.update(key(KeyCode::Esc));
        assert!(app.should_quit);
    }
}
