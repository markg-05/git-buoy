# v0.1 release acceptance

This is the release gate for the assembled Git Buoy executable. It complements
the fast CI checks and the [idle-resource profile](profiling.md) by exercising a
release build in a native pseudo-terminal against disposable Git repositories.

## Current decision

**Blocked pending the native Windows run.** The packaged macOS and Linux
artifacts passed all 20 automated rows on July 13, 2026. The Windows row is
implemented in the release-acceptance workflow, but it has not run for the
commit containing this document. Do not tag v0.1 until that workflow reports
`20 passed, 0 failed` on `windows-latest` and this section records its artifact
hash and run link.

There are no failed macOS or Linux rows and no open defect from those runs. One
acceptance finding was fixed in the candidate: Harbor Controls persisted
changes globally while its footer incorrectly said `session only`. The footer
now says `saved globally`, matching the panel and behavior.

Git Buoy does not yet have signing or installer packaging. For this gate, the
final artifacts are the platform-native optimized executables in the `.tar.gz`
or `.zip` produced by
[`release-acceptance.yml`](../.github/workflows/release-acceptance.yml). The
workflow extracts each archive and passes that extracted executable—not the
debug build or pre-archive path—to the acceptance runner before uploading the
same archive.

## Recorded results

Both completed runs used Rust 1.97.0 and the source tree that contains this
matrix, the acceptance runner, and the Harbor Controls footer fix.

| Platform | Environment | Final artifact | Result |
| --- | --- | --- | --- |
| macOS | Darwin 25.3.0, arm64; Git 2.50.1 | `git-buoy-macOS-ARM64.tar.gz` | **Pass — 20/20** |
| Linux | Debian Bookworm container, arm64; Git 2.39.5; pinned image digest below | `git-buoy-Linux-ARM64.tar.gz` | **Pass — 20/20** |
| Windows | `windows-latest`, native ConPTY | `git-buoy-Windows-*.zip` | **Not run — release blocker** |

Artifact digests from the completed runs:

| Platform | File | SHA-256 |
| --- | --- | --- |
| macOS | Archive | `f22b214384005d295253e9b753ef42abdc2a75f50a9f7851961563870513e821` |
| macOS | Extracted executable | `148c605d3eb4e7d8b29fc3a4ed3be5db1054c7b39678768b7dc2ae200dca5907` |
| Linux | Archive | `3cac7f3931578eba70b782efa0366d1aa00feabc2c8704020e1982e10328e45b` |
| Linux | Extracted executable | `a92cba5531bc239ee0a5f55b5a36d40f24e578f9657cab04cbc54803e1d60dbe` |

The two completed logs ended with:

```text
summary: 20 passed, 0 failed
```

## Acceptance matrix

The standalone runner is
[`tools/release-acceptance`](../tools/release-acceptance). It uses
`portable-pty` only in that tool, so the normal application dependency graph
and fast CI path are unchanged. On Windows it uses the native ConPTY backend.

### Real Git states

The runner creates a local bare origin, a primary clone, a publisher clone,
ten linked worktrees, and separate keyboard, unborn, and clean-operation
repositories. It uses fixed local identities and no network remotes.

| Required state | Fixture and proof | Expected observation |
| --- | --- | --- |
| Local/no upstream | `local-only`; no upstream is configured | `local` |
| Calm | `main...origin/main` is `0 0` | `calm` |
| Loading | Three unstaged/untracked paths | `loading` |
| Sealed | Two staged paths | `sealed` |
| Ahead | `outbound...origin/outbound` is `1 0` | `↑1 outbound` |
| Behind | `incoming...origin/incoming` is `0 1` | `↓1 incoming` |
| Diverged | `diverged...origin/diverged` is `1 1` | `↑1 ↓1 diverged` |
| Conflict and in-progress merge | `UU shared.txt` and a resolvable `MERGE_HEAD` | `blocked` |
| Operation without a conflict | Clean repository after `git bisect start HEAD HEAD~4` | `blocked`; inspect says `bisect in progress` |
| Detached HEAD | Linked detached worktree | `@<short-id> · detached` |
| Unborn repository | Fresh `git init --initial-branch=main` | `(no commits yet)` and `local` |
| Linked worktrees | Main workspace plus ten linked worktrees | Occupied docks and truthful overflow pages |

