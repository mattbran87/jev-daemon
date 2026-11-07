//! One connection (spec section 2, "Transport"): one JSON object per line,
//! with a size cap, `hello` first, one request in flight at a time, and a
//! limit on the wait for the next message.

use std::sync::Arc;
use std::time::Duration;

use jev_proto::{ErrorCode, Reply, Request, PROTOCOL_VERSION};
use serde_json::json;
use tokio::io::{
    AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader,
};

use crate::daemon::{failure, Daemon, Peer};

/// The longest request line, newline not counted. One value for jevd and
/// its clients.
pub use jev_proto::MAX_LINE;
/// A reply write that takes longer than this closes the connection.
pub const WRITE_LIMIT: Duration = Duration::from_secs(10);
/// A connection closes after this long with no message. The timer runs only
/// while jevd waits for a message, never while a request runs.
pub const IDLE_LIMIT: Duration = Duration::from_secs(60);

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Line {
    Text(Vec<u8>),
    TooLong,
    Closed,
}

/// Reads one line of at most `max` bytes, newline not counted. A last line
/// with no newline still counts as a line.
pub(crate) async fn read_line<R: AsyncBufRead + Unpin>(
    reader: &mut R,
    max: usize,
) -> std::io::Result<Line> {
    let mut buffer = Vec::new();
    let limit = max as u64 + 1;
    (&mut *reader)
        .take(limit)
        .read_until(b'\n', &mut buffer)
        .await?;
    if buffer.is_empty() {
        return Ok(Line::Closed);
    }
    if buffer.last() == Some(&b'\n') {
        buffer.pop();
        return Ok(Line::Text(buffer));
    }
    if buffer.len() > max {
        return Ok(Line::TooLong);
    }
    Ok(Line::Text(buffer))
}

/// Serves one connection until the client closes it, it idles past
/// `IDLE_LIMIT`, it breaks a rule, or the daemon stops. A stop closes the
/// connection only between requests, so a request in flight finishes.
pub async fn serve<S>(stream: S, peer: Peer, daemon: Arc<Daemon>)
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let (read, mut write) = tokio::io::split(stream);
    let mut reader = BufReader::new(read);
    let mut stopping = daemon.stopping();
    let mut greeted = false;
    loop {
        let line = tokio::select! {
            biased;
            _ = Daemon::stopped(&mut stopping) => return,
            line = tokio::time::timeout(IDLE_LIMIT, read_line(&mut reader, MAX_LINE)) => line,
        };
        let line = match line {
            Ok(Ok(Line::Text(line))) => line,
            Ok(Ok(Line::TooLong)) => {
                let message = format!("a line can hold at most {MAX_LINE} bytes");
                let _ = send(&mut write, &failure(ErrorCode::TooLarge, message)).await;
                return;
            }
            Ok(Ok(Line::Closed)) | Ok(Err(_)) | Err(_) => return,
        };
        let _busy = daemon.busy();
        let request = match serde_json::from_slice::<Request>(&line) {
            Ok(request) => request,
            Err(_) => {
                let reply = failure(ErrorCode::BadRequest, "not a message that jevd knows");
                if !send(&mut write, &reply).await || !greeted {
                    return;
                }
                continue;
            }
        };
        if !greeted {
            let (reply, keep) = greet(&request);
            if !send(&mut write, &reply).await || !keep {
                return;
            }
            greeted = true;
            continue;
        }
        let reply = daemon.handle(request, peer).await;
        if !send(&mut write, &reply).await {
            return;
        }
    }
}

/// The reply to the first message, and whether the connection stays open.
fn greet(request: &Request) -> (Reply, bool) {
    match request {
        Request::Hello { protocol } if *protocol == PROTOCOL_VERSION => (
            Reply::success(json!({
                "protocol": PROTOCOL_VERSION,
                "daemon_version": env!("CARGO_PKG_VERSION"),
            })),
            true,
        ),
        Request::Hello { .. } => (
            failure(
                ErrorCode::ProtocolUnsupported,
                format!("this jevd speaks protocol {PROTOCOL_VERSION}"),
            ),
            false,
        ),
        _ => (failure(ErrorCode::BadRequest, "send hello first"), false),
    }
}

