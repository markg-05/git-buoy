use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use super::model::{
    Check, CheckState, HostingSurvey, MergeState, PullRequest, Release, ReviewState,
};

const PR_FIELDS: &str = "number,title,headRefName,headRepository,headRepositoryOwner,isCrossRepository,url,isDraft,reviewDecision,mergeStateStatus,statusCheckRollup";
const RELEASE_FIELDS: &str = "tagName,name,isDraft,isPrerelease,isLatest,publishedAt";

/// Survey GitHub through the optional `gh` executable.
///
/// This is only called after the user opts in through `--github` or Harbor
/// Controls; the local Git collector and core harbor remain offline and do not
/// require `gh`.
pub fn collect_github(root: &Path) -> HostingSurvey {
    let prs = run_gh(
        root,
        &[
            "pr", "list", "--state", "open", "--limit", "100", "--json", PR_FIELDS,
        ],
    );
    let releases = run_gh(
        root,
        &["release", "list", "--limit", "20", "--json", RELEASE_FIELDS],
    );
    parse_survey(prs, releases)
}

fn run_gh(root: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let output = Command::new("gh")
        .args(args)
        .current_dir(root)
        .output()
        .with_context(|| "cannot run `gh`; install and authenticate GitHub CLI or omit --github")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("gh {} failed: {}", args[..2].join(" "), stderr.trim());
    }
    Ok(output.stdout)
}

