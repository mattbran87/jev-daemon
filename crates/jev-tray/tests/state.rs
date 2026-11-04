//! The state rule (jev-tray spec section 6) against a daemon in this
//! process, and against fake servers built on jevd's own listener.

use std::sync::Arc;
use std::time::{Duration, Instant};

use jev_proto::{Paths, PROTOCOL_VERSION};
use jev_tray::state::{check, check_within, State, CHECK_LIMIT};
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
    assert_eq!(check(&paths), State::Off);
    paths.prepare_runtime_dir().unwrap();
    assert_eq!(check(&paths), State::Off);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_error_reply_is_off() {
    let dir = tempfile::tempdir().unwrap();
    let paths = paths_in(&dir);
    let server = fake_server(
        &paths,
        Some(r#"{"ok":false,"error":{"code":"internal","message":"no"}}"#),
    );
    let state = check_off_thread(&paths).await;
    server.abort();
    assert_eq!(state, State::Off);
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
    assert!(took < CHECK_LIMIT + Duration::from_secs(1), "{took:?}");
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
