/// The visual scene representing one observed repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Harbor {
    pub name: String,
    /// Main terminal first, then occupied docks, then moored branches.
    pub docks: Vec<Dock>,
}

/// One berth in the harbor: a branch or a checked-out worktree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dock {
    pub name: String,
    pub kind: DockKind,
    pub condition: Condition,
    /// Present when a workspace has this dock's work checked out.
    pub vessel: Option<Vessel>,
    /// Commits ahead of and behind the upstream, when one exists.
    pub sync: Option<(usize, usize)>,
    /// Label and value pairs shown verbatim in inspect mode. The metaphor
    /// offers orientation; these carry the exact Git facts.
    pub detail: Vec<(&'static str, String)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DockKind {
    /// The repository's default branch.
    MainTerminal,
    Branch,
    /// A worktree whose HEAD is not on any local branch.
    DetachedWorktree,
}

/// What a dock communicates at a glance. Exactly one condition applies,
/// chosen by priority: blocked work outranks pending cargo, which outranks
/// outbound commits, which outranks calm water.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Condition {
    /// Conflicts or an unfinished operation prevent work from landing.
    Blocked,
    /// Staged changes: cargo sealed and ready to become a commit.
    Sealed,
    /// Uncommitted changes: cargo still being loaded.
    Loading,
    /// Commits ahead of upstream, ready to leave the harbor.
    Outbound,
    /// An occupied dock with nothing pending.
    Calm,
    /// A branch with no active workspace.
    Moored,
}

impl Condition {
    pub fn label(&self) -> &'static str {
        match self {
            Condition::Blocked => "blocked",
            Condition::Sealed => "sealed",
            Condition::Loading => "loading",
            Condition::Outbound => "outbound",
            Condition::Calm => "calm",
            Condition::Moored => "moored",
        }
    }
}

/// The activity at an occupied dock, measured in cargo.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Vessel {
    pub staged: usize,
    pub unstaged: usize,
    pub untracked: usize,
    pub conflicted: usize,
}
