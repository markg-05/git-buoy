use std::env;
use std::error::Error;
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use portable_pty::{Child, CommandBuilder, PtySize, native_pty_system};

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

const WAIT_TIMEOUT: Duration = Duration::from_secs(10);
const EXIT_TIMEOUT: Duration = Duration::from_secs(5);

fn main() {
    if is_fake_gh() {
        fake_gh();
    }

    if let Err(error) = run() {
        eprintln!("release acceptance failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let mut args = env::args_os().skip(1);
    let mut binary = None;
    let mut fixture_root = None;
    while let Some(argument) = args.next() {
        match argument.to_string_lossy().as_ref() {
            "--binary" => binary = args.next().map(PathBuf::from),
            "--fixture-root" => fixture_root = args.next().map(PathBuf::from),
            "--help" | "-h" => {
                println!(
                    "usage: cargo run --manifest-path tools/release-acceptance/Cargo.toml -- --binary PATH [--fixture-root PATH]"
                );
                return Ok(());
            }
            other => return Err(message(format!("unknown argument: {other}"))),
        }
    }

    let binary = binary.ok_or_else(|| message("--binary is required"))?;
    let binary = absolute_path(&binary)?;
    let root = fixture_root.unwrap_or_else(|| {
        env::temp_dir().join(format!(
            "git-buoy-release-acceptance-{}",
            std::process::id()
        ))
    });
    let fixtures = Fixtures::create(&root)?;

    println!("release artifact: {}", binary.display());
    println!("fixture root: {}", fixtures.root.display());
    println!("platform: {} {}", env::consts::OS, env::consts::ARCH);

    let mut results = AcceptanceResults::default();
    results.check("real Git state proofs", || fixtures.verify_git_states());
    results.check("real Git states render", || {
        git_state_render_check(&binary, &fixtures)
    });
    results.check("unborn repository", || unborn_check(&binary, &fixtures));
    results.check("operation-only blocking", || {
        operation_check(&binary, &fixtures)
    });

    for (name, rows, cols) in [
        ("narrow 44x16", 16, 44),
        ("normal 80x24", 24, 80),
        ("wide 120x40", 40, 120),
        ("short 80x6", 6, 80),
    ] {
        results.check(name, || {
            launch_and_quit(
                &binary,
                &fixtures.harbor,
                &fixtures.config(name),
                rows,
                cols,
            )
        });
    }

    results.check("ANSI 16-color palette", || {
        palette_check(&binary, &fixtures, "xterm", None, b"\x1b[38;5;4")
    });
    results.check("256-color palette", || {
        palette_check(&binary, &fixtures, "xterm-256color", None, b"\x1b[38;5;74")
    });
    results.check("truecolor palette", || {
        palette_check(
            &binary,
            &fixtures,
            "xterm-256color",
            Some("truecolor"),
            b"\x1b[38;2;86;148;195",
        )
    });
    results.check("reduced motion is static", || {
        reduced_motion_check(&binary, &fixtures)
    });
    results.check("overflow pages cycle and hold", || {
        overflow_check(&binary, &fixtures)
    });
    results.check("keyboard drill-down, legend, settings, escape", || {
        keyboard_check(&binary, &fixtures)
    });
    results.check("settings persist across launch", || {
        persistence_check(&binary, &fixtures)
    });
    results.check("short-height controls remain navigable", || {
        short_controls_check(&binary, &fixtures)
    });
    results.check("GitHub off makes no gh request", || {
        github_off_check(&binary, &fixtures)
    });
    results.check("GitHub on happy path", || {
        github_happy_check(&binary, &fixtures)
    });
    results.check("missing gh is non-fatal", || {
        github_missing_check(&binary, &fixtures)
    });
    results.check("unauthenticated gh is non-fatal", || {
        github_unauthenticated_check(&binary, &fixtures)
    });
    results.finish(&fixtures.root)
}

#[derive(Default)]
struct AcceptanceResults {
    passed: usize,
    failures: Vec<String>,
}

impl AcceptanceResults {
    fn check(&mut self, name: &str, check: impl FnOnce() -> Result<()>) {
        let started = Instant::now();
        match check() {
            Ok(()) => {
                self.passed += 1;
                println!("PASS {name} ({:.2}s)", started.elapsed().as_secs_f64());
            }
            Err(error) => {
                println!("FAIL {name}: {error}");
                self.failures.push(format!("{name}: {error}"));
            }
        }
    }

    fn finish(self, fixture_root: &Path) -> Result<()> {
        println!(
            "summary: {} passed, {} failed",
            self.passed,
            self.failures.len()
        );
        if self.failures.is_empty() {
            fs::remove_dir_all(fixture_root)?;
            Ok(())
        } else {
            Err(message(format!(
                "{} row(s) failed; fixtures retained at {}",
                self.failures.len(),
                fixture_root.display()
            )))
        }
    }
}

struct Fixtures {
    root: PathBuf,
    harbor: PathBuf,
    worktrees: PathBuf,
    keyboard: PathBuf,
    unborn: PathBuf,
    operation: PathBuf,
    config_dir: PathBuf,
    shim_dir: PathBuf,
    gh_log: PathBuf,
}

impl Fixtures {
    fn create(root: &Path) -> Result<Self> {
        replace_fixture_root(root)?;
        let harbor = root.join("harbor");
        let origin = root.join("origin.git");
        let publisher = root.join("publisher");
        let worktrees = root.join("worktrees");
        let config_dir = root.join("config");
        let shim_dir = root.join("bin");
        fs::create_dir_all(&worktrees)?;
        fs::create_dir_all(&config_dir)?;
        fs::create_dir_all(&shim_dir)?;

        git(None, ["init", "--bare", "--initial-branch=main"], [&origin])?;
        git(None, ["clone"], [&origin, &harbor])?;
        configure_repo(&harbor)?;
        fs::write(harbor.join("README.md"), "# Acceptance harbor\n")?;
        fs::write(harbor.join("shared.txt"), "calm channel\n")?;
        fs::create_dir_all(harbor.join("src"))?;
        fs::write(harbor.join("src/navigation.rs"), "pub fn route() {}\n")?;
        git(Some(&harbor), ["add", "."], [])?;
        git(Some(&harbor), ["commit", "-m", "Seed harbor"], [])?;
        git(Some(&harbor), ["push", "-u", "origin", "main"], [])?;
        git(
            Some(&origin),
            ["symbolic-ref", "HEAD", "refs/heads/main"],
            [],
        )?;

        let published = [
            "loading", "sealed", "outbound", "incoming", "diverged", "blocked", "idle-one",
            "idle-two", "moored", "parked",
        ];
        for branch in published {
            let branch = PathBuf::from(branch);
            git(Some(&harbor), ["switch", "-c"], [&branch])?;
            git(Some(&harbor), ["push", "-u", "origin"], [&branch])?;
        }
        git(Some(&harbor), ["switch", "main"], [])?;
        git(Some(&harbor), ["remote", "set-head", "origin", "-a"], [])?;
        git(Some(&harbor), ["branch", "local-only", "main"], [])?;

        for branch in [
            "loading",
            "sealed",
            "outbound",
            "incoming",
            "diverged",
            "blocked",
            "idle-one",
            "idle-two",
            "local-only",
        ] {
            git(
                Some(&harbor),
                ["worktree", "add"],
                [&worktrees.join(branch), &PathBuf::from(branch)],
            )?;
        }
        let detached = worktrees.join("detached");
        git(
            Some(&harbor),
            ["worktree", "add", "--detach"],
            [&detached, &PathBuf::from("main")],
        )?;

        fs::write(worktrees.join("loading/README.md"), "loading one\n")?;
        fs::write(worktrees.join("loading/shared.txt"), "loading two\n")?;
        fs::write(worktrees.join("loading/untracked.txt"), "loading three\n")?;

        fs::write(worktrees.join("sealed/manifest.txt"), "sealed one\n")?;
        fs::write(worktrees.join("sealed/checklist.txt"), "sealed two\n")?;
        git(Some(&worktrees.join("sealed")), ["add", "."], [])?;

        fs::write(worktrees.join("outbound/departure.txt"), "depart\n")?;
        git(Some(&worktrees.join("outbound")), ["add", "."], [])?;
        git(
            Some(&worktrees.join("outbound")),
            ["commit", "-m", "Prepare outbound cargo"],
            [],
        )?;

        git(None, ["clone"], [&origin, &publisher])?;
        configure_repo(&publisher)?;
        git(Some(&publisher), ["switch", "incoming"], [])?;
        fs::write(publisher.join("incoming.txt"), "remote incoming\n")?;
        git(Some(&publisher), ["add", "."], [])?;
        git(Some(&publisher), ["commit", "-m", "Publish incoming"], [])?;
        git(Some(&publisher), ["push", "origin", "incoming"], [])?;

        fs::write(worktrees.join("diverged/local.txt"), "local side\n")?;
        git(Some(&worktrees.join("diverged")), ["add", "."], [])?;
        git(
            Some(&worktrees.join("diverged")),
            ["commit", "-m", "Local divergence"],
            [],
        )?;
        git(Some(&publisher), ["switch", "diverged"], [])?;
        fs::write(publisher.join("remote.txt"), "remote side\n")?;
        git(Some(&publisher), ["add", "."], [])?;
        git(Some(&publisher), ["commit", "-m", "Remote divergence"], [])?;
        git(Some(&publisher), ["push", "origin", "diverged"], [])?;

        let conflict_source = worktrees.join("conflict-source");
        git(
            Some(&harbor),
            ["worktree", "add", "-b", "conflict-source"],
            [&conflict_source, &PathBuf::from("main")],
        )?;
        fs::write(conflict_source.join("shared.txt"), "incoming conflict\n")?;
        git(Some(&conflict_source), ["add", "."], [])?;
        git(
            Some(&conflict_source),
            ["commit", "-m", "Incoming conflict"],
            [],
        )?;
        let source_id = git_output(Some(&conflict_source), ["rev-parse", "HEAD"], [])?;
        git(Some(&harbor), ["worktree", "remove"], [&conflict_source])?;
        git(Some(&harbor), ["branch", "-D", "conflict-source"], [])?;
        let blocked = worktrees.join("blocked");
        fs::write(blocked.join("shared.txt"), "local conflict\n")?;
        git(Some(&blocked), ["add", "."], [])?;
        git(Some(&blocked), ["commit", "-m", "Local conflict"], [])?;
        let merge = git_command(Some(&blocked), ["merge", source_id.trim()], []).output()?;
        if merge.status.success() {
            return Err(message("expected the blocked fixture merge to conflict"));
        }
        git(Some(&harbor), ["fetch", "origin"], [])?;

        let keyboard = root.join("keyboard");
        initialize_repo(&keyboard)?;
        fs::write(keyboard.join("tracked.txt"), "initial\n")?;
        git(Some(&keyboard), ["add", "."], [])?;
        git(
            Some(&keyboard),
            ["commit", "-m", "Seed keyboard fixture"],
            [],
        )?;
        fs::write(keyboard.join("tracked.txt"), "changed\n")?;
        fs::write(keyboard.join("untracked.txt"), "new\n")?;

        let unborn = root.join("unborn");
        fs::create_dir_all(&unborn)?;
        git(Some(&unborn), ["init", "--initial-branch=main"], [])?;

        let operation = root.join("operation");
        initialize_repo(&operation)?;
        for number in 1..=5 {
            fs::write(operation.join("history.txt"), format!("commit {number}\n"))?;
            git(Some(&operation), ["add", "."], [])?;
            git(
                Some(&operation),
                ["commit", "-m", &format!("Commit {number}")],
                [],
            )?;
        }
        git(Some(&operation), ["bisect", "start", "HEAD", "HEAD~4"], [])?;

        Ok(Self {
            root: root.to_path_buf(),
            harbor,
            worktrees,
            keyboard,
            unborn,
            operation,
            config_dir,
            shim_dir,
            gh_log: root.join("gh-invocations.log"),
        })
    }

    fn config(&self, name: &str) -> PathBuf {
        let safe = name
            .chars()
            .map(|character| {
                if character.is_ascii_alphanumeric() {
                    character
                } else {
                    '-'
                }
            })
            .collect::<String>();
        self.config_dir.join(format!("{safe}.json"))
    }

    fn verify_git_states(&self) -> Result<()> {
        expect_command(
            Some(&self.harbor),
            ["rev-list", "--left-right", "--count", "main...origin/main"],
            [],
            "0\t0",
        )?;
        expect_command(
            Some(&self.harbor),
            [
                "rev-list",
                "--left-right",
                "--count",
                "outbound...origin/outbound",
            ],
            [],
            "1\t0",
        )?;
        expect_command(
            Some(&self.harbor),
            [
                "rev-list",
                "--left-right",
                "--count",
                "incoming...origin/incoming",
            ],
            [],
            "0\t1",
        )?;
        expect_command(
            Some(&self.harbor),
            [
                "rev-list",
                "--left-right",
                "--count",
                "diverged...origin/diverged",
            ],
            [],
            "1\t1",
        )?;
        expect_command(
            Some(&self.worktrees.join("blocked")),
            ["status", "--short"],
            [],
            "UU shared.txt",
        )?;
        git_output(
            Some(&self.worktrees.join("blocked")),
            ["rev-parse", "MERGE_HEAD"],
            [],
        )?;
        if git_output(
            Some(&self.worktrees.join("loading")),
            ["status", "--short"],
            [],
        )?
        .lines()
        .count()
            != 3
        {
            return Err(message("loading fixture does not have three changed paths"));
        }
        if git_output(
            Some(&self.worktrees.join("sealed")),
            ["status", "--short"],
            [],
        )?
        .lines()
        .filter(|line| line.starts_with("A "))
        .count()
            != 2
        {
            return Err(message("sealed fixture does not have two staged paths"));
        }
        let worktree_count =
            git_output(Some(&self.harbor), ["worktree", "list", "--porcelain"], [])?
                .lines()
                .filter(|line| line.starts_with("worktree "))
                .count();
        if worktree_count < 11 {
            return Err(message(format!(
                "expected at least 11 worktrees, found {worktree_count}"
            )));
        }
        Ok(())
    }
}

fn replace_fixture_root(root: &Path) -> Result<()> {
    let marker = root.join(".git-buoy-release-acceptance");
    if root.exists() {
        if !marker.is_file() {
            return Err(message(format!(
                "refusing to replace unmarked fixture directory {}",
                root.display()
            )));
        }
        fs::remove_dir_all(root)?;
    }
    fs::create_dir_all(root)?;
    fs::write(marker, "disposable release acceptance fixture\n")?;
    Ok(())
}

fn configure_repo(repo: &Path) -> Result<()> {
    git(
        Some(repo),
        ["config", "user.name", "Git Buoy Acceptance"],
        [],
    )?;
    git(
        Some(repo),
        ["config", "user.email", "acceptance@example.invalid"],
        [],
    )?;
    git(Some(repo), ["config", "core.autocrlf", "false"], [])
}

fn initialize_repo(repo: &Path) -> Result<()> {
    fs::create_dir_all(repo)?;
    git(Some(repo), ["init", "--initial-branch=main"], [])?;
    configure_repo(repo)
}

fn git<const N: usize, const P: usize>(
    cwd: Option<&Path>,
    args: [&str; N],
    paths: [&PathBuf; P],
) -> Result<()> {
    let output = git_command(cwd, args, paths).output()?;
    if output.status.success() {
        Ok(())
    } else {
        Err(command_error("git", &output))
    }
}

fn git_output<const N: usize, const P: usize>(
    cwd: Option<&Path>,
    args: [&str; N],
    paths: [&PathBuf; P],
) -> Result<String> {
    let output = git_command(cwd, args, paths).output()?;
    if !output.status.success() {
        return Err(command_error("git", &output));
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_string())
}

fn expect_command<const N: usize, const P: usize>(
    cwd: Option<&Path>,
    args: [&str; N],
    paths: [&PathBuf; P],
    expected: &str,
) -> Result<()> {
    let actual = git_output(cwd, args, paths)?;
    if actual == expected {
        Ok(())
    } else {
        Err(message(format!("expected {expected:?}, got {actual:?}")))
    }
}

fn git_command<const N: usize, const P: usize>(
    cwd: Option<&Path>,
    args: [&str; N],
    paths: [&PathBuf; P],
) -> Command {
    let mut command = Command::new("git");
    if let Some(cwd) = cwd {
        command.arg("-C").arg(cwd);
    }
    command.args(args);
    for path in paths {
        command.arg(path);
    }
    command.env("GIT_CONFIG_NOSYSTEM", "1");
    command
}

fn command_error(name: &str, output: &Output) -> Box<dyn Error + Send + Sync> {
    message(format!(
        "{name} exited with {}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr).trim()
    ))
}

struct Session {
    child: Box<dyn Child + Send + Sync>,
    writer: Box<dyn Write + Send>,
    output: Vec<u8>,
    receiver: Receiver<Vec<u8>>,
}

impl Session {
    fn spawn(
        binary: &Path,
        repo: &Path,
        config: &Path,
        rows: u16,
        cols: u16,
        arguments: &[&str],
        environment: &[(OsString, Option<OsString>)],
    ) -> Result<Self> {
        let pty = native_pty_system();
        let pair = pty.openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        let mut command = CommandBuilder::new(binary);
        command.args(arguments);
        command.arg(repo);
        command.env("GIT_BUOY_CONFIG", config);
        command.env("TERM", "xterm-256color");
        command.env_remove("COLORTERM");
        command.env_remove("NO_COLOR");
        for (name, value) in environment {
            match value {
                Some(value) => command.env(name, value),
                None => command.env_remove(name),
            }
        }
        let child = pair.slave.spawn_command(command)?;
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            loop {
                let mut buffer = vec![0; 8_192];
                match reader.read(&mut buffer) {
                    Ok(0) | Err(_) => return,
                    Ok(count) => {
                        buffer.truncate(count);
                        if sender.send(buffer).is_err() {
                            return;
                        }
                    }
                }
            }
        });
        Ok(Self {
            child,
            writer,
            output: Vec::new(),
            receiver,
        })
    }

    fn wait_for(&mut self, needle: &[u8]) -> Result<()> {
        let deadline = Instant::now() + WAIT_TIMEOUT;
        loop {
            self.drain_available();
            if contains(&self.output, needle) {
                return Ok(());
            }
            if let Some(status) = self.child.try_wait()? {
                return Err(message(format!(
                    "process exited early with {status}; wanted {:?}; output: {}",
                    String::from_utf8_lossy(needle),
                    visible_excerpt(&self.output)
                )));
            }
            if Instant::now() >= deadline {
                return Err(message(format!(
                    "timed out waiting for {:?}; output: {}",
                    String::from_utf8_lossy(needle),
                    visible_excerpt(&self.output)
                )));
            }
            match self.receiver.recv_timeout(Duration::from_millis(20)) {
                Ok(bytes) => self.output.extend(bytes),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    return self.closed_before(needle);
                }
            }
        }
    }

    fn closed_before(&mut self, needle: &[u8]) -> Result<()> {
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            self.drain_available();
            if let Some(status) = self.child.try_wait()? {
                return Err(message(format!(
                    "terminal output closed after process exited with {status}; wanted {:?}; output: {}",
                    String::from_utf8_lossy(needle),
                    visible_excerpt(&self.output)
                )));
            }
            if Instant::now() >= deadline {
                self.child.kill()?;
                return Err(message(format!(
                    "terminal output closed while process was still running; wanted {:?}; output: {}",
                    String::from_utf8_lossy(needle),
                    visible_excerpt(&self.output)
                )));
            }
            thread::sleep(Duration::from_millis(20));
        }
    }

    fn send(&mut self, bytes: &[u8]) -> Result<()> {
        self.writer.write_all(bytes)?;
        self.writer.flush()?;
        Ok(())
    }

    fn clear_output(&mut self) {
        self.drain_available();
        self.output.clear();
    }

    fn collect_for(&mut self, duration: Duration) -> Result<()> {
        let deadline = Instant::now() + duration;
        while Instant::now() < deadline {
            if let Some(status) = self.child.try_wait()? {
                return Err(message(format!("process exited early with {status}")));
            }
            match self.receiver.recv_timeout(Duration::from_millis(20)) {
                Ok(bytes) => self.output.extend(bytes),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
        self.drain_available();
        Ok(())
    }

    fn finish(mut self, key: &[u8]) -> Result<Vec<u8>> {
        self.send(key)?;
        let deadline = Instant::now() + EXIT_TIMEOUT;
        let status = loop {
            self.drain_available();
            if let Some(status) = self.child.try_wait()? {
                break status;
            }
            if Instant::now() >= deadline {
                self.child.kill()?;
                return Err(message("process did not exit after quit input"));
            }
            thread::sleep(Duration::from_millis(20));
        };
        self.drain_available();
        if !status.success() {
            return Err(message(format!("process exited with {status}")));
        }
        Ok(self.output)
    }

    fn drain_available(&mut self) {
        while let Ok(bytes) = self.receiver.try_recv() {
            self.output.extend(bytes);
        }
    }
}

fn base_session(
    binary: &Path,
    repo: &Path,
    config: &Path,
    rows: u16,
    cols: u16,
    arguments: &[&str],
) -> Result<Session> {
    Session::spawn(binary, repo, config, rows, cols, arguments, &[])
}

fn launch_and_quit(binary: &Path, repo: &Path, config: &Path, rows: u16, cols: u16) -> Result<()> {
    let mut session = base_session(binary, repo, config, rows, cols, &[])?;
    session.wait_for(b"harbor")?;
    session.finish(b"q")?;
    Ok(())
}

fn git_state_render_check(binary: &Path, fixtures: &Fixtures) -> Result<()> {
    let config = fixtures.config("git-states");
    let mut session = base_session(
        binary,
        &fixtures.harbor,
        &config,
        70,
        180,
        &["--reduced-motion", "--poll-interval", "10"],
    )?;
    for label in [
        "calm", "loading", "sealed", "outbound", "incoming", "diverged", "blocked", "local",
        "detached",
    ] {
        session.wait_for(label.as_bytes())?;
    }
    session.finish(b"q")?;
    Ok(())
}

fn unborn_check(binary: &Path, fixtures: &Fixtures) -> Result<()> {
    let mut session = base_session(
        binary,
        &fixtures.unborn,
        &fixtures.config("unborn"),
        20,
        80,
        &["--reduced-motion"],
    )?;
    session.wait_for(b"(no commits yet)")?;
    session.wait_for(b"local")?;
    session.finish(b"q")?;
    Ok(())
}

fn operation_check(binary: &Path, fixtures: &Fixtures) -> Result<()> {
    let mut session = base_session(
        binary,
        &fixtures.operation,
        &fixtures.config("operation"),
        24,
        100,
        &["--reduced-motion"],
    )?;
    session.wait_for(b"blocked")?;
    session.send(b"i")?;
    session.wait_for(b"inspect")?;
    session.wait_for(b"bisect in progress")?;
    session.finish(b"q")?;
    Ok(())
}

fn palette_check(
    binary: &Path,
    fixtures: &Fixtures,
    term: &str,
    colorterm: Option<&str>,
    expected: &[u8],
) -> Result<()> {
    let environment = [
        (OsString::from("TERM"), Some(OsString::from(term))),
        (OsString::from("COLORTERM"), colorterm.map(OsString::from)),
        (OsString::from("NO_COLOR"), None),
    ];
    let mut session = Session::spawn(
        binary,
        &fixtures.keyboard,
        &fixtures.config(&format!("palette-{term}-{}", colorterm.unwrap_or("none"))),
        24,
        80,
        &["--reduced-motion"],
        &environment,
    )?;
    session.wait_for(b"loading")?;
    session.wait_for(expected)?;
    session.finish(b"q")?;
    Ok(())
}

fn reduced_motion_check(binary: &Path, fixtures: &Fixtures) -> Result<()> {
    let mut session = base_session(
        binary,
        &fixtures.keyboard,
        &fixtures.config("reduced-motion"),
        24,
        80,
        &["--reduced-motion", "--poll-interval", "10"],
    )?;
    session.wait_for(b"reduced motion")?;
    session.wait_for(b"loading")?;
    session.clear_output();
    session.collect_for(Duration::from_millis(700))?;
    let visible = strip_terminal_controls(&session.output);
    if visible.iter().any(|byte| !byte.is_ascii_whitespace()) {
        return Err(message(format!(
            "settled reduced-motion frame changed: {}",
            String::from_utf8_lossy(&visible)
        )));
    }
    session.finish(b"q")?;
    Ok(())
}

fn absolute_path(path: &Path) -> Result<PathBuf> {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir()?.join(path)
    };
    fs::metadata(&path).map_err(|error| {
        message(format!(
            "cannot resolve release binary {}: {error}",
            path.display()
        ))
    })?;
    Ok(path)
}

