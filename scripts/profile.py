#!/usr/bin/env python3
"""Reproduce Git Buoy CPU, RSS, and repository-survey measurements.

The script uses only Python's standard library, Git, Cargo, and the host's
process accounting. It supports macOS and Linux and creates all repositories
from deterministic local data.
"""

from __future__ import annotations

import argparse
import ctypes
import datetime as dt
import fcntl
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import pty
import select
import shutil
import struct
import subprocess
import sys
import tempfile
import time
import termios
from typing import Any


SCHEMA_VERSION = 1
FIXTURE_VERSION = 1
TERMINAL_COLUMNS = 120
TERMINAL_ROWS = 40
DEFAULT_FPS = 12
DEFAULT_POLL_SECONDS = 2.0
ACTIVE_CHANGE_SECONDS = 0.5
RSS_SAMPLE_SECONDS = 0.25
CPU_BUDGET_PERCENT = 8.0
RSS_P95_BUDGET_MIB = 64.0
SURVEY_P95_BUDGET_MS = 250.0


def command(
    args: list[str], cwd: Path | None = None, capture: bool = False
) -> str:
    result = subprocess.run(
        args,
        cwd=cwd,
        check=True,
        text=True,
        stdout=subprocess.PIPE if capture else subprocess.DEVNULL,
        stderr=subprocess.PIPE if capture else None,
    )
    return result.stdout.strip() if capture else ""


def git(repo: Path, *args: str) -> str:
    return command(["git", "-C", str(repo), *args], capture=True)


def initialize_repo(path: Path, file_count: int) -> None:
    path.mkdir(parents=True)
    command(["git", "init", "-b", "main", str(path)])
    git(path, "config", "user.name", "Git Buoy Profiler")
    git(path, "config", "user.email", "profile@git-buoy.invalid")
    git(path, "config", "core.autocrlf", "false")
    (path / "activity.txt").write_text("initial\n", encoding="utf-8")
    files = path / "files"
    for index in range(file_count):
        directory = files / f"group-{index // 100:03d}"
        directory.mkdir(parents=True, exist_ok=True)
        (directory / f"file-{index:05d}.txt").write_text(
            f"fixture line {index:05d}\n", encoding="utf-8"
        )
    git(path, "add", ".")
    git(path, "commit", "-m", "Create profiling fixture")


def add_branches(repo: Path, total_branches: int) -> None:
    for index in range(total_branches - 1):
        git(repo, "branch", f"branch-{index:03d}")


def add_representative_changes(repo: Path, modified: int, staged: int) -> None:
    for index in range(modified + staged):
        path = repo / "files" / f"group-{index // 100:03d}" / f"file-{index:05d}.txt"
        with path.open("a", encoding="utf-8") as handle:
            handle.write("pending change\n")
    if staged:
        staged_paths = [
            str(
                Path("files")
                / f"group-{index // 100:03d}"
                / f"file-{index:05d}.txt"
            )
            for index in range(modified, modified + staged)
        ]
        git(repo, "add", "--", *staged_paths)
    (repo / "untracked-profile-note.txt").write_text(
        "untracked fixture data\n", encoding="utf-8"
    )


