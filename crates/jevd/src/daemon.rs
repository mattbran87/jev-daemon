//! The daemon's state, and its reply to each message (spec section 2,
//! "Messages"). The transport parses each line and calls `handle`.
//!
//! The state sits behind one `std::sync::Mutex`. No lock is held across an
//! `await`: the keyring read and the Jev call run with the lock released.
//!
//! The pairing file is written under the lock: a commit, a revoke, a
//! set-limit, and a last-used time (at most once a minute per client), each
//! with an fsync. For a per-user daemon that is a short, rare wait, and it
//! keeps memory and disk in one order. The pending file is too, at each
//! change to the waiting pairing requests.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use jev_proto::{ErrorCode, Reply, Request, WireError};
use serde_json::{json, Value};
use tokio::sync::watch;

use crate::jev_http::JevClient;
use crate::keycache::KeyCache;
use crate::keystore::KeyStore;
use crate::pairing::{PairState, Pairings};
use crate::ratelimit::RateLimiter;
use crate::settings::Settings;
use crate::units::{Limit, Period};

/// Who sent a message, from the peer check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Peer {
    /// The `jev` binary next to `jevd`.
    Jev,
    Other,
}

/// What the daemon needs at start.
pub struct Config {
    pub settings_file: PathBuf,
    pub pairing_file: PathBuf,
    /// The waiting pairing requests, so they survive a restart.
    pub pending_file: PathBuf,
    pub store: Arc<dyn KeyStore>,
    /// Built once at start. A change of proxy settings needs a restart.
    pub jev: JevClient,
}

/// The outcome of the newest keyring read: its read id and whether it
/// failed. `None` after `reload` and at start. See `keycache::Lookup`.
pub(crate) type ReadOutcome = Option<(u64, Result<(), WireError>)>;

pub(crate) struct State {
    pub(crate) settings: Settings,
    pub(crate) pairings: Pairings,
    pub(crate) limiter: RateLimiter,
    pub(crate) keys: KeyCache,
}

pub struct Daemon {
    state: Mutex<State>,
    pub(crate) store: Arc<dyn KeyStore>,
    pub(crate) jev: JevClient,
    settings_file: PathBuf,
    pub(crate) reads: watch::Sender<ReadOutcome>,
    stop: watch::Sender<bool>,
    started: Instant,
    busy: AtomicUsize,
    /// A tokio instant, so tests with paused time can move it.
    last_active: Mutex<tokio::time::Instant>,
}

/// Marks a request in flight. Idle exit never fires while one exists.
pub struct Busy<'a>(&'a Daemon);

impl Drop for Busy<'_> {
    fn drop(&mut self) {
        *lock(&self.0.last_active) = tokio::time::Instant::now();
        self.0.busy.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Recovering from a poisoned lock is safe: `Pairings` changes memory only
/// after a successful save.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub(crate) fn failure(code: ErrorCode, message: impl Into<String>) -> Reply {
    Reply::failure(WireError::new(code, message))
}

fn done() -> Reply {
    Reply::success(json!({}))
}

pub(crate) fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

const SAVE_FAILED: &str = "jevd cannot save the pairing file";

impl Daemon {
    /// Reads the settings file, the pairing file, and the pending file.
    /// Reads no key. A pending file that jevd cannot use never stops it.
    pub fn new(config: Config) -> Result<Arc<Daemon>, String> {
        let settings = Settings::load(&config.settings_file)?;
        let mut pairings = Pairings::load(&config.pairing_file)?;
        let now = Instant::now();
        if let Some(problem) = pairings.load_pending(&config.pending_file, now, unix_now()) {
            crate::log::event(&problem);
        }
        let keys = KeyCache::new(settings.key_cache);
        Ok(Arc::new(Daemon {
            state: Mutex::new(State {
                settings,
                pairings,
                limiter: RateLimiter::new(),
                keys,
            }),
            store: config.store,
            jev: config.jev,
            settings_file: config.settings_file,
            reads: watch::Sender::new(None),
            stop: watch::Sender::new(false),
            started: now,
            busy: AtomicUsize::new(0),
            last_active: Mutex::new(tokio::time::Instant::now()),
        }))
    }