fn parse_survey(prs: Result<Vec<u8>>, releases: Result<Vec<u8>>) -> HostingSurvey {
    HostingSurvey {
        pull_requests: prs
            .and_then(|json| {
                serde_json::from_slice::<Vec<RawPullRequest>>(&json)
                    .context("cannot parse pull requests returned by gh")
            })
            .map(|raw| raw.into_iter().map(PullRequest::from).collect())
            .map_err(|error| error.to_string()),
        releases: releases
            .and_then(|json| {
                serde_json::from_slice::<Vec<RawRelease>>(&json)
                    .context("cannot parse releases returned by gh")
            })
            .map(|raw| {
                raw.into_iter()
                    .filter(|release| !release.is_draft)
                    .map(Release::from)
                    .collect()
            })
            .map_err(|error| error.to_string()),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawPullRequest {
    number: u64,
    title: String,
    head_ref_name: String,
    head_repository: Option<RawRepository>,
    head_repository_owner: Option<RawRepositoryOwner>,
    is_cross_repository: bool,
    url: String,
    is_draft: bool,
    #[serde(default)]
    review_decision: String,
    #[serde(default)]
    merge_state_status: String,
    #[serde(default)]
    status_check_rollup: Vec<RawCheck>,
}

#[derive(Debug, Deserialize)]
struct RawRepository {
    name: String,
}

#[derive(Debug, Deserialize)]
struct RawRepositoryOwner {
    login: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawCheck {
    name: Option<String>,
    context: Option<String>,
    workflow_name: Option<String>,
    status: Option<String>,
    conclusion: Option<String>,
    state: Option<String>,
    details_url: Option<String>,
    target_url: Option<String>,
}

impl From<RawPullRequest> for PullRequest {
    fn from(raw: RawPullRequest) -> Self {
        let head_repository = raw
            .head_repository_owner
            .zip(raw.head_repository)
            .map(|(owner, repository)| format!("{}/{}", owner.login, repository.name));
        Self {
            number: raw.number,
            title: raw.title,
            head_branch: raw.head_ref_name,
            head_repository,
            is_cross_repository: raw.is_cross_repository,
            url: raw.url,
            is_draft: raw.is_draft,
            review: review_state(&raw.review_decision),
            merge: merge_state(&raw.merge_state_status, raw.is_draft),
            checks: raw
                .status_check_rollup
                .into_iter()
                .map(Check::from)
                .collect(),
        }
    }
}

impl From<RawCheck> for Check {
    fn from(raw: RawCheck) -> Self {
        let name = raw
            .name
            .or(raw.context)
            .or(raw.workflow_name)
            .unwrap_or_else(|| "unnamed check".to_string());
        let state = check_state(
            [
                raw.conclusion.as_deref(),
                raw.state.as_deref(),
                raw.status.as_deref(),
            ]
            .into_iter()
            .flatten()
            .find(|value| !value.is_empty())
            .unwrap_or(""),
        );
        Self {
            name,
            state,
            url: raw.details_url.or(raw.target_url),
        }
    }
}

fn review_state(value: &str) -> ReviewState {
    match value {
        "APPROVED" => ReviewState::Approved,
        "CHANGES_REQUESTED" => ReviewState::ChangesRequested,
        "REVIEW_REQUIRED" => ReviewState::Required,
        _ => ReviewState::None,
    }
}

fn merge_state(value: &str, draft: bool) -> MergeState {
    if draft {
        return MergeState::Blocked;
    }
    match value {
        "CLEAN" | "UNSTABLE" | "HAS_HOOKS" => MergeState::Ready,
        "BLOCKED" | "BEHIND" | "DIRTY" | "DRAFT" => MergeState::Blocked,
        _ => MergeState::Unknown,
    }
}

fn check_state(value: &str) -> CheckState {
    match value {
        "SUCCESS" | "NEUTRAL" | "SKIPPED" => CheckState::Passing,
        "FAILURE" | "ERROR" | "CANCELLED" | "TIMED_OUT" | "ACTION_REQUIRED" | "STARTUP_FAILURE"
        | "STALE" => CheckState::Failing,
        "PENDING" | "EXPECTED" | "QUEUED" | "IN_PROGRESS" | "WAITING" | "REQUESTED" => {
            CheckState::Pending
        }
        _ => CheckState::Unknown,
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawRelease {
    tag_name: String,
    name: String,
    is_draft: bool,
    is_prerelease: bool,
    is_latest: bool,
    published_at: Option<String>,
}

impl From<RawRelease> for Release {
    fn from(raw: RawRelease) -> Self {
        Self {
            tag: raw.tag_name,
            name: raw.name,
            is_latest: raw.is_latest,
            is_prerelease: raw.is_prerelease,
            published_at: raw.published_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pr_review_checks_and_release() {
        let prs = br#"[{"number":42,"title":"Ship it","headRefName":"feature/ship","headRepository":{"name":"git-buoy"},"headRepositoryOwner":{"login":"markg-05"},"isCrossRepository":false,"url":"https://example/pr/42","isDraft":false,"reviewDecision":"APPROVED","mergeStateStatus":"CLEAN","statusCheckRollup":[{"name":"test","status":"COMPLETED","conclusion":"SUCCESS","detailsUrl":"https://example/check"},{"context":"deploy","state":"PENDING","targetUrl":null}]}]"#;
        let releases = br#"[{"tagName":"v1.0.0","name":"One","isDraft":false,"isPrerelease":false,"isLatest":true,"publishedAt":"2026-07-13T00:00:00Z"}]"#;

        let survey = parse_survey(Ok(prs.to_vec()), Ok(releases.to_vec()));
        let pull_requests = survey.pull_requests.unwrap();
        let releases = survey.releases.unwrap();
        let pr = &pull_requests[0];
        assert_eq!(pr.head_branch, "feature/ship");
        assert_eq!(pr.head_repository.as_deref(), Some("markg-05/git-buoy"));
        assert!(!pr.is_cross_repository);
        assert_eq!(pr.review, ReviewState::Approved);
        assert_eq!(pr.merge, MergeState::Ready);
        assert_eq!(pr.checks[0].state, CheckState::Passing);
        assert_eq!(pr.checks[1].state, CheckState::Pending);
        assert_eq!(releases[0].tag, "v1.0.0");
    }

    #[test]
    fn parser_failures_are_independent() {
        let releases = br#"[{"tagName":"v1.0.0","name":"One","isDraft":false,"isPrerelease":false,"isLatest":true,"publishedAt":null}]"#;

        let survey = parse_survey(Ok(b"not json".to_vec()), Ok(releases.to_vec()));

        assert!(survey.pull_requests.unwrap_err().contains("pull requests"));
        assert_eq!(survey.releases.unwrap()[0].tag, "v1.0.0");
    }

    #[test]
    fn classifies_failures_and_drafts_as_blocked() {
        assert_eq!(check_state("FAILURE"), CheckState::Failing);
        assert_eq!(merge_state("CLEAN", true), MergeState::Blocked);
        assert_eq!(
            review_state("CHANGES_REQUESTED"),
            ReviewState::ChangesRequested
        );
    }
}
