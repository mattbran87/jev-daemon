//! The state rule (jev-tray spec section 6) against a daemon in this
//! process, and against fake servers built on jevd's own listener.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use jev_proto::{Paths, PROTOCOL_VERSION};
use jev_tray::state::{check, check_within, Checker, State, CHECK_LIMIT};
use jevd::daemon::{Config, Daemon};
use jevd::jev_http::JevClient;
use jevd::keystore::MemoryKeyring;
use jevd::listen::bind;
use jevd::peer::AdminCheck;
use jevd::settings::Settings;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

fn paths_in(dir: &tempfile::TempDir) -> Paths {
    Paths::under(dir.path()).unwrap()
}

/// A real daemon in this process. Returns the daemon and its task.
fn start_daemon(paths: &Paths) -> (Arc<Daemon>, tokio::task::JoinHandle<()>) {
    let mut settings = Settings::default();
    settings.add_profile("default").unwrap();
    settings.save(&paths.settings_file()).unwrap();
    let jev = JevClient::with_endpoint(
        "http://127.0.0.1:9/",
        Duration::from_secs(5),
        Duration::from_millis(10),
    )
    .unwrap();
    let daemon = Daemon::new(Config {
        settings_file: paths.settings_file(),
        pairing_file: paths.pairing_file(),
        store: Arc::new(MemoryKeyring::new()),
        jev,
    })
    .unwrap();
    let listener = bind(paths).unwrap();
    let me = std::env::current_exe().unwrap();
    let task = tokio::spawn(jevd::run::run(
        Arc::clone(&daemon),
        listener,
        AdminCheck::new(&me),
    ));
    (daemon, task)
}

/// A fake server: it answers `hello` as jevd does, then answers the next
/// line with `then` (when `Some`), or never (when `None`).
fn fake_server(paths: &Paths, then: Option<&'static str>) -> tokio::task::JoinHandle<()> {
    paths.prepare_runtime_dir().unwrap();
    let mut listener = bind(paths).unwrap();
    tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((stream, _)) = listener.accept().await {
            let (read, mut write) = tokio::io::split(stream);
            held.push(tokio::spawn(async move {
                let mut lines = BufReader::new(read).lines();
                let hello = format!("{{\"ok\":true,\"protocol\":{PROTOCOL_VERSION}}}\n");
                if let Ok(Some(_)) = lines.next_line().await {
                    let _ = write.write_all(hello.as_bytes()).await;
                }
                while let Ok(Some(_)) = lines.next_line().await {
                    if let Some(reply) = then {
                        let _ = write.write_all(format!("{reply}\n").as_bytes()).await;
                    }
                }
            }));
        }
    })
}

/// A fake server that accepts each connection, holds it open, and never
/// answers, not even `hello`. Returns the server task and the number of
/// connections it has accepted.
fn silent_server(paths: &Paths) -> (tokio::task::JoinHandle<()>, Arc<AtomicUsize>) {
    paths.prepare_runtime_dir().unwrap();
    let mut listener = bind(paths).unwrap();
    let accepted = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&accepted);
    let task = tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((stream, _)) = listener.accept().await {
            counter.fetch_add(1, Ordering::SeqCst);
            held.push(stream);
        }
    });
    (task, accepted)
}

