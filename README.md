# Git Buoy

> A living terminal harbor for understanding parallel software work at a glance.

Git Buoy is an experimental terminal application that turns the state of a Git repository into an animated seaport. Instead of presenting another commit graph or a wall of status text, it gives branches, worktrees, coding agents, pull requests, and CI activity a shared visual language.

The goal is not to disguise Git. It is to make a busy repository feel legible, especially when several worktrees or coding agents are active at once, while creating something calm and interesting enough to leave running in a spare terminal pane.

## Status

Git Buoy is currently at the product-definition stage. This repository intentionally contains no implementation yet. The first milestone is to validate the information model and visual metaphor before choosing a rendering approach or application stack.

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
| Default branch | Main terminal |
| Branch or worktree | Dock |
| Active developer or coding agent | Vessel at a dock |
| Uncommitted changes | Cargo being loaded |
| Commit | Sealed cargo container |
| Push | Outbound vessel |
| Pull request | Vessel awaiting clearance |
| CI checks | Harbor inspection |
| Merge conflict | Blocked shipping lane |
| Successful merge | Cargo arriving at the main terminal |
| Release | Convoy departing the harbor |

The mapping will evolve as the product is prototyped. Clarity takes precedence over completing the metaphor.

## Intended experience

Git Buoy should work in two complementary modes:

1. **Ambient mode:** A quiet, animated overview suitable for a spare terminal pane. Important state changes should be noticeable without demanding attention.
2. **Inspect mode:** Keyboard-driven navigation for selecting a dock, vessel, change set, pull request, or check and seeing the underlying Git information.

The visual style should feel cozy, precise, and restrained. Animation should carry information rather than merely add activity. The application must remain understandable with reduced motion and in terminals with limited color support.

## Initial scope

The first useful version should focus on one local repository and establish the core model:

- Discover branches and linked worktrees.
- Observe clean, modified, staged, ahead, behind, and conflicted states.
- Update the scene as local repository state changes.
- Represent concurrent work without requiring any particular coding agent.
- Provide keyboard inspection of the real Git data behind each visual object.
- Degrade gracefully across terminal sizes and color capabilities.

Remote hosting data, including pull requests, reviews, CI, and releases, belongs in a later milestone after the local experience is convincing.

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

The project is not yet accepting implementation contributions because foundational product and architecture decisions have not been made. Discussion, prior-art references, accessibility concerns, and critiques of the information model are welcome once issue tracking is opened.

Before making changes, read [AGENTS.md](AGENTS.md).

## License

Git Buoy is open-source software licensed under the [MIT License](LICENSE).