def create_fixtures(root: Path) -> list[dict[str, Any]]:
    marker = root / ".git-buoy-profile-fixtures"
    if root.exists():
        if not marker.is_file():
            raise RuntimeError(
                f"refusing to replace unmarked fixture directory: {root}"
            )
        shutil.rmtree(root)
    root.mkdir(parents=True)
    marker.write_text(f"fixture-version={FIXTURE_VERSION}\n", encoding="utf-8")

    small = root / "small" / "repo"
    initialize_repo(small, 50)
    add_branches(small, 6)
    add_representative_changes(small, modified=2, staged=1)

    large = root / "large" / "repo"
    initialize_repo(large, 10_000)
    add_branches(large, 200)
    add_representative_changes(large, modified=20, staged=10)

    worktrees = root / "worktrees" / "repo"
    initialize_repo(worktrees, 1_000)
    add_branches(worktrees, 16)
    linked_root = root / "worktrees" / "linked"
    linked_paths = []
    for index in range(8):
        linked = linked_root / f"worktree-{index:02d}"
        git(worktrees, "worktree", "add", "-b", f"worktree-{index:02d}", str(linked))
        linked_paths.append(linked)
        changed = linked / "files" / "group-000" / f"file-{index:05d}.txt"
        with changed.open("a", encoding="utf-8") as handle:
            handle.write(f"worktree {index} pending change\n")
    add_representative_changes(worktrees, modified=5, staged=2)

    return [
        {
            "name": "small",
            "repo": str(small),
            "tracked_files": 51,
            "local_branches": 6,
            "linked_worktrees": 0,
            "activity_paths": [str(small / "activity.txt")],
        },
        {
            "name": "large",
            "repo": str(large),
            "tracked_files": 10_001,
            "local_branches": 200,
            "linked_worktrees": 0,
            "activity_paths": [str(large / "activity.txt")],
        },
        {
            "name": "worktrees",
            "repo": str(worktrees),
            "tracked_files": 1_001,
            "local_branches": 24,
            "linked_worktrees": 8,
            "activity_paths": [str(worktrees / "activity.txt")]
            + [str(path / "activity.txt") for path in linked_paths],
        },
    ]


class DarwinTaskInfo(ctypes.Structure):
    _fields_ = [
        ("virtual_size", ctypes.c_uint64),
        ("resident_size", ctypes.c_uint64),
        ("total_user", ctypes.c_uint64),
        ("total_system", ctypes.c_uint64),
        ("threads_user", ctypes.c_uint64),
        ("threads_system", ctypes.c_uint64),
        ("policy", ctypes.c_int32),
        ("faults", ctypes.c_int32),
        ("pageins", ctypes.c_int32),
        ("cow_faults", ctypes.c_int32),
        ("messages_sent", ctypes.c_int32),
        ("messages_received", ctypes.c_int32),
        ("syscalls_mach", ctypes.c_int32),
        ("syscalls_unix", ctypes.c_int32),
        ("context_switches", ctypes.c_int32),
        ("thread_count", ctypes.c_int32),
        ("running_threads", ctypes.c_int32),
        ("priority", ctypes.c_int32),
    ]


class MachTimebaseInfo(ctypes.Structure):
    _fields_ = [("numerator", ctypes.c_uint32), ("denominator", ctypes.c_uint32)]


def process_sample(pid: int) -> tuple[float, int]:
    """Return total CPU seconds and current RSS KiB for one process."""
    if sys.platform == "darwin":
        libproc = ctypes.CDLL("/usr/lib/libproc.dylib")
        info = DarwinTaskInfo()
        size = libproc.proc_pidinfo(
            pid, 4, 0, ctypes.byref(info), ctypes.sizeof(info)
        )
        if size != ctypes.sizeof(info):
            raise RuntimeError(f"proc_pidinfo failed for process {pid}")
        timebase = MachTimebaseInfo()
        libsystem = ctypes.CDLL("/usr/lib/libSystem.B.dylib")
        if libsystem.mach_timebase_info(ctypes.byref(timebase)) != 0:
            raise RuntimeError("mach_timebase_info failed")
        nanoseconds = (
            (info.total_user + info.total_system)
            * timebase.numerator
            / timebase.denominator
        )
        cpu_seconds = nanoseconds / 1_000_000_000
        return cpu_seconds, info.resident_size // 1024
    if sys.platform.startswith("linux"):
        stat = Path(f"/proc/{pid}/stat").read_text(encoding="utf-8")
        fields = stat[stat.rfind(")") + 2 :].split()
        ticks = int(fields[11]) + int(fields[12])
        cpu_seconds = ticks / os.sysconf("SC_CLK_TCK")
        resident_pages = int(
            Path(f"/proc/{pid}/statm").read_text(encoding="utf-8").split()[1]
        )
        rss_kib = resident_pages * os.sysconf("SC_PAGE_SIZE") // 1024
        return cpu_seconds, rss_kib
    raise RuntimeError("profiling is supported only on macOS and Linux")


