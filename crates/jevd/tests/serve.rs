//! The daemon over its real channel, in this process, with a fake Jev and an
//! in-memory keyring. The test binary counts as `jev`, unless a test says not,
//! so each `pair` here is a CLI request. The daemon tests in `src/daemon.rs`
//! cover the socket-client flow, with `Peer::Other`.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use jev_proto::client::Connection;
use jev_proto::{ErrorCode, Paths, Reply, Request};
use jevd::daemon::{Config, Daemon};
use jevd::jev_http::JevClient;
use jevd::keystore::{client_account, profile_account, KeyStore, MemoryKeyring};
use jevd::listen::bind;
use jevd::peer::AdminCheck;
use jevd::settings::Settings;
use serde_json::{json, Value};
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

const KEY: &str = "tsk-test-key-0123456789";

struct Running {
    _dir: tempfile::TempDir,
    paths: Paths,
    store: Arc<MemoryKeyring>,
    task: tokio::task::JoinHandle<()>,
}

/// Starts a daemon under a new `JEV_HOME`. `admin` is the binary that counts
/// as `jev`, and `edit` changes the settings before the start.
async fn start(jev_url: &str, admin: &Path, edit: impl FnOnce(&mut Settings)) -> Running {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::under(dir.path()).unwrap();
    let mut settings = Settings::default();
    settings.add_profile("default").unwrap();
    edit(&mut settings);
    settings.save(&paths.settings_file()).unwrap();
    let store = Arc::new(MemoryKeyring::new());
    store.set(&profile_account("default"), KEY).unwrap();
    let jev = JevClient::with_endpoint(jev_url, Duration::from_secs(5), Duration::from_millis(10))
        .unwrap();
    let daemon = Daemon::new(Config {
        settings_file: paths.settings_file(),
        pairing_file: paths.pairing_file(),
        store: store.clone(),
        jev,
    })
    .unwrap();
    let listener = bind(&paths).unwrap();
    // `run` installs signal handlers in this test process, so Ctrl+C may not
    // stop a stuck test binary; every wait in these tests has a limit.
    let task = tokio::spawn(jevd::run::run(daemon, listener, AdminCheck::new(admin)));
    Running {
        _dir: dir,
        paths,
        store,
        task,
    }
}

fn me() -> std::path::PathBuf {
    std::env::current_exe().unwrap()
}

async fn jev(delay: Duration) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(delay)
                .set_body_json(json!({"answers": {"q": {"type": "noul", "noul": 0.25}}})),
        )
        .mount(&server)
        .await;
    server
}

/// Runs blocking client calls off the runtime.
async fn calls(paths: &Paths, requests: Vec<Request>) -> Vec<Reply> {
    let paths = paths.clone();
    tokio::task::spawn_blocking(move || {
        let mut connection = Connection::connect(&paths).unwrap();
        requests
            .iter()
            .map(|request| connection.call(request).unwrap())
            .collect()
    })
    .await
    .unwrap()
}

async fn call(paths: &Paths, request: Request) -> Reply {
    calls(paths, vec![request]).await.remove(0)
}

fn code(reply: &Reply) -> Option<ErrorCode> {
    reply.error.as_ref().and_then(|error| error.code.known())
}

fn text(reply: &Reply, field: &str) -> String {
    reply.data[field].as_str().unwrap().to_string()
}