fn overflow_check(binary: &Path, fixtures: &Fixtures) -> Result<()> {
    let config = fixtures.config("overflow-cycle");
    fs::write(&config, "{\"cycle_interval_ms\":1000}\n")?;
    let mut session = base_session(binary, &fixtures.harbor, &config, 8, 100, &["--fps", "30"])?;
    session.wait_for(b"below")?;
    session.clear_output();
    session.wait_for(b"blocked")?;
    session.finish(b"q")?;

    let reduced_config = fixtures.config("overflow-reduced");
    fs::write(&reduced_config, "{\"cycle_interval_ms\":1000}\n")?;
    let mut reduced = base_session(
        binary,
        &fixtures.harbor,
        &reduced_config,
        8,
        100,
        &["--fps", "30", "--reduced-motion"],
    )?;
    reduced.wait_for(b"below")?;
    reduced.clear_output();
    reduced.collect_for(Duration::from_millis(1_400))?;
    if contains(&reduced.output, b"blocked") {
        return Err(message("reduced motion advanced the overflow page"));
    }
    reduced.finish(b"q")?;
    Ok(())
}

fn keyboard_check(binary: &Path, fixtures: &Fixtures) -> Result<()> {
    let config = fixtures.config("keyboard");
    let mut session = base_session(binary, &fixtures.keyboard, &config, 24, 100, &[])?;
    session.wait_for(b"loading")?;
    session.clear_output();
    session.send(b"i")?;
    session.wait_for(b"upstream")?;
    session.send(b"\r")?;
    session.wait_for(b"vessel")?;
    session.send(b"\r")?;
    session.wait_for(b"changed files")?;
    session.wait_for(b"tracked.txt")?;
    session.wait_for(b"untracked.txt")?;
    session.send(b"jk")?;
    session.send(b"l")?;
    session.wait_for(b"Legend")?;
    session.send(b"jk\x1b")?;
    session.send(b"s")?;
    session.wait_for(b"Harbor controls")?;
    session.send(b"\x1b[C")?;
    session.wait_for(b"reduced")?;
    session.send(b"\x1b")?;
    session.collect_for(Duration::from_millis(100))?;
    for _ in 0..3 {
        session.send(b"\x1b")?;
        session.collect_for(Duration::from_millis(100))?;
    }
    let output = session.finish(b"\x1b")?;
    if !contains(&output, b"\x1b[?1049l") {
        return Err(message("terminal did not restore the main screen"));
    }
    let settings = fs::read_to_string(config)?;
    if !settings.contains("\"reduced_motion\": true") {
        return Err(message("motion change was not persisted"));
    }
    Ok(())
}