def drain_terminal(master_fd: int) -> None:
    while True:
        readable, _, _ = select.select([master_fd], [], [], 0)
        if not readable:
            return
        try:
            if not os.read(master_fd, 65_536):
                return
        except (BlockingIOError, OSError):
            return


def wait_while_draining(master_fd: int, process: subprocess.Popen[bytes], seconds: float) -> None:
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        if process.poll() is not None:
            raise RuntimeError(f"Git Buoy exited early with status {process.returncode}")
        drain_terminal(master_fd)
        time.sleep(min(0.02, max(0.0, deadline - time.monotonic())))


def percentile(values: list[float], fraction: float) -> float:
    ordered = sorted(values)
    index = max(0, math.ceil(len(ordered) * fraction) - 1)
    return ordered[index]


def summarize(values: list[float], digits: int = 2) -> dict[str, float]:
    if not values:
        raise RuntimeError("measurement produced no samples")
    return {
        "mean": round(sum(values) / len(values), digits),
        "p95": round(percentile(values, 0.95), digits),
        "max": round(max(values), digits),
    }


def restore_activity_files(fixture: dict[str, Any]) -> None:
    for path_text in fixture["activity_paths"]:
        path = Path(path_text)
        git(path.parent, "checkout", "--", "activity.txt")


def profile_run(
    binary: Path,
    fixture: dict[str, Any],
    motion: str,
    state: str,
    warmup_seconds: float,
    duration_seconds: float,
    temporary: Path,
) -> dict[str, Any]:
    profile_path = temporary / f"{fixture['name']}-{motion}-{state}.json"
    config_path = temporary / f"settings-{fixture['name']}-{motion}-{state}.json"
    arguments = [
        str(binary),
        "--fps",
        str(DEFAULT_FPS),
        "--poll-interval",
        str(DEFAULT_POLL_SECONDS),
        "--profile-output",
        str(profile_path),
    ]
    if motion == "reduced":
        arguments.append("--reduced-motion")
    arguments.append(fixture["repo"])

    master_fd, slave_fd = pty.openpty()
    fcntl.ioctl(
        slave_fd,
        termios.TIOCSWINSZ,
        struct.pack("HHHH", TERMINAL_ROWS, TERMINAL_COLUMNS, 0, 0),
    )
    flags = fcntl.fcntl(master_fd, fcntl.F_GETFL)
    fcntl.fcntl(master_fd, fcntl.F_SETFL, flags | os.O_NONBLOCK)
    environment = os.environ.copy()
    environment.update(
        {
            "TERM": "xterm-256color",
            "GIT_BUOY_CONFIG": str(config_path),
        }
    )
    process = subprocess.Popen(
        arguments,
        stdin=slave_fd,
        stdout=slave_fd,
        stderr=slave_fd,
        env=environment,
        start_new_session=True,
    )
    os.close(slave_fd)

    try:
        wait_while_draining(master_fd, process, warmup_seconds)
        cpu_start, _ = process_sample(process.pid)
        measured_start = time.monotonic()
        measured_end = measured_start + duration_seconds
        next_rss = measured_start
        next_change = measured_start
        change_index = 0
        rss_samples: list[float] = []

        while time.monotonic() < measured_end:
            if process.poll() is not None:
                raise RuntimeError(f"Git Buoy exited early with status {process.returncode}")
            now = time.monotonic()
            if state == "active" and now >= next_change:
                activity_paths = fixture["activity_paths"]
                path = Path(activity_paths[change_index % len(activity_paths)])
                path.write_text(f"active change {change_index:06d}\n", encoding="utf-8")
                change_index += 1
                next_change += ACTIVE_CHANGE_SECONDS
            if now >= next_rss:
                _, rss_kib = process_sample(process.pid)
                rss_samples.append(rss_kib / 1024)
                next_rss += RSS_SAMPLE_SECONDS
            drain_terminal(master_fd)
            time.sleep(0.02)

        cpu_end, _ = process_sample(process.pid)
        measured_wall = time.monotonic() - measured_start
        os.write(master_fd, b"q")
        deadline = time.monotonic() + 5
        while process.poll() is None and time.monotonic() < deadline:
            drain_terminal(master_fd)
            time.sleep(0.02)
        if process.poll() is None:
            process.terminate()
        return_code = process.wait(timeout=5)
        drain_terminal(master_fd)
        if return_code != 0:
            raise RuntimeError(f"Git Buoy exited with status {return_code}")
    finally:
        os.close(master_fd)
        restore_activity_files(fixture)

    profile = json.loads(profile_path.read_text(encoding="utf-8"))
    start_ms = warmup_seconds * 1_000
    end_ms = (warmup_seconds + duration_seconds) * 1_000 + 500
    surveys_ms = [
        sample["duration_us"] / 1_000
        for sample in profile["survey_samples"]
        if start_ms <= sample["completed_ms"] <= end_ms
    ]
    return {
        "fixture": fixture["name"],
        "motion": motion,
        "state": state,
        "cpu_percent_one_core": round(
            max(0.0, cpu_end - cpu_start) / measured_wall * 100, 2
        ),
        "rss_mib": summarize(rss_samples),
        "survey_ms": summarize(surveys_ms),
        "rss_samples": len(rss_samples),
        "survey_samples": len(surveys_ms),
    }


