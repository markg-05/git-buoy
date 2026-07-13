/// The visual scene representing one observed repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Harbor {
    pub name: String,
    /// Main terminal first, then occupied docks, then moored branches.
    pub docks: Vec<Dock>,
    /// Published releases reported by an optional hosting observer.
    pub convoys: Vec<Convoy>,
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
    /// Short-lived transitions observed while Git Buoy is running.
    pub events: Vec<DockEvent>,
    /// Pull requests associated with this branch.
    pub clearances: Vec<Clearance>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clearance {
    pub number: u64,
    pub title: String,
    pub url: String,
    pub is_draft: bool,
    pub review: ReviewStatus,
    pub landing: LandingStatus,
    pub inspections: Vec<Inspection>,
}

impl Clearance {
    pub fn inspection_status(&self) -> InspectionStatus {
        if self
            .inspections
            .iter()
            .any(|inspection| inspection.status == InspectionStatus::Failing)
        {
            InspectionStatus::Failing
        } else if self
            .inspections
            .iter()
            .any(|inspection| inspection.status == InspectionStatus::Pending)
        {
            InspectionStatus::Pending
        } else if !self.inspections.is_empty()
            && self
                .inspections
                .iter()
                .all(|inspection| inspection.status == InspectionStatus::Passing)
        {
            InspectionStatus::Passing
        } else {
            InspectionStatus::Unknown
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inspection {
    pub name: String,
    pub status: InspectionStatus,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InspectionStatus {
    Passing,
    Failing,
    Pending,
    Unknown,
}

impl InspectionStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Passing => "passing",
            Self::Failing => "failing",
            Self::Pending => "pending",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewStatus {
    Approved,
    ChangesRequested,
    Required,
    None,
}

impl ReviewStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Approved => "approved",
            Self::ChangesRequested => "changes requested",
            Self::Required => "review required",
            Self::None => "no review decision",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LandingStatus {
    Ready,
    Blocked,
    Unknown,
}

impl LandingStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Blocked => "blocked",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Convoy {
    pub tag: String,
    pub name: String,
    pub is_latest: bool,
    pub is_prerelease: bool,
    pub published_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DockEvent {
    pub kind: EventKind,
    pub summary: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    Commit,
    Push,
    Merge,
}

impl EventKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Commit => "committed",
            Self::Push => "pushed",
            Self::Merge => "merged",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DockKind {
    /// The repository's default branch.
    MainTerminal,
    Branch,
    /// A worktree whose HEAD is not on any local branch.
    DetachedWorktree,
    /// A pull-request head branch not present among local branches.
    RemoteBranch,
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
    /// A remote-only pull-request branch awaiting clearance.
    Awaiting,
    /// An occupied dock with nothing pending and an in-sync upstream.
    Calm,
    /// A branch with no active workspace.
    Moored,
}

impl Condition {
    /// Every condition in a natural reading order, from settled work through
    /// to problems and empty docks. The in-app legend and the README table
    /// both follow this order so the two never drift apart.
    pub const ALL: [Condition; 10] = [
        Condition::Calm,
        Condition::Local,
        Condition::Loading,
        Condition::Sealed,
        Condition::Outbound,
        Condition::Incoming,
        Condition::Diverged,
        Condition::Awaiting,
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
            Condition::Awaiting => "awaiting",
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
            Condition::Awaiting => "remote pull request awaiting clearance",
            Condition::Blocked => "a conflict or in-progress operation stops work",
            Condition::Moored => "a branch with no worktree checked out",
        }
    }
}

/// The activity at an occupied dock, measured in cargo.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Vessel {
    pub workspace: PathBuf,
    pub staged: usize,
    pub unstaged: usize,
    pub untracked: usize,
    pub conflicted: usize,
    pub activity: VesselActivity,
    pub cargo: Vec<CargoItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CargoItem {
    pub path: PathBuf,
    pub kind: CargoKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CargoKind {
    Staged,
    Unstaged,
    Untracked,
    Conflicted,
}

impl CargoKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Staged => "staged",
            Self::Unstaged => "unstaged",
            Self::Untracked => "untracked",
            Self::Conflicted => "conflicted",
        }
    }
}

/// Whether observable Git/worktree state has changed during this run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum VesselActivity {
    /// The first survey has arrived, but there is not enough history yet.
    #[default]
    Observing,
    /// This workspace changed within the configured idle threshold.
    Recent,
    /// No observable change occurred during the configured idle threshold.
    Idle,
}

impl VesselActivity {
    pub fn label(self) -> &'static str {
        match self {
            Self::Observing => "observing",
            Self::Recent => "recent",
            Self::Idle => "idle",
        }
    }
}
use std::path::PathBuf;
