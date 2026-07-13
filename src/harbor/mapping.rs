use crate::git::{BranchInfo, HeadState, RepoSnapshot, Workspace};

use super::model::{Condition, Dock, DockKind, Harbor, Vessel, VesselActivity};

/// Build the harbor scene for a snapshot. Pure: the same snapshot always
/// produces the same scene.
pub fn to_harbor(snapshot: &RepoSnapshot) -> Harbor {
    to_harbor_with_activity(snapshot, |_| VesselActivity::Observing)
}

pub(crate) fn to_harbor_with_activity(
    snapshot: &RepoSnapshot,
    activity_for: impl Fn(&Workspace) -> VesselActivity,
) -> Harbor {
    let mut docks = Vec::new();

    for branch in &snapshot.branches {
        let workspace = snapshot
            .workspaces
            .iter()
            .find(|w| matches!(&w.head, HeadState::Branch(name) if name == &branch.name));
        docks.push(branch_dock(
            snapshot,
            branch,
            workspace,
            workspace.map_or(VesselActivity::Observing, &activity_for),
        ));
    }

    // Workspaces not sitting on a local branch still deserve a dock.
    for workspace in &snapshot.workspaces {
        match &workspace.head {
            HeadState::Branch(_) => {}
            HeadState::Detached(id) => {
                docks.push(headless_dock(
                    format!("@{id}"),
                    DockKind::DetachedWorktree,
                    "detached HEAD",
                    workspace,
                    activity_for(workspace),
                ));
            }
            HeadState::Unborn => {
                docks.push(headless_dock(
                    "(no commits yet)".to_string(),
                    DockKind::Branch,
                    "unborn HEAD",
                    workspace,
                    activity_for(workspace),
                ));
            }
        }
    }

    // Main terminal first, then occupied docks, then moored branches.
    docks.sort_by(|a, b| {
        let rank = |d: &Dock| (d.kind != DockKind::MainTerminal, d.vessel.is_none());
        rank(a).cmp(&rank(b)).then_with(|| a.name.cmp(&b.name))
    });

    Harbor {
        name: snapshot.name.clone(),
        docks,
    }
}

