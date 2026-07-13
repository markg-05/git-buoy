# ADR 0001: Implementation stack

- Status: accepted
- Date: 2026-07-12

## Context

Git Buoy needs an implementation stack for an animated, keyboard-driven
terminal application that observes local Git repositories. `AGENTS.md`
requires the choice to weigh terminal rendering quality and Unicode support,
animation timing and performance, cross-platform behavior, structured access
to Git state, testability of the state mapping independent of rendering,
distribution as a small executable, and graceful degradation (reduced motion,
limited color).

## Options considered

1. **Go with Bubble Tea, Lip Gloss, and go-git.** The most common stack for
   polished modern TUIs. Elm-style architecture separates state and rendering
   well, and cross-compilation is easy. Strong option; not chosen mainly on
   preference and the desire for the smallest, fastest binary.
2. **Rust with ratatui, crossterm, and git2.** The gitui/atuin lineage.
   Immediate-mode rendering suits a continuously animated scene, libgit2
   bindings expose branches, worktrees, statuses, and ahead/behind counts as
   structured data, and release builds are small static binaries on all three
   major platforms.
3. **TypeScript with Ink.** Fastest prototyping, but requires a Node runtime,
   which conflicts with distribution as a small straightforward executable.
4. **Python with Textual.** Excellent animation engine, but distribution and
   idle resource use are the weakest of the four.

## Decision

Rust, with:

- `ratatui` + `crossterm` for rendering and input.
- `git2` with default features disabled (no network transports; the core
  product is local-first and only ever reads local repositories).
- `clap` for the CLI, `anyhow`/`thiserror` for errors.

Supporting decisions made at the same time:

- **Layering:** repository collection (`src/git/`), scene model and mapping
  (`src/harbor/`), and rendering (`src/ui/`) are separate modules; the
  snapshot-to-scene mapping is a pure function tested without a terminal.
- **Animation:** a tick-driven frame counter owned by the scene model, so
  animation is deterministic under test and reduced motion means "stop
  feeding ticks".
- **State updates:** poll the repository on an interval from a background
  thread. Filesystem watching is a possible later refinement; polling is the
  smallest design that keeps the render loop independent of repository reads.
- **Rendering style:** character-cell scenes using widely supported Unicode
  block and wave glyphs, with a compact plain fallback for narrow terminals.
  The project is not strictly ASCII.

## Consequences

- Contributors need a Rust toolchain (`rustup`, stable channel); build and
  check commands are recorded in `AGENTS.md`.
- `git2` builds libgit2 from source via `cc`, which slows the first build but
  keeps runtime dependencies at zero.
- Remote-hosting features in later milestones (pull requests, CI checks) must
  arrive as a separate layer, since the core intentionally has no network
  transport compiled in.
- Reverting the pure-Rust alternative `gix` remains possible behind the
  `src/git/` boundary if libgit2 becomes a limitation.
