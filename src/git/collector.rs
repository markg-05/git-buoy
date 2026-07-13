use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use git2::{Branch, BranchType, Repository, RepositoryState, Status, StatusOptions};

use super::snapshot::{
    BranchInfo, ChangeCounts, HeadState, Operation, RepoSnapshot, SyncState, Workspace,
};

/// Locate the repository containing `path` and return its root directory.
pub fn discover_root(path: &Path) -> Result<PathBuf> {
    let repo = Repository::discover(path)
        .with_context(|| format!("no Git repository found at {}", path.display()))?;
    Ok(repo_root(&repo))
}

/// Read the current state of the repository at `root`.
///
/// Unusual repository states (detached HEAD, unborn branch, missing upstream,
/// in-progress merge, bare repository) are reported as data, not errors; only
/// a repository that cannot be opened at all fails.
pub fn collect(root: &Path) -> Result<RepoSnapshot> {
    let repo = Repository::discover(root)
        .with_context(|| format!("cannot open repository at {}", root.display()))?;

    let branches = collect_branches(&repo);
    let default_branch = default_branch(&repo, &branches);

    let mut workspaces = Vec::new();
    if !repo.is_bare() {
        workspaces.push(collect_workspace(&repo, true));
    }
    if let Ok(names) = repo.worktrees() {
        for name in names.iter().filter_map(|n| n.ok().flatten()) {
            let Ok(worktree) = repo.find_worktree(name) else {
                continue;
            };
            let Ok(wt_repo) = Repository::open_from_worktree(&worktree) else {
                continue;
            };
            workspaces.push(collect_workspace(&wt_repo, false));
        }
    }

    Ok(RepoSnapshot {
        name: repo_name(&repo),
        default_branch,
        branches,
        workspaces,
    })
}

fn repo_root(repo: &Repository) -> PathBuf {
    repo.workdir().unwrap_or_else(|| repo.path()).to_path_buf()
}

fn repo_name(repo: &Repository) -> String {
    repo_root(repo)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "repository".to_string())
}

fn collect_branches(repo: &Repository) -> Vec<BranchInfo> {
    let Ok(branches) = repo.branches(Some(BranchType::Local)) else {
        return Vec::new();
    };
    let mut out: Vec<BranchInfo> = branches
        .flatten()
        .filter_map(|(branch, _)| {
            let name = branch.name().ok().flatten()?.to_string();
            let last_commit = branch
                .get()
                .peel_to_commit()
                .ok()
                .and_then(|c| c.summary().ok().flatten().map(str::to_string));
            Some(BranchInfo {
                sync: branch_sync(repo, &branch),
                name,
                last_commit,
            })
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

fn branch_sync(repo: &Repository, branch: &Branch) -> Option<SyncState> {
    let upstream = branch.upstream().ok()?;
    let local = branch.get().target()?;
    let remote = upstream.get().target()?;
    let (ahead, behind) = repo.graph_ahead_behind(local, remote).ok()?;
    Some(SyncState {
        upstream: upstream
            .name()
            .ok()
            .flatten()
            .unwrap_or("upstream")
            .to_string(),
        ahead,
        behind,
    })
}

/// Best-effort default branch: the remote HEAD if origin has one, then a
/// conventional local name, then the first branch alphabetically.
fn default_branch(repo: &Repository, branches: &[BranchInfo]) -> Option<String> {
    if let Ok(reference) = repo.find_reference("refs/remotes/origin/HEAD")
        && let Ok(Some(target)) = reference.symbolic_target()
        && let Some(name) = target.strip_prefix("refs/remotes/origin/")
        && branches.iter().any(|b| b.name == name)
    {
        return Some(name.to_string());
    }
    for candidate in ["main", "master", "trunk"] {
        if branches.iter().any(|b| b.name == candidate) {
            return Some(candidate.to_string());
        }
    }
    branches.first().map(|b| b.name.clone())
}

fn collect_workspace(repo: &Repository, is_main: bool) -> Workspace {
    Workspace {
        path: repo_root(repo),
        is_main,
        head: head_state(repo),
        changes: change_counts(repo),
        operation: operation(repo.state()),
    }
}

fn head_state(repo: &Repository) -> HeadState {
    let Ok(head) = repo.head() else {
        // Most commonly an unborn branch in a fresh repository.
        return HeadState::Unborn;
    };
    if repo.head_detached().unwrap_or(false) {
        let id = head
            .target()
            .map(|oid| oid.to_string().chars().take(8).collect())
            .unwrap_or_else(|| "unknown".to_string());
        HeadState::Detached(id)
    } else {
        HeadState::Branch(head.shorthand().unwrap_or("HEAD").to_string())
    }
}

fn change_counts(repo: &Repository) -> ChangeCounts {
    let mut counts = ChangeCounts::default();
    let mut options = StatusOptions::new();
    options.include_untracked(true).exclude_submodules(true);
    let Ok(statuses) = repo.statuses(Some(&mut options)) else {
        return counts;
    };
    const STAGED: Status = Status::INDEX_NEW
        .union(Status::INDEX_MODIFIED)
        .union(Status::INDEX_DELETED)
        .union(Status::INDEX_RENAMED)
        .union(Status::INDEX_TYPECHANGE);
    const UNSTAGED: Status = Status::WT_MODIFIED
        .union(Status::WT_DELETED)
        .union(Status::WT_RENAMED)
        .union(Status::WT_TYPECHANGE);
    for entry in statuses.iter() {
        let status = entry.status();
        if status.is_conflicted() {
            counts.conflicted += 1;
            continue;
        }
        if status.intersects(STAGED) {
            counts.staged += 1;
        }
        if status.intersects(UNSTAGED) {
            counts.unstaged += 1;
        }
        if status.contains(Status::WT_NEW) {
            counts.untracked += 1;
        }
    }
    counts
}

fn operation(state: RepositoryState) -> Option<Operation> {
    use RepositoryState as S;
    match state {
        S::Clean => None,
        S::Merge => Some(Operation::Merge),
        S::Rebase
        | S::RebaseInteractive
        | S::RebaseMerge
        | S::ApplyMailbox
        | S::ApplyMailboxOrRebase => Some(Operation::Rebase),
        S::CherryPick | S::CherryPickSequence => Some(Operation::CherryPick),
        S::Revert | S::RevertSequence => Some(Operation::Revert),
        S::Bisect => Some(Operation::Bisect),
    }
}
