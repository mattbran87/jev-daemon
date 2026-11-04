//! The daemon's life (spec section 2, "Lifecycle"): serve connections until
//! a stop, an idle exit, or a signal, then let requests in flight finish,
//! wipe the keys, and close the channel.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use jev_proto::ErrorCode;
use tokio::io::AsyncWriteExt;
use tokio::task::JoinSet;

use crate::conn;
use crate::daemon::{failure, Daemon};
use crate::listen::Listener;
use crate::peer::AdminCheck;
use crate::units::Period;

/// After a stop, requests in flight get this long to finish. One `ask` can
/// take about 31 s in Jev alone, plus a keyring wait.
pub const DRAIN_LIMIT: Duration = Duration::from_secs(45);
/// The longest sleep of the idle and key timers, so a changed setting takes
/// effect within this period.
const TICK: Duration = Duration::from_secs(60);
/// How long a refused client gets to send its first line.
const REFUSE_WAIT: Duration = Duration::from_secs(5);
/// A refused client's first line is read to this size only.
const REFUSE_LINE: usize = 4096;
/// The most `refuse` tasks alive at once; a refused stream over this is dropped.
const MAX_REFUSING: usize = 64;
/// The count of live `refuse` tasks.
static REFUSING: AtomicUsize = AtomicUsize::new(0);
const TOO_MANY: &str = "jevd has too many connections open; see max-connections";
const STOPPING: &str = "jevd is stopping";

/// Serves until the daemon stops, then drains and cleans up.
pub async fn run(daemon: Arc<Daemon>, mut listener: Listener, admin: AdminCheck) {
    let watchers = [
        tokio::spawn(idle_exit(Arc::clone(&daemon))),
        tokio::spawn(sweep_keys(Arc::clone(&daemon))),
        tokio::spawn(signals(Arc::clone(&daemon))),
    ];
    let mut stopping = daemon.stopping();
    let mut connections = JoinSet::new();
    let mut accept_failed = false;
    loop {
        tokio::select! {
            biased;
            _ = Daemon::stopped(&mut stopping) => break,
            Some(done) = connections.join_next(), if !connections.is_empty() => note_ended(done),
            accepted = listener.accept() => {
                let (stream, pid) = match accepted {
                    Ok(accepted) => accepted,
                    Err(error) => {
                        // A failed accept, such as a client that left at once.
                        note_accept_failure(&mut accept_failed, &error);
                        tokio::time::sleep(Duration::from_millis(50)).await;
                        continue;
                    }
                };
                while let Some(done) = connections.try_join_next() {
                    note_ended(done);
                }
                if connections.len() >= daemon.max_connections() {
                    spawn_refuse(stream, TOO_MANY);
                    continue;
                }
                connections.spawn(conn::serve(stream, admin.peer(pid), Arc::clone(&daemon)));
            }
        }
    }
    // The drain: refuse new connections, and give requests in flight
    // `DRAIN_LIMIT` to finish. Then cut the rest, and wait until they are gone.
    let deadline = tokio::time::sleep(DRAIN_LIMIT);
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok((stream, _)) => {
                    spawn_refuse(stream, STOPPING);
                }
                Err(error) => {
                    note_accept_failure(&mut accept_failed, &error);
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
            },
            done = connections.join_next() => match done {
                Some(done) => note_ended(done),
                None => break,
            },
            _ = &mut deadline => {
                connections.shutdown().await;
                break;
            }
        }
    }
    for watcher in watchers {
        watcher.abort();
    }
    daemon.clear_keys();
    listener.close();
}

/// Ends the count of one `refuse` task when it drops.
struct RefuseGuard;

