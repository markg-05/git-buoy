//! End-to-end tests of the state pipeline: build real repositories with the
//! git CLI, collect them with the git2-based collector, and assert the harbor
//! scene that results. The git CLI is used only here, as a fixture tool; the
//! application itself never parses git output.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use git_buoy::git::{ChangeKind, HeadState, Operation, TipAction, collect};
use git_buoy::harbor::{Condition, DockKind, to_harbor};

fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "Harbor Test")
        .env("GIT_AUTHOR_EMAIL", "harbor@example.invalid")
        .env("GIT_COMMITTER_NAME", "Harbor Test")
        .env("GIT_COMMITTER_EMAIL", "harbor@example.invalid")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .expect("git must be runnable in tests");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Run git and allow failure; merges that produce conflicts exit nonzero.
fn git_allow_failure(dir: &Path, args: &[&str]) {
    let _ = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "Harbor Test")
        .env("GIT_AUTHOR_EMAIL", "harbor@example.invalid")
        .env("GIT_COMMITTER_NAME", "Harbor Test")
        .env("GIT_COMMITTER_EMAIL", "harbor@example.invalid")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .expect("git must be runnable in tests");
}

fn commit_file(dir: &Path, file: &str, content: &str, message: &str) {
    fs::write(dir.join(file), content).unwrap();
    git(dir, &["add", file]);
    git(dir, &["commit", "-m", message]);
}

/// A fresh repository with one commit on `main`.
fn seeded_repo() -> (tempfile::TempDir, PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-b", "main"]);
    commit_file(&repo, "README.md", "# harbor\n", "initial commit");
    (temp, repo)
}

#[test]
fn local_repository_without_remote_does_not_guess_a_main_terminal() {
    let (_temp, repo) = seeded_repo();
    let harbor = to_harbor(&collect(&repo).unwrap());

    assert_eq!(harbor.docks.len(), 1);
    let dock = &harbor.docks[0];
    assert_eq!(dock.name, "main");
    assert_eq!(dock.kind, DockKind::Branch);
    assert_eq!(dock.condition, Condition::Local);
    let vessel = dock.vessel.as_ref().expect("main worktree is occupied");
    assert_eq!(vessel.staged + vessel.unstaged + vessel.untracked, 0);
}

#[test]
fn uncommitted_changes_load_cargo() {
    let (_temp, repo) = seeded_repo();
    fs::write(repo.join("README.md"), "# harbor, revised\n").unwrap();
    fs::write(repo.join("notes.txt"), "untracked\n").unwrap();

    let harbor = to_harbor(&collect(&repo).unwrap());
    let dock = &harbor.docks[0];
    assert_eq!(dock.condition, Condition::Loading);
    let vessel = dock.vessel.as_ref().unwrap();
    assert_eq!(vessel.unstaged, 1);
    assert_eq!(vessel.untracked, 1);
    assert!(
        vessel
            .cargo
            .iter()
            .any(|item| item.path == Path::new("README.md"))
    );
    assert!(
        vessel
            .cargo
            .iter()
            .any(|item| item.path == Path::new("notes.txt"))
    );
}

#[test]
fn repeated_edits_to_a_dirty_file_change_the_activity_token() {
    let (_temp, repo) = seeded_repo();
    fs::write(repo.join("README.md"), "# first revision\n").unwrap();
    let first = collect(&repo).unwrap().workspaces[0].activity_token;

    fs::write(
        repo.join("README.md"),
        "# second revision with different metadata\n",
    )
    .unwrap();
    let second = collect(&repo).unwrap().workspaces[0].activity_token;

    assert_ne!(first, second);
}

#[test]
fn nested_untracked_directory_keeps_summary_and_activity_behavior() {
    let (_temp, repo) = seeded_repo();
    let draft = repo.join("drafts").join("nested").join("note.txt");
    fs::create_dir_all(draft.parent().unwrap()).unwrap();
    fs::write(&draft, "first draft\n").unwrap();

    let first = collect(&repo).unwrap();
    let workspace = &first.workspaces[0];
    assert_eq!(workspace.changes.untracked, 1);
    assert_eq!(
        workspace
            .change_files
            .iter()
            .filter(|file| file.kind == ChangeKind::Untracked)
            .count(),
        1
    );

    fs::write(&draft, "second draft with different metadata\n").unwrap();
    let second = collect(&repo).unwrap();

    assert_ne!(
        workspace.activity_token,
        second.workspaces[0].activity_token
    );
}

#[test]
fn staged_changes_seal_the_cargo() {
    let (_temp, repo) = seeded_repo();
    fs::write(repo.join("README.md"), "# harbor, revised\n").unwrap();
    git(&repo, &["add", "README.md"]);

    let harbor = to_harbor(&collect(&repo).unwrap());
    assert_eq!(harbor.docks[0].condition, Condition::Sealed);
    assert_eq!(harbor.docks[0].vessel.as_ref().unwrap().staged, 1);
}

