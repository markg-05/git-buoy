# Idle resource profiling

This procedure is the v0.1 release gate for Git Buoy's idle CPU, resident
memory, and repository survey cost. It measures the release binary in a real
pseudo-terminal. The fixtures are generated locally and do not use private
repositories or network data.

## Release budgets

Every run in the matrix must satisfy all three limits:

| Resource | Budget | Reason |
| --- | ---: | --- |
| Process CPU | mean at or below 8% of one logical core | Leaves more than 90% of a core available while the ambient view is open and gives the current worst result about 70% regression headroom. |
| Resident memory | p95 at or below 64 MiB | Keeps a long-running pane small while allowing for allocator and operating-system differences. The current worst result is about 12 MiB. |
| Repository survey latency | p95 at or below 250 ms | Limits collector duty to one eighth of the default two-second poll interval and keeps observed changes subsecond even on the largest fixture. |

The same budgets apply to full motion, reduced motion, unchanged state, and
the active-change interval. Reduced motion is an accessibility behavior, not a
power-saving mode: it stops animation state advancement, but the application
currently retains the configured render cadence. A reduced-motion result may
therefore be close to a full-motion result.

An over-budget result blocks a release. Fix the regression or open a blocking
follow-up issue that names the failing fixture, platform, metric, measured
value, and intended resolution. Do not silently replace or relax a budget.

## What the script measures

[`scripts/profile.py`](../scripts/profile.py) uses only Python's standard
library, Git, Cargo, and operating-system process accounting. It performs a
release build, creates three deterministic fixtures, and runs this matrix for
each fixture:

- full motion at the default 12 FPS, unchanged;
- full motion at 12 FPS, with a tracked file rewritten every 500 ms;
- reduced motion, unchanged;
- reduced motion, with the same active-change interval.

All runs use the default two-second repository survey interval, a 120 by 40
pseudo-terminal, a five-second warm-up, and a 20-second measured window. The
GitHub observer remains disabled. The script drains terminal output while the
process runs so a full pseudo-terminal buffer cannot stall rendering.

The fixtures intentionally contain stable pending work, so "unchanged" means
that repository state does not change during the measured window rather than
that every worktree is clean.

| Fixture | Shape | Stable pending work |
| --- | --- | --- |
| `small` | 51 tracked files, 6 local branches, no linked worktrees | 2 unstaged, 1 staged, and 1 untracked file |
| `large` | 10,001 tracked files, 200 local branches, no linked worktrees | 20 unstaged, 10 staged, and 1 untracked file |
| `worktrees` | 1,001 tracked files, 24 local branches, 8 linked worktrees | main worktree has 5 unstaged, 2 staged, and 1 untracked file; every linked worktree has 1 unstaged file |

During the active run, changes rotate across the main and linked worktrees.
The script restores the activity files between runs. It only replaces a
fixture directory containing its own marker file, preventing an accidental
deletion of an unrelated directory.

Metrics are calculated as follows:

- **CPU** is the process's user plus system CPU delta divided by measured wall
  time. Linux values come from `/proc/<pid>/stat`; macOS values come from
  `proc_pidinfo` and are converted from Mach ticks with the kernel timebase.
  A result of 8% means 0.08 of one logical core, not 8% of the whole machine.
- **RSS** is current resident memory sampled every 250 ms. The report records
  mean, nearest-rank p95, and maximum MiB.
- **Survey latency** is elapsed wall time around the actual `git::collect`
  call on the collector thread. The hidden `--profile-output` application flag
  enables this timing; without it, samples are neither allocated nor written.
  Warm-up samples are excluded. The report records mean, nearest-rank p95,
  maximum, and sample count.

The complete machine metadata, parameters, samples, and summary statistics are
stored as JSON under [`docs/profiles/`](profiles/). Keep the JSON when updating
a baseline; the tables below are a readable summary, not a substitute for the
raw report. The script writes the report and exits with status 2 when any run
exceeds a release budget.

## Reproduce on macOS or native Linux

Run from the repository root:

```sh
python3 scripts/profile.py \
  --output docs/profiles/local-$(date +%Y-%m-%d).json
```

The default matrix takes about five minutes after the release build. Use
`--fixture-root` only when generated fixtures should live outside `target/`.
Use `--binary` to measure an already-built release binary. Shorter
`--duration` and `--warmup` values are useful for checking the procedure, but
do not replace the defaults for a recorded release baseline.