fn branch_dock(
    snapshot: &RepoSnapshot,
    branch: &BranchInfo,
    workspace: Option<&Workspace>,
    activity: VesselActivity,
) -> Dock {
    let kind = if snapshot.default_branch.as_deref() == Some(branch.name.as_str()) {
        DockKind::MainTerminal
    } else {
        DockKind::Branch
    };
    let vessel = workspace.map(|workspace| vessel_for(workspace, activity));
    let sync = branch.sync.as_ref().map(|s| (s.ahead, s.behind));
    let condition = condition_for(vessel.as_ref(), sync, workspace.and_then(|w| w.operation));

    let mut detail: Vec<(&'static str, String)> = vec![("branch", branch.name.clone())];
    match &branch.sync {
        Some(s) => {
            detail.push(("upstream", s.upstream.clone()));
            detail.push(("ahead / behind", format!("{} / {}", s.ahead, s.behind)));
        }
        None => detail.push(("upstream", "none".to_string())),
    }
    match workspace {
        Some(w) => push_workspace_detail(&mut detail, w, activity),
        None => detail.push(("workspace", "not checked out".to_string())),
    }
    if let Some(summary) = &branch.last_commit {
        detail.push(("last commit", summary.clone()));
    }

    Dock {
        name: branch.name.clone(),
        kind,
        condition,
        vessel,
        sync,
        detail,
    }
}

fn headless_dock(
    name: String,
    kind: DockKind,
    head_note: &'static str,
    workspace: &Workspace,
    activity: VesselActivity,
) -> Dock {
    let vessel = vessel_for(workspace, activity);
    let condition = condition_for(Some(&vessel), None, workspace.operation);
    let mut detail: Vec<(&'static str, String)> = vec![("head", head_note.to_string())];
    push_workspace_detail(&mut detail, workspace, activity);
    Dock {
        name,
        kind,
        condition,
        vessel: Some(vessel),
        sync: None,
        detail,
    }
}

fn push_workspace_detail(
    detail: &mut Vec<(&'static str, String)>,
    workspace: &Workspace,
    activity: VesselActivity,
) {
    detail.push(("workspace", workspace.path.display().to_string()));
    detail.push(("activity", activity.label().to_string()));
    detail.push(("staged", workspace.changes.staged.to_string()));
    detail.push(("unstaged", workspace.changes.unstaged.to_string()));
    detail.push(("untracked", workspace.changes.untracked.to_string()));
    detail.push(("conflicted", workspace.changes.conflicted.to_string()));
    if let Some(operation) = workspace.operation {
        detail.push(("operation", operation.label().to_string()));
    }
}

fn vessel_for(workspace: &Workspace, activity: VesselActivity) -> Vessel {
    Vessel {
        staged: workspace.changes.staged,
        unstaged: workspace.changes.unstaged,
        untracked: workspace.changes.untracked,
        conflicted: workspace.changes.conflicted,
        activity,
    }
}

fn condition_for(
    vessel: Option<&Vessel>,
    sync: Option<(usize, usize)>,
    operation: Option<crate::git::Operation>,
) -> Condition {
    let Some(vessel) = vessel else {
        return Condition::Moored;
    };
    if vessel.conflicted > 0 || operation.is_some() {
        Condition::Blocked
    } else if vessel.staged > 0 {
        Condition::Sealed
    } else if vessel.unstaged > 0 || vessel.untracked > 0 {
        Condition::Loading
    } else {
        match sync {
            Some((ahead, behind)) if ahead > 0 && behind > 0 => Condition::Diverged,
            Some((ahead, _)) if ahead > 0 => Condition::Outbound,
            Some((_, behind)) if behind > 0 => Condition::Incoming,
            Some(_) => Condition::Calm,
            None => Condition::Local,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::git::{
        BranchInfo, ChangeCounts, HeadState, Operation, RepoSnapshot, SyncState, Workspace,
    };

    use super::*;

    fn workspace(head: HeadState, changes: ChangeCounts) -> Workspace {
        Workspace {
            path: PathBuf::from("/tmp/repo"),
            is_main: true,
            head,
            changes,
            activity_token: 0,
            operation: None,
        }
    }

    fn branch(name: &str) -> BranchInfo {
        BranchInfo {
            name: name.to_string(),
            sync: None,
            last_commit: Some("initial".to_string()),
        }
    }

    fn snapshot(branches: Vec<BranchInfo>, workspaces: Vec<Workspace>) -> RepoSnapshot {
        RepoSnapshot {
            name: "harbor-test".to_string(),
            default_branch: Some("main".to_string()),
            branches,
            workspaces,
        }
    }

    #[test]
    fn condition_priority_puts_blocked_first() {
        let busy = Vessel {
            staged: 2,
            unstaged: 3,
            untracked: 1,
            conflicted: 1,
            ..Vessel::default()
        };
        assert_eq!(
            condition_for(Some(&busy), Some((5, 0)), None),
            Condition::Blocked
        );
        let staged = Vessel {
            conflicted: 0,
            ..busy
        };
        assert_eq!(
            condition_for(Some(&staged), Some((5, 0)), None),
            Condition::Sealed
        );
        let dirty = Vessel {
            staged: 0,
            ..staged
        };
        assert_eq!(
            condition_for(Some(&dirty), Some((5, 0)), None),
            Condition::Loading
        );
        let clean = Vessel::default();
        assert_eq!(
            condition_for(Some(&clean), Some((5, 0)), None),
            Condition::Outbound
        );
        assert_eq!(
            condition_for(Some(&clean), Some((0, 2)), None),
            Condition::Incoming
        );
        assert_eq!(
            condition_for(Some(&clean), Some((2, 1)), None),
            Condition::Diverged
        );
        assert_eq!(
            condition_for(Some(&clean), Some((0, 0)), None),
            Condition::Calm
        );
        assert_eq!(condition_for(Some(&clean), None, None), Condition::Local);
        assert_eq!(condition_for(None, Some((5, 0)), None), Condition::Moored);
    }

    #[test]
    fn operation_in_progress_blocks_even_without_conflicts() {
        let clean = Vessel::default();
        assert_eq!(
            condition_for(Some(&clean), None, Some(Operation::Rebase)),
            Condition::Blocked
        );
    }

    #[test]
    fn main_terminal_sorts_first_then_occupied_then_moored() {
        let snap = snapshot(
            vec![branch("aaa-idle"), branch("feature"), branch("main")],
            vec![workspace(
                HeadState::Branch("feature".to_string()),
                ChangeCounts::default(),
            )],
        );
        let harbor = to_harbor(&snap);
        let names: Vec<&str> = harbor.docks.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, vec!["main", "feature", "aaa-idle"]);
        assert_eq!(harbor.docks[0].kind, DockKind::MainTerminal);
        assert!(harbor.docks[1].vessel.is_some());
        assert_eq!(harbor.docks[2].condition, Condition::Moored);
    }

    #[test]
    fn detached_workspace_gets_its_own_dock() {
        let snap = snapshot(
            vec![branch("main")],
            vec![workspace(
                HeadState::Detached("abc12345".to_string()),
                ChangeCounts::default(),
            )],
        );
        let harbor = to_harbor(&snap);
        let detached = harbor
            .docks
            .iter()
            .find(|d| d.kind == DockKind::DetachedWorktree)
            .expect("detached dock");
        assert_eq!(detached.name, "@abc12345");
        // The main branch itself is unoccupied.
        assert_eq!(harbor.docks[0].condition, Condition::Moored);
    }

    #[test]
    fn upstream_sync_is_carried_onto_the_dock() {
        let mut with_upstream = branch("main");
        with_upstream.sync = Some(SyncState {
            upstream: "origin/main".to_string(),
            ahead: 2,
            behind: 1,
        });
        let snap = snapshot(
            vec![with_upstream],
            vec![workspace(
                HeadState::Branch("main".to_string()),
                ChangeCounts::default(),
            )],
        );
        let harbor = to_harbor(&snap);
        assert_eq!(harbor.docks[0].sync, Some((2, 1)));
        assert_eq!(harbor.docks[0].condition, Condition::Diverged);
    }
}