async fn check_off_thread(paths: &Paths) -> State {
    let paths = paths.clone();
    tokio::task::spawn_blocking(move || check(&paths))
        .await
        .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_running_daemon_is_on() {
    let dir = tempfile::tempdir().unwrap();
    let paths = paths_in(&dir);
    let (_daemon, task) = start_daemon(&paths);
    let state = check_off_thread(&paths).await;
    task.abort();
    assert_eq!(state, State::On);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_stopped_daemon_is_off() {
    let dir = tempfile::tempdir().unwrap();
    let paths = paths_in(&dir);
    let (daemon, task) = start_daemon(&paths);
    assert_eq!(check_off_thread(&paths).await, State::On);
    daemon.request_stop();
    tokio::time::timeout(Duration::from_secs(10), task)
        .await
        .expect("the daemon stops")
        .unwrap();
    assert_eq!(check_off_thread(&paths).await, State::Off);
}

#[test]
fn no_channel_is_off() {
    let dir = tempfile::tempdir().unwrap();
    let paths = paths_in(&dir);
    let started = Instant::now();
    assert_eq!(check(&paths), State::Off);
    paths.prepare_runtime_dir().unwrap();
    assert_eq!(check(&paths), State::Off);
    let took = started.elapsed();
    // Well under the 5 s limit, so the off comes fast, not from a timeout;
    // wide enough for a slow, busy machine.
    assert!(took < Duration::from_secs(3), "{took:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_error_reply_is_off() {
    let dir = tempfile::tempdir().unwrap();
    let paths = paths_in(&dir);
    let server = fake_server(
        &paths,
        Some(r#"{"ok":false,"error":{"code":"internal","message":"no"}}"#),
    );
    let started = Instant::now();
    let state = check_off_thread(&paths).await;
    let took = started.elapsed();
    server.abort();
    assert_eq!(state, State::Off);
    // Under the 5 s limit, so the off comes from the reply.
    assert!(took < Duration::from_secs(3), "{took:?}");
}

/// The positive control for `an_error_reply_is_off`: the same fake with a
/// success reply is on.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_success_reply_from_the_fake_is_on() {
    let dir = tempfile::tempdir().unwrap();
    let paths = paths_in(&dir);
    let server = fake_server(&paths, Some(r#"{"ok":true}"#));
    let state = check_off_thread(&paths).await;
    server.abort();
    assert_eq!(state, State::On);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_reply_that_never_comes_is_off_after_the_limit() {
    let dir = tempfile::tempdir().unwrap();
    let paths = paths_in(&dir);
    let server = fake_server(&paths, None);
    let started = Instant::now();
    let state = check_off_thread(&paths).await;
    let took = started.elapsed();
    server.abort();
    assert_eq!(state, State::Off);
    assert!(took >= CHECK_LIMIT - Duration::from_millis(100), "{took:?}");
    assert!(took < CHECK_LIMIT + Duration::from_secs(3), "{took:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_shorter_limit_holds_too() {
    let dir = tempfile::tempdir().unwrap();
    let paths = paths_in(&dir);
    let server = fake_server(&paths, None);
    let check_paths = paths.clone();
    let started = Instant::now();
    let state =
        tokio::task::spawn_blocking(move || check_within(&check_paths, Duration::from_millis(300)))
            .await
            .unwrap();
    let took = started.elapsed();
    server.abort();
    assert_eq!(state, State::Off);
    assert!(took < Duration::from_secs(2), "{took:?}");
}

/// `hello` alone waits up to 5 s; the check still answers at its limit.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn no_hello_reply_is_off_at_the_limit() {
    let dir = tempfile::tempdir().unwrap();
    let paths = paths_in(&dir);
    let (server, _accepted) = silent_server(&paths);
    let check_paths = paths.clone();
    let started = Instant::now();
    let state =
        tokio::task::spawn_blocking(move || check_within(&check_paths, Duration::from_millis(300)))
            .await
            .unwrap();
    let took = started.elapsed();
    server.abort();
    assert_eq!(state, State::Off);
    assert!(took < Duration::from_secs(2), "{took:?}");
}

/// While a check hangs, the next check is off at once and starts no thread.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_checker_runs_one_check_at_a_time() {
    let dir = tempfile::tempdir().unwrap();
    let paths = paths_in(&dir);
    let (server, accepted) = silent_server(&paths);
    let check_paths = paths.clone();
    let (first, first_took, second, second_took) = tokio::task::spawn_blocking(move || {
        let mut checker = Checker::new();
        let limit = Duration::from_millis(300);
        let started = Instant::now();
        let first = checker.check(&check_paths, limit);
        let first_took = started.elapsed();
        let started = Instant::now();
        let second = checker.check(&check_paths, limit);
        let second_took = started.elapsed();
        (first, first_took, second, second_took)
    })
    .await
    .unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;
    let connections = accepted.load(Ordering::SeqCst);
    server.abort();
    assert_eq!(first, State::Off);
    assert!(first_took >= Duration::from_millis(250), "{first_took:?}");
    assert!(first_took < Duration::from_secs(2), "{first_took:?}");
    assert_eq!(second, State::Off);
    assert!(second_took < Duration::from_millis(100), "{second_took:?}");
    assert_eq!(connections, 1);
}