def tool_version(args: list[str]) -> str:
    return command(args, capture=True).splitlines()[0]


def cpu_model() -> str:
    if sys.platform == "darwin":
        fallback = ""
        for key in ("machdep.cpu.brand_string", "hw.model"):
            try:
                value = command(["sysctl", "-n", key], capture=True)
                if value and value.lower() not in {"arm", "arm64"}:
                    return value
                fallback = value or fallback
            except subprocess.CalledProcessError:
                pass
        try:
            hardware = command(
                ["system_profiler", "SPHardwareDataType", "-detailLevel", "mini"],
                capture=True,
            )
            for line in hardware.splitlines():
                if line.strip().startswith("Chip:"):
                    return line.split(":", 1)[1].strip()
        except subprocess.CalledProcessError:
            pass
        if fallback:
            return fallback
    elif sys.platform.startswith("linux"):
        for line in Path("/proc/cpuinfo").read_text(encoding="utf-8").splitlines():
            if line.startswith("model name"):
                return line.split(":", 1)[1].strip()
    return platform.processor() or "unknown"


def binary_digest(binary: Path) -> str:
    digest = hashlib.sha256()
    with binary.open("rb") as handle:
        for block in iter(lambda: handle.read(1_048_576), b""):
            digest.update(block)
    return digest.hexdigest()


