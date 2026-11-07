//! `jev` commands against a jevd that runs in this process, on its real
//! channel, with a shared in-memory keyring and a fake Jev. The test binary
//! counts as `jev`, so admin messages pass the peer check.

use std::ffi::OsString;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use jev::env::{Buffer, Env, TextInput};
use jev::term::ScriptedTerminal;
use jev_proto::client::Connection;
use jev_proto::{ErrorCode, Paths, Request};
use jevd::daemon::{Config, Daemon};
use jevd::jev_http::JevClient;
use jevd::keystore::{client_account, profile_account, KeyStore, MemoryKeyring, StoreError};
use jevd::listen::bind;
use jevd::peer::AdminCheck;
use jevd::settings::Settings;
use serde_json::{json, Value};
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};
use zeroize::Zeroizing;

const KEY: &str = "tsk-test-key-0123456789";

struct Setup {
    _dir: tempfile::TempDir,
    paths: Paths,
    store: Arc<MemoryKeyring>,
    jev_url: String,
    daemon: Option<tokio::task::JoinHandle<()>>,
    _server: MockServer,
}

struct Ran {
    code: i32,
    out: String,
    err: String,
    shown: Vec<String>,
}

impl Ran {
    fn json(&self) -> Value {
        serde_json::from_str(self.out.trim()).unwrap_or_else(|_| panic!("not JSON: {:?}", self.out))
    }
}

async fn fake_jev(status: u16) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(status)
                .set_body_json(json!({"answers": {"q": {"type": "noul", "noul": 0.75}, "check": {"type": "noul", "noul": 0.5}}})),
        )
        .mount(&server)
        .await;
    server
}

/// A home with no daemon yet.
async fn setup() -> Setup {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::under(dir.path()).unwrap();
    let server = fake_jev(200).await;
    Setup {
        _dir: dir,
        paths,
        store: Arc::new(MemoryKeyring::new()),
        jev_url: server.uri(),
        daemon: None,
        _server: server,
    }
}

/// A home with a daemon running in this process.
async fn running() -> Setup {
    let mut setup = setup().await;
    let jev = JevClient::with_endpoint(
        &setup.jev_url,
        Duration::from_secs(5),
        Duration::from_millis(10),
    )
    .unwrap();
    let daemon = Daemon::new(Config {
        settings_file: setup.paths.settings_file(),
        pairing_file: setup.paths.pairing_file(),
        store: setup.store.clone(),
        jev,
    })
    .unwrap();
    let listener = bind(&setup.paths).unwrap();
    let admin = AdminCheck::new(&std::env::current_exe().unwrap());
    // run() installs signal handlers in this test process; every wait here has a limit.
    setup.daemon = Some(tokio::spawn(jevd::run::run(daemon, listener, admin)));
    setup
}

impl Setup {
    /// Runs one `jev` command line off the runtime, as the blocking CLI does.
    async fn jev(&self, line: &[&str], terminal: ScriptedTerminal, stdin: Option<&str>) -> Ran {
        self.jev_with(self.store.clone(), line, terminal, stdin)
            .await
    }

    /// As `jev`, with `store` as jev's keyring.
    async fn jev_with(
        &self,
        store: Arc<dyn KeyStore>,
        line: &[&str],
        terminal: ScriptedTerminal,
        stdin: Option<&str>,
    ) -> Ran {
        let (out, err) = (Buffer::default(), Buffer::default());
        let url = self.jev_url.clone();
        let mut env = Env {
            paths: self.paths.clone(),
            // No jevd binary: these tests always run a daemon in process, or none.
            jevd: self.paths.config_dir.join("no-such-jevd"),
            store,
            terminal: Box::new(terminal.clone()),
            stdin: Box::new(TextInput(stdin.map(str::to_string))),
            out: Box::new(out.clone()),
            err: Box::new(err.clone()),
            vars: Box::new(|_| None),
            jev_client: Box::new(move || {
                JevClient::with_endpoint(&url, Duration::from_secs(5), Duration::from_millis(10))
            }),
        };
        let mut args: Vec<OsString> = vec!["jev".into()];
        args.extend(line.iter().map(OsString::from));
        let code = tokio::time::timeout(
            Duration::from_secs(30),
            tokio::task::spawn_blocking(move || jev::run(args, &mut env)),
        )
        .await
        .expect("jev finished in 30 s")
        .unwrap();
        Ran {
            code,
            out: out.text(),
            err: err.text(),
            shown: terminal.shown(),
        }
    }

