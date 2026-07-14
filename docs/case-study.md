# Git Buoy project case study

Git Buoy is an independently designed and implemented open-source terminal
application for understanding concurrent work across Git branches and linked
worktrees. It explores whether a spatial, animated overview can make a busy
repository legible without replacing the exact Git details developers rely on.

## The problem

Git status describes one checkout well, and commit graphs explain repository
history. Neither is designed to answer a more immediate question: what is
happening across all of the active workspaces in this repository right now?

That gap becomes visible when several branches or linked worktrees are active.
Developers must mentally combine branch names, worktree paths, changed files,
staging state, upstream divergence, conflicts, pull requests, and checks. A
conventional dashboard can list those facts, but it does not necessarily make
the system understandable at a glance from a spare terminal pane.

## Product approach

Git Buoy maps one observed repository to a harbor. Branches and worktrees become
docks, checked-out workspaces become vessels, and uncommitted changes become
cargo. Vessel direction and dock conditions communicate upstream synchronization
and blocked operations. The metaphor is constrained by real repository state;
when it would obscure meaning, inspect mode exposes plain Git terminology,
exact paths, counts, pull requests, and checks.

The result supports two complementary uses:

- **Ambient mode** provides a passive overview that can remain open in a
  terminal pane.
- **Inspect mode** provides keyboard-driven access to the exact evidence behind
  the scene.

## Engineering constraints

Several constraints shaped the design from the beginning:

- The core workflow must remain local-first and work without GitHub, an AI
  provider, or any hosted service.
- Animation must explain state changes without delaying current truth, and it
  must be deterministic under test.
- Color and motion cannot be the only carriers of meaning.
- Unusual paths, detached or unborn heads, missing remotes, large repositories,
  and incomplete Git operations are ordinary input rather than exceptional
  cases.
- An ambient application must be quiet enough in CPU and memory use to leave
  running.

## Key decisions

| Decision | Reason | Result |
| --- | --- | --- |
| Rust with ratatui, crossterm, and git2 | Produce a small terminal executable, access repository state without parsing human-oriented command output, and keep rendering cross-platform | One native executable with explicit collection and rendering boundaries |
| Plain snapshots between layers | Prevent git2 and terminal types from coupling the information model to collection or rendering | Repository collection, harbor mapping, application state, and UI can be tested independently |
| Deterministic frame clock | Make motion reproducible and ensure reduced motion can resolve immediately to truthful final state | Animation and transition behavior are covered by state and rendering tests |
| Optional `gh` adapter | Add pull requests, checks, reviews, and releases without making hosted state a core dependency | GitHub failures are visible but non-fatal; the local harbor continues operating |
| Disposable demo and acceptance fixtures | Demonstrate complex states without exposing a developer repository or relying on network data | README captures and release tests are reproducible from generated local repositories |

The implementation-stack alternatives and tradeoffs are recorded in
[ADR 0001](adr/0001-implementation-stack.md).

## Verification

The normal CI gate runs formatting, Clippy with warnings denied, and the full
test suite on Linux, macOS, and Windows. Tests cover repository collection,
snapshot mapping, application state transitions, animation, settings,
provider parsing, and terminal rendering.

Two additional release gates cover behavior that unit tests alone cannot:

- [Native release acceptance](release-acceptance.md) launches packaged
  executables in pseudo-terminals against disposable repositories covering Git
  states, terminal sizes, color capabilities, reduced motion, keyboard
  navigation, persistence, and optional-provider failures.
- [Idle resource profiling](profiling.md) records CPU, resident memory, and
  repository-survey latency against published budgets and retains the raw
  measurement data.

## Outcome and boundaries

Version 0.1.0 is distributed through GitHub release archives, crates.io, and an
Apple Silicon Homebrew formula. The core application observes an ordinary
local repository without credentials or network access, while inspect mode
keeps the visual overview accountable to exact repository facts.

The project intentionally does not perform Git operations, replay complete
repository history, or observe multiple repositories at once. The most useful
future work is expected to come from real compatibility findings and continued
evaluation of whether each part of the harbor metaphor improves comprehension.