impl Drop for RefuseGuard {
    fn drop(&mut self) {
        REFUSING.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Starts a `refuse` task, or drops the stream at once when `MAX_REFUSING`
/// of them are alive.
fn spawn_refuse(stream: crate::listen::Stream, message: &'static str) {
    if REFUSING.fetch_add(1, Ordering::AcqRel) >= MAX_REFUSING {
        REFUSING.fetch_sub(1, Ordering::AcqRel);
        return;
    }
    let guard = RefuseGuard;
    tokio::spawn(async move {
        let _guard = guard;
        refuse(stream, message).await;
    });
}

/// Answers the first line of any refused client (over `max-connections`, or
/// while jevd stops) with the given reason, then closes. Reading the line
/// first lets the client's `hello` write finish, so the client reads the
/// reason.
async fn refuse(stream: crate::listen::Stream, message: &'static str) {
    let (read, mut write) = tokio::io::split(stream);
    let mut reader = tokio::io::BufReader::new(read);
    let first = conn::read_line(&mut reader, REFUSE_LINE);
    let _ = tokio::time::timeout(REFUSE_WAIT, first).await;
    let reply = failure(ErrorCode::DaemonUnavailable, message);
    let _ = tokio::time::timeout(conn::WRITE_LIMIT, async {
        let _ = write.write_all(reply.to_line().as_bytes()).await;
        let _ = write.flush().await;
    })
    .await;
}

/// Logs a connection task that panicked.
fn note_ended(done: Result<(), tokio::task::JoinError>) {
    if let Err(error) = done {
        if error.is_panic() {
            crate::log::event("a connection task panicked");
        }
    }
}

/// Logs the first failed accept of a run, not each retry.
fn note_accept_failure(logged: &mut bool, error: &std::io::Error) {
    if !*logged {
        *logged = true;
        crate::log::event(&format!("accept failed: {error}"));
    }
}

/// Stops the daemon after `idle-timeout` with no request. Never while a
/// request runs. With `off`, never.
pub(crate) async fn idle_exit(daemon: Arc<Daemon>) {
    loop {
        let wait = match (daemon.idle_timeout(), daemon.idle_since()) {
            (Period::After(limit), Some(since)) => {
                let idle = since.elapsed();
                if idle >= limit {
                    crate::log::event("idle exit");
                    daemon.request_stop();
                    return;
                }
                limit - idle
            }
            _ => TICK,
        };
        tokio::time::sleep(wait.min(TICK)).await;
    }
}

/// Wipes each key after `key-cache` with no use, at most `TICK` late. A
/// lookup also sweeps, so a key past its period is never used.
pub(crate) async fn sweep_keys(daemon: Arc<Daemon>) {
    loop {
        // The tokio clock is the same clock in production, and it moves with
        // paused time in tests.
        let now = tokio::time::Instant::now().into_std();
        let next = daemon.sweep_keys(now);
        let wait = next.map_or(TICK, |at| at.saturating_duration_since(now));
        tokio::time::sleep(wait.min(TICK)).await;
    }
}

/// A termination signal, or Ctrl+C on a terminal, stops the daemon.
async fn signals(daemon: Arc<Daemon>) {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let (Ok(mut term), Ok(mut interrupt), Ok(mut hangup)) = (
            signal(SignalKind::terminate()),
            signal(SignalKind::interrupt()),
            signal(SignalKind::hangup()),
        ) else {
            return;
        };
        tokio::select! {
            _ = term.recv() => {}
            _ = interrupt.recv() => {}
            _ = hangup.recv() => {}
        }
    }
    #[cfg(windows)]
    {
        // Registration succeeds, but a detached jevd has no console, so it
        // gets no console events and this never fires: the daemon ends with
        // the session. Run from a terminal, Ctrl+C stops it.
        if tokio::signal::ctrl_c().await.is_err() {
            return;
        }
    }
    crate::log::event("signal");
    daemon.request_stop();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::daemon::tests::fixture_with;
    use crate::keystore::MemoryKeyring;
    use crate::settings::Settings;
    use jev_proto::Request;
    use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite};

    fn fixture(idle: &str) -> crate::daemon::tests::Fixture {
        let f = fixture_with(Arc::new(MemoryKeyring::new()), "http://127.0.0.1:9/");
        let change = Settings::load(&f.settings_file)
            .unwrap()
            .with_change("idle-timeout", idle)
            .unwrap();
        change.settings.save(&f.settings_file).unwrap();
        f
    }

    #[tokio::test(start_paused = true)]
    async fn idle_exit_stops_after_the_timeout() {
        let f = fixture("1m");
        assert!(
            f.daemon
                .handle(Request::Reload, crate::daemon::Peer::Jev)
                .await
                .ok
        );
        let started = tokio::time::Instant::now();
        idle_exit(Arc::clone(&f.daemon)).await;
        assert!(*f.daemon.stopping().borrow());
        assert!(started.elapsed() >= Duration::from_secs(60));
    }

    #[tokio::test(start_paused = true)]
    async fn idle_exit_waits_while_a_request_runs() {
        let f = fixture("1m");
        assert!(
            f.daemon
                .handle(Request::Reload, crate::daemon::Peer::Jev)
                .await
                .ok
        );
        let busy = f.daemon.busy();
        let watcher = tokio::spawn(idle_exit(Arc::clone(&f.daemon)));
        tokio::time::sleep(Duration::from_secs(600)).await;
        assert!(!*f.daemon.stopping().borrow(), "no idle exit while busy");
        drop(busy);
        watcher.await.unwrap();
        assert!(*f.daemon.stopping().borrow());
    }