/// Writes one reply. False when the write failed or took over `WRITE_LIMIT`:
/// the connection must close.
async fn send<W: AsyncWrite + Unpin>(write: &mut W, reply: &Reply) -> bool {
    let line = reply.to_line();
    let sent = tokio::time::timeout(WRITE_LIMIT, async {
        write.write_all(line.as_bytes()).await?;
        write.flush().await
    })
    .await;
    matches!(sent, Ok(Ok(())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::daemon::tests::{fixture_with, gated, pair_socket, Outcome};
    use crate::keystore::MemoryKeyring;
    use tokio::io::{DuplexStream, ReadHalf, WriteHalf};
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    struct Client {
        lines: tokio::io::Lines<BufReader<ReadHalf<DuplexStream>>>,
        write: WriteHalf<DuplexStream>,
    }

    impl Client {
        async fn send(&mut self, line: &str) {
            self.write.write_all(line.as_bytes()).await.unwrap();
            self.write.write_all(b"\n").await.unwrap();
        }

        /// The next reply, or `None` when the server closed the connection.
        async fn reply(&mut self) -> Option<Reply> {
            let line = self.lines.next_line().await.unwrap()?;
            Some(serde_json::from_str(&line).unwrap())
        }
    }

    fn connect(daemon: &Arc<Daemon>, peer: Peer) -> Client {
        connect_sized(daemon, peer, 4 * MAX_LINE).0
    }

    /// Like `connect`, with a pipe of `buffer` bytes, and the `serve` task.
    fn connect_sized(
        daemon: &Arc<Daemon>,
        peer: Peer,
        buffer: usize,
    ) -> (Client, tokio::task::JoinHandle<()>) {
        let (ours, theirs) = tokio::io::duplex(buffer);
        let task = tokio::spawn(serve(theirs, peer, Arc::clone(daemon)));
        let (read, write) = tokio::io::split(ours);
        let client = Client {
            lines: BufReader::new(read).lines(),
            write,
        };
        (client, task)
    }

    fn code(reply: &Reply) -> Option<ErrorCode> {
        reply.error.as_ref().and_then(|error| error.code.known())
    }

    fn daemon() -> crate::daemon::tests::Fixture {
        fixture_with(Arc::new(MemoryKeyring::new()), "http://127.0.0.1:9/")
    }

    /// True when the server closed the connection within 5 s, so the idle
    /// limit did not do it.
    async fn closed_at_once(client: &mut Client) -> bool {
        tokio::time::timeout(Duration::from_secs(5), client.reply())
            .await
            .expect("closed at once, not at the idle limit")
            .is_none()
    }

    const HELLO: &str = r#"{"type":"hello","protocol":1}"#;

    #[tokio::test]
    async fn read_line_keeps_the_cap() {
        let mut exact = BufReader::new(&b"abc\nabcd\nab"[..]);
        assert_eq!(
            read_line(&mut exact, 3).await.unwrap(),
            Line::Text(b"abc".to_vec())
        );
        assert_eq!(read_line(&mut exact, 3).await.unwrap(), Line::TooLong);
        let mut tail = BufReader::new(&b"ab"[..]);
        assert_eq!(
            read_line(&mut tail, 3).await.unwrap(),
            Line::Text(b"ab".to_vec())
        );
        assert_eq!(read_line(&mut tail, 3).await.unwrap(), Line::Closed);
    }

    #[tokio::test]
    async fn hello_comes_first_and_gives_the_versions() {
        let f = daemon();
        let mut client = connect(&f.daemon, Peer::Other);
        client.send(HELLO).await;
        let reply = client.reply().await.unwrap();
        assert!(reply.ok);
        assert_eq!(reply.data["protocol"], 1);
        assert_eq!(reply.data["daemon_version"], env!("CARGO_PKG_VERSION"));
        client.send(r#"{"type":"status"}"#).await;
        assert!(client.reply().await.unwrap().ok);
    }

    #[tokio::test]
    async fn a_message_before_hello_closes_the_connection() {
        let f = daemon();
        let mut client = connect(&f.daemon, Peer::Other);
        client.send(r#"{"type":"status"}"#).await;
        assert_eq!(
            code(&client.reply().await.unwrap()),
            Some(ErrorCode::BadRequest)
        );
        assert!(closed_at_once(&mut client).await);
    }

    #[tokio::test]
    async fn a_bad_line_before_hello_closes_the_connection() {
        let f = daemon();
        let mut client = connect(&f.daemon, Peer::Other);
        client.send("not json").await;
        assert_eq!(
            code(&client.reply().await.unwrap()),
            Some(ErrorCode::BadRequest)
        );
        let closed = tokio::time::timeout(Duration::from_secs(5), client.reply()).await;
        assert!(closed
            .expect("closed at once, not at the idle limit")
            .is_none());
    }

    #[tokio::test]
    async fn another_protocol_version_is_refused() {
        let f = daemon();
        let mut client = connect(&f.daemon, Peer::Other);
        client.send(r#"{"type":"hello","protocol":2}"#).await;
        assert_eq!(
            code(&client.reply().await.unwrap()),
            Some(ErrorCode::ProtocolUnsupported)
        );
        assert!(closed_at_once(&mut client).await);
    }

    #[tokio::test]
    async fn a_bad_line_after_hello_keeps_the_connection() {
        let f = daemon();
        let mut client = connect(&f.daemon, Peer::Other);
        client.send(HELLO).await;
        client.reply().await.unwrap();
        client.send("not json").await;
        assert_eq!(
            code(&client.reply().await.unwrap()),
            Some(ErrorCode::BadRequest)
        );
        client.send(r#"{"type":"nope"}"#).await;
        assert_eq!(
            code(&client.reply().await.unwrap()),
            Some(ErrorCode::BadRequest)
        );
        client.send(r#"{"type":"status"}"#).await;
        assert!(client.reply().await.unwrap().ok);
    }

    #[tokio::test]
    async fn a_line_over_the_cap_is_too_large_and_the_daemon_keeps_serving() {
        let f = daemon();
        let mut client = connect(&f.daemon, Peer::Other);
        client.send(HELLO).await;
        client.reply().await.unwrap();
        client.send(&"x".repeat(MAX_LINE + 1)).await;
        assert_eq!(
            code(&client.reply().await.unwrap()),
            Some(ErrorCode::TooLarge)
        );
        assert!(closed_at_once(&mut client).await);
        let mut next = connect(&f.daemon, Peer::Other);
        next.send(HELLO).await;
        assert!(next.reply().await.unwrap().ok);
    }

    #[tokio::test]
    async fn admin_messages_follow_the_peer() {
        let f = daemon();
        let mut other = connect(&f.daemon, Peer::Other);
        other.send(HELLO).await;
        other.reply().await.unwrap();
        other.send(r#"{"type":"clients"}"#).await;
        assert_eq!(
            code(&other.reply().await.unwrap()),
            Some(ErrorCode::Forbidden)
        );
        let mut jev = connect(&f.daemon, Peer::Jev);
        jev.send(HELLO).await;
        jev.reply().await.unwrap();
        jev.send(r#"{"type":"clients"}"#).await;
        assert!(jev.reply().await.unwrap().ok);
    }

    #[tokio::test(start_paused = true)]
    async fn an_idle_connection_closes_after_the_limit() {
        let f = daemon();
        let mut client = connect(&f.daemon, Peer::Other);
        client.send(HELLO).await;
        client.reply().await.unwrap();
        let started = tokio::time::Instant::now();
        assert!(client.reply().await.is_none());
        assert!(started.elapsed() >= IDLE_LIMIT);
        assert!(started.elapsed() < IDLE_LIMIT + Duration::from_secs(1));
    }

    #[tokio::test]
    async fn a_stop_closes_an_idle_connection() {
        let f = daemon();
        let mut client = connect(&f.daemon, Peer::Other);
        client.send(HELLO).await;
        client.reply().await.unwrap();
        f.daemon.request_stop();
        let closed = tokio::time::timeout(Duration::from_secs(5), client.reply()).await;
        assert!(closed
            .expect("closed at once, not at the idle limit")
            .is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn a_client_that_never_reads_is_dropped_at_the_write_limit() {
        let f = daemon();
        let (mut client, task) = connect_sized(&f.daemon, Peer::Other, 64);
        client.send(HELLO).await;
        client.reply().await.unwrap();
        // The reply to `status` is far over the 64 bytes of the pipe, and
        // this client never reads it.
        client.send(r#"{"type":"status"}"#).await;
        tokio::time::timeout(WRITE_LIMIT + Duration::from_secs(5), task)
            .await
            .expect("serve gave up on the write")
            .unwrap();
        assert!(f.daemon.idle_since().is_some());
    }

    const ASK: &str = r#"{"type":"ask","token":"TOKEN","state":"s","questions":{"q":{"type":"noul","instructions":"Is it done?"}}}"#;

    async fn jev() -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"answers": {"q": {"type": "noul", "noul": 0.8}}})),
            )
            .mount(&server)
            .await;
        server
    }

    #[tokio::test]
    async fn the_idle_timer_does_not_run_while_a_request_runs() {
        let server = jev().await;
        let (store, release) = gated(Outcome::Key);
        let f = fixture_with(store.clone(), &server.uri());
        let token = pair_socket(&f.daemon, "app", "default").await;
        let mut client = connect(&f.daemon, Peer::Other);
        client.send(HELLO).await;
        client.reply().await.unwrap();
        client.send(&ASK.replace("TOKEN", &token)).await;
        store.wait_for_calls(1).await;
        assert!(f.daemon.idle_since().is_none());
        // More than the idle limit passes while the key read waits.
        tokio::time::pause();
        tokio::time::advance(IDLE_LIMIT + Duration::from_secs(1)).await;
        tokio::time::resume();
        assert!(f.daemon.idle_since().is_none());
        release.send(()).unwrap();
        let reply = tokio::time::timeout(Duration::from_secs(10), client.reply())
            .await
            .expect("the reply came")
            .expect("the connection stayed open");
        assert!(reply.ok, "{reply:?}");
    }

    #[tokio::test]
    async fn a_stop_during_a_request_lets_it_finish_then_closes() {
        let server = jev().await;
        let (store, release) = gated(Outcome::Key);
        let f = fixture_with(store.clone(), &server.uri());
        let token = pair_socket(&f.daemon, "app", "default").await;
        let mut client = connect(&f.daemon, Peer::Other);
        client.send(HELLO).await;
        client.reply().await.unwrap();
        client.send(&ASK.replace("TOKEN", &token)).await;
        store.wait_for_calls(1).await;
        f.daemon.request_stop();
        release.send(()).unwrap();
        let reply = tokio::time::timeout(Duration::from_secs(10), client.reply())
            .await
            .expect("the reply came")
            .expect("the request in flight got its reply");
        assert!(reply.ok, "{reply:?}");
        assert!(closed_at_once(&mut client).await);
    }

    #[tokio::test]
    async fn a_stop_wins_over_a_line_already_waiting() {
        for _ in 0..20 {
            let f = daemon();
            let mut client = connect(&f.daemon, Peer::Other);
            client.send(HELLO).await;
            client.reply().await.unwrap();
            f.daemon.request_stop();
            client.send(r#"{"type":"status"}"#).await;
            assert!(
                closed_at_once(&mut client).await,
                "a pipelined line was served"
            );
        }
    }
}