Ahead and behind are Git state names; the UI calls them `outbound` and
`incoming`. Detached and unborn identify docks rather than introducing new
conditions. Condition priority remains blocked, sealed, loading, then sync
state, so the ahead/behind/diverged fixtures are intentionally clean.

The runner also checks these exact Git facts before launching Git Buoy:

```sh
git -C "$HARBOR" rev-list --left-right --count main...origin/main
git -C "$HARBOR" rev-list --left-right --count outbound...origin/outbound
git -C "$HARBOR" rev-list --left-right --count incoming...origin/incoming
git -C "$HARBOR" rev-list --left-right --count diverged...origin/diverged
git -C "$WORKTREES/blocked" status --short
git -C "$WORKTREES/blocked" rev-parse MERGE_HEAD
git -C "$HARBOR" worktree list --porcelain
```

The expected sync results are `0 0`, `1 0`, `0 1`, and `1 1`, respectively.

### Terminal and keyboard behavior

| Row | Automated evidence |
| --- | --- |
| Narrow | Launches at 44×16, reaches the harbor, accepts `q`, exits 0, and restores the terminal |
| Normal | Same at 80×24 |
| Wide | Same at 120×40 |
| Short | Same at 80×6; overflow remains represented |
| ANSI 16-color | `TERM=xterm`, no `COLORTERM`; ANSI palette SGR is emitted and condition words remain present |
| 256-color | `TERM=xterm-256color`; indexed palette SGR is emitted and condition words remain present |
| Truecolor | `TERM=xterm-256color`, `COLORTERM=truecolor`; RGB palette SGR is emitted and condition words remain present |
| Reduced motion | `reduced motion` is visible; after settling, 700 ms contains no visible cell changes; facts and cargo remain |
| Overflow pages | A short ambient view advances to a later dock; the same view holds page one under reduced motion |
| Inspect navigation | `i`, `Enter`, `Enter` reaches exact `tracked.txt` and `untracked.txt` changed-file rows; `j`/`k` remains responsive |
| Legend | `l` opens the legend; `j`/`k` scroll; `Esc` closes only the legend |
| Settings | `s` opens Harbor Controls; arrow adjustment changes Motion and writes the disposable settings file immediately |
| Settings persistence | A second process with the same settings file starts in reduced motion |
| Short controls | At 60×7, selection wraps to the final GitHub survey row and remains visible |
| Escape and quit | Layered `Esc` returns files → vessel → dock → ambient → exit; direct `q` exits every size row |

The palette checks prove capability selection and hue-independent labels. ANSI
colors inherit the user's terminal theme, so no automated test can guarantee
contrast for every custom palette; the exact condition words remain the
authoritative fallback. The reduced-motion quietness row checks screen
stability. CPU, memory, and survey duty remain covered by
[the release profiling gate](profiling.md).

### Optional GitHub observation

The runner installs a deterministic `gh` stand-in earlier on `PATH`. The
stand-in is the acceptance executable copied to `gh`/`gh.exe`, so the same
procedure works without shell scripts on all three platforms.

| Row | Automated evidence |
| --- | --- |
| Observer off | Fresh settings, no `--github`; the invocation sentinel is absent after launch and quit |
| Observer on, happy path | Exactly one `gh pr list` and one `gh release list`; a PR/check and `v0.1.0` convoy render |
| `gh` missing | Empty `PATH`; footer reports `cannot run gh`, local calm docks remain, navigation and quit work |
| `gh` unauthenticated | Stand-in exits 4 with `authentication required`; footer reports it, local calm docks remain, quit exits 0 |

These rows intentionally test Git Buoy's adapter contract without network or
credentials. A live GitHub account is not required for local release
acceptance.

## Exact commands

Run from the repository root. The runner refuses to replace a fixture directory
unless its marker file proves that it created the directory.

### macOS