    async fn quiet(&self, line: &[&str]) -> Ran {
        self.jev(line, ScriptedTerminal::none(), Some("")).await
    }

    async fn add_key(&self, profile: &str) {
        let ran = self
            .jev(
                &["key", "add", "--stdin", "--profile", profile],
                ScriptedTerminal::none(),
                Some(KEY),
            )
            .await;
        assert_eq!(ran.code, 0, "{}", ran.err);
    }

    /// Pairs a CLI client in one step with a terminal, and returns its token.
    async fn pair(&self, name: &str) -> String {
        let ran = self
            .jev(
                &["pair", "--name", name],
                ScriptedTerminal::answering(&["", "y"]),
                Some(""),
            )
            .await;
        assert_eq!(ran.code, 0, "{} {:?}", ran.err, ran.shown);
        self.token(name).expect("the token is in the keyring")
    }

    fn token(&self, name: &str) -> Option<String> {
        self.store
            .get(&client_account(name))
            .unwrap()
            .map(|token| token.to_string())
    }

    fn ask(&self, client: &str) -> Vec<String> {
        [
            "ask",
            "--client",
            client,
            "--state",
            "the state",
            "--noul",
            "q=Done?",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    }
}

/// Connects and keeps the connection. Retries for up to 5 s while jevd is
/// full, because it can still count a connection that just closed.
async fn hold(paths: &Paths) -> Connection {
    let paths = paths.clone();
    tokio::task::spawn_blocking(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match Connection::connect(&paths) {
                Ok(connection) => return connection,
                Err(error)
                    if error.code.known() == Some(ErrorCode::DaemonUnavailable)
                        && Instant::now() < deadline =>
                {
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(error) => panic!("cannot hold a connection: {}", error.message),
            }
        }
    })
    .await
    .unwrap()
}

/// Runs after a `set` on a client account, with the account and the count.
type AfterSet = Box<dyn Fn(&str, usize) + Send + Sync>;

/// A MemoryKeyring that can fail the nth `set` on a client account, and
/// can act after each `set` on a client account.
struct HookedStore {
    inner: Arc<MemoryKeyring>,
    client_sets: AtomicUsize,
    fail_set: Option<usize>,
    after_set: AfterSet,
}

impl HookedStore {
    fn new(inner: Arc<MemoryKeyring>) -> HookedStore {
        HookedStore {
            inner,
            client_sets: AtomicUsize::new(0),
            fail_set: None,
            after_set: Box::new(|_, _| {}),
        }
    }
}

impl KeyStore for HookedStore {
    fn get(&self, account: &str) -> Result<Option<Zeroizing<String>>, StoreError> {
        self.inner.get(account)
    }

    fn set(&self, account: &str, secret: &str) -> Result<(), StoreError> {
        if !account.starts_with("client:") {
            return self.inner.set(account, secret);
        }
        let count = self.client_sets.fetch_add(1, Ordering::SeqCst) + 1;
        if self.fail_set == Some(count) {
            return Err(StoreError::Locked);
        }
        self.inner.set(account, secret)?;
        (self.after_set)(account, count);
        Ok(())
    }

    fn delete(&self, account: &str) -> Result<bool, StoreError> {
        self.inner.delete(account)
    }
}