    #[tokio::test(start_paused = true)]
    async fn idle_exit_off_never_stops() {
        let f = fixture("off");
        assert!(
            f.daemon
                .handle(Request::Reload, crate::daemon::Peer::Jev)
                .await
                .ok
        );
        let watcher = tokio::spawn(idle_exit(Arc::clone(&f.daemon)));
        tokio::time::sleep(Duration::from_secs(24 * 3600)).await;
        assert!(!*f.daemon.stopping().borrow());
        watcher.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn sweep_keys_wipes_an_expired_key() {
        use crate::keycache::Lookup;
        let f = fixture("1m");
        let now = tokio::time::Instant::now().into_std();
        {
            let mut state = f.daemon.lock();
            state.keys.reload(Duration::from_secs(300));
            let Lookup::Miss(ticket) = state.keys.lookup("default", now) else {
                panic!("an empty cache must miss");
            };
            let _ = state.keys.finish(
                ticket,
                Ok(zeroize::Zeroizing::new("tsk-k".to_string())),
                now,
            );
            assert_eq!(state.keys.len(), 1);
        }
        let watcher = tokio::spawn(sweep_keys(Arc::clone(&f.daemon)));
        tokio::time::sleep(Duration::from_secs(300) + TICK + Duration::from_secs(1)).await;
        assert_eq!(f.daemon.lock().keys.len(), 0);
        watcher.abort();
    }

    async fn dial(paths: &jev_proto::Paths) -> impl AsyncRead + AsyncWrite + Unpin {
        for _ in 0..500 {
            #[cfg(unix)]
            let opened = tokio::net::UnixStream::connect(&paths.socket).await;
            #[cfg(windows)]
            let opened = tokio::net::windows::named_pipe::ClientOptions::new().open(&paths.socket);
            if let Ok(stream) = opened {
                return stream;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("could not connect to the test channel");
    }

    /// Sends `line`, and reads one reply line, within 10 s.
    async fn exchange<S: AsyncRead + AsyncWrite + Unpin>(
        stream: &mut tokio::io::BufReader<S>,
        line: &str,
    ) -> jev_proto::Reply {
        send_line(stream, line).await;
        read_reply(stream).await
    }

    async fn send_line<S: AsyncRead + AsyncWrite + Unpin>(
        stream: &mut tokio::io::BufReader<S>,
        line: &str,
    ) {
        let write = async {
            stream.write_all(line.as_bytes()).await?;
            stream.write_all(b"\n").await?;
            stream.flush().await
        };
        tokio::time::timeout(Duration::from_secs(10), write)
            .await
            .expect("the write finished")
            .unwrap();
    }

    async fn read_reply<S: AsyncRead + AsyncWrite + Unpin>(
        stream: &mut tokio::io::BufReader<S>,
    ) -> jev_proto::Reply {
        let mut line = String::new();
        let read = tokio::time::timeout(Duration::from_secs(10), stream.read_line(&mut line))
            .await
            .expect("a reply came in 10 s")
            .unwrap();
        assert!(read > 0, "the connection closed with no reply");
        serde_json::from_str(&line).unwrap()
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_stop_refuses_new_connections_and_lets_requests_finish() {
        use crate::daemon::tests::{gated, pair_socket, Outcome};
        use wiremock::matchers::method;
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                serde_json::json!({"answers": {"q": {"type": "noul", "noul": 0.8}}}),
            ))
            .mount(&server)
            .await;
        let (store, release) = gated(Outcome::Key);
        let f = fixture_with(store.clone(), &server.uri());
        let token = pair_socket(&f.daemon, "app", "default").await;
        let home = tempfile::tempdir().unwrap();
        let paths = jev_proto::Paths::under(home.path()).unwrap();
        let listener = crate::listen::bind(&paths).unwrap();
        let admin = AdminCheck::new(&std::env::current_exe().unwrap());
        let running = tokio::spawn(run(Arc::clone(&f.daemon), listener, admin));

        let hello = r#"{"type":"hello","protocol":1}"#;
        let ask = r#"{"type":"ask","token":"TOKEN","state":"s","questions":{"q":{"type":"noul","question":"Is it done?"}}}"#
            .replace("TOKEN", &token);
        let mut first = tokio::io::BufReader::new(dial(&paths).await);
        assert!(exchange(&mut first, hello).await.ok);
        send_line(&mut first, &ask).await;
        store.wait_for_calls(1).await;

        f.daemon.request_stop();

        let mut second = tokio::io::BufReader::new(dial(&paths).await);
        let refused = exchange(&mut second, hello).await;
        let error = refused.error.expect("the new connection is refused");
        assert_eq!(error.code.known(), Some(ErrorCode::DaemonUnavailable));
        assert!(error.message.contains("stopping"), "{}", error.message);

        release.send(()).unwrap();
        let answered = read_reply(&mut first).await;
        assert!(answered.ok, "{answered:?}");
        tokio::time::timeout(Duration::from_secs(10), running)
            .await
            .expect("run finished in 10 s")
            .unwrap();
    }
}
