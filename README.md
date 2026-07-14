# Git Buoy

[![CI](https://github.com/markg-05/git-buoy/actions/workflows/ci.yml/badge.svg)](https://github.com/markg-05/git-buoy/actions/workflows/ci.yml)
[![Latest release](https://img.shields.io/github/v/release/markg-05/git-buoy?display_name=tag&sort=semver)](https://github.com/markg-05/git-buoy/releases)
[![License: MIT](https://img.shields.io/badge/license-MIT-7ec091.svg)](LICENSE)

Parallel branches and linked worktrees are hard to understand as one live system. Git status can describe one checkout precisely, and commit graphs explain history, but neither makes current activity across a busy repository easy to see at a glance.

Git Buoy turns that current state into a terminal harbor. Branches and worktrees become docks, checked-out work becomes vessels, uncommitted changes become cargo, and sync or conflict states change how each vessel sits or moves. The scene is an overview, not a substitute for Git: press a key to inspect the exact branch, worktree, pull request, check, or changed path behind it.

![Git Buoy observing live cargo and paging through an overflowing harbor](docs/demo-motion.svg)

<sup>This current capture comes from the deterministic local demo fixture. Twelve changed paths load onto `demo/live-loading`, directional vessels move, and the overflowing harbor advances to its next page. The same capture respects reduced-motion preferences; a [static ambient frame](docs/demo.svg) is also available.</sup>

Git Buoy is local-first and agent-agnostic. The core view reads an ordinary local repository without a hosted service, GitHub account, or AI provider. Optional GitHub observation adds pull requests, reviews, checks, and published releases through the authenticated `gh` CLI.

## Try it in 60 seconds

With Git and a stable Rust toolchain installed:

```sh
git clone https://github.com/markg-05/git-buoy.git
cd git-buoy
./scripts/demo.sh setup
./scripts/demo.sh run
```

The script replaces only its marked `/tmp/git-buoy-demo` fixture. It creates a local bare origin, a main worktree, nine linked worktrees, and two moored branches using fixed demo identities and timestamps. It does not use the network, credentials, global Git configuration, or private repository data.

Press `i`, select a dirty dock with `j` or `k`, then press `Enter` twice to reach exact changed paths. In another terminal, run the following command while Git Buoy is open:

```sh
./scripts/demo.sh transition
```

Each call adds or removes twelve untracked paths in `demo/live-loading`; the next repository survey visibly loads or unloads that cargo.

## Installation

### Install with Cargo

With a stable Rust toolchain and Git installed:

```sh
cargo install git-buoy --version 0.1.0
git-buoy path/to/repository
```

### Download a release

Once v0.1.0 is published on GitHub, download the archive for your platform from
[GitHub Releases](https://github.com/markg-05/git-buoy/releases):

| Platform | Release asset |
| --- | --- |
| macOS on Apple Silicon | `git-buoy-v0.1.0-macos-aarch64.tar.gz` |
| Linux on x86-64 with glibc | `git-buoy-v0.1.0-linux-x86_64.tar.gz` |
| Windows on x86-64 | `git-buoy-v0.1.0-windows-x86_64.zip` |

Extract the archive, place `git-buoy` (or `git-buoy.exe`) somewhere on your
`PATH`, and check the installed version:

```sh
git-buoy --version
git-buoy path/to/repository
```

Git and an interactive terminal are required at runtime.

Each archive has a companion `.sha256` file. On Linux, verify it with
`sha256sum --check <archive>.sha256`; on macOS, use
`shasum -a 256 --check <archive>.sha256`. The release notes contain the
equivalent PowerShell command. v0.1 archives and checksums are not code-signed.

### Build from source

Git Buoy currently requires a stable [Rust toolchain](https://rustup.rs) and Git:

```sh
git clone https://github.com/markg-05/git-buoy.git
cd git-buoy
cargo run --release -- path/to/repository
```

With no path argument, Git Buoy observes the repository containing the current directory.

The crate is published on crates.io. Versioned GitHub archives are published
separately by the tagged release workflow.

### Optional GitHub state

Install and authenticate [GitHub CLI](https://cli.github.com/), then start with `--github`:

```sh
gh auth login
cargo run --release -- --github path/to/repository
```

The observer is off by default. When enabled, it runs independently from local Git collection; a GitHub failure appears in the footer but does not stop the local harbor.

## Keyboard use

| Key | Action |
| --- | --- |
| `i` or `Enter` | Enter inspect mode on the selected dock |
| `Enter` / right arrow | Drill from dock to vessel to exact changed files or checks |
| `Esc` / `h` / left arrow | Step back one inspection level; from ambient mode, exit |
| `Tab` / `Shift-Tab` | Select a dock |
| `j` / `k` / up/down arrows | Move within docks, files, pull requests, checks, controls, or the legend |
| `p` | Inspect pull requests on the selected dock |
| `s` | Open Harbor Controls |
| `l` or `?` | Toggle the legend; `l` adjusts a selected control while controls are open |
| `m` | Toggle reduced motion |
| `q` | Quit from any view |

Harbor Controls change motion, overflow paging, help, local survey timing, workspace idle timing, and the optional GitHub observer. Changes save immediately as global viewing preferences; the controls panel and footer both say `saved globally`. Explicit CLI flags override saved values for that run without rewriting them.

Useful flags include `--reduced-motion`, `--fps`, `--poll-interval`, `--idle-after`, `--github`, and `--github-poll-interval`. Run `cargo run --release -- --help` for their accepted values.

Preferences are stored at `$XDG_CONFIG_HOME/git-buoy/settings.json`, falling back to `$HOME/.config/git-buoy/settings.json`, on Unix-like systems and at `%APPDATA%\git-buoy\settings.json` on Windows. `GIT_BUOY_CONFIG` selects a specific file. The file contains viewing preferences only, never repository data or GitHub credentials.

## Reading the harbor

The metaphor communicates repository state; it is not decoration over a conventional Git graph.

| Git concept | Harbor representation |
| --- | --- |
| Repository | Harbor |
| Authoritative default branch | Main terminal |
| Branch or linked worktree | Dock |
| Checked-out workspace | Vessel |
| Uncommitted change | Cargo |
| Commit ready to push | Outbound vessel |
| Pull request | Vessel awaiting clearance |
| Check | Harbor inspection |
| Merge conflict or Git operation | Blocked shipping lane |
| Published release | Departing convoy |

Each dock also carries a plain-language condition so the view does not depend on color or motion alone:

| Condition | Git state |
| --- | --- |
| `calm` | Checked out, committed, and in sync with the upstream |
| `local` | Checked out with no upstream |
| `loading` | Unstaged or untracked changes |
| `sealed` | Staged changes |
| `outbound` | Commits ahead of the upstream |
| `incoming` | Commits behind the upstream |
| `diverged` | Local and upstream histories both have unique commits |
| `awaiting` | Remote-only pull-request branch |
| `blocked` | Conflict or in-progress Git operation |
| `moored` | Branch with no checked-out worktree |

Occupied workspaces begin as `observing`, become `recent` after Git Buoy sees a change, and become `idle` after the configured quiet period. These labels describe observed repository activity, not whether a person or process is present.

Reduced motion collapses transitions immediately and pauses overflow cycling. Conditions, arrows, activity words, cargo counts, and inspect mode remain available in a fixed frame.

![Inspect mode showing exact changed paths over the harbor](docs/inspect.svg)

<sup>Inspect mode shows the real categories and paths returned by the collector while keeping the harbor visible on terminals wide enough to hold both.</sup>

## Architecture

The conceptual boundary is `git → harbor → app → ui`, with hosting as an optional input to the application:

| Layer | Responsibility | Boundary |
| --- | --- | --- |
| [`src/git/`](src/git/) | Read one repository with git2 | Produces a plain `RepoSnapshot`; git2 types do not escape |
| [`src/harbor/`](src/harbor/) | Map repository facts to `Harbor`, `Dock`, and `Vessel`; own deterministic animation time | Pure scene data; no git2 or ratatui types |
| [`src/app.rs`](src/app.rs) | Receive snapshots, apply activity and live-event semantics, combine optional hosting state, and own mode/selection/settings transitions | Deterministic state machine over messages |
| [`src/ui/`](src/ui/) | Render the current application state and terminal overlays with ratatui | All terminal-specific types remain here |
| [`src/hosting/`](src/hosting/) | Optionally survey GitHub through `gh` | Produces a plain `HostingSurvey`; provider failures are non-fatal |

In the update loop, the application asks the harbor layer to map each incoming repository snapshot, enriches that scene with observed events and optional hosting data, then gives the resulting state to the UI. The executable in [`src/main.rs`](src/main.rs) owns polling, terminal setup, configuration I/O, and message delivery.

The stack rationale and rejected alternatives are recorded in [ADR 0001](docs/adr/0001-implementation-stack.md).

## Testing and recorded evidence

CI runs the same required checks on Linux, macOS, and Windows:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

The test suite covers collector edge cases, snapshot-to-harbor mapping, state transitions, animation, settings persistence, hosting parsing, and ratatui output. Integration tests construct disposable repositories with the Git CLI; no developer repository is used as a fixture.

Two slower gates support claims that ordinary unit tests cannot:

- [Release acceptance](docs/release-acceptance.md) exercises packaged executables in native pseudo-terminals across real Git states, terminal sizes, color modes, reduced motion, keyboard navigation, saved settings, and optional-GitHub failure paths. macOS and Linux pass the full 20-row suite; the non-interactive hosted Windows runner verifies archive extraction, help, and version output.
- [Idle resource profiling](docs/profiling.md) records the method, budgets, machine context, and raw JSON for CPU, resident memory, and repository-survey latency. All 24 recorded macOS and Debian Bookworm runs are within the published budgets; the document links the exact baseline files.

These records are the compatibility and performance evidence. Git Buoy does not claim behavior on an unrecorded platform or terminal solely because it compiles there.

## Limitations

- Git Buoy observes one local repository at a time. It complements Git commands; it does not stage, commit, merge, rebase, or push.
- Live events are inferred from consecutive surveys, topology, and local reflog evidence. The first survey never replays history, changes that begin and end between polls can be missed, squash merges look like commits, and some reference movement can only be called `updated`. The full evidence table is in [live-event semantics](docs/live-events.md).
- GitHub is the only hosting adapter. It requires an authenticated `gh` executable and makes network requests only when the observer is enabled. Pull-request/check and release surveys can fail independently, and local observation continues.
- A pull request attaches to a local dock only when GitHub reports that its head belongs to the same repository. Fork heads and other remote heads become separate awaiting docks; deleted fork metadata may fall back to pull-request identity.
- The hosted Windows runner cannot exercise the interactive terminal suite. Git Buoy has no installer, package-manager formula, or signed binary yet.
- ANSI 16-color mode inherits the user's terminal palette, so exact contrast cannot be guaranteed. Condition words remain the authoritative non-color fallback.

## Releases and support

Version `0.1.0` is published on crates.io. There is no public GitHub release
yet; a matching tag runs the format, Clippy, test, packaging, terminal smoke,
and checksum gates described in [Publishing a release](docs/releasing.md). When
the GitHub release is published, the badge at the top of this README and the
[Releases page](https://github.com/markg-05/git-buoy/releases) update
automatically.

Until then, install the published crate or build from source at a reviewed
commit. Report reproducible defects and accessibility or compatibility findings
in [GitHub Issues](https://github.com/markg-05/git-buoy/issues). This is an early
project maintained on a best-effort basis; there is no commercial support
commitment or private security-response channel.

## Contributing

Public contribution setup, change expectations, and pull-request checks are in [CONTRIBUTING.md](CONTRIBUTING.md). Coding agents should additionally read [AGENTS.md](AGENTS.md), which contains agent-oriented repository constraints and execution guidance.

To regenerate the README captures from the current collector, state machine, and ratatui renderer:

```sh
./scripts/demo.sh capture
```

The command rebuilds the deterministic fixture, records a real cargo transition, writes `docs/demo.svg`, `docs/demo-motion.svg`, and `docs/inspect.svg`, restores the changed worktree, and rejects private-looking local paths in the generated files.

## License

Git Buoy is open-source software licensed under the [MIT License](LICENSE).
