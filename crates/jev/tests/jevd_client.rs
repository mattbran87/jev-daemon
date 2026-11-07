//! The blocking client against fake servers (driven by hand) and real
//! daemons. Every wait has a limit, and every test that starts a `jevd`
//! process stops it, also when an assertion fails.

use std::path::{Path, PathBuf};
mod common;
use common::Reaper;

use std::sync::mpsc;
use std::sync::{Arc, Barrier, Mutex};
use std::time::{Duration, Instant};

use jev_proto::client::{connect_or_start, Connection, HELLO_TIMEOUT};
use jev_proto::{ErrorCode, Paths, Request};
use jevd::daemon::{Config, Daemon};
use jevd::jev_http::JevClient;
use jevd::keystore::{client_account, profile_account, KeyStore, MemoryKeyring, StoreError};
use jevd::listen::bind;
use jevd::peer::AdminCheck;
use jevd::settings::Settings;
use serde_json::json;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use zeroize::Zeroizing;

const KEY: &str = "tsk-test-key-0123456789";
const JEVD: &str = env!("CARGO_BIN_EXE_jevd");

/// A keyring whose read of the profile key waits for a release.
struct Gated {
    inner: MemoryKeyring,
    gate: Mutex<mpsc::Receiver<()>>,
}

impl KeyStore for Gated {
    fn get(&self, account: &str) -> Result<Option<Zeroizing<String>>, StoreError> {
        if account.starts_with("profile") {
            let _ = self.gate.lock().unwrap().recv();
        }
        self.inner.get(account)
    }
    fn set(&self, account: &str, secret: &str) -> Result<(), StoreError> {
        self.inner.set(account, secret)
    }
    fn delete(&self, account: &str) -> Result<bool, StoreError> {
        self.inner.delete(account)
    }
}

fn me() -> PathBuf {
    std::env::current_exe().unwrap()
}

fn paths_in(dir: &tempfile::TempDir) -> Paths {
    Paths::under(dir.path()).unwrap()
}

fn write_settings(paths: &Paths, edit: impl FnOnce(&mut Settings)) {
    let mut settings = Settings::default();
    settings.add_profile("default").unwrap();
    edit(&mut settings);
    settings.save(&paths.settings_file()).unwrap();
}

/// A real daemon in this process. Returns its task.
fn start_daemon(
    paths: &Paths,
    store: Arc<dyn KeyStore>,
    edit: impl FnOnce(&mut Settings),
) -> tokio::task::JoinHandle<()> {
    write_settings(paths, edit);
    let jev = JevClient::with_endpoint(
        "http://127.0.0.1:9/",
        Duration::from_secs(5),
        Duration::from_millis(10),
    )
    .unwrap();
    let daemon = Daemon::new(Config {
        settings_file: paths.settings_file(),
        pairing_file: paths.pairing_file(),
        pending_file: paths.pending_file(),
        store,
        jev,
    })
    .unwrap();
    let listener = bind(paths).unwrap();
    // `run` installs signal handlers in this test process, so Ctrl+C may not
    // stop a stuck test binary; every wait in these tests has a limit.
    tokio::spawn(jevd::run::run(daemon, listener, AdminCheck::new(&me())))
}