    pub(crate) fn lock(&self) -> MutexGuard<'_, State> {
        lock(&self.state)
    }

    /// Call once per message, and hold the guard until its reply is sent.
    pub fn busy(&self) -> Busy<'_> {
        self.busy.fetch_add(1, Ordering::SeqCst);
        *lock(&self.last_active) = tokio::time::Instant::now();
        Busy(self)
    }

    /// When the last request ended, or `None` while a request runs. The two
    /// reads are not one snapshot, so a request that starts between them can
    /// count as idle. Idle exit only requests a stop, and a request in flight
    /// still finishes.
    pub fn idle_since(&self) -> Option<tokio::time::Instant> {
        let since = *lock(&self.last_active);
        (self.busy.load(Ordering::SeqCst) == 0).then_some(since)
    }

    pub fn idle_timeout(&self) -> Period {
        self.lock().settings.idle_timeout
    }

    pub fn max_connections(&self) -> usize {
        self.lock().settings.max_connections as usize
    }

    /// Wipes expired keys. Returns when the next key expires.
    pub fn sweep_keys(&self, now: Instant) -> Option<Instant> {
        let mut state = self.lock();
        state.keys.sweep(now);
        state.keys.next_expiry()
    }

    /// Wipes every cached key, for exit.
    pub fn clear_keys(&self) {
        self.lock().keys.clear();
        self.reads.send_replace(None);
    }

    /// True once `stop` arrived, or `request_stop` ran.
    pub fn stopping(&self) -> watch::Receiver<bool> {
        self.stop.subscribe()
    }

    /// Waits until the daemon stops. The watch guard drops inside, so a
    /// future that awaits this stays `Send`.
    pub async fn stopped(stopping: &mut watch::Receiver<bool>) {
        let _ = stopping.wait_for(|stop| *stop).await;
    }

    pub fn request_stop(&self) {
        self.stop.send_replace(true);
    }

    /// The reply to one message. `hello` belongs to the transport, so it
    /// gets `bad-request` here.
    pub async fn handle(self: &Arc<Self>, request: Request, peer: Peer) -> Reply {
        if request.is_admin() && peer != Peer::Jev {
            return failure(
                ErrorCode::Forbidden,
                "only the jev command next to jevd can send this message",
            );
        }
        match request {
            Request::Hello { .. } => failure(ErrorCode::BadRequest, "hello comes first, once"),
            Request::Status => self.status(),
            Request::Pair { name, profile } => self.pair(&name, profile.as_deref(), peer),
            Request::PairStatus { request_id } => self.pair_status(&request_id),
            Request::Ask {
                token,
                state,
                questions,
            } => self.ask(&token, &state, &questions).await,
            Request::Pending => self.pending(),
            Request::Approve { code, profile } => self.approve(&code, &profile),
            Request::Deny { code } => self.deny(&code),
            Request::Revoke { name } => self.revoke(&name),
            Request::Clients => self.clients(),
            Request::SetLimit { name, limit } => self.set_limit(&name, limit.as_deref()),
            Request::Reload => self.reload(),
            Request::Stop => {
                self.request_stop();
                done()
            }
        }
    }

    fn status(&self) -> Reply {
        let profiles = self.lock().settings.profiles.clone();
        Reply::success(json!({
            "daemon_version": env!("CARGO_PKG_VERSION"),
            "uptime": self.started.elapsed().as_secs(),
            "profiles": profiles,
        }))
    }

    fn pair(&self, name: &str, profile: Option<&str>, peer: Peer) -> Reply {
        let started = self
            .lock()
            .pairings
            .start(name, profile, peer == Peer::Jev, Instant::now());
        match started {
            Ok((request_id, code)) => Reply::success(json!({
                "request_id": request_id,
                "code": code,
            })),
            Err(ErrorCode::RateLimited) => failure(
                ErrorCode::RateLimited,
                "too many pairing requests wait; try again in a few minutes",
            ),
            Err(code) => failure(
                code,
                "names use lowercase letters, digits, and hyphens, 1 to 64 characters",
            ),
        }
    }

    fn pair_status(&self, request_id: &str) -> Reply {
        let mut state = self.lock();
        let profiles = state.settings.profiles.clone();
        match state
            .pairings
            .status(request_id, &profiles, Instant::now(), unix_now())
        {
            Ok(PairState::Approved(Some(token))) => Reply::success(json!({
                "state": "approved",
                "token": token,
            })),
            Ok(other) => Reply::success(json!({ "state": other.as_str() })),
            Err(_) => failure(ErrorCode::Internal, format!("{SAVE_FAILED}; poll again")),
        }
    }

    fn pending(&self) -> Reply {
        let list = self.lock().pairings.pending_list(Instant::now());
        let requests: Vec<Value> = list
            .into_iter()
            .map(|view| json!({ "code": view.code, "name": view.name, "profile": view.profile }))
            .collect();
        Reply::success(json!({ "requests": requests }))
    }

    fn approve(&self, code: &str, profile: &str) -> Reply {
        let mut state = self.lock();
        if !state.settings.profiles.iter().any(|known| known == profile) {
            return failure(
                ErrorCode::BadRequest,
                format!("no profile named {profile:?}; run jev key add --profile <name>"),
            );
        }
        match state.pairings.approve(code, profile, Instant::now()) {
            Ok(approval) => {
                let mut data = json!({ "name": approval.name, "replaces": approval.replaces });
                if let Some(cli) = approval.cli {
                    data["request_id"] = json!(cli.request_id);
                    data["token"] = json!(cli.token);
                }
                Reply::success(data)
            }
            Err(code) => failure(code, "no waiting request has that code"),
        }
    }

    fn deny(&self, code: &str) -> Reply {
        match self.lock().pairings.deny(code, Instant::now()) {
            Ok(()) => done(),
            Err(code) => failure(code, "no waiting request has that code"),
        }
    }

    fn revoke(&self, name: &str) -> Reply {
        let mut state = self.lock();
        match state.pairings.revoke(name) {
            Ok(()) => {
                state.limiter.forget(name);
                done()
            }
            Err(ErrorCode::Internal) => failure(ErrorCode::Internal, SAVE_FAILED),
            Err(code) => failure(code, format!("no client named {name:?}")),
        }
    }

    fn clients(&self) -> Reply {
        let clients: Vec<Value> = self
            .lock()
            .pairings
            .clients()
            .into_iter()
            .map(|client| {
                json!({
                    "name": client.name,
                    "profile": client.profile,
                    "created": client.created,
                    "last_used": client.last_used,
                    "limit": client.limit,
                })
            })
            .collect();
        Reply::success(json!({ "clients": clients }))
    }

    fn set_limit(&self, name: &str, limit: Option<&str>) -> Reply {
        let limit = match limit.map(str::parse::<Limit>).transpose() {
            Ok(limit) => limit,
            Err(problem) => return failure(ErrorCode::BadRequest, problem),
        };
        match self.lock().pairings.set_limit(name, limit) {
            Ok(()) => done(),
            Err(ErrorCode::Internal) => failure(ErrorCode::Internal, SAVE_FAILED),
            Err(code) => failure(code, format!("no client named {name:?}")),
        }
    }

    /// Reads the settings file again and wipes every cached key.
    fn reload(&self) -> Reply {
        let settings = match Settings::load(&self.settings_file) {
            Ok(settings) => settings,
            Err(problem) => return failure(ErrorCode::Internal, problem),
        };
        let mut state = self.lock();
        state.keys.reload(settings.key_cache);
        state.settings = settings;
        drop(state);
        self.reads.send_replace(None);
        done()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::keystore::MemoryKeyring;
    use crate::keystore::StoreError;
    use crate::pairing::TOKEN_PREFIX;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{mpsc, Mutex};
    use std::time::Duration;
    use zeroize::Zeroizing;

    pub(crate) const KEY: &str = "tsk-test-key-0123456789";

    pub(crate) struct Fixture {
        pub(crate) _dir: tempfile::TempDir,
        pub(crate) daemon: Arc<Daemon>,
        pub(crate) settings_file: PathBuf,
        pub(crate) pairing_file: PathBuf,
        pub(crate) pending_file: PathBuf,
    }

    /// A daemon with the profiles `default` and `work`, a keyring of
    /// `store`, and a Jev client that sends to `endpoint`.
    pub(crate) fn fixture_with(store: Arc<dyn KeyStore>, endpoint: &str) -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let settings_file = dir.path().join("config.toml");
        let pairing_file = dir.path().join("pairings.json");
        let pending_file = dir.path().join("pending.json");
        let mut settings = Settings::default();
        settings.add_profile("default").unwrap();
        settings.add_profile("work").unwrap();
        settings.save(&settings_file).unwrap();
        let jev =
            JevClient::with_endpoint(endpoint, Duration::from_secs(5), Duration::from_millis(10))
                .unwrap();
        let daemon = Daemon::new(Config {
            settings_file: settings_file.clone(),
            pairing_file: pairing_file.clone(),
            pending_file: pending_file.clone(),
            store,
            jev,
        })
        .unwrap();
        Fixture {
            _dir: dir,
            daemon,
            settings_file,
            pairing_file,
            pending_file,
        }
    }

    #[derive(Clone, Copy)]
    pub(crate) enum Outcome {
        Key,
        Locked,
        PanicFirst,
    }

    /// A keyring whose `get` waits for a release, and counts its calls.
    pub(crate) struct Gated {
        calls: AtomicUsize,
        gate: Mutex<mpsc::Receiver<()>>,
        outcome: Outcome,
    }

    /// Waits, for up to 5 s, until `n` requests subscribed to the read
    /// outcomes. The first ask's receiver lives while it awaits its read, so
    /// 2 means the second ask subscribed and goes straight to its lookup.
    pub(crate) async fn wait_for_readers(daemon: &Arc<Daemon>, n: usize) {
        for _ in 0..500 {
            if daemon.reads.receiver_count() >= n {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("fewer than {n} requests are waiting on the read");
    }

    /// The result of a spawned ask, or a failure after 10 s.
    pub(crate) async fn finished<T>(task: tokio::task::JoinHandle<T>) -> T {
        tokio::time::timeout(Duration::from_secs(10), task)
            .await
            .expect("the ask did not finish in 10 s")
            .unwrap()
    }

    pub(crate) fn gated(outcome: Outcome) -> (Arc<Gated>, mpsc::Sender<()>) {
        let (release, gate) = mpsc::channel();
        let store = Gated {
            calls: AtomicUsize::new(0),
            gate: Mutex::new(gate),
            outcome,
        };
        (Arc::new(store), release)
    }

    impl Gated {
        pub(crate) fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }

        /// Waits until `get` was called `n` times, for up to 5 s.
        pub(crate) async fn wait_for_calls(&self, n: usize) {
            for _ in 0..500 {
                if self.calls() >= n {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            panic!("get was called {} times, not {n}", self.calls());
        }
    }

    impl KeyStore for Gated {
        fn get(&self, _account: &str) -> Result<Option<Zeroizing<String>>, StoreError> {
            let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
            let _ = self.gate.lock().unwrap().recv();
            match self.outcome {
                Outcome::Locked => Err(StoreError::Locked),
                Outcome::PanicFirst if call == 1 => panic!("the keyring broke"),
                _ => Ok(Some(Zeroizing::new(KEY.to_string()))),
            }
        }
        fn set(&self, _account: &str, _secret: &str) -> Result<(), StoreError> {
            Ok(())
        }
        fn delete(&self, _account: &str) -> Result<bool, StoreError> {
            Ok(false)
        }
    }

    fn fixture() -> Fixture {
        fixture_with(Arc::new(MemoryKeyring::new()), "http://127.0.0.1:9/")
    }

    pub(crate) fn code_of(reply: &Reply) -> Option<ErrorCode> {
        reply.error.as_ref().and_then(|error| error.code.known())
    }

    pub(crate) async fn call(daemon: &Arc<Daemon>, peer: Peer, request: Request) -> Reply {
        daemon.handle(request, peer).await
    }

    /// Pair a socket client all the way, and return its token.
    pub(crate) async fn pair_socket(daemon: &Arc<Daemon>, name: &str, profile: &str) -> String {
        let reply = call(
            daemon,
            Peer::Other,
            Request::Pair {
                name: name.into(),
                profile: None,
            },
        )
        .await;
        let request_id = reply.data["request_id"].as_str().unwrap().to_string();
        let code = reply.data["code"].as_str().unwrap().to_string();
        let approved = call(
            daemon,
            Peer::Jev,
            Request::Approve {
                code,
                profile: profile.into(),
            },
        )
        .await;
        assert!(approved.ok, "{approved:?}");
        let collected = call(daemon, Peer::Other, Request::PairStatus { request_id }).await;
        assert_eq!(collected.data["state"], "approved");
        collected.data["token"].as_str().unwrap().to_string()
    }

    fn admin_requests() -> Vec<Request> {
        vec![
            Request::Pending,
            Request::Approve {
                code: "ABCD2345".into(),
                profile: "default".into(),
            },
            Request::Deny {
                code: "ABCD2345".into(),
            },
            Request::Revoke { name: "a".into() },
            Request::Clients,
            Request::SetLimit {
                name: "a".into(),
                limit: None,
            },
            Request::Reload,
            Request::Stop,
        ]
    }

    #[tokio::test]
    async fn admin_messages_from_another_process_are_forbidden() {
        let f = fixture();
        for request in admin_requests() {
            let reply = call(&f.daemon, Peer::Other, request).await;
            assert_eq!(code_of(&reply), Some(ErrorCode::Forbidden));
        }
        assert!(
            !*f.daemon.stopping().borrow(),
            "a forbidden stop does nothing"
        );
    }

    #[tokio::test]
    async fn status_shows_the_version_and_the_profiles() {
        let f = fixture();
        let reply = call(&f.daemon, Peer::Other, Request::Status).await;
        assert!(reply.ok);
        assert_eq!(reply.data["daemon_version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(reply.data["profiles"], json!(["default", "work"]));
        assert!(reply.data["uptime"].is_u64());
    }

    #[tokio::test]
    async fn hello_after_the_start_is_a_bad_request() {
        let f = fixture();
        let reply = call(&f.daemon, Peer::Other, Request::Hello { protocol: 1 }).await;
        assert_eq!(code_of(&reply), Some(ErrorCode::BadRequest));
    }

    #[tokio::test]
    async fn the_socket_flow_gives_the_token_once_at_collection() {
        let f = fixture();
        let reply = call(
            &f.daemon,
            Peer::Other,
            Request::Pair {
                name: "app".into(),
                profile: Some("work".into()),
            },
        )
        .await;
        let request_id = reply.data["request_id"].as_str().unwrap().to_string();
        let code = reply.data["code"].as_str().unwrap().to_string();
        let pending = call(&f.daemon, Peer::Jev, Request::Pending).await;
        assert_eq!(
            pending.data["requests"],
            json!([{ "code": code, "name": "app", "profile": "work" }])
        );
        let unknown = call(
            &f.daemon,
            Peer::Jev,
            Request::Approve {
                code: code.clone(),
                profile: "nope".into(),
            },
        )
        .await;
        assert_eq!(code_of(&unknown), Some(ErrorCode::BadRequest));
        let approved = call(
            &f.daemon,
            Peer::Jev,
            Request::Approve {
                code,
                profile: "work".into(),
            },
        )
        .await;
        assert_eq!(
            approved.data,
            json!({"name": "app", "replaces": false})
                .as_object()
                .unwrap()
                .clone()
        );
        let clients = call(&f.daemon, Peer::Jev, Request::Clients).await;
        assert_eq!(
            clients.data["clients"],
            json!([]),
            "nothing saved before the commit"
        );
        let status = Request::PairStatus {
            request_id: request_id.clone(),
        };
        let collected = call(&f.daemon, Peer::Other, status.clone()).await;
        assert_eq!(collected.data["state"], "approved");
        assert!(collected.data["token"]
            .as_str()
            .unwrap()
            .starts_with(TOKEN_PREFIX));
        let again = call(&f.daemon, Peer::Other, status).await;
        assert_eq!(again.data["state"], "expired");
        let clients = call(&f.daemon, Peer::Jev, Request::Clients).await;
        assert_eq!(clients.data["clients"][0]["name"], "app");
        assert_eq!(clients.data["clients"][0]["profile"], "work");
    }

    #[tokio::test]
    async fn the_cli_flow_gives_the_token_at_approval() {
        let f = fixture();
        let pair = Request::Pair {
            name: "script".into(),
            profile: None,
        };
        let first = call(&f.daemon, Peer::Jev, pair.clone()).await;
        let repeat = call(&f.daemon, Peer::Jev, pair).await;
        assert_eq!(
            first.data, repeat.data,
            "a repeat from jev gets the same request"
        );
        let code = first.data["code"].as_str().unwrap().to_string();
        let approved = call(
            &f.daemon,
            Peer::Jev,
            Request::Approve {
                code,
                profile: "default".into(),
            },
        )
        .await;
        assert_eq!(approved.data["request_id"], first.data["request_id"]);
        assert!(approved.data["token"]
            .as_str()
            .unwrap()
            .starts_with(TOKEN_PREFIX));
        let committed = call(
            &f.daemon,
            Peer::Jev,
            Request::PairStatus {
                request_id: first.data["request_id"].as_str().unwrap().into(),
            },
        )
        .await;
        assert_eq!(
            committed.data,
            json!({"state": "approved"}).as_object().unwrap().clone()
        );
        let clients = call(&f.daemon, Peer::Jev, Request::Clients).await;
        assert_eq!(clients.data["clients"][0]["name"], "script");
    }

    /// A second daemon on the files of `f`, as after a restart.
    fn restart(f: &Fixture) -> Arc<Daemon> {
        let jev = JevClient::with_endpoint(
            "http://127.0.0.1:9/",
            Duration::from_secs(5),
            Duration::from_millis(10),
        )
        .unwrap();
        Daemon::new(Config {
            settings_file: f.settings_file.clone(),
            pairing_file: f.pairing_file.clone(),
            pending_file: f.pending_file.clone(),
            store: Arc::new(MemoryKeyring::new()),
            jev,
        })
        .unwrap()
    }

    #[tokio::test]
    async fn a_waiting_socket_request_survives_a_restart() {
        let f = fixture();
        let reply = call(
            &f.daemon,
            Peer::Other,
            Request::Pair {
                name: "app".into(),
                profile: None,
            },
        )
        .await;
        let request_id = reply.data["request_id"].as_str().unwrap().to_string();
        let code = reply.data["code"].as_str().unwrap().to_string();
        let daemon = restart(&f);
        let status = Request::PairStatus { request_id };
        let waiting = call(&daemon, Peer::Other, status.clone()).await;
        assert_eq!(waiting.data["state"], "pending");
        let approve = Request::Approve {
            code,
            profile: "default".into(),
        };
        assert!(call(&daemon, Peer::Jev, approve).await.ok);
        let collected = call(&daemon, Peer::Other, status).await;
        assert_eq!(collected.data["state"], "approved");
        assert!(collected.data["token"].is_string());
    }

    #[tokio::test]
    async fn a_pending_file_that_cannot_be_used_does_not_stop_the_start() {
        let f = fixture();
        std::fs::write(&f.pending_file, "not json").unwrap();
        let daemon = restart(&f);
        let pending = call(&daemon, Peer::Jev, Request::Pending).await;
        assert_eq!(pending.data["requests"], json!([]));
        assert!(!f.pending_file.exists(), "jevd deletes the file");
    }

    #[tokio::test]
    async fn a_failed_save_at_collection_is_internal_and_a_later_poll_works() {
        let f = fixture();
        pair_socket(&f.daemon, "first", "default").await;
        let reply = call(
            &f.daemon,
            Peer::Other,
            Request::Pair {
                name: "app".into(),
                profile: None,
            },
        )
        .await;
        let request_id = reply.data["request_id"].as_str().unwrap().to_string();
        let code = reply.data["code"].as_str().unwrap().to_string();
        let approve = Request::Approve {
            code,
            profile: "default".into(),
        };
        assert!(call(&f.daemon, Peer::Jev, approve).await.ok);
        std::fs::remove_file(&f.pairing_file).unwrap();
        std::fs::create_dir(&f.pairing_file).unwrap();
        std::fs::write(f.pairing_file.join("inside"), "x").unwrap();
        let status = Request::PairStatus { request_id };
        let failed = call(&f.daemon, Peer::Other, status.clone()).await;
        assert_eq!(code_of(&failed), Some(ErrorCode::Internal));
        std::fs::remove_dir_all(&f.pairing_file).unwrap();
        let collected = call(&f.daemon, Peer::Other, status).await;
        assert_eq!(collected.data["state"], "approved");
        assert!(collected.data["token"].is_string());
    }

    #[tokio::test]
    async fn deny_revoke_and_set_limit_name_what_they_cannot_find() {
        let f = fixture();
        let deny = call(
            &f.daemon,
            Peer::Jev,
            Request::Deny {
                code: "ABCD2345".into(),
            },
        )
        .await;
        assert_eq!(code_of(&deny), Some(ErrorCode::BadRequest));
        let revoke = call(&f.daemon, Peer::Jev, Request::Revoke { name: "x".into() }).await;
        assert_eq!(code_of(&revoke), Some(ErrorCode::BadRequest));
        let limit = call(
            &f.daemon,
            Peer::Jev,
            Request::SetLimit {
                name: "x".into(),
                limit: Some("10/60".into()),
            },
        )
        .await;
        assert_eq!(code_of(&limit), Some(ErrorCode::BadRequest));
    }

    #[tokio::test]
    async fn set_limit_checks_the_limit_and_revoke_ends_a_client() {
        let f = fixture();
        pair_socket(&f.daemon, "app", "default").await;
        let bad = call(
            &f.daemon,
            Peer::Jev,
            Request::SetLimit {
                name: "app".into(),
                limit: Some("fast".into()),
            },
        )
        .await;
        assert_eq!(code_of(&bad), Some(ErrorCode::BadRequest));
        let set = Request::SetLimit {
            name: "app".into(),
            limit: Some("10/60".into()),
        };
        assert!(call(&f.daemon, Peer::Jev, set).await.ok);
        let clients = call(&f.daemon, Peer::Jev, Request::Clients).await;
        assert_eq!(clients.data["clients"][0]["limit"], "10/60");
        let revoke = Request::Revoke { name: "app".into() };
        assert!(call(&f.daemon, Peer::Jev, revoke).await.ok);
        let clients = call(&f.daemon, Peer::Jev, Request::Clients).await;
        assert_eq!(clients.data["clients"], json!([]));
    }

    #[tokio::test]
    async fn deny_ends_a_socket_request() {
        let f = fixture();
        let reply = call(
            &f.daemon,
            Peer::Other,
            Request::Pair {
                name: "app".into(),
                profile: None,
            },
        )
        .await;
        let code = reply.data["code"].as_str().unwrap().to_string();
        assert!(call(&f.daemon, Peer::Jev, Request::Deny { code }).await.ok);
        let status = call(
            &f.daemon,
            Peer::Other,
            Request::PairStatus {
                request_id: reply.data["request_id"].as_str().unwrap().into(),
            },
        )
        .await;
        assert_eq!(status.data["state"], "denied");
    }

    #[tokio::test]
    async fn reload_reads_the_settings_again() {
        let f = fixture();
        let mut settings = Settings::load(&f.settings_file).unwrap();
        settings.add_profile("third").unwrap();
        settings.save(&f.settings_file).unwrap();
        assert!(call(&f.daemon, Peer::Jev, Request::Reload).await.ok);
        let status = call(&f.daemon, Peer::Other, Request::Status).await;
        assert_eq!(status.data["profiles"], json!(["default", "third", "work"]));
        std::fs::write(&f.settings_file, "idle-timeout = 5").unwrap();
        let broken = call(&f.daemon, Peer::Jev, Request::Reload).await;
        assert_eq!(code_of(&broken), Some(ErrorCode::Internal));
        let status = call(&f.daemon, Peer::Other, Request::Status).await;
        assert_eq!(
            status.data["profiles"],
            json!(["default", "third", "work"]),
            "a bad file leaves the settings as they were"
        );
    }

    #[tokio::test]
    async fn stop_from_jev_starts_the_stop() {
        let f = fixture();
        let stopping = f.daemon.stopping();
        assert!(call(&f.daemon, Peer::Jev, Request::Stop).await.ok);
        assert!(*stopping.borrow());
    }

    #[tokio::test]
    async fn a_request_in_flight_keeps_the_daemon_busy() {
        let f = fixture();
        assert!(f.daemon.idle_since().is_some());
        let busy = f.daemon.busy();
        assert!(f.daemon.idle_since().is_none());
        let before = tokio::time::Instant::now();
        drop(busy);
        assert!(f.daemon.idle_since().unwrap() >= before);
    }
}