fn persistence_check(binary: &Path, fixtures: &Fixtures) -> Result<()> {
    let config = fixtures.config("persistence");
    let mut first = base_session(binary, &fixtures.keyboard, &config, 24, 80, &[])?;
    first.wait_for(b"loading")?;
    first.send(b"m")?;
    first.wait_for(b"reduced motion")?;
    first.finish(b"q")?;
    let settings = fs::read_to_string(&config)?;
    if !settings.contains("\"reduced_motion\": true") {
        return Err(message("first launch did not save reduced motion"));
    }

    let mut second = base_session(binary, &fixtures.keyboard, &config, 24, 80, &[])?;
    second.wait_for(b"reduced motion")?;
    second.finish(b"q")?;
    Ok(())
}

fn short_controls_check(binary: &Path, fixtures: &Fixtures) -> Result<()> {
    let mut session = base_session(
        binary,
        &fixtures.keyboard,
        &fixtures.config("short-controls"),
        7,
        60,
        &[],
    )?;
    session.wait_for(b"loading")?;
    session.send(b"s")?;
    session.wait_for(b"Harbor controls")?;
    session.send(b"k")?;
    session.wait_for(b"GitHub survey")?;
    session.finish(b"q")?;
    Ok(())
}

fn github_off_check(binary: &Path, fixtures: &Fixtures) -> Result<()> {
    let shim = install_fake_gh(fixtures)?;
    let _ = fs::remove_file(&fixtures.gh_log);
    let environment = fake_gh_environment(fixtures, &shim, "sentinel")?;
    let mut session = Session::spawn(
        binary,
        &fixtures.harbor,
        &fixtures.config("github-off"),
        30,
        120,
        &[],
        &environment,
    )?;
    session.wait_for(b"calm")?;
    session.collect_for(Duration::from_millis(500))?;
    session.finish(b"q")?;
    if fixtures.gh_log.exists() {
        return Err(message("gh was invoked while the observer was off"));
    }
    Ok(())
}