fn ask(token: &str) -> Request {
    Request::Ask {
        token: token.into(),
        state: json!({"note": "the state"}),
        questions: json!({"q": {"type": "noul", "question": "Done?"}}),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_cli_client_stores_its_token_before_the_commit() {
    let server = jev(Duration::ZERO).await;
    let daemon = start(&server.uri(), &me(), |_| {}).await;
    let pair = Request::Pair {
        name: "script".into(),
        profile: None,
    };
    let replies = calls(&daemon.paths, vec![pair.clone(), pair]).await;
    assert!(replies[0].ok, "{:?}", replies[0]);
    assert_eq!(
        replies[0].data, replies[1].data,
        "a repeat gets the same code"
    );
    let approved = call(
        &daemon.paths,
        Request::Approve {
            code: text(&replies[0], "code"),
            profile: "default".into(),
        },
    )
    .await;
    let token = text(&approved, "token");
    // What jev does: write the keyring first, then commit.
    daemon.store.set(&client_account("script"), &token).unwrap();
    let early = call(&daemon.paths, ask(&token)).await;
    assert_eq!(code(&early), Some(ErrorCode::NotPaired), "{early:?}");
    let committed = call(
        &daemon.paths,
        Request::PairStatus {
            request_id: text(&approved, "request_id"),
        },
    )
    .await;
    assert_eq!(committed.data.get("state"), Some(&json!("approved")));
    assert!(committed.data.get("token").is_none());
    let stored = daemon
        .store
        .get(&client_account("script"))
        .unwrap()
        .unwrap();
    assert_eq!(
        stored.as_str(),
        token,
        "the stored token is the approved one"
    );
    let answer = call(&daemon.paths, ask(&token)).await;
    assert_eq!(answer.data["answers"]["q"]["noul"], 0.25);
    let revoked = calls(
        &daemon.paths,
        vec![
            Request::Revoke {
                name: "script".into(),
            },
            ask(&stored),
        ],
    )
    .await;
    assert_eq!(code(&revoked[1]), Some(ErrorCode::Revoked));
    daemon.task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn admin_messages_from_another_binary_are_forbidden() {
    let daemon = start("http://127.0.0.1:9/", Path::new("/no/such/jev"), |_| {}).await;
    let replies = calls(
        &daemon.paths,
        vec![
            Request::Pending,
            Request::Approve {
                code: "ABCD2345".into(),
                profile: "default".into(),
            },
            Request::Stop,
        ],
    )
    .await;
    for reply in &replies {
        assert_eq!(code(reply), Some(ErrorCode::Forbidden));
    }
    assert!(
        call(&daemon.paths, Request::Status).await.ok,
        "still running"
    );
    daemon.task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_stop_lets_the_request_in_flight_finish_then_cleans_up() {
    let server = jev(Duration::from_millis(800)).await;
    let daemon = start(&server.uri(), &me(), |_| {}).await;
    let token = {
        let pair = call(
            &daemon.paths,
            Request::Pair {
                name: "app".into(),
                profile: None,
            },
        )
        .await;
        let approved = call(
            &daemon.paths,
            Request::Approve {
                code: text(&pair, "code"),
                profile: "default".into(),
            },
        )
        .await;
        call(
            &daemon.paths,
            Request::PairStatus {
                request_id: text(&approved, "request_id"),
            },
        )
        .await;
        text(&approved, "token")
    };
    let paths = daemon.paths.clone();
    let in_flight = tokio::spawn(async move { call(&paths, ask(&token)).await });
    // Wait until the fake Jev holds the ask, then stop.
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while server
        .received_requests()
        .await
        .unwrap_or_default()
        .is_empty()
    {
        assert!(
            std::time::Instant::now() < deadline,
            "the ask never reached the fake Jev"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(call(&daemon.paths, Request::Stop).await.ok);
    let answer = in_flight.await.unwrap();
    assert!(answer.ok, "the ask in flight finished: {answer:?}");
    tokio::time::timeout(Duration::from_secs(5), daemon.task)
        .await
        .expect("the daemon stops after the drain")
        .unwrap();
    // Unix empties the PID file and keeps it; Windows deletes it.
    let pid = std::fs::read_to_string(daemon.paths.pid_file()).unwrap_or_default();
    assert!(
        pid.trim().is_empty(),
        "the PID file is gone or empty: {pid:?}"
    );
    let after = Connection::connect(&daemon.paths).err().unwrap();
    assert_eq!(after.code.known(), Some(ErrorCode::DaemonUnavailable));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_connection_over_max_connections_is_refused() {
    let daemon = start("http://127.0.0.1:9/", &me(), |settings| {
        *settings = settings
            .with_change("max-connections", "1")
            .unwrap()
            .settings;
    })
    .await;
    let paths = daemon.paths.clone();
    let result = tokio::task::spawn_blocking(move || {
        let _first = Connection::connect(&paths).unwrap();
        Connection::connect(&paths).err()
    })
    .await
    .unwrap();
    let error = result.expect("the second connection is refused");
    assert_eq!(error.code.known(), Some(ErrorCode::DaemonUnavailable));
    assert!(
        error.message.contains("max-connections"),
        "{}",
        error.message
    );
    daemon.task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_line_over_the_cap_leaves_the_daemon_running() {
    let daemon = start("http://127.0.0.1:9/", &me(), |_| {}).await;
    let huge = Request::Ask {
        token: "jevt_x".into(),
        state: Value::String("x".repeat(jevd::conn::MAX_LINE)),
        questions: json!({"q": {"type": "noul"}}),
    };
    let paths = daemon.paths.clone();
    let reply = tokio::task::spawn_blocking(move || {
        let mut connection = Connection::connect(&paths).unwrap();
        connection.call(&huge).unwrap()
    })
    .await
    .unwrap();
    assert_eq!(code(&reply), Some(ErrorCode::TooLarge));
    assert!(call(&daemon.paths, Request::Status).await.ok);
    daemon.task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_copy_of_jev_in_another_folder_does_not_count() {
    let folder = tempfile::tempdir().unwrap();
    let copy = folder.path().join(me().file_name().unwrap());
    std::fs::copy(me(), &copy).unwrap();
    let daemon = start("http://127.0.0.1:9/", &copy, |_| {}).await;
    let reply = call(
        &daemon.paths,
        Request::Approve {
            code: "ABCD2345".into(),
            profile: "default".into(),
        },
    )
    .await;
    assert_eq!(code(&reply), Some(ErrorCode::Forbidden), "{reply:?}");
    daemon.task.abort();
}