def build_binary(repository: Path) -> Path:
    command(["cargo", "build", "--release"], cwd=repository)
    target = Path(os.environ.get("CARGO_TARGET_DIR", repository / "target"))
    if not target.is_absolute():
        target = repository / target
    return target / "release" / "git-buoy"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--output",
        type=Path,
        required=True,
        help="JSON file to receive the complete result",
    )
    parser.add_argument(
        "--fixture-root",
        type=Path,
        help="replaceable generated-fixture directory (defaults under target)",
    )
    parser.add_argument("--binary", type=Path, help="existing release binary to measure")
    parser.add_argument("--warmup", type=float, default=5.0, help="warm-up seconds per run")
    parser.add_argument("--duration", type=float, default=20.0, help="measured seconds per run")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    if not (sys.platform == "darwin" or sys.platform.startswith("linux")):
        raise RuntimeError("profiling is supported only on macOS and Linux")
    if args.warmup < DEFAULT_POLL_SECONDS or args.duration < DEFAULT_POLL_SECONDS * 3:
        raise RuntimeError("use at least 2 seconds of warm-up and 6 seconds of measurement")

    repository = Path(__file__).resolve().parent.parent
    binary = args.binary.resolve() if args.binary else build_binary(repository)
    if not binary.is_file():
        raise RuntimeError(f"release binary not found: {binary}")
    fixture_root = (
        args.fixture_root.resolve()
        if args.fixture_root
        else repository / "target" / "profile-fixtures"
    )
    fixtures = create_fixtures(fixture_root)
    results = []
    with tempfile.TemporaryDirectory(prefix="git-buoy-profile-") as temp:
        temporary = Path(temp)
        for fixture in fixtures:
            for motion in ("full", "reduced"):
                for state in ("unchanged", "active"):
                    label = f"{fixture['name']:9} {motion:7} {state:9}"
                    print(f"measuring {label}", flush=True)
                    result = profile_run(
                        binary,
                        fixture,
                        motion,
                        state,
                        args.warmup,
                        args.duration,
                        temporary,
                    )
                    results.append(result)
                    print(
                        f"  CPU {result['cpu_percent_one_core']:5.2f}%  "
                        f"RSS p95 {result['rss_mib']['p95']:6.2f} MiB  "
                        f"survey p95 {result['survey_ms']['p95']:7.2f} ms",
                        flush=True,
                    )

    git_status = command(["git", "status", "--porcelain"], cwd=repository, capture=True)
    fixture_report = []
    for fixture in fixtures:
        fixture_report.append({key: value for key, value in fixture.items() if key != "activity_paths"})
    report = {
        "schema_version": SCHEMA_VERSION,
        "generated_at": dt.datetime.now(dt.timezone.utc).isoformat(),
        "source": {
            "revision": command(["git", "rev-parse", "HEAD"], cwd=repository, capture=True),
            "dirty": bool(git_status),
            "binary_sha256": binary_digest(binary),
        },
        "system": {
            "os": platform.system(),
            "os_release": platform.release(),
            "architecture": platform.machine(),
            "cpu": cpu_model(),
            "logical_cpus": os.cpu_count(),
            "rustc": tool_version(["rustc", "--version"]),
            "git": tool_version(["git", "--version"]),
        },
        "parameters": {
            "warmup_seconds": args.warmup,
            "measurement_seconds": args.duration,
            "terminal_columns": TERMINAL_COLUMNS,
            "terminal_rows": TERMINAL_ROWS,
            "fps": DEFAULT_FPS,
            "poll_interval_seconds": DEFAULT_POLL_SECONDS,
            "rss_sample_seconds": RSS_SAMPLE_SECONDS,
            "active_change_seconds": ACTIVE_CHANGE_SECONDS,
            "github_observer": False,
        },
        "budgets": {
            "cpu_percent_one_core_mean_max": CPU_BUDGET_PERCENT,
            "rss_mib_p95_max": RSS_P95_BUDGET_MIB,
            "survey_ms_p95_max": SURVEY_P95_BUDGET_MS,
        },
        "fixtures": fixture_report,
        "runs": results,
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(f"wrote {args.output}")
    failures = [
        result
        for result in results
        if result["cpu_percent_one_core"] > CPU_BUDGET_PERCENT
        or result["rss_mib"]["p95"] > RSS_P95_BUDGET_MIB
        or result["survey_ms"]["p95"] > SURVEY_P95_BUDGET_MS
    ]
    if failures:
        for result in failures:
            print(
                "over budget: "
                f"{result['fixture']} {result['motion']} {result['state']}",
                file=sys.stderr,
            )
        return 2
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, RuntimeError, subprocess.CalledProcessError) as error:
        print(f"profile failed: {error}", file=sys.stderr)
        raise SystemExit(1)