fn github_happy_check(binary: &Path, fixtures: &Fixtures) -> Result<()> {
    let shim = install_fake_gh(fixtures)?;
    let _ = fs::remove_file(&fixtures.gh_log);
    let environment = fake_gh_environment(fixtures, &shim, "happy")?;
    let mut session = Session::spawn(
        binary,
        &fixtures.harbor,
        &fixtures.config("github-happy"),
        40,
        140,
        &["--github", "--reduced-motion"],
        &environment,
    )?;
    session.wait_for(b"PR#42")?;
    session.wait_for(b"v0.1.0")?;
    session.finish(b"q")?;
    let log = fs::read_to_string(&fixtures.gh_log)?;
    if log.lines().count() != 2 || !log.contains("pr list") || !log.contains("release list") {
        return Err(message(format!("unexpected gh calls: {log:?}")));
    }
    Ok(())
}

fn github_missing_check(binary: &Path, fixtures: &Fixtures) -> Result<()> {
    let empty_path = fixtures.root.join("empty-path");
    fs::create_dir_all(&empty_path)?;
    let environment = [(OsString::from("PATH"), Some(empty_path.into_os_string()))];
    let mut session = Session::spawn(
        binary,
        &fixtures.harbor,
        &fixtures.config("github-missing"),
        30,
        140,
        &["--github", "--reduced-motion"],
        &environment,
    )?;
    session.wait_for(b"cannot run `gh`")?;
    session.wait_for(b"calm")?;
    session.finish(b"q")?;
    Ok(())
}