/// A fake server: for each connection it reads lines, and answers each with
/// `reply` (when `Some`) as a raw line. With `None` it reads and never answers.
fn fake_server(paths: &Paths, reply: Option<&'static str>) -> tokio::task::JoinHandle<()> {
    paths.prepare_runtime_dir().unwrap();
    let mut listener = bind(paths).unwrap();
    tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((stream, _)) = listener.accept().await {
            let (read, mut write) = tokio::io::split(stream);
            held.push(tokio::spawn(async move {
                let mut lines = BufReader::new(read).lines();
                while let Ok(Some(_)) = lines.next_line().await {
                    if let Some(reply) = reply {
                        let _ = write.write_all(format!("{reply}\n").as_bytes()).await;
                    }
                }
            }));
        }
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_server_with_another_protocol_is_protocol_unsupported() {
    let dir = tempfile::tempdir().unwrap();
    let paths = paths_in(&dir);
    let server = fake_server(&paths, Some(r#"{"ok":true,"protocol":2}"#));
    let error = tokio::task::spawn_blocking(move || Connection::connect(&paths).err())
        .await
        .unwrap()
        .expect("connect must fail");
    assert_eq!(error.code.known(), Some(ErrorCode::ProtocolUnsupported));
    server.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_server_that_never_answers_hello_times_out_at_the_hello_limit() {
    let dir = tempfile::tempdir().unwrap();
    let paths = paths_in(&dir);
    let server = fake_server(&paths, None);
    let started = Instant::now();
    let error = tokio::task::spawn_blocking(move || Connection::connect(&paths).err())
        .await
        .unwrap()
        .expect("connect must fail");
    let took = started.elapsed();
    assert_eq!(error.code.known(), Some(ErrorCode::DaemonUnavailable));
    assert!(
        took >= HELLO_TIMEOUT - Duration::from_millis(500),
        "{took:?}"
    );
    assert!(took < Duration::from_secs(15), "{took:?}");
    server.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_daemon_that_is_full_does_not_get_a_second_daemon() {
    let dir = tempfile::tempdir().unwrap();
    let paths = paths_in(&dir);
    let task = start_daemon(&paths, Arc::new(MemoryKeyring::new()), |settings| {
        *settings = settings
            .with_change("max-connections", "1")
            .unwrap()
            .settings;
    });
    let missing = dir.path().join("no-such-jevd");
    let result = tokio::task::spawn_blocking(move || {
        let _first = Connection::connect(&paths).unwrap();
        let started = Instant::now();
        let error = connect_or_start(&paths, &missing).err();
        (error, started.elapsed())
    })
    .await
    .unwrap();
    let (error, took) = result;
    let error = error.expect("the second connection is refused");
    assert!(
        error.message.contains("max-connections"),
        "not the daemon's own refusal: {}",
        error.message
    );
    assert!(took < Duration::from_secs(3), "{took:?}");
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn connect_if_running_reports_a_full_daemon_as_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let paths = paths_in(&dir);
    let task = start_daemon(&paths, Arc::new(MemoryKeyring::new()), |settings| {
        *settings = settings
            .with_change("max-connections", "1")
            .unwrap()
            .settings;
    });
    let result = tokio::task::spawn_blocking(move || {
        let _first = Connection::connect(&paths).unwrap();
        match Connection::connect_if_running(&paths) {
            Ok(None) => Err("no daemon"),
            Ok(Some(_)) => Err("a second connection"),
            Err(error) => Ok(error),
        }
    })
    .await
    .unwrap();
    task.abort();
    let error = result.expect("a full daemon is an error");
    assert!(
        error.message.contains("max-connections"),
        "{}",
        error.message
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_call_that_times_out_poisons_the_connection() {
    let dir = tempfile::tempdir().unwrap();
    let paths = paths_in(&dir);
    let (release, gate) = mpsc::channel();
    let store = Arc::new(Gated {
        inner: MemoryKeyring::new(),
        gate: Mutex::new(gate),
    });
    store.set(&profile_account("default"), KEY).unwrap();
    let task = start_daemon(&paths, store.clone(), |_| {});
    let work_paths = paths.clone();
    let outcome = tokio::task::spawn_blocking(move || {
        let mut connection = Connection::connect(&work_paths).unwrap();
        // Pair a CLI client (the test binary counts as jev).
        let pair = connection
            .call(&Request::Pair {
                name: "app".into(),
                profile: None,
            })
            .unwrap();
        let code = pair.data["code"].as_str().unwrap().to_string();
        let approved = connection
            .call(&Request::Approve {
                code,
                profile: "default".into(),
            })
            .unwrap();
        let token = approved.data["token"].as_str().unwrap().to_string();
        // What jev does: write the keyring first, then commit.
        store.set(&client_account("app"), &token).unwrap();
        let committed = connection
            .call(&Request::PairStatus {
                request_id: approved.data["request_id"].as_str().unwrap().to_string(),
            })
            .unwrap();
        assert!(committed.ok, "{committed:?}");
        let ask = Request::Ask {
            token,
            state: json!({"note": "the state"}),
            questions: json!({"q": {"type": "noul", "instructions": "Done?"}}),
        };
        connection.set_timeout(Duration::from_secs(1));
        let first = connection.call(&ask).err();
        let started = Instant::now();
        let second = connection.call(&Request::Status).err();
        (first, second, started.elapsed())
    })
    .await
    .unwrap();
    // Release the gate, also when an assertion below fails.
    drop(release);
    let (first, second, quick) = outcome;
    let first = first.expect("the first call must time out");
    assert_eq!(first.code.known(), Some(ErrorCode::DaemonUnavailable));
    assert!(
        first.message.contains("did not answer in time"),
        "{}",
        first.message
    );
    let second = second.expect("the next call must fail");
    assert_eq!(second.code.known(), Some(ErrorCode::DaemonUnavailable));
    assert!(
        second.message.contains("timed out"),
        "not the timed-out flag: {}",
        second.message
    );
    assert!(quick < Duration::from_millis(500), "{quick:?}");
    task.abort();
}

#[test]
fn a_daemon_that_cannot_start_comes_back_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let paths = paths_in(&dir);
    let _reaper = Reaper(paths.clone());
    std::fs::create_dir_all(&paths.config_dir).unwrap();
    std::fs::write(paths.settings_file(), "idle-timeout = 5\n").unwrap();
    let started = Instant::now();
    let error = connect_or_start(&paths, Path::new(JEVD)).err().unwrap();
    // jevd binds the channel before it reads the settings, so a client can
    // connect to a daemon that then exits; it still reports the exit status.
    assert!(
        error.message.contains("jevd exited with status"),
        "{}",
        error.message
    );
    assert_eq!(error.code.known(), Some(ErrorCode::DaemonUnavailable));
    assert!(started.elapsed() < Duration::from_secs(9));
}

#[test]
fn two_clients_that_start_at_once_both_connect() {
    let dir = tempfile::tempdir().unwrap();
    let paths = paths_in(&dir);
    let _reaper = Reaper(paths.clone());
    let barrier = Arc::new(Barrier::new(2));
    let threads: Vec<_> = (0..2)
        .map(|_| {
            let paths = paths.clone();
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                let mut connection = connect_or_start(&paths, Path::new(JEVD))?;
                connection.call(&Request::Status).map(|reply| reply.ok)
            })
        })
        .collect();
    for thread in threads {
        let ok = thread.join().unwrap().expect("connect_or_start works");
        assert!(ok, "status is ok");
    }
    let pid = std::fs::read_to_string(paths.pid_file()).unwrap();
    assert!(pid.trim().parse::<u32>().is_ok(), "{pid:?}");
}
