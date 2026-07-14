# AGENTS.md

This file defines operational expectations for coding agents contributing to Git Buoy. Human contributors should start with [CONTRIBUTING.md](CONTRIBUTING.md); the product and quality constraints below still apply to every change. These instructions apply to the entire repository unless a more specific `AGENTS.md` is added in a subdirectory later.

## Start here

Read `README.md` before proposing or implementing changes. The README is the current source of truth for the product intent, scope, metaphor, and non-goals.

Git Buoy is a Rust application built with ratatui. The stack decision and its rationale are recorded in [docs/adr/0001-implementation-stack.md](docs/adr/0001-implementation-stack.md). Do not add new dependencies, services, or generated assets unless the task calls for them.

## Product guardrails

Changes should preserve these distinctions:

- Git Buoy is a live view of current development activity, not primarily a replay of repository history.
- The harbor metaphor must communicate real state; it is not a decorative theme over a conventional Git graph.
- Ambient viewing and precise inspection are equally important.
- The core product is local-first and must not depend on GitHub, an AI provider, or a hosted service.
- Coding agents are one source of repository activity, not a prerequisite or the center of the product.
- Git Buoy complements Git tooling rather than attempting to replace every Git operation.

If a proposed feature weakens one of these constraints, explain the tradeoff before implementing it.

## Making changes

1. Inspect the repository and relevant documentation before editing.
2. Keep the change narrowly aligned with the requested outcome.
3. Prefer the smallest coherent design that leaves room for iteration.
4. Update documentation whenever behavior, terminology, scope, or setup changes.
5. Verify the result in proportion to its risk and report what was actually checked.

Do not add speculative abstractions, placeholder modules, sample services, or dependencies for hypothetical future needs.

## Terminology

Use product terms consistently:

- **Harbor**: the visual scene representing one observed repository.
- **Main terminal**: the repository's default branch.
- **Dock**: a branch or linked worktree, depending on the final information model.
- **Vessel**: a checked-out workspace at a dock. Its activity describes observed repository changes, not the presence of a person or process.
- **Cargo**: changes moving toward a commit, push, review, merge, or release.
- **Ambient mode**: the passive overview experience.
- **Inspect mode**: keyboard-driven access to exact repository details.

These mappings are hypotheses, not immutable branding. When the metaphor conflicts with comprehension, use plain Git terminology and document the decision.

## Architecture decisions

The implementation stack is Rust with ratatui, crossterm, and git2; see [docs/adr/0001-implementation-stack.md](docs/adr/0001-implementation-stack.md). The code is layered so each concern can change alone:

- `src/git/` collects repository state into a plain `RepoSnapshot`.
- `src/hosting/` optionally collects remote-hosting state into a plain `HostingSnapshot`; it must remain opt-in and non-fatal.
- `src/harbor/` maps snapshots to the pure scene model (`Harbor`, `Dock`, `Vessel`) and owns the deterministic animation clock.
- `src/ui/` renders the scene with ratatui and owns all terminal specifics.
- `src/app.rs` is the state machine (mode, selection, message handling) between them.

Keep new work within this layering: git2 types must not leak past `src/git/`, hosting-provider response types must not leak past `src/hosting/`, and rendering types must not leak below `src/ui/`.

Proposals for consequential architecture changes should evaluate at least:

- Terminal rendering quality and Unicode support.
- Animation timing and performance.
- Cross-platform behavior.
- Access to Git repository state without fragile output parsing.
- Testability of state mapping independently from rendering.
- Distribution as a small, straightforward executable.
- Accessibility, reduced-motion behavior, and limited-color terminals.

Record consequential, difficult-to-reverse decisions in a short architecture decision record rather than burying them in code or a pull-request discussion.

## Quality expectations

- Separate repository-state collection from the harbor scene model and rendering.
- Treat Git data as untrusted input: unusual paths, large repositories, detached HEADs, missing remotes, and incomplete operations are normal cases.
- Keep animation deterministic under test by abstracting time and randomness.
- Avoid network access in core local workflows.
- Ensure the application remains useful when animation is disabled.
- Test state transitions, not just static snapshots.
- Measure idle CPU and memory use; ambient software should be quiet.

## Toolchain commands

Requires a stable Rust toolchain via `rustup` (selected by `rust-toolchain.toml`). CI runs exactly these on Linux, macOS, and Windows:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Run the application with `cargo run -- [path-to-repo]`. Integration tests use the `git` CLI to build fixture repositories, so `git` must be on the PATH when testing.

## Git hygiene

- Do not modify unrelated files or discard existing work.
- Keep commits focused and use messages that explain the user-visible or architectural outcome.
- Do not commit secrets, credentials, local paths, recordings containing private repository data, or generated build artifacts.
- Do not rewrite shared history unless explicitly instructed.

## Documentation style

Write plainly and concretely. Prefer examples and observable behavior over promotional language. Avoid claims about performance, compatibility, or support that have not been verified.

README changes should describe the product for potential users and contributors. Contributor mechanics and agent instructions belong here. Detailed design decisions should eventually live under `docs/` once they exist.

## Current validation

Before finishing a change:

- Run the toolchain commands above; all three must pass.
- For behavior changes, run the application against a real repository and confirm the scene reflects its state.
- Documentation renders as valid, readable Markdown; links and filenames resolve with the repository's exact casing.
- Product terminology agrees across `README.md`, `AGENTS.md`, and `docs/`.
