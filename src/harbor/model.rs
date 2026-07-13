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
/// branch synchronization, which outranks calm water.
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
    /// Commits behind upstream, ready to enter the harbor.
    Incoming,
    /// Local and upstream both have commits the other does not.
    Diverged,
    /// An occupied branch with no upstream configured.
    Local,
    /// An occupied dock with nothing pending and an in-sync upstream.
    Calm,
    /// A branch with no active workspace.
    Moored,
}

impl Condition {
    /// Every condition in a natural reading order, from settled work through
    /// to problems and empty docks. The in-app legend and the README table
    /// both follow this order so the two never drift apart.
    pub const ALL: [Condition; 9] = [
        Condition::Calm,
        Condition::Local,
        Condition::Loading,
        Condition::Sealed,
        Condition::Outbound,
        Condition::Incoming,
        Condition::Diverged,
        Condition::Blocked,
        Condition::Moored,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            Condition::Blocked => "blocked",
            Condition::Sealed => "sealed",
            Condition::Loading => "loading",
            Condition::Outbound => "outbound",
            Condition::Incoming => "incoming",
            Condition::Diverged => "diverged",
            Condition::Local => "local",
            Condition::Calm => "calm",
            Condition::Moored => "moored",
        }
    }

    /// One-line explanation shown in the legend and mirrored in the README.
    pub fn description(&self) -> &'static str {
        match self {
            Condition::Calm => "checked out, committed, and in sync",
            Condition::Loading => "uncommitted changes still being loaded",
            Condition::Sealed => "changes staged, ready to become a commit",
            Condition::Outbound => "commits ahead of upstream, ready to push",
            Condition::Incoming => "commits behind upstream, ready to pull",
            Condition::Diverged => "local and upstream histories have diverged",
            Condition::Local => "checked out with no upstream configured",
            Condition::Blocked => "a conflict or in-progress operation stops work",
            Condition::Moored => "a branch with no worktree checked out",
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