fn github_unauthenticated_check(binary: &Path, fixtures: &Fixtures) -> Result<()> {
    let shim = install_fake_gh(fixtures)?;
    let _ = fs::remove_file(&fixtures.gh_log);
    let environment = fake_gh_environment(fixtures, &shim, "unauthenticated")?;
    let mut session = Session::spawn(
        binary,
        &fixtures.harbor,
        &fixtures.config("github-unauthenticated"),
        30,
        160,
        &["--github", "--reduced-motion"],
        &environment,
    )?;
    session.wait_for(b"authentication required")?;
    session.wait_for(b"calm")?;
    session.finish(b"q")?;
    Ok(())
}

fn install_fake_gh(fixtures: &Fixtures) -> Result<PathBuf> {
    let extension = env::consts::EXE_EXTENSION;
    let name = if extension.is_empty() {
        "gh".to_string()
    } else {
        format!("gh.{extension}")
    };
    let destination = fixtures.shim_dir.join(name);
    fs::copy(env::current_exe()?, &destination)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(&destination)?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&destination, permissions)?;
    }
    Ok(destination)
}

fn fake_gh_environment(
    fixtures: &Fixtures,
    shim: &Path,
    mode: &str,
) -> Result<Vec<(OsString, Option<OsString>)>> {
    let parent = shim
        .parent()
        .ok_or_else(|| message("gh shim has no parent"))?;
    let existing = env::var_os("PATH").unwrap_or_default();
    let mut paths = vec![parent.to_path_buf()];
    paths.extend(env::split_paths(&existing));
    let path = env::join_paths(paths)?;
    Ok(vec![
        (OsString::from("PATH"), Some(path)),
        (
            OsString::from("GIT_BUOY_FAKE_GH_MODE"),
            Some(OsString::from(mode)),
        ),
        (
            OsString::from("GIT_BUOY_FAKE_GH_LOG"),
            Some(fixtures.gh_log.clone().into_os_string()),
        ),
    ])
}

