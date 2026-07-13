use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use anyhow::{Context, Result};
use git2::{Branch, BranchType, Repository, RepositoryState, Status, StatusOptions};

use super::snapshot::{
    BranchInfo, ChangeCounts, ChangeFile, ChangeKind, HeadState, Operation, RepoSnapshot,
    SyncState, TipAction, TipInfo, Workspace,
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
            let tip = branch.get().peel_to_commit().ok().map(|commit| {
                let summary = commit
                    .summary()
                    .ok()
                    .flatten()
                    .unwrap_or("(no summary)")
                    .to_string();
                TipInfo {
                    id: commit.id().to_string(),
                    summary,
                    parent_count: commit.parent_count(),
                    action: tip_action(repo, &name),
                }
            });
            Some(BranchInfo {
                sync: branch_sync(repo, &branch),
                name,
                last_commit: tip.as_ref().map(|tip| tip.summary.clone()),
                tip,
            })
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

fn tip_action(repo: &Repository, branch_name: &str) -> TipAction {
    let reference = format!("refs/heads/{branch_name}");
    let message = repo.reflog(&reference).ok().and_then(|reflog| {
        reflog
            .get(0)
            .and_then(|entry| entry.message().ok().flatten().map(str::to_string))
    });
    match message.as_deref() {
        Some(message) if message.starts_with("commit") => TipAction::Commit,
        Some(message) if message.starts_with("merge ") || message.starts_with("pull ") => {
            TipAction::Merge
        }
        _ => TipAction::Other,
    }
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

/// Determine the default branch only from an authoritative local reference.
///
/// A normal local repository does not record which branch is the default:
/// `HEAD` is merely the branch currently checked out. A remote-tracking HEAD
/// does carry that meaning, as does `HEAD` in a bare repository. If neither is
/// available, returning `None` is more truthful than guessing from a name.
fn default_branch(repo: &Repository, branches: &[BranchInfo]) -> Option<String> {
    let mut remotes: Vec<String> = repo
        .remotes()
        .ok()
        .into_iter()
        .flat_map(|names| {
            names
                .iter()
                .filter_map(|name| name.ok().flatten().map(str::to_string))
                .collect::<Vec<_>>()
        })
        .collect();
    remotes.sort_by_key(|remote| (remote != "origin", remote.clone()));

    for remote in remotes {
        let reference_name = format!("refs/remotes/{remote}/HEAD");
        let prefix = format!("refs/remotes/{remote}/");
        if let Ok(reference) = repo.find_reference(&reference_name)
            && let Ok(Some(target)) = reference.symbolic_target()
            && let Some(name) = target.strip_prefix(&prefix)
            && branches.iter().any(|branch| branch.name == name)
        {
            return Some(name.to_string());
        }
    }

    if repo.is_bare()
        && let Ok(reference) = repo.find_reference("HEAD")
        && let Ok(Some(target)) = reference.symbolic_target()
        && let Some(name) = target.strip_prefix("refs/heads/")
        && branches.iter().any(|branch| branch.name == name)
    {
        return Some(name.to_string());
    }

    None
}

fn collect_workspace(repo: &Repository, is_main: bool) -> Workspace {
    let (changes, change_files) = collect_changes(repo);
    Workspace {
        path: repo_root(repo),
        is_main,
        head: head_state(repo),
        changes,
        change_files,
        activity_token: activity_token(repo),
        operation: operation(repo.state()),
    }
}

/// Hash the parts of a workspace that can change while work is being done.
/// File metadata supplements Git status so repeated edits to an already-dirty
/// file are still observable even when its status category does not change.
fn activity_token(repo: &Repository) -> u64 {
    let mut hasher = DefaultHasher::new();
    repo.head()
        .ok()
        .and_then(|head| head.target())
        .hash(&mut hasher);
    format!("{:?}", repo.state()).hash(&mut hasher);
    hash_metadata(repo.path().join("index"), &mut hasher);

    let mut options = StatusOptions::new();
    options
        .include_untracked(true)
        .recurse_untracked_dirs(true)
        .exclude_submodules(true);
    if let Ok(statuses) = repo.statuses(Some(&mut options)) {
        for entry in statuses.iter() {
            entry.status().bits().hash(&mut hasher);
            entry.path_bytes().hash(&mut hasher);

            if let Some(workdir) = repo.workdir()
                && let Some(delta) = entry.index_to_workdir()
                && let Some(path) = delta.new_file().path().or_else(|| delta.old_file().path())
            {
                hash_metadata(workdir.join(path), &mut hasher);
            }
        }
    }
    hasher.finish()
}

fn hash_metadata(path: PathBuf, hasher: &mut impl Hasher) {
    let Ok(metadata) = path.metadata() else {
        return;
    };
    metadata.len().hash(hasher);
    metadata.is_dir().hash(hasher);
    if let Ok(modified) = metadata.modified()
        && let Ok(since_epoch) = modified.duration_since(UNIX_EPOCH)
    {
        since_epoch.as_secs().hash(hasher);
        since_epoch.subsec_nanos().hash(hasher);
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

fn collect_changes(repo: &Repository) -> (ChangeCounts, Vec<ChangeFile>) {
    let mut counts = ChangeCounts::default();
    let mut files = Vec::new();
    let mut options = StatusOptions::new();
    options.include_untracked(true).exclude_submodules(true);
    let Ok(statuses) = repo.statuses(Some(&mut options)) else {
        return (counts, files);
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
        let path = status_path(&entry);
        if status.is_conflicted() {
            counts.conflicted += 1;
            files.push(ChangeFile {
                path,
                kind: ChangeKind::Conflicted,
            });
            continue;
        }
        if status.intersects(STAGED) {
            counts.staged += 1;
            files.push(ChangeFile {
                path: path.clone(),
                kind: ChangeKind::Staged,
            });
        }
        if status.intersects(UNSTAGED) {
            counts.unstaged += 1;
            files.push(ChangeFile {
                path: path.clone(),
                kind: ChangeKind::Unstaged,
            });
        }
        if status.contains(Status::WT_NEW) {
            counts.untracked += 1;
            files.push(ChangeFile {
                path,
                kind: ChangeKind::Untracked,
            });
        }
    }
    files.sort_by(|a, b| a.path.cmp(&b.path).then_with(|| a.kind.cmp(&b.kind)));
    (counts, files)
}

fn status_path(entry: &git2::StatusEntry<'_>) -> PathBuf {
    entry
        .index_to_workdir()
        .and_then(|delta| {
            delta
                .new_file()
                .path()
                .or_else(|| delta.old_file().path())
                .map(Path::to_path_buf)
        })
        .or_else(|| {
            entry.head_to_index().and_then(|delta| {
                delta
                    .new_file()
                    .path()
                    .or_else(|| delta.old_file().path())
                    .map(Path::to_path_buf)
            })
        })
        .or_else(|| entry.path().ok().map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("<path unavailable>"))
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

#[cfg(test)]
mod tests {
    use super::default_branch;
    use crate::git::BranchInfo;
    use git2::Repository;

    fn branches(names: &[&str]) -> Vec<BranchInfo> {
        names
            .iter()
            .map(|name| BranchInfo {
                name: (*name).to_string(),
                sync: None,
                last_commit: None,
                tip: None,
            })
            .collect()
    }

    #[test]
    fn does_not_guess_a_default_branch_from_conventional_names() {
        let directory = tempfile::tempdir().unwrap();
        let repo = Repository::init(directory.path()).unwrap();

        assert_eq!(default_branch(&repo, &branches(&["main", "topic"])), None);
    }

    #[test]
    fn bare_head_identifies_the_default_branch() {
        let directory = tempfile::tempdir().unwrap();
        let repo = Repository::init_bare(directory.path()).unwrap();
        repo.reference_symbolic("HEAD", "refs/heads/trunk", true, "test")
            .unwrap();

        assert_eq!(
            default_branch(&repo, &branches(&["topic", "trunk"])),
            Some("trunk".to_string())
        );
    }
}
