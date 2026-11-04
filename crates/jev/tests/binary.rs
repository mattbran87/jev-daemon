//! The real `jev` binary, with the real `jevd` next to it in the target
//! folder, under a new `JEV_HOME`. No command here touches the keyring.
//!
//! The `jev` package owns both binaries, so cargo builds `jevd` for these
//! tests, next to `jev`, as a release archive holds them.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use jev_proto::client::Connection;
use jev_proto::paths::HOME_VAR;
use jev_proto::Paths;
use serde_json::Value;

const JEV: &str = env!("CARGO_BIN_EXE_jev");
const JEVD: &str = env!("CARGO_BIN_EXE_jevd");
/// How long one `jev` command may take, with its pipes closed.
const DEADLINE: Duration = Duration::from_secs(30);

fn jevd_next_to_jev() -> PathBuf {
    let jevd = PathBuf::from(JEVD);
    assert_eq!(
        jevd.parent(),
        Path::new(JEV).parent(),
        "cargo puts jevd next to jev"
    );
    jevd
}

/// Runs `program` and reads its output until both pipes close, for at most
/// `DEADLINE`. A `jevd` that inherited the pipes would hold them open.
fn try_run(program: &Path, home: &Path, args: &[&str]) -> Result<Output, String> {
    let child = Command::new(program)
        .args(args)
        .env(HOME_VAR, home)
        .env_remove("JEV_CLIENT")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("cannot run {}: {error}", program.display()))?;
    let pid = child.id();
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(child.wait_with_output());
    });
    match receiver.recv_timeout(DEADLINE) {
        Ok(Ok(output)) => Ok(output),
        Ok(Err(error)) => Err(format!("cannot read the output of jev: {error}")),
        Err(_) => {
            kill(pid);
            Err("jev held its output pipes open; did jevd inherit them?".to_string())
        }
    }
}

/// Kills the process `pid`, so a test that times out leaves no `jev` behind.
/// Errors are ignored: the process can have exited already.
fn kill(pid: u32) {
    let pid = pid.to_string();
    #[cfg(windows)]
    let _ = Command::new("taskkill")
        .args(["/F", "/PID", &pid])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    #[cfg(unix)]
    let _ = Command::new("kill")
        .args(["-KILL", &pid])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

fn run(program: &Path, home: &Path, args: &[&str]) -> Output {
    try_run(program, home, args).unwrap_or_else(|message| panic!("{message}"))
}

/// A new `JEV_HOME`. When dropped, also after a failed assert, it stops the
/// daemon under it, waits until the daemon is gone, and removes the folder,
/// each for a bounded time. It never panics.
struct Stopper {
    dir: tempfile::TempDir,
    paths: Paths,
}

impl Stopper {
    fn new() -> Stopper {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::under(dir.path()).unwrap();
        Stopper { dir, paths }
    }

    fn home(&self) -> &Path {
        self.dir.path()
    }
}

impl Drop for Stopper {
    fn drop(&mut self) {
        let _ = try_run(Path::new(JEV), self.home(), &["stop"]);
        let deadline = Instant::now() + Duration::from_secs(10);
        while !stopped(&self.paths) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        // A jevd that is still exiting can hold its log file for a moment.
        while std::fs::remove_dir_all(self.home()).is_err()
            && self.home().exists()
            && Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

/// True when no daemon answers and its PID file is empty (Unix) or gone
/// (Windows), so the jevd process has ended its exit.
fn stopped(paths: &Paths) -> bool {
    if Connection::connect(paths).is_ok() {
        return false;
    }
    match std::fs::metadata(paths.pid_file()) {
        Err(_) => true,
        Ok(file) => cfg!(unix) && file.len() == 0,
    }
}

fn wait_until_stopped(paths: &Paths) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !stopped(paths) {
        assert!(Instant::now() < deadline, "jevd did not stop in 10 s");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn version_and_bad_command_lines() {
    let dir = tempfile::tempdir().unwrap();
    let version = run(Path::new(JEV), dir.path(), &["--version"]);
    assert!(version.status.success());
    assert_eq!(
        String::from_utf8_lossy(&version.stdout).trim(),
        format!("jev {}", env!("CARGO_PKG_VERSION"))
    );
    assert_eq!(
        run(Path::new(JEV), dir.path(), &["nope"]).status.code(),
        Some(2)
    );
    assert_eq!(run(Path::new(JEV), dir.path(), &[]).status.code(), Some(2));
}

#[test]
fn with_no_jev_folders_version_works_and_ask_prints_json() {
    // With no JEV_HOME and no APPDATA (Windows) or HOME (Unix), jev cannot
    // find its folders.
    let run_bare = |args: &[&str]| {
        let mut command = Command::new(JEV);
        command
            .args(args)
            .env_remove(HOME_VAR)
            .env_remove("JEV_CLIENT")
            .env_remove("APPDATA")
            .env_remove("HOME")
            .stdin(Stdio::null());
        command.output().unwrap()
    };
    let version = run_bare(&["--version"]);
    assert!(
        version.status.success(),
        "{}",
        String::from_utf8_lossy(&version.stderr)
    );
    let ask = run_bare(&["ask", "--client", "app", "--request", "{}"]);
    assert_eq!(ask.status.code(), Some(1));
    let shown: Value = serde_json::from_slice(&ask.stdout).unwrap_or_else(|error| {
        panic!(
            "ask printed no JSON ({error}): {}",
            String::from_utf8_lossy(&ask.stdout)
        )
    });
    assert_eq!(shown["ok"], Value::Bool(false), "{shown}");
    assert_eq!(shown["error"]["code"], "internal", "{shown}");
}

#[test]
fn the_real_jev_starts_and_stops_jevd() {
    jevd_next_to_jev();
    let home = Stopper::new();
    let dir = home.home();
    let start = run(Path::new(JEV), dir, &["start"]);
    assert!(
        start.status.success(),
        "{}",
        String::from_utf8_lossy(&start.stderr)
    );
    let status = run(Path::new(JEV), dir, &["status", "--json"]);
    assert!(status.status.success());
    let shown: Value = serde_json::from_slice(&status.stdout).unwrap_or_else(|error| {
        panic!(
            "status --json is not JSON ({error}): {}",
            String::from_utf8_lossy(&status.stdout)
        )
    });
    assert_eq!(shown["ok"], Value::Bool(true), "{shown}");
    assert_eq!(
        shown["daemon_version"],
        Value::from(env!("CARGO_PKG_VERSION")),
        "{shown}"
    );
    assert!(shown["uptime"].is_u64(), "{shown}");
    let stop = run(Path::new(JEV), dir, &["stop"]);
    assert!(stop.status.success(), "the real jev passes the peer check");
    wait_until_stopped(&home.paths);
}

#[test]
fn a_copy_of_jev_in_another_folder_cannot_stop_jevd() {
    jevd_next_to_jev();
    let home = Stopper::new();
    let dir = home.home();
    let elsewhere = tempfile::tempdir().unwrap();
    let copy = elsewhere.path().join(Path::new(JEV).file_name().unwrap());
    std::fs::copy(JEV, &copy).unwrap();
    assert!(run(Path::new(JEV), dir, &["start"]).status.success());
    let stop = run(&copy, dir, &["stop"]);
    assert_eq!(
        stop.status.code(),
        Some(9),
        "{}",
        String::from_utf8_lossy(&stop.stderr)
    );
    assert!(String::from_utf8_lossy(&stop.stderr).contains("forbidden"));
}
