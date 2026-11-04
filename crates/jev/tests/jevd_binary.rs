//! The real `jevd` binary, under a new `JEV_HOME`. These tests never send an
//! `ask`, so the OS keyring and the real Jev endpoint stay untouched.

use std::path::Path;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

mod common;
use common::Reaper;
use jev_proto::client::{connect_or_start, Connection};
use jev_proto::paths::HOME_VAR;
use jev_proto::{ErrorCode, Paths, Request};

const JEVD: &str = env!("CARGO_BIN_EXE_jevd");

fn spawn(home: &Path) -> Child {
    Command::new(JEVD).env(HOME_VAR, home).spawn().unwrap()
}

/// Kills and reaps its children when it drops, also after a panic.
struct KillOnDrop(Vec<Child>);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        for child in &mut self.0 {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Waits until `child` exits, for at most `limit`.
fn exited(child: &mut Child, limit: Duration) -> Option<std::process::ExitStatus> {
    let deadline = Instant::now() + limit;
    while Instant::now() < deadline {
        if let Some(status) = child.try_wait().unwrap() {
            return Some(status);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    None
}

fn connect(paths: &Paths) -> Connection {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match Connection::connect(paths) {
            Ok(connection) => return connection,
            Err(_) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            Err(error) => panic!("no daemon: {error:?}"),
        }
    }
}

#[test]
fn version_prints_and_exits() {
    let output = Command::new(JEVD).arg("--version").output().unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert_eq!(text.trim(), format!("jevd {}", env!("CARGO_PKG_VERSION")));
}

#[test]
fn of_two_daemons_started_at_once_exactly_one_survives() {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::under(dir.path()).unwrap();
    let mut children = KillOnDrop(vec![spawn(dir.path()), spawn(dir.path())]);
    let mut connection = connect(&paths);
    let status = connection.call(&Request::Status).unwrap();
    assert!(status.ok);
    let a_done = exited(&mut children.0[0], Duration::from_secs(5));
    let b_done = exited(&mut children.0[1], Duration::from_millis(200));
    let loser = match (a_done, b_done) {
        (Some(status), None) | (None, Some(status)) => status,
        other => panic!("expected exactly one exit, got {other:?}"),
    };
    assert!(loser.success(), "the second daemon exits with success");
    assert!(connection.call(&Request::Status).unwrap().ok);
    // The test binary is not jev, so stop is forbidden; the guard ends the winner.
    let stop = connection.call(&Request::Stop).unwrap();
    assert_eq!(
        stop.error.and_then(|error| error.code.known()),
        Some(ErrorCode::Forbidden)
    );
}

#[test]
fn connect_or_start_starts_jevd_under_paths_home() {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::under(dir.path()).unwrap();
    let _reaper = Reaper(paths.clone());
    // `paths.home` reaches the child as JEV_HOME; this process sets nothing.
    let mut connection = connect_or_start(&paths, Path::new(JEVD)).unwrap();
    assert!(connection.call(&Request::Status).unwrap().ok);
    let pid = std::fs::read_to_string(paths.pid_file()).unwrap();
    assert!(pid.trim().parse::<u32>().is_ok(), "{pid:?}");
}