fn strs(owned: &[String]) -> Vec<&str> {
    owned.iter().map(String::as_str).collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn status_and_stop_without_a_daemon() {
    let setup = setup().await;
    let status = setup.quiet(&["status"]).await;
    assert_eq!(status.code, 8, "{}", status.err);
    let stop = setup.quiet(&["stop"]).await;
    assert_eq!(stop.code, 0);
    assert!(stop.out.contains("not running"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn start_status_and_stop_with_a_daemon() {
    let mut setup = running().await;
    assert_eq!(setup.quiet(&["start"]).await.code, 0);
    let status = setup.quiet(&["status", "--json"]).await;
    assert_eq!(status.code, 0);
    assert_eq!(status.json()["daemon_version"], env!("CARGO_PKG_VERSION"));
    let stop = setup.quiet(&["stop"]).await;
    assert_eq!(stop.code, 0, "{}", stop.err);
    let task = setup.daemon.take().unwrap();
    tokio::time::timeout(Duration::from_secs(10), task)
        .await
        .expect("jevd stopped")
        .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn key_add_from_stdin_stores_the_key_and_reloads_the_daemon() {
    let setup = running().await;
    let ran = setup
        .jev(
            &["key", "add", "--stdin", "--profile", "work"],
            ScriptedTerminal::none(),
            Some(&format!("  {KEY}\n")),
        )
        .await;
    assert_eq!(ran.code, 0, "{}", ran.err);
    assert!(
        !ran.out.contains(KEY) && !ran.err.contains(KEY),
        "the key is never printed"
    );
    assert_eq!(
        setup
            .store
            .get(&profile_account("work"))
            .unwrap()
            .unwrap()
            .as_str(),
        KEY
    );
    assert_eq!(
        Settings::load(&setup.paths.settings_file())
            .unwrap()
            .profiles,
        ["work"]
    );
    let status = setup.quiet(&["status", "--json"]).await;
    assert_eq!(
        status.json()["profiles"],
        json!(["work"]),
        "the daemon reloaded"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn key_add_needs_a_terminal_or_stdin_and_checks_the_shape() {
    let setup = running().await;
    let no_terminal = setup.quiet(&["key", "add"]).await;
    assert_eq!(no_terminal.code, 9);
    let typed = setup
        .jev(
            &["key", "add"],
            ScriptedTerminal::answering(&[KEY]),
            Some(""),
        )
        .await;
    assert_eq!(typed.code, 0, "{}", typed.err);
    assert_eq!(
        setup
            .store
            .get(&profile_account("default"))
            .unwrap()
            .unwrap()
            .as_str(),
        KEY
    );
    let bad = setup
        .jev(
            &["key", "add", "--stdin", "--profile", "bad"],
            ScriptedTerminal::none(),
            Some("has a space"),
        )
        .await;
    assert_eq!(bad.code, 6);
    assert!(setup.store.get(&profile_account("bad")).unwrap().is_none());
    let name = setup
        .jev(
            &["key", "add", "--stdin", "--profile", "Bad"],
            ScriptedTerminal::none(),
            Some(KEY),
        )
        .await;
    assert_eq!(name.code, 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn key_add_check_stores_only_a_key_that_jev_accepts() {
    let mut setup = running().await;
    let ok = setup
        .jev(
            &["key", "add", "--stdin", "--check"],
            ScriptedTerminal::none(),
            Some(KEY),
        )
        .await;
    assert_eq!(ok.code, 0, "{}", ok.err);
    assert!(ok.out.contains("accepted"));
    assert_eq!(
        setup._server.received_requests().await.unwrap().len(),
        1,
        "--check sends exactly one request"
    );
    let refusing = fake_jev(401).await;
    setup.jev_url = refusing.uri();
    let refused = setup
        .jev(
            &["key", "add", "--stdin", "--check", "--profile", "other"],
            ScriptedTerminal::none(),
            Some(KEY),
        )
        .await;
    assert_eq!(refused.code, 6, "{}", refused.err);
    assert!(refused.err.contains("jev-unauthorized"));
    let refused_json = setup
        .jev(
            &[
                "key",
                "add",
                "--stdin",
                "--check",
                "--profile",
                "other",
                "--json",
            ],
            ScriptedTerminal::none(),
            Some(KEY),
        )
        .await;
    assert_eq!(refused_json.json()["error"]["profile"], "other");
    assert!(
        !refused.out.contains(KEY) && !refused.err.contains(KEY),
        "a refused key is never printed"
    );
    assert_eq!(refusing.received_requests().await.unwrap().len(), 2);
    assert!(setup
        .store
        .get(&profile_account("other"))
        .unwrap()
        .is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pair_without_a_terminal_prints_the_code_and_exits_3() {
    let setup = running().await;
    setup.add_key("default").await;
    let first = setup.quiet(&["pair", "--name", "script", "--json"]).await;
    assert_eq!(first.code, 3, "{}", first.out);
    let code = first.json()["pair_code"].as_str().unwrap().to_string();
    assert_eq!(first.json()["error"]["code"], "pending");
    let repeat = setup.quiet(&["pair", "--name", "script", "--json"]).await;
    assert_eq!(
        repeat.json()["pair_code"],
        code,
        "a repeat gets the same code"
    );
    let allow = setup.quiet(&["allow", &code]).await;
    assert_eq!(allow.code, 9, "jev allow needs a terminal");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pair_with_no_prompt_never_uses_the_terminal() {
    let setup = running().await;
    setup.add_key("default").await;
    let pending = setup
        .jev(
            &["pair", "--name", "cc-acme", "--no-prompt", "--json"],
            ScriptedTerminal::answering(&["y"]),
            Some(""),
        )
        .await;
    assert_eq!(pending.code, 3, "{}", pending.out);
    assert_eq!(pending.json()["error"]["code"], "pending");
    assert!(pending.json()["pair_code"].is_string());
    assert_eq!(pending.json()["name"], "cc-acme");
    assert!(
        pending.shown.is_empty(),
        "no prompt on a terminal that is there: {:?}",
        pending.shown
    );
    assert_eq!(setup.token("cc-acme"), None, "nothing was approved");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_missing_key_names_its_profile_in_the_ask_json() {
    let setup = running().await;
    setup.add_key("default").await;
    let pending = setup.quiet(&["pair", "--name", "script", "--json"]).await;
    let code = pending.json()["pair_code"].as_str().unwrap().to_string();
    let allow = setup
        .jev(
            &["allow"],
            ScriptedTerminal::answering(&[&code, "", "y"]),
            Some(""),
        )
        .await;
    assert_eq!(allow.code, 0, "{}", allow.err);
    setup.store.delete(&profile_account("default")).unwrap();
    let ask = setup.ask("script");
    let answer = setup.quiet(&strs(&ask)).await;
    assert_eq!(answer.code, 6, "{}", answer.out);
    let error = &answer.json()["error"];
    assert_eq!(error["code"], "no-key");
    assert_eq!(error["profile"], "default");
    assert!(
        error["message"]
            .as_str()
            .unwrap()
            .contains("jev key add --profile default"),
        "the hint names a command that works: {error}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_cli_flow_stores_the_token_and_ask_uses_it() {
    let setup = running().await;
    setup.add_key("default").await;
    let pending = setup.quiet(&["pair", "--name", "script", "--json"]).await;
    let code = pending.json()["pair_code"].as_str().unwrap().to_string();
    let allow = setup
        .jev(
            &["allow"],
            ScriptedTerminal::answering(&[&code.to_lowercase(), "", "y"]),
            Some(""),
        )
        .await;
    assert_eq!(allow.code, 0, "{} {:?}", allow.err, allow.shown);
    let token = setup.token("script").expect("jev wrote the token");
    assert!(
        !allow.out.contains(&token) && !allow.err.contains(&token),
        "the token is never printed"
    );
    assert!(allow.shown.iter().all(|line| !line.contains(&token)));
    let ask = setup.ask("script");
    let answer = setup.quiet(&strs(&ask)).await;
    assert_eq!(answer.code, 0, "{}", answer.out);
    assert_eq!(answer.json()["answers"]["q"]["noul"], 0.75);
    let mut value = ask.clone();
    value.extend(["--value".to_string(), "q".to_string()]);
    let number = setup.quiet(&strs(&value)).await;
    assert_eq!(number.out.trim(), "0.75");
    let piped = setup
        .jev(
            &["ask", "--client", "script"],
            ScriptedTerminal::none(),
            Some(r#"{"state":"s","questions":{"q":{"type":"noul","instructions":"Done?"}}}"#),
        )
        .await;
    assert_eq!(piped.code, 0, "{}", piped.out);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_new_pairing_replaces_the_old_token_at_the_commit() {
    let setup = running().await;
    setup.add_key("default").await;
    let old = setup.pair("app").await;
    let new = setup.pair("app").await;
    assert_ne!(old, new);
    let ran = setup.quiet(&strs(&setup.ask("app"))).await;
    assert_eq!(ran.code, 0, "the keyring holds the new token: {}", ran.out);
    let clients = setup.quiet(&["clients", "--json"]).await;
    assert_eq!(clients.json()["clients"].as_array().unwrap().len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_failed_commit_puts_the_old_token_back() {
    let setup = running().await;
    setup.add_key("default").await;
    let old = setup.pair("app").await;
    let file = setup.paths.pairing_file();
    std::fs::remove_file(&file).unwrap();
    std::fs::create_dir(&file).unwrap();
    std::fs::write(file.join("inside"), "x").unwrap();
    let failed = setup
        .jev(
            &["pair", "--name", "app"],
            ScriptedTerminal::answering(&["", "y"]),
            Some(""),
        )
        .await;
    assert_eq!(failed.code, 1, "internal: {}", failed.err);
    assert_eq!(setup.token("app").as_deref(), Some(old.as_str()));
    std::fs::remove_dir_all(&file).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_failed_first_commit_leaves_no_token() {
    let setup = running().await;
    setup.add_key("default").await;
    let file = setup.paths.pairing_file();
    std::fs::create_dir_all(file.join("inside")).unwrap();
    let failed = setup
        .jev(
            &["pair", "--name", "fresh"],
            ScriptedTerminal::answering(&["", "y"]),
            Some(""),
        )
        .await;
    assert_eq!(failed.code, 1);
    assert_eq!(setup.token("fresh"), None);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_unconfirmed_approval_approves_nothing() {
    let setup = running().await;
    setup.add_key("default").await;
    let ran = setup
        .jev(
            &["pair", "--name", "app"],
            ScriptedTerminal::answering(&["", "n"]),
            Some(""),
        )
        .await;
    assert_eq!(ran.code, 9);
    assert_eq!(setup.token("app"), None);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn revoke_stops_a_client_and_deny_ends_a_request() {
    let setup = running().await;
    setup.add_key("default").await;
    setup.pair("app").await;
    assert_eq!(setup.quiet(&["revoke", "app"]).await.code, 0);
    let ran = setup.quiet(&strs(&setup.ask("app"))).await;
    assert_eq!(ran.code, 4);
    assert_eq!(ran.json()["error"]["code"], "revoked");
    let pending = setup.quiet(&["pair", "--name", "other", "--json"]).await;
    let code = pending.json()["pair_code"].as_str().unwrap().to_string();
    assert_eq!(setup.quiet(&["deny", &code]).await.code, 0);
    assert_eq!(setup.quiet(&["deny", &code]).await.code, 9);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_looser_client_limit_needs_a_terminal() {
    let setup = running().await;
    setup.add_key("default").await;
    setup.pair("app").await;
    assert_eq!(
        setup
            .quiet(&["clients", "limit", "app", "10/60"])
            .await
            .code,
        0,
        "tighter"
    );
    let looser = setup.quiet(&["clients", "limit", "app", "100/60"]).await;
    assert_eq!(looser.code, 9);
    let confirmed = setup
        .jev(
            &["clients", "limit", "app", "100/60"],
            ScriptedTerminal::answering(&["y"]),
            Some(""),
        )
        .await;
    assert_eq!(confirmed.code, 0, "{}", confirmed.err);
    let default = setup.quiet(&["clients", "limit", "app", "default"]).await;
    assert_eq!(default.code, 9, "120/60 is looser than 100/60");
    assert_eq!(
        setup.quiet(&["clients", "limit", "app", "fast"]).await.code,
        2
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn config_changes_follow_the_loosening_rule() {
    let setup = running().await;
    assert_eq!(
        setup
            .quiet(&["config", "set", "idle-timeout", "30m"])
            .await
            .code,
        0,
        "tighter"
    );
    assert_eq!(
        setup
            .quiet(&["config", "set", "idle-timeout", "4h"])
            .await
            .code,
        9
    );
    assert_eq!(
        setup
            .quiet(&["config", "set", "redact-strict", "on"])
            .await
            .code,
        0
    );
    let confirmed = setup
        .jev(
            &["config", "set", "idle-timeout", "4h"],
            ScriptedTerminal::answering(&["y"]),
            Some(""),
        )
        .await;
    assert_eq!(confirmed.code, 0);
    assert_eq!(
        setup
            .quiet(&["config", "get", "idle-timeout"])
            .await
            .out
            .trim(),
        "4h"
    );
    let list = setup.quiet(&["config", "list", "--json"]).await;
    assert_eq!(list.json()["settings"]["redact-strict"], "on");
    assert_eq!(setup.quiet(&["config", "get", "nope"]).await.code, 2);
    assert_eq!(
        setup
            .quiet(&["config", "set", "key-cache", "off"])
            .await
            .code,
        2
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn key_remove_refuses_while_clients_use_the_profile() {
    let setup = running().await;
    setup.add_key("default").await;
    setup.pair("app").await;
    let refused = setup
        .quiet(&["key", "remove", "--profile", "default"])
        .await;
    assert_eq!(refused.code, 9);
    assert!(refused.err.contains("app"));
    let forced = setup
        .quiet(&["key", "remove", "--profile", "default", "--force"])
        .await;
    assert_eq!(forced.code, 0, "{}", forced.err);
    assert!(setup
        .store
        .get(&profile_account("default"))
        .unwrap()
        .is_none());
    assert!(Settings::load(&setup.paths.settings_file())
        .unwrap()
        .profiles
        .is_empty());
    let ran = setup.quiet(&strs(&setup.ask("app"))).await;
    assert_eq!(ran.json()["error"]["code"], "revoked");
    let list = setup.quiet(&["key", "list", "--json"]).await;
    assert_eq!(list.json()["profiles"], json!([]));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_bad_command_line_exits_2() {
    let setup = setup().await;
    for (line, flag) in [(&["nope"][..], "nope"), (&["pair"][..], "--name")] {
        let ran = setup.quiet(line).await;
        assert_eq!(ran.code, 2, "{line:?}");
        assert!(ran.err.contains(flag), "{line:?}: {}", ran.err);
        assert_eq!(ran.out, "", "{line:?}: stdout stays empty");
    }
    for (line, flag) in [
        (&["ask", "--bogus"][..], "--bogus"),
        (&["--json", "nope"][..], "nope"),
        (&["pair", "--json"][..], "--name"),
    ] {
        let ran = setup.quiet(line).await;
        assert_eq!(ran.code, 2, "{line:?}");
        assert!(ran.err.contains(flag), "{line:?}: {}", ran.err);
        assert_eq!(ran.out.lines().count(), 1, "{line:?}: {}", ran.out);
        let value = ran.json();
        assert_eq!(value["ok"], false, "{line:?}");
        assert_eq!(value["error"]["code"], "bad-request", "{line:?}");
    }
    let version = setup.quiet(&["--version"]).await;
    assert_eq!(version.code, 0);
    assert!(version.out.contains(env!("CARGO_PKG_VERSION")));
    assert_eq!(version.err, "");
    let help = setup.quiet(&["ask", "--help", "--json"]).await;
    assert_eq!(help.code, 0);
    assert!(help.out.contains("--state"), "{}", help.out);
    assert!(!help.out.starts_with('{'), "help stays text");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_tightening_change_asks_nothing_even_with_a_terminal() {
    let setup = running().await;
    setup.add_key("default").await;
    setup.pair("app").await;
    for line in [
        &["config", "set", "idle-timeout", "30m"][..],
        &["clients", "limit", "app", "10/60"][..],
    ] {
        let terminal = ScriptedTerminal::answering(&[]);
        let ran = setup.jev(line, terminal, Some("")).await;
        assert_eq!(ran.code, 0, "{line:?}: {}", ran.err);
        assert!(ran.shown.is_empty(), "{line:?}: {:?}", ran.shown);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn allow_refuses_no_request_a_wrong_code_and_an_unknown_profile() {
    let setup = running().await;
    setup.add_key("default").await;
    let none = setup
        .jev(&["allow"], ScriptedTerminal::answering(&[]), Some(""))
        .await;
    assert_eq!(none.code, 9);
    assert!(
        none.err.contains("no pairing request waits"),
        "{}",
        none.err
    );
    let pending = setup.quiet(&["pair", "--name", "app", "--json"]).await;
    let code = pending.json()["pair_code"].as_str().unwrap().to_string();
    let wrong = setup
        .jev(
            &["allow", "ZZZZZZZZ"],
            ScriptedTerminal::answering(&[]),
            Some(""),
        )
        .await;
    assert_eq!(wrong.code, 9);
    assert!(wrong.err.contains("no waiting request"), "{}", wrong.err);
    let unknown = setup
        .jev(
            &["allow", &code],
            ScriptedTerminal::answering(&["nope"]),
            Some(""),
        )
        .await;
    assert_eq!(unknown.code, 9);
    assert!(unknown.err.contains("no profile named"), "{}", unknown.err);
    assert_eq!(setup.token("app"), None);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn key_add_reads_no_key_when_the_settings_cannot_be_read() {
    let setup = setup().await;
    let file = setup.paths.settings_file();
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, "this is [[ not toml").unwrap();
    let ran = setup
        .jev(
            &["key", "add"],
            ScriptedTerminal::answering(&[KEY]),
            Some(""),
        )
        .await;
    assert_eq!(ran.code, 1, "{}", ran.err);
    assert!(ran.shown.is_empty(), "no prompt: {:?}", ran.shown);
    assert!(setup
        .store
        .get(&profile_account("default"))
        .unwrap()
        .is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_failed_reload_says_the_settings_were_saved() {
    let setup = running().await;
    assert_eq!(
        setup
            .quiet(&["config", "set", "max-connections", "1"])
            .await
            .code,
        0
    );
    let held = hold(&setup.paths).await;
    let ran = setup.quiet(&["config", "set", "idle-timeout", "30m"]).await;
    drop(held);
    assert_eq!(ran.code, 8, "{}", ran.err);
    assert!(
        ran.err.contains("saved; the running jevd did not reload"),
        "{}",
        ran.err
    );
    assert_eq!(
        setup
            .quiet(&["config", "get", "idle-timeout"])
            .await
            .out
            .trim(),
        "30m"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_approval_cancelled_before_the_commit_puts_the_old_token_back() {
    let setup = running().await;
    setup.add_key("default").await;
    let old = setup.pair("app").await;
    let paths = setup.paths.clone();
    let mut store = HookedStore::new(setup.store.clone());
    // Right after jev writes the new token, revoke the client, which also
    // cancels the approval, so pair-status answers expired.
    store.after_set = Box::new(move |account, count| {
        if account == client_account("app") && count == 1 {
            let mut connection = Connection::connect(&paths).unwrap();
            let reply = connection
                .call(&Request::Revoke {
                    name: "app".to_string(),
                })
                .unwrap();
            assert!(reply.ok, "{:?}", reply.error);
        }
    });
    let ran = setup
        .jev_with(
            Arc::new(store),
            &["pair", "--name", "app"],
            ScriptedTerminal::answering(&["", "y"]),
            Some(""),
        )
        .await;
    assert_eq!(ran.code, 1, "{}", ran.err);
    assert!(ran.err.contains("cancelled or expired"), "{}", ran.err);
    assert_eq!(setup.token("app").as_deref(), Some(old.as_str()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_failed_restore_reports_both_failures() {
    let setup = running().await;
    setup.add_key("default").await;
    setup.pair("app").await;
    let file = setup.paths.pairing_file();
    std::fs::remove_file(&file).unwrap();
    std::fs::create_dir(&file).unwrap();
    std::fs::write(file.join("inside"), "x").unwrap();
    let mut store = HookedStore::new(setup.store.clone());
    // The first set writes the new token; the second, the restore, fails.
    store.fail_set = Some(2);
    let ran = setup
        .jev_with(
            Arc::new(store),
            &["pair", "--name", "app"],
            ScriptedTerminal::answering(&["", "y"]),
            Some(""),
        )
        .await;
    std::fs::remove_dir_all(&file).unwrap();
    assert_eq!(ran.code, 1, "{}", ran.err);
    assert!(
        ran.err.contains("cannot save the pairing file"),
        "the commit failure: {}",
        ran.err
    );
    assert!(ran.err.contains("then the keyring failed"), "{}", ran.err);
    assert!(
        ran.err.contains(&StoreError::Locked.to_string()),
        "{}",
        ran.err
    );
    assert!(
        ran.err.contains("run jev pair --name app again"),
        "{}",
        ran.err
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stop_with_a_full_daemon_fails_and_the_daemon_runs() {
    let setup = running().await;
    assert_eq!(
        setup
            .quiet(&["config", "set", "max-connections", "1"])
            .await
            .code,
        0
    );
    let held = hold(&setup.paths).await;
    let ran = setup.quiet(&["stop", "--json"]).await;
    drop(held);
    assert_eq!(ran.code, 8, "{}", ran.out);
    assert_eq!(ran.json()["error"]["code"], "daemon-unavailable");
    // The daemon still runs: a new connection gets an answer.
    let _again = hold(&setup.paths).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn key_remove_force_reads_the_settings_before_it_revokes() {
    let setup = running().await;
    setup.add_key("default").await;
    setup.pair("app").await;
    std::fs::write(setup.paths.settings_file(), "this is [[ not toml").unwrap();
    let ran = setup
        .quiet(&["key", "remove", "--profile", "default", "--force"])
        .await;
    assert_eq!(ran.code, 1, "{}", ran.err);
    let clients = setup.quiet(&["clients", "--json"]).await;
    assert_eq!(
        clients.json()["clients"][0]["name"],
        "app",
        "{}",
        clients.out
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_failed_key_prompt_is_forbidden() {
    let setup = setup().await;
    let ran = setup
        .jev(&["key", "add"], ScriptedTerminal::answering(&[]), Some(""))
        .await;
    assert_eq!(ran.code, 9, "{}", ran.err);
    assert!(ran.err.contains("forbidden"), "{}", ran.err);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_key_over_the_room_on_stdin_is_bad_key() {
    let setup = setup().await;
    let long = "k".repeat(5000);
    let ran = setup
        .jev(
            &["key", "add", "--stdin"],
            ScriptedTerminal::none(),
            Some(&long),
        )
        .await;
    assert_eq!(ran.code, 6, "{}", ran.err);
    assert!(ran.err.contains("bad-key"), "{}", ran.err);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_bad_key_in_json_names_its_profile() {
    let setup = setup().await;
    let ran = setup
        .jev(
            &["key", "add", "--stdin", "--json", "--profile", "work"],
            ScriptedTerminal::none(),
            Some("has a space"),
        )
        .await;
    assert_eq!(ran.code, 6, "{}", ran.err);
    let error = &ran.json()["error"];
    assert_eq!(error["code"], "bad-key");
    assert_eq!(error["profile"], "work");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pair_with_a_bad_name_exits_2_before_any_connection() {
    // No daemon and no jevd binary: a connection attempt would exit 8.
    let setup = setup().await;
    let ran = setup.quiet(&["pair", "--name", "Bad_Name", "--json"]).await;
    assert_eq!(ran.code, 2, "{}", ran.out);
    assert_eq!(ran.json()["error"]["code"], "bad-request");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn clap_text_is_cleaned() {
    let setup = setup().await;
    let ran = setup.quiet(&["nope\u{1b}[2J\u{202e}"]).await;
    assert_eq!(ran.code, 2);
    assert!(ran.err.contains("nope"), "{}", ran.err);
    assert!(!ran.err.contains('\u{1b}'), "{:?}", ran.err);
    assert!(!ran.err.contains('\u{202e}'), "{:?}", ran.err);
}