## Reproduce in CI-friendly Debian

The recorded Linux baseline used the official multi-architecture
`rust:1.97-bookworm` image with manifest digest
`sha256:7d0723df719e7f213b69dc7c8c595985c3f4b060cfbee4f7bc0e347a86fe3b6a`.
This command installs Python in the disposable container, builds into `/tmp`,
and writes only the result JSON to the checkout:

```sh
docker run --rm \
  -e CARGO_TARGET_DIR=/tmp/git-buoy-target \
  -e HOST_UID="$(id -u)" \
  -e HOST_GID="$(id -g)" \
  -v "$PWD:/work" \
  -w /work \
  rust:1.97-bookworm@sha256:7d0723df719e7f213b69dc7c8c595985c3f4b060cfbee4f7bc0e347a86fe3b6a \
  bash -c 'apt-get update >/dev/null && \
    apt-get install -y python3 >/dev/null && \
    python3 scripts/profile.py \
      --fixture-root /tmp/git-buoy-fixtures \
      --output /work/docs/profiles/linux-local.json && \
    chown "$HOST_UID:$HOST_GID" /work/docs/profiles/linux-local.json'
```

The first run needs network access for the container image, Debian package,
Rust components, and crates. The measured Git Buoy process itself performs no
network access.

## v0.1 baseline

These results were collected on July 13, 2026, from release binaries built on
top of revision `3ed939e` plus the profiling changes described here. The JSON
reports mark the source dirty for that reason and include the exact binary
SHA-256 digest.

macOS was Darwin 25.3.0 on an 18-core Apple M5 Pro with 24 GB of memory. Linux
was Debian Bookworm in Docker Desktop on the same host, LinuxKit kernel
6.12.76, and `aarch64`. Containerized Linux numbers include virtualization and
filesystem-sharing effects; they are a repeatable CI-oriented baseline, not a
claim about native x86_64 performance.

### macOS 26.3, arm64

| Fixture | Motion | State | CPU, one core | RSS p95 (MiB) | Survey p95 (ms) |
| --- | --- | --- | ---: | ---: | ---: |
| small | full | unchanged | 0.81% | 8.92 | 4.15 |
| small | full | active | 0.83% | 8.91 | 3.96 |
| small | reduced | unchanged | 0.87% | 8.92 | 4.10 |
| small | reduced | active | 0.81% | 8.95 | 4.08 |
| large | full | unchanged | 4.60% | 11.98 | 59.51 |
| large | full | active | 4.29% | 11.92 | 59.49 |
| large | reduced | unchanged | 4.69% | 12.00 | 60.57 |
| large | reduced | active | 4.53% | 12.05 | 65.50 |
| worktrees | full | unchanged | 3.56% | 9.66 | 56.03 |
| worktrees | full | active | 3.34% | 9.70 | 55.11 |
| worktrees | reduced | unchanged | 3.50% | 9.59 | 55.08 |
| worktrees | reduced | active | 3.49% | 9.84 | 55.34 |

Full report: [`macos-2026-07-13.json`](profiles/macos-2026-07-13.json).

### Debian Bookworm container, arm64

| Fixture | Motion | State | CPU, one core | RSS p95 (MiB) | Survey p95 (ms) |
| --- | --- | --- | ---: | ---: | ---: |
| small | full | unchanged | 0.65% | 4.00 | 1.75 |
| small | full | active | 0.65% | 4.01 | 1.80 |
| small | reduced | unchanged | 0.70% | 4.01 | 1.82 |
| small | reduced | active | 0.70% | 4.00 | 1.63 |
| large | full | unchanged | 3.25% | 7.65 | 37.05 |
| large | full | active | 3.20% | 7.66 | 36.61 |
| large | reduced | unchanged | 3.25% | 7.61 | 36.68 |
| large | reduced | active | 3.25% | 7.56 | 36.61 |
| worktrees | full | unchanged | 2.60% | 4.79 | 37.13 |
| worktrees | full | active | 2.45% | 4.77 | 34.25 |
| worktrees | reduced | unchanged | 2.45% | 4.77 | 33.79 |
| worktrees | reduced | active | 2.50% | 4.73 | 33.69 |

Full report:
[`linux-bookworm-2026-07-13.json`](profiles/linux-bookworm-2026-07-13.json).

All 24 recorded runs are inside all three release budgets. No blocking
performance follow-up is required for this baseline.
