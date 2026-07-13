use std::path::PathBuf;

/// A point-in-time description of one repository, decoupled from git2 types
/// so the harbor mapping can be exercised in tests without a live repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoSnapshot {
    /// Directory name of the repository, used as the harbor name.
    pub name: String,
    /// The repository's default branch, when one can be determined.
    pub default_branch: Option<String>,
    /// All local branches, sorted by name.
    pub branches: Vec<BranchInfo>,
    /// The main worktree (unless bare) followed by linked worktrees.
    pub workspaces: Vec<Workspace>,
}

/// One local branch, whether or not it is checked out anywhere.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchInfo {
    pub name: String,
    /// Position relative to the branch's upstream, if it has one.
    pub sync: Option<SyncState>,
    /// Summary line of the branch tip commit.
    pub last_commit: Option<String>,
    /// Branch-tip identity and provenance used to recognize live transitions
    /// without exposing git2 commit or reflog types.
    pub tip: Option<TipInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TipInfo {
    pub id: String,
    pub summary: String,
    pub parent_count: usize,
    pub action: TipAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TipAction {
    Commit,
    Merge,
    Other,
}

/// A checked-out working directory: the main worktree or a linked worktree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workspace {
    pub path: PathBuf,
    pub is_main: bool,
    pub head: HeadState,
    pub changes: ChangeCounts,
    /// Changed paths, with one entry per category. A path can appear twice
    /// when it has both staged and unstaged changes.
    pub change_files: Vec<ChangeFile>,
    /// Fingerprint of observable workspace state used by the application to
    /// recognize recent activity across surveys. It has no meaning outside
    /// comparisons within one run.
    pub activity_token: u64,
    /// A multi-step operation that has started but not finished.
    pub operation: Option<Operation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeFile {
    pub path: PathBuf,
    pub kind: ChangeKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ChangeKind {
    Staged,
    Unstaged,
    Untracked,
    Conflicted,
}

impl ChangeKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Staged => "staged",
            Self::Unstaged => "unstaged",
            Self::Untracked => "untracked",
            Self::Conflicted => "conflicted",
        }
    }
}

/// Where a workspace's HEAD points. Detached and unborn HEADs are normal
/// states, not errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeadState {
    Branch(String),
    /// Short commit id.
    Detached(String),
    /// Fresh repository with no commits yet.
    Unborn,
}

/// Counts of files by change category in one workspace.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ChangeCounts {
    pub staged: usize,
    pub unstaged: usize,
    pub untracked: usize,
    pub conflicted: usize,
}

impl ChangeCounts {
    pub fn is_clean(&self) -> bool {
        self.staged == 0 && self.unstaged == 0 && self.untracked == 0 && self.conflicted == 0
    }
}

/// Position of a branch relative to its upstream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncState {
    pub upstream: String,
    pub ahead: usize,
    pub behind: usize,
}

/// An in-progress Git operation that blocks normal work until resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    Merge,
    Rebase,
    CherryPick,
    Revert,
    Bisect,
}

impl Operation {
    pub fn label(&self) -> &'static str {
        match self {
            Operation::Merge => "merge in progress",
            Operation::Rebase => "rebase in progress",
            Operation::CherryPick => "cherry-pick in progress",
            Operation::Revert => "revert in progress",
            Operation::Bisect => "bisect in progress",
        }
    }
}