#[test]
fn merge_conflict_blocks_the_dock() {
    let (_temp, repo) = seeded_repo();
    git(&repo, &["checkout", "-b", "feature"]);
    commit_file(&repo, "README.md", "# feature version\n", "feature change");
    git(&repo, &["checkout", "main"]);
    commit_file(&repo, "README.md", "# main version\n", "main change");
    git_allow_failure(&repo, &["merge", "feature"]);

    let snapshot = collect(&repo).unwrap();
    let workspace = &snapshot.workspaces[0];
    assert_eq!(workspace.operation, Some(Operation::Merge));
    assert!(workspace.changes.conflicted > 0);

    let harbor = to_harbor(&snapshot);
    let main = harbor.docks.iter().find(|d| d.name == "main").unwrap();
    assert_eq!(main.condition, Condition::Blocked);
}

#[test]
fn successful_merge_tip_records_merge_provenance() {
    let (_temp, repo) = seeded_repo();
    git(&repo, &["checkout", "-b", "feature"]);
    commit_file(&repo, "feature.txt", "feature\n", "feature change");
    git(&repo, &["checkout", "main"]);
    commit_file(&repo, "main.txt", "main\n", "main change");
    git(
        &repo,
        &["merge", "--no-ff", "feature", "-m", "merge feature"],
    );

    let snapshot = collect(&repo).unwrap();
    let tip = snapshot
        .branches
        .iter()
        .find(|branch| branch.name == "main")
        .and_then(|branch| branch.tip.as_ref())
        .unwrap();
    assert_eq!(tip.parent_count, 2);
    assert_eq!(tip.action, TipAction::Merge);
}

#[test]
fn linked_worktree_gets_its_own_occupied_dock() {
    let (temp, repo) = seeded_repo();
    let worktree_path = temp.path().join("wt");
    git(
        &repo,
        &[
            "worktree",
            "add",
            worktree_path.to_str().unwrap(),
            "-b",
            "feature",
        ],
    );

    let harbor = to_harbor(&collect(&repo).unwrap());
    assert_eq!(harbor.docks.len(), 2);
    let feature = harbor.docks.iter().find(|d| d.name == "feature").unwrap();
    assert!(feature.vessel.is_some(), "worktree dock should be occupied");
    let main = harbor.docks.iter().find(|d| d.name == "main").unwrap();
    assert!(main.vessel.is_some(), "main worktree stays occupied");
    assert_eq!(main.kind, DockKind::Branch);
}

#[test]
fn detached_head_becomes_its_own_dock() {
    let (_temp, repo) = seeded_repo();
    git(&repo, &["checkout", "--detach"]);

    let snapshot = collect(&repo).unwrap();
    assert!(matches!(
        snapshot.workspaces[0].head,
        HeadState::Detached(_)
    ));

    let harbor = to_harbor(&snapshot);
    assert_eq!(harbor.docks.len(), 2);
    let detached = harbor
        .docks
        .iter()
        .find(|d| d.kind == DockKind::DetachedWorktree)
        .expect("a dock for the detached worktree");
    assert!(detached.name.starts_with('@'));
    let main = harbor.docks.iter().find(|d| d.name == "main").unwrap();
    assert_eq!(main.condition, Condition::Moored);
}

#[test]
fn commits_ahead_of_upstream_are_outbound() {
    let (temp, repo) = seeded_repo();
    let clone_path = temp.path().join("clone");
    git(
        temp.path(),
        &[
            "clone",
            repo.to_str().unwrap(),
            clone_path.to_str().unwrap(),
        ],
    );
    commit_file(&clone_path, "ahead.txt", "outbound\n", "local commit");

    let snapshot = collect(&clone_path).unwrap();
    assert_eq!(snapshot.default_branch.as_deref(), Some("main"));
    let main = &snapshot.branches[0];
    let sync = main.sync.as_ref().expect("clone has an upstream");
    assert_eq!((sync.ahead, sync.behind), (1, 0));

    let harbor = to_harbor(&snapshot);
    assert_eq!(harbor.docks[0].condition, Condition::Outbound);
    assert_eq!(harbor.docks[0].sync, Some((1, 0)));
}

#[test]
fn clean_branch_with_in_sync_upstream_is_calm() {
    let (temp, repo) = seeded_repo();
    let clone_path = temp.path().join("clone");
    git(
        temp.path(),
        &[
            "clone",
            repo.to_str().unwrap(),
            clone_path.to_str().unwrap(),
        ],
    );

    let harbor = to_harbor(&collect(&clone_path).unwrap());
    assert_eq!(harbor.docks[0].condition, Condition::Calm);
}

#[test]
fn unborn_repository_still_produces_a_harbor() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-b", "main"]);
    fs::write(repo.join("draft.txt"), "first cargo\n").unwrap();

    let snapshot = collect(&repo).unwrap();
    assert_eq!(snapshot.workspaces[0].head, HeadState::Unborn);
    assert!(snapshot.branches.is_empty());

    let harbor = to_harbor(&snapshot);
    assert_eq!(harbor.docks.len(), 1);
    assert_eq!(harbor.docks[0].condition, Condition::Loading);
}