fn is_fake_gh() -> bool {
    env::var_os("GIT_BUOY_FAKE_GH_MODE").is_some()
        && env::current_exe()
            .ok()
            .and_then(|path| path.file_stem().map(|name| name.to_owned()))
            .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("gh"))
}

fn fake_gh() -> ! {
    let mode = env::var("GIT_BUOY_FAKE_GH_MODE").unwrap_or_default();
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    if let Some(log) = env::var_os("GIT_BUOY_FAKE_GH_LOG") {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(log)
            .unwrap();
        writeln!(file, "{}", arguments.join(" ")).unwrap();
    }
    match mode.as_str() {
        "happy" if arguments.starts_with(&["pr".to_string(), "list".to_string()]) => {
            let json = r##"[{"number":42,"title":"Ready to ship","headRefName":"loading","headRepository":{"name":"git-buoy"},"headRepositoryOwner":{"login":"acceptance"},"isCrossRepository":false,"url":"https://example.invalid/pr/42","isDraft":false,"reviewDecision":"APPROVED","mergeStateStatus":"CLEAN","statusCheckRollup":[{"name":"test","status":"COMPLETED","conclusion":"SUCCESS","detailsUrl":"https://example.invalid/check"}]}]"##;
            println!("{json}");
            std::process::exit(0);
        }
        "happy" if arguments.starts_with(&["release".to_string(), "list".to_string()]) => {
            let json = r##"[{"tagName":"v0.1.0","name":"Git Buoy 0.1.0","isDraft":false,"isPrerelease":false,"isLatest":true,"publishedAt":"2026-07-13T00:00:00Z"}]"##;
            println!("{json}");
            std::process::exit(0);
        }
        "unauthenticated" => {
            eprintln!("authentication required; run gh auth login");
            std::process::exit(4);
        }
        "sentinel" => std::process::exit(0),
        _ => {
            eprintln!("unexpected fake gh invocation: {}", arguments.join(" "));
            std::process::exit(2);
        }
    }
}

fn strip_terminal_controls(bytes: &[u8]) -> Vec<u8> {
    let mut output = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == 0x1b && bytes.get(index + 1) == Some(&b'[') {
            index += 2;
            while index < bytes.len() {
                let byte = bytes[index];
                index += 1;
                if (0x40..=0x7e).contains(&byte) {
                    break;
                }
            }
        } else if bytes[index] < 0x20 || bytes[index] == 0x7f {
            index += 1;
        } else {
            output.push(bytes[index]);
            index += 1;
        }
    }
    output
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    needle.is_empty()
        || haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

fn visible_excerpt(bytes: &[u8]) -> String {
    let visible = strip_terminal_controls(bytes);
    let start = visible.len().saturating_sub(800);
    String::from_utf8_lossy(&visible[start..]).replace(['\r', '\n'], " ")
}

fn message(text: impl Into<String>) -> Box<dyn Error + Send + Sync> {
    Box::new(io::Error::other(text.into()))
}
