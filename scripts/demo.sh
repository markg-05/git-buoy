#!/bin/sh

set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
PROJECT_ROOT=$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)
DEMO_ROOT=${GIT_BUOY_DEMO_DIR:-/tmp/git-buoy-demo}
HARBOR="$DEMO_ROOT/git-buoy-demo"
WORKTREES="$DEMO_ROOT/worktrees"
ORIGIN="$DEMO_ROOT/origin.git"

case "$DEMO_ROOT" in
    /tmp/* | /private/tmp/*) ;;
    *)
        echo "GIT_BUOY_DEMO_DIR must be a disposable path under /tmp" >&2
        exit 2
        ;;
esac

git_demo() {
    env \
        GIT_CONFIG_GLOBAL=/dev/null \
        GIT_CONFIG_NOSYSTEM=1 \
        GIT_AUTHOR_NAME="Git Buoy Demo" \
        GIT_AUTHOR_EMAIL="demo@example.invalid" \
        GIT_AUTHOR_DATE="2025-01-02T03:04:05Z" \
        GIT_COMMITTER_NAME="Git Buoy Demo" \
        GIT_COMMITTER_EMAIL="demo@example.invalid" \
        GIT_COMMITTER_DATE="2025-01-02T03:04:05Z" \
        git "$@"
}

write_numbered_files() {
    directory=$1
    prefix=$2
    count=$3
    content=$4
    mkdir -p "$directory"
    number=1
    while [ "$number" -le "$count" ]; do
        suffix=$(printf '%03d' "$number")
        printf '%s\n' "$content $suffix" > "$directory/$prefix-$suffix.txt"
        number=$((number + 1))
    done
}

setup_fixture() {
    rm -rf -- "$DEMO_ROOT"
    mkdir -p "$DEMO_ROOT" "$WORKTREES"

    git_demo init --bare --initial-branch=main "$ORIGIN" >/dev/null
    git_demo clone "$ORIGIN" "$HARBOR" >/dev/null

    mkdir -p "$HARBOR/src" "$HARBOR/notes"
    printf '# Git Buoy demo harbor\n' > "$HARBOR/README.md"
    printf 'calm water\n' > "$HARBOR/shared.txt"
    printf 'pub fn harbor_route() {}\n' > "$HARBOR/src/navigation.rs"
    printf 'No channel changes yet.\n' > "$HARBOR/notes/tide-plan.md"
    git_demo -C "$HARBOR" add README.md shared.txt src/navigation.rs notes/tide-plan.md
    git_demo -C "$HARBOR" commit -m "Seed the demo harbor" >/dev/null
    git_demo -C "$HARBOR" push -u origin main >/dev/null
    git_demo -C "$ORIGIN" symbolic-ref HEAD refs/heads/main

    for branch in \
        demo/blocked \
        demo/cargo-overflow \
        demo/diverged \
        demo/idle \
        demo/incoming \
        demo/live-loading \
        demo/loading \
        demo/outbound \
        demo/sealed \
        demo/moored \
        demo/parked
    do
        git_demo -C "$HARBOR" switch -c "$branch" main >/dev/null
        git_demo -C "$HARBOR" push -u origin "$branch" >/dev/null
    done
    git_demo -C "$HARBOR" switch main >/dev/null
    git_demo -C "$HARBOR" remote set-head origin -a >/dev/null

    for branch in blocked cargo-overflow diverged idle incoming live-loading loading outbound sealed
    do
        git_demo -C "$HARBOR" worktree add "$WORKTREES/$branch" "demo/$branch" >/dev/null
    done

    printf '# Loading navigation changes\n' > "$WORKTREES/loading/README.md"
    mkdir -p "$WORKTREES/loading/src" "$WORKTREES/loading/notes"
    printf "pub fn harbor_route() -> &'static str { \"east\" }\n" > "$WORKTREES/loading/src/navigation.rs"
    printf 'Check the eastern channel.\n' > "$WORKTREES/loading/notes/tide-plan.md"

    mkdir -p "$WORKTREES/sealed/src" "$WORKTREES/sealed/docs"
    printf 'pub const READY: bool = true;\n' > "$WORKTREES/sealed/src/manifest.rs"
    printf 'Cargo checked and sealed.\n' > "$WORKTREES/sealed/docs/checklist.md"
    git_demo -C "$WORKTREES/sealed" add src/manifest.rs docs/checklist.md

    mkdir -p "$WORKTREES/outbound/src"
    printf 'pub fn departure_lane() {}\n' > "$WORKTREES/outbound/src/departure.rs"
    git_demo -C "$WORKTREES/outbound" add src/departure.rs
    git_demo -C "$WORKTREES/outbound" commit -m "Prepare outbound cargo" >/dev/null

    write_numbered_files "$WORKTREES/cargo-overflow/staged" cargo 90 "sealed cargo"
    write_numbered_files "$WORKTREES/cargo-overflow/untracked" note 50 "untracked cargo"
    git_demo -C "$WORKTREES/cargo-overflow" add staged

    git_demo -C "$HARBOR" worktree add -b demo/conflict-source "$WORKTREES/conflict-source" main >/dev/null
    printf 'incoming channel\n' > "$WORKTREES/conflict-source/shared.txt"
    git_demo -C "$WORKTREES/conflict-source" add shared.txt
    git_demo -C "$WORKTREES/conflict-source" commit -m "Change the incoming channel" >/dev/null
    conflict_source=$(git_demo -C "$WORKTREES/conflict-source" rev-parse HEAD)
    git_demo -C "$HARBOR" worktree remove "$WORKTREES/conflict-source"
    git_demo -C "$HARBOR" branch -D demo/conflict-source >/dev/null

    printf 'blocked channel\n' > "$WORKTREES/blocked/shared.txt"
    git_demo -C "$WORKTREES/blocked" add shared.txt
    git_demo -C "$WORKTREES/blocked" commit -m "Change the blocked channel" >/dev/null
    if git_demo -C "$WORKTREES/blocked" merge "$conflict_source" -m "Demonstrate a blocked lane" >/dev/null 2>&1; then
        echo "expected the demo merge to conflict" >&2
        exit 1
    fi

    git_demo clone "$ORIGIN" "$DEMO_ROOT/publisher" >/dev/null
    git_demo -C "$DEMO_ROOT/publisher" switch demo/incoming >/dev/null
    printf 'remote cargo arriving\n' > "$DEMO_ROOT/publisher/incoming.txt"
    git_demo -C "$DEMO_ROOT/publisher" add incoming.txt
    git_demo -C "$DEMO_ROOT/publisher" commit -m "Publish incoming cargo" >/dev/null
    git_demo -C "$DEMO_ROOT/publisher" push origin demo/incoming >/dev/null

    printf 'local navigation choice\n' > "$WORKTREES/diverged/diverged.txt"
    git_demo -C "$WORKTREES/diverged" add diverged.txt
    git_demo -C "$WORKTREES/diverged" commit -m "Choose the local channel" >/dev/null

    git_demo -C "$DEMO_ROOT/publisher" switch demo/diverged >/dev/null
    printf 'remote navigation choice\n' > "$DEMO_ROOT/publisher/diverged.txt"
    git_demo -C "$DEMO_ROOT/publisher" add diverged.txt
    git_demo -C "$DEMO_ROOT/publisher" commit -m "Choose the remote channel" >/dev/null
    git_demo -C "$DEMO_ROOT/publisher" push origin demo/diverged >/dev/null
    git_demo -C "$HARBOR" fetch origin >/dev/null

    echo "Demo harbor created at $HARBOR"
    echo "No network remotes were used; origin is $ORIGIN"
}

run_demo() {
    if [ ! -d "$HARBOR/.git" ]; then
        echo "Run '$0 setup' first." >&2
        exit 1
    fi
    cd "$PROJECT_ROOT"
    GIT_BUOY_CONFIG="$DEMO_ROOT/settings.json" \
        cargo run --locked -- \
        --poll-interval 0.5 \
        --idle-after 1 \
        "$HARBOR"
}

trigger_transition() {
    if [ ! -d "$WORKTREES/live-loading" ]; then
        echo "Run '$0 setup' first." >&2
        exit 1
    fi
    live="$WORKTREES/live-loading"
    if [ -f "$live/live-crate-001.txt" ]; then
        number=1
        while [ "$number" -le 12 ]; do
            suffix=$(printf '%03d' "$number")
            rm -f -- "$live/live-crate-$suffix.txt"
            number=$((number + 1))
        done
        echo "Removed live cargo; Git Buoy will show it unloading."
    else
        write_numbered_files "$live" live-crate 12 "new live cargo"
        echo "Added 12 changed paths; Git Buoy will show them loading."
    fi
}

capture_media() {
    setup_fixture
    cd "$PROJECT_ROOT"
    COLORTERM=truecolor cargo run --quiet --locked --example capture_demo -- "$HARBOR" "$PROJECT_ROOT/docs"
    if grep -E '/Users/|/home/|markgeorge|Documents/GitHub' \
        "$PROJECT_ROOT/docs/demo.svg" \
        "$PROJECT_ROOT/docs/demo-motion.svg" \
        "$PROJECT_ROOT/docs/inspect.svg" >/dev/null
    then
        echo "generated media contains a private-looking local path" >&2
        exit 1
    fi
    echo "Captured docs/demo.svg, docs/demo-motion.svg, and docs/inspect.svg"
}

case ${1:-} in
    setup) setup_fixture ;;
    run) run_demo ;;
    transition) trigger_transition ;;
    capture) capture_media ;;
    *)
        echo "usage: $0 {setup|run|transition|capture}" >&2
        exit 2
        ;;
esac
