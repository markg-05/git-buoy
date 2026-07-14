# Contributing to Git Buoy

Thanks for helping make concurrent repository activity easier to understand. Bug reports, small fixes, accessibility findings, terminal-compatibility evidence, prior art, and critiques of the information model are welcome.

Git Buoy is still settling its harbor metaphor. Changes should make real Git state clearer, preserve precise inspection, and keep the default local workflow independent of GitHub, an AI provider, or a hosted service.

## Before opening a change

1. Read the [product intent and limitations](README.md).
2. Check [open issues](https://github.com/markg-05/git-buoy/issues) for related work. For a consequential new feature or architecture change, start with an issue so its information model and tradeoffs can be discussed before implementation.
3. Install a stable Rust toolchain through [rustup](https://rustup.rs) and make sure Git is on `PATH`.

Coding agents must also read [AGENTS.md](AGENTS.md). It contains operational constraints for automated repository work; this file is the public contributor entry point.

## Run the project

Use a disposable fixture to see the supported states without exposing a private repository:

```sh
./scripts/demo.sh setup
./scripts/demo.sh run
```

Or observe an existing local repository:

```sh
cargo run -- path/to/repository
```

The core workflow must remain offline. Enable optional GitHub observation only when that behavior is relevant to the change:

```sh
cargo run -- --github path/to/repository
```

## Design and architecture expectations

- Keep Git collection in `src/git/`, pure scene mapping and deterministic animation in `src/harbor/`, application state transitions in `src/app.rs`, and ratatui rendering in `src/ui/`.
- Keep `src/hosting/` optional and non-fatal. Hosting-provider response types must not enter the application or harbor layers.
- Treat unusual paths, detached and unborn heads, missing remotes, large repositories, and incomplete Git operations as ordinary input.
- Keep the application understandable without animation and without relying on color alone.
- Prefer the smallest coherent change. Do not add dependencies, services, generated assets, or abstractions for hypothetical future work.

Consequential, difficult-to-reverse architecture decisions belong in a short record under [`docs/adr/`](docs/adr/). The current stack decision is [ADR 0001](docs/adr/0001-implementation-stack.md).

## Test a change

Run the exact fast checks used by CI:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Behavior changes should also be run against a real or disposable repository. Confirm that ambient mode communicates the state and inspect mode exposes the corresponding Git details. Changes affecting packaged behavior or resource use may need the procedures in [release acceptance](docs/release-acceptance.md) or [idle resource profiling](docs/profiling.md).

For README media changes, regenerate captures with:

```sh
./scripts/demo.sh capture
```

Generated media must not contain private repository data or local user paths.

## Open a pull request

Keep each pull request focused and explain:

- the observable problem and resulting behavior;
- any metaphor or architecture tradeoff;
- the commands and manual cases actually checked;
- any platform, terminal, hosting, or live-event limitation that remains.

Update public documentation when behavior, terminology, setup, limitations, or support changes. Avoid performance or compatibility claims that do not link to recorded evidence.
