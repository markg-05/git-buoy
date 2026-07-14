# Git Buoy

> A living terminal harbor for understanding parallel software work at a glance.

![Git Buoy rendering a repository's branches and worktrees as an animated harbor](docs/demo.svg)

<sup>Ambient mode against a demo repository with synthetic branches and worktrees, arranged to show every dock condition at once. `demo/big-cargo` has so many pending changes that its cargo wraps onto a second row.</sup>

Git Buoy is an experimental terminal application that turns the state of a Git repository into an animated seaport. Instead of presenting another commit graph or a wall of status text, it gives branches, worktrees, coding agents, pull requests, and CI activity a shared visual language.

The goal is not to disguise Git. It is to make a busy repository feel legible, especially when several worktrees or coding agents are active at once, while creating something calm and interesting enough to leave running in a spare terminal pane.

## Status

Git Buoy is in early development. The stack is Rust with [ratatui](https://ratatui.rs), chosen and recorded in [docs/adr/0001-implementation-stack.md](docs/adr/0001-implementation-stack.md). The current build discovers branches and linked worktrees in one local repository, watches their state, and renders them as an animated harbor with keyboard inspection. An opt-in GitHub observer adds pull requests, reviews, checks, and releases without making them a core requirement.

## Getting started

Requires a stable [Rust toolchain](https://rustup.rs) and Git.

```sh
git clone https://github.com/markg-05/git-buoy.git
cd git-buoy
cargo run --release -- path/to/some/repository
```

With no path argument, Git Buoy observes the repository containing the current directory.

To start with GitHub state enabled, install and authenticate [GitHub CLI](https://cli.github.com/), then add `--github`:

```sh
gh auth login
cargo run --release -- --github path/to/some/repository
```

GitHub observation can also be enabled during a session from Harbor Controls. GitHub failures are shown in the footer and do not stop local repository observation.

| Key | Action |
| --- | --- |
| `i` or `Enter` | Enter inspect mode on the current dock |
| `s` | Open Harbor Controls |
| `Enter` / right arrow | Drill from dock to vessel to changed files |
| `Esc` / `h` / left arrow | Step back one inspection level |
| `Tab` / `Shift-Tab` | Select a dock |
| `j` / `k` / up/down arrows | Select a dock, drilled-in item, or harbor control; scroll the legend while it is open |
| `p` | Inspect a pull request on the selected dock |
| `l` or `?` | Toggle the legend overlay; `l` adjusts a selected harbor control while that panel is open |
| `Esc` | Close the active overlay, then leave inspect mode, then quit |
| `m` | Toggle reduced motion |
| `q` | Quit |

Useful flags: `--reduced-motion` starts with a static scene, `--fps` sets the ambient animation rate, `--poll-interval` sets how often the repository is re-read, and `--idle-after` controls when an unchanged workspace is labeled idle. `--github` starts with optional hosting data enabled; `--github-poll-interval` sets its initial independent refresh rate. Explicit flags override saved preferences for that run.

Press `s` to open Harbor Controls. Use `j`/`k` to select a row, left/right or `h`/`l` to adjust it, and `s` or `Esc` to close the panel. Changes save immediately as global viewing preferences and apply the next time Git Buoy starts. The `m` reduced-motion shortcut saves the same preference. Explicit CLI flags take precedence without changing the saved value.

| Harbor control | Behavior |
| --- | --- |
| Motion | Switch between full and reduced motion. The `m` shortcut remains available. |
| Overflow pages | Cycle automatically or hold the first page when docks exceed the available height. |
| Page interval | Choose how long each overflowing page remains visible. |
| Setting help | Show or hide the immediate logbook note explaining the selected control. |
| Repository survey | Change the local repository polling interval. |
| Workspace idle | Change how long unchanged work remains active before it is labeled idle. |
| GitHub observer | Enable or disable the optional GitHub layer. No GitHub request is made while it is off. |
| GitHub survey | Change the GitHub polling interval. |

Setting help is on by default. Its fixed logbook-note region updates immediately as selection moves and is omitted when terminal height is too constrained. Reduced motion pauses overflow cycling without changing its setting. If motion is restored, cycling resumes only when **Overflow pages** is still set to cycle.

On macOS and other Unix-like systems, preferences are stored at `$XDG_CONFIG_HOME/git-buoy/settings.json`, falling back to `$HOME/.config/git-buoy/settings.json`. On Windows they are stored at `%APPDATA%\git-buoy\settings.json`. Set `GIT_BUOY_CONFIG` to use a specific file instead. The file contains viewing preferences only—never repository data or GitHub credentials. Enabling the GitHub observer is itself a saved preference, so later sessions will resume remote surveys until it is disabled.

## Product intent

Modern development increasingly involves several streams of work happening in parallel: a developer in one branch, agents in separate worktrees, automated checks running remotely, and pull requests waiting to merge. Existing Git tools are excellent at operating on those objects, but few make the whole workspace feel observable as one system.

Git Buoy should answer questions such as:

- What work is active right now?
- Which worktrees are clean, changing, idle, or blocked?
- What is ready to leave the machine as a commit or push?
- Which pull requests are waiting for review or CI?
- Where are conflicts or failures preventing work from landing?

It should answer them primarily through motion and spatial relationships, with precise details available on demand.

## The harbor metaphor

The metaphor is functional, not decorative. Every object in the scene should communicate repository state consistently.

| Development concept | Harbor representation |
| --- | --- |
| Repository | Harbor |
| Default branch, when identified by a remote or bare-repository `HEAD` | Main terminal |
| Branch or worktree | Dock |
| Checked-out workspace | Vessel at a dock |
| Uncommitted changes | Cargo being loaded |
| Commit | Sealed cargo container |
| Push | Outbound vessel |
| Pull request | Vessel awaiting clearance |
| CI checks | Harbor inspection |
| Merge conflict | Blocked shipping lane |
| Successful merge | Cargo arriving at the main terminal |
| Release | Convoy departing the harbor |

The mapping will evolve as the product is prototyped. Clarity takes precedence over completing the metaphor.

## Reading the harbor

Every dock resolves to a single **condition**, shown by color and by a word on its pier. The same legend is available inside the application at any time by pressing `l`.

| | Condition | What it means |
| --- | --- | --- |
| 🟩 | **calm** | Checked out, committed, and in sync with the upstream. |
| ⬜ | **local** | Checked out with no upstream configured. |
| 🟨 | **loading** | Uncommitted changes are still being loaded (modified or new files). |
| 🟪 | **sealed** | Changes are staged, ready to become a commit. |
| 🟦 | **outbound** | Commits are ahead of the upstream, ready to push. |
| 🔵 | **incoming** | Commits are behind the upstream, ready to pull. |
| 🟧 | **diverged** | Local and upstream histories both contain unique commits. |
| 🟦 | **awaiting** | A remote-only pull-request branch is awaiting clearance. |
| 🟥 | **blocked** | A merge conflict or an in-progress operation is stopping work from landing. |
| ⬜ | **moored** | A branch with no worktree checked out. |

A vessel's hull carries **cargo** that counts the pending change categories, and a few **symbols** stand in for the rest:

| Symbol | Meaning |
| --- | --- |
| `▣` | Staged files |
| `▢` | Unstaged (modified) files |
| `+` | Untracked files |
| `✕` | Conflicted files |
| `▙▄▄▟` | A vessel: work is checked out at this dock |
| `◍` | A mooring buoy: a branch with no worktree |
| `↑` / `↓` | Commits ahead of / behind the upstream |
| `≈~` | A wake from recent or directional activity |
| `▣ committed` | A commit observed while Git Buoy is running |
| `▙▄▄▟→ pushed` | Ahead commits sent upstream |
| `←▣ merged` | A merge commit arriving at a dock |
| `PR#42 ✓!` | Pull request 42: review approved, at least one check failing |
| `▙▄▄▟ ▙▄▄▟→` | Latest published release convoy |

An occupied dock is initially labeled `observing`. After Git Buoy sees its repository state change, it is `recent` until the idle threshold passes; an unchanged workspace is then `idle`. This describes observable repository activity, not whether a particular process or person is present.

Motion reinforces those facts: recent vessels work against a wake, outbound vessels travel away from the pier, incoming vessels travel toward it, and diverged vessels shift without making progress. Newly observed cargo changes load or unload over one short transition, vessels slide in or out when a workspace arrives at or leaves a dock, and a changed lane condition briefly signals from the pier. Each dock shows at most one of these cues at a time, with blocked or cleared lanes taking priority over vessel and cargo motion.

With reduced motion, transitions collapse immediately to the current state. The same conditions, arrows, activity words, and cargo remain visible in a fixed frame.

Commits, pushes, and successful merges appear as short-lived transitions only when Git Buoy observes them happen. The initial survey does not replay existing history. Commit and merge events come from branch reflogs; a push is reported only when the same local tip becomes less far ahead of its upstream, avoiding guesses from unrelated branch movement.

## Intended experience

Git Buoy should work in two complementary modes:

1. **Ambient mode:** A quiet, animated overview suitable for a spare terminal pane. Important state changes should be noticeable without demanding attention.
2. **Inspect mode:** Keyboard-driven navigation drills from a dock into its vessel and exact changed paths. Pull requests and checks join the same hierarchy when remote-hosting observation is enabled.

When every dock does not fit, ambient mode advances through dock-sized pages by default and reports how many docks remain above or below the current view. Harbor Controls can hold the first page instead. Reduced motion also keeps the first page static while preserving the independent cycling preference and the same overflow information.

![Inspect mode floating a detail panel over the harbor](docs/inspect.svg)

<sup>Inspect mode: the detail panel floats over the full-width harbor and sizes itself to its content, so workspace paths and commit messages stay on one line.</sup>

The visual style should feel cozy, precise, and restrained. Animation should carry information rather than merely add activity. The application must remain understandable with reduced motion and in terminals with limited color support.

## Initial scope

The first useful version focuses on one local repository and establishes the core model:

- Discover branches and linked worktrees.
- Identify the main terminal only when Git provides an authoritative default-branch reference.
- Observe clean, modified, staged, ahead, behind, and conflicted states.
- Update the scene as local repository state changes.
- Represent concurrent work without requiring any particular coding agent.
- Provide keyboard inspection of the real Git data behind each visual object.
- Degrade gracefully across terminal sizes and color capabilities.
- Optionally attach GitHub pull requests, review decisions, individual checks, and releases to the local harbor.

Remote hosting is an opt-in layer implemented through the authenticated `gh` executable. It is surveyed independently, failures are non-fatal, and the default local workflow performs no network access. Pull requests whose head branch is not available locally appear as remote docks awaiting clearance.

## Non-goals

Git Buoy is not intended to be:

- A complete replacement for Git, a shell, or established Git clients.
- A Git tutorial that simulates commands.
- A historical commit-replay tool.
- A dashboard that requires an AI provider or proprietary service.
- A generic animation with repository statistics painted on top.

## Product principles

- **Truth before theater.** The scene must accurately reflect repository state.
- **Useful while idle.** Ambient mode should provide value without interaction.
- **Details on demand.** The metaphor offers orientation; inspect mode supplies precision.
- **Local first.** Core functionality should work offline against an ordinary Git repository.
- **Agent agnostic.** Work should be visible whether produced by a person, script, or coding agent.
- **Terminal native.** Keyboard control, low overhead, and broad terminal compatibility are fundamental.
- **Delight through restraint.** A small number of excellent animations is better than constant spectacle.

## Contributing

The project is young and the information model is still settling, so expect churn. Discussion, prior-art references, accessibility concerns, and critiques of the information model are especially welcome.

Before making changes, read [AGENTS.md](AGENTS.md). It records the working expectations, the module layering, and the exact commands CI runs.
The repeatable idle-resource release gate and recorded platform baselines are in
[docs/profiling.md](docs/profiling.md).

## License

Git Buoy is open-source software licensed under the [MIT License](LICENSE).