```sh
cargo build --locked --release

ARTIFACT_ROOT=/tmp/git-buoy-release-artifact
mkdir -p "$ARTIFACT_ROOT/stage" "$ARTIFACT_ROOT/smoke"
cp target/release/git-buoy "$ARTIFACT_ROOT/stage/git-buoy"
tar -C "$ARTIFACT_ROOT/stage" \
  -czf "$ARTIFACT_ROOT/git-buoy-macOS-ARM64.tar.gz" git-buoy
tar -C "$ARTIFACT_ROOT/smoke" \
  -xzf "$ARTIFACT_ROOT/git-buoy-macOS-ARM64.tar.gz"

cargo run --locked \
  --manifest-path tools/release-acceptance/Cargo.toml \
  -- \
  --binary "$ARTIFACT_ROOT/smoke/git-buoy"

shasum -a 256 \
  "$ARTIFACT_ROOT/git-buoy-macOS-ARM64.tar.gz" \
  "$ARTIFACT_ROOT/smoke/git-buoy"
```

Use a new `ARTIFACT_ROOT` or remove only a prior disposable acceptance
directory before reproducing the run.

### Linux in the pinned Debian container

The recorded container image is
`rust:1.97-bookworm@sha256:7d0723df719e7f213b69dc7c8c595985c3f4b060cfbee4f7bc0e347a86fe3b6a`.

```sh
mkdir -p /tmp/git-buoy-linux-artifact

docker run --rm \
  -v "$PWD:/work" \
  -v /tmp/git-buoy-linux-artifact:/output \
  -w /work \
  -e CARGO_TARGET_DIR=/output/target \
  rust:1.97-bookworm@sha256:7d0723df719e7f213b69dc7c8c595985c3f4b060cfbee4f7bc0e347a86fe3b6a \
  bash -c 'cargo build --locked --release && \
    mkdir -p /output/stage /output/smoke && \
    cp /output/target/release/git-buoy /output/stage/git-buoy && \
    tar -C /output/stage \
      -czf /output/git-buoy-Linux-ARM64.tar.gz git-buoy && \
    tar -C /output/smoke \
      -xzf /output/git-buoy-Linux-ARM64.tar.gz && \
    cargo run --locked \
      --manifest-path tools/release-acceptance/Cargo.toml \
      -- --binary /output/smoke/git-buoy && \
    sha256sum \
      /output/git-buoy-Linux-ARM64.tar.gz \
      /output/smoke/git-buoy'
```

### Windows PowerShell

Run this on native Windows, not WSL:

```powershell
cargo build --locked --release

$ArtifactRoot = Join-Path $env:TEMP "git-buoy-release-artifact"
New-Item -ItemType Directory -Force `
  "$ArtifactRoot/stage", "$ArtifactRoot/smoke" | Out-Null
Copy-Item target/release/git-buoy.exe "$ArtifactRoot/stage/git-buoy.exe"
Compress-Archive `
  -Path "$ArtifactRoot/stage/git-buoy.exe" `
  -DestinationPath "$ArtifactRoot/git-buoy-Windows.zip"
Expand-Archive `
  -Path "$ArtifactRoot/git-buoy-Windows.zip" `
  -DestinationPath "$ArtifactRoot/smoke"

cargo run --locked `
  --manifest-path tools/release-acceptance/Cargo.toml `
  -- `
  --binary "$ArtifactRoot/smoke/git-buoy.exe"

Get-FileHash -Algorithm SHA256 `
  "$ArtifactRoot/git-buoy-Windows.zip", `
  "$ArtifactRoot/smoke/git-buoy.exe"
```

### Three-platform workflow

After the commit is available on GitHub, run the manual workflow:

```sh
gh workflow run release-acceptance.yml --ref <candidate-commit-or-branch>
gh run list --workflow release-acceptance.yml --limit 1
gh run watch <run-id> --exit-status
```

Download and inspect all three `release-acceptance.log` files. Every job must
end with `summary: 20 passed, 0 failed`. Record the Windows archive and
executable SHA-256 values in this document, link the successful workflow run,
and change the current decision to **Accepted** before tagging v0.1.

If any row fails, do not upload or tag that candidate as a release. Fix and
rerun it, or link a blocking issue here with the platform, fixture, exact row,
captured output, and intended resolution.
