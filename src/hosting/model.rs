#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HostingSnapshot {
    pub pull_requests: Vec<PullRequest>,
    pub releases: Vec<Release>,
}

/// Independently useful results from one remote-hosting survey.
///
/// A failed observation does not erase the other category or its last
/// successful value in the application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostingSurvey {
    pub pull_requests: Result<Vec<PullRequest>, String>,
    pub releases: Result<Vec<Release>, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequest {
    pub number: u64,
    pub title: String,
    pub head_branch: String,
    /// `owner/repository` for the hosted head, when GitHub still has it.
    pub head_repository: Option<String>,
    /// True when the head and base repositories differ.
    pub is_cross_repository: bool,
    pub url: String,
    pub is_draft: bool,
    pub review: ReviewState,
    pub merge: MergeState,
    pub checks: Vec<Check>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub name: String,
    pub state: CheckState,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckState {
    Passing,
    Failing,
    Pending,
    Unknown,
}

impl CheckState {
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
pub enum ReviewState {
    Approved,
    ChangesRequested,
    Required,
    None,
}

impl ReviewState {
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
pub enum MergeState {
    Ready,
    Blocked,
    Unknown,
}

impl MergeState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Blocked => "blocked",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub tag: String,
    pub name: String,
    pub is_latest: bool,
    pub is_prerelease: bool,
    pub published_at: Option<String>,
}
