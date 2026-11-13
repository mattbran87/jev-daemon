//! Sends one request to Jev over HTTPS (spec section 4, step 7).

use std::time::Duration;

use jev_proto::ErrorCode;
use reqwest::header::{HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use reqwest::redirect::Policy;
use serde_json::Value;

use crate::jev::{
    make_detail, read_response, request_body, JevFailure, ENDPOINT, MAX_ERROR_BYTES,
    MAX_RESPONSE_BYTES,
};

pub const ATTEMPT_TIMEOUT: Duration = Duration::from_secs(15);
pub const RETRY_PAUSE: Duration = Duration::from_secs(1);
pub const RETRY_STATUSES: [u16; 2] = [429, 529];

pub struct JevClient {
    http: reqwest::Client,
    endpoint: String,
    retry_pause: Duration,
}

impl JevClient {
    /// The client for TypeSafe's Jev endpoint.
    pub fn new() -> Result<JevClient, String> {
        JevClient::with_endpoint(ENDPOINT, ATTEMPT_TIMEOUT, RETRY_PAUSE)
    }

    /// A client for another endpoint, for tests. A loopback endpoint skips any
    /// proxy and may use plain http; any other endpoint must be https.
    pub fn with_endpoint(
        endpoint: &str,
        attempt_timeout: Duration,
        retry_pause: Duration,
    ) -> Result<JevClient, String> {
        let url =
            reqwest::Url::parse(endpoint).map_err(|error| format!("bad Jev endpoint: {error}"))?;
        let loopback = matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"));
        let mut builder = reqwest::Client::builder()
            // A redirect would resend the Authorization header to the new host.
            .redirect(Policy::none())
            // The Authorization header never goes over plain http to a real host.
            .https_only(!loopback)
            // Covers the connection, the headers, and the whole body.
            .timeout(attempt_timeout);
        if loopback {
            builder = builder.no_proxy();
        }
        let http = builder
            .build()
            .map_err(|error| format!("cannot build the HTTP client: {error}"))?;
        Ok(JevClient {
            http,
            endpoint: endpoint.to_string(),
            retry_pause,
        })
    }

    /// Sends `state` and `questions` to Jev and returns the checked answers.
    /// `key` must come from `check_key`. Retries once on a 429 or a 529, after
    /// the pause. Never retries a timeout or a network failure.
    pub async fn ask(
        &self,
        state: &Value,
        questions: &Value,
        key: &str,
        strict: bool,
    ) -> Result<Value, JevFailure> {
        let body = request_body(state, questions, key, strict).to_string();
        let mut attempt = 1;
        loop {
            let (status, bytes) = self.attempt(&body, key).await?;
            if attempt == 1 && RETRY_STATUSES.contains(&status) {
                attempt = 2;
                tokio::time::sleep(self.retry_pause).await;
                continue;
            }
            return read_response(status, bytes.as_deref(), questions, key);
        }
    }

    /// One POST. The body comes back as `None` when it goes over its cap, or
    /// when an error body fails partway, so the status still counts.
    async fn attempt(&self, body: &str, key: &str) -> Result<(u16, Option<Vec<u8>>), JevFailure> {
        let mut auth = HeaderValue::from_str(&format!("Bearer {key}"))
            .map_err(|_| JevFailure::new(ErrorCode::BadKey))?;
        auth.set_sensitive(true);
        let mut response = self
            .http
            .post(&self.endpoint)
            .header(AUTHORIZATION, auth)
            .header(CONTENT_TYPE, "application/json")
            .body(body.to_string())
            .send()
            .await
            .map_err(|error| failure(&error, key))?;
        let status = response.status().as_u16();
        let limit = if status == 200 {
            MAX_RESPONSE_BYTES
        } else {
            MAX_ERROR_BYTES
        };
        let mut bytes = Vec::new();
        loop {
            let chunk = match response.chunk().await {
                Ok(Some(chunk)) => chunk,
                Ok(None) => break,
                Err(error) if status != 200 && !error.is_timeout() => return Ok((status, None)),
                Err(error) => return Err(failure(&error, key)),
            };
            bytes.extend_from_slice(&chunk);
            if bytes.len() > limit {
                return Ok((status, None));
            }
        }
        Ok((status, Some(bytes)))
    }
}

/// A timeout, or any other failure to get a response. The error text can
/// name the URL, so it goes through redaction.
fn failure(error: &reqwest::Error, key: &str) -> JevFailure {
    if error.is_timeout() {
        return JevFailure::new(ErrorCode::JevTimeout);
    }
    JevFailure {
        code: ErrorCode::JevUnreachable,
        status: None,
        detail: make_detail(&error_text(error), key),
    }
}

/// The error and each of its sources, joined.
fn error_text(error: &dyn std::error::Error) -> String {
    let mut text = error.to_string();
    let mut source = error.source();
    while let Some(next) = source {
        text.push_str(": ");
        text.push_str(&next.to_string());
        source = next.source();
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::time::Instant;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const KEY: &str = "plainkey123";

    fn questions() -> Value {
        json!({"q": {"type": "noul"}})
    }

    fn good() -> ResponseTemplate {
        ResponseTemplate::new(200)
            .set_body_json(json!({"answers": {"q": {"type": "noul", "noul": 0.5}}}))
    }

    fn client(server: &MockServer) -> JevClient {
        JevClient::with_endpoint(
            &format!("{}/v1/systemone", server.uri()),
            Duration::from_secs(10),
            Duration::from_millis(10),
        )
        .unwrap()
    }

    async fn ask(client: &JevClient) -> Result<Value, JevFailure> {
        client.ask(&json!({}), &questions(), KEY, false).await
    }

    #[tokio::test]
    async fn a_good_request_sends_the_key_and_a_redacted_body() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/systemone"))
            .and(header("authorization", "Bearer plainkey123"))
            .and(header("content-type", "application/json"))
            .respond_with(good())
            .expect(1)
            .mount(&server)
            .await;
        let answers = client(&server)
            .ask(&json!({"note": "plainkey123"}), &questions(), KEY, false)
            .await
            .unwrap();
        assert_eq!(answers, json!({"q": {"type": "noul", "noul": 0.5}}));
        let requests = server.received_requests().await.unwrap();
        let sent: Value = serde_json::from_slice(&requests[0].body).unwrap();
        assert_eq!(
            sent,
            json!({"model": "jev-latest", "state": {"note": "[redacted]"}, "questions": questions()})
        );
    }

    #[tokio::test]
    async fn a_429_then_a_200_retries_once() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(429))
            .up_to_n_times(1)
            .with_priority(1)
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .respond_with(good())
            .expect(1)
            .mount(&server)
            .await;
        assert!(ask(&client(&server)).await.is_ok());
    }

    #[tokio::test]
    async fn a_529_then_a_200_retries_once() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(529))
            .up_to_n_times(1)
            .with_priority(1)
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .respond_with(good())
            .expect(1)
            .mount(&server)
            .await;
        assert!(ask(&client(&server)).await.is_ok());
    }

    #[tokio::test]
    async fn two_429s_give_the_http_error_after_2_attempts() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(429))
            .expect(2)
            .mount(&server)
            .await;
        let failure = ask(&client(&server)).await.unwrap_err();
        assert_eq!(
            (failure.code, failure.status),
            (ErrorCode::JevHttpError, Some(429))
        );
    }

    #[tokio::test]
    async fn a_500_is_not_retried() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(500))
            .expect(1)
            .mount(&server)
            .await;
        let failure = ask(&client(&server)).await.unwrap_err();
        assert_eq!(
            (failure.code, failure.status),
            (ErrorCode::JevHttpError, Some(500))
        );
    }

    #[tokio::test]
    async fn a_401_is_unauthorized_and_its_detail_is_redacted() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(401).set_body_string("unknown key plainkey123"))
            .expect(1)
            .mount(&server)
            .await;
        assert_eq!(
            ask(&client(&server)).await,
            Err(JevFailure {
                code: ErrorCode::JevUnauthorized,
                status: Some(401),
                detail: Some("unknown key [redacted]".to_string()),
            })
        );
    }

    #[tokio::test]
    async fn a_redirect_is_never_followed() {
        let server = MockServer::start().await;
        Mock::given(path("/v1/systemone"))
            .respond_with(
                ResponseTemplate::new(302)
                    .insert_header("location", format!("{}/elsewhere", server.uri()).as_str()),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(path("/elsewhere"))
            .respond_with(good())
            .expect(0)
            .mount(&server)
            .await;
        let failure = ask(&client(&server)).await.unwrap_err();
        assert_eq!(
            (failure.code, failure.status),
            (ErrorCode::JevHttpError, Some(302))
        );
    }

    #[tokio::test]
    async fn a_body_over_one_mib_is_a_bad_answer() {
        let server = MockServer::start().await;
        let big = format!(
            r#"{{"answers": {{"q": {{"type": "noul", "noul": 0.5}}}}, "pad": "{}"}}"#,
            "x".repeat(2 << 20)
        );
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_string(big))
            .mount(&server)
            .await;
        assert_eq!(
            ask(&client(&server)).await,
            Err(JevFailure::new(ErrorCode::BadAnswer))
        );
    }

    #[tokio::test]
    async fn an_error_body_over_4_kib_gives_no_detail() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(500).set_body_string("x".repeat(5000)))
            .mount(&server)
            .await;
        assert_eq!(
            ask(&client(&server)).await,
            Err(JevFailure {
                code: ErrorCode::JevHttpError,
                status: Some(500),
                detail: None,
            })
        );
    }

    #[tokio::test]
    async fn a_malformed_answer_is_a_bad_answer() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"answers": {"q": {"type": "noul", "noul": 2}}})),
            )
            .mount(&server)
            .await;
        assert_eq!(
            ask(&client(&server)).await,
            Err(JevFailure::new(ErrorCode::BadAnswer))
        );
    }

    #[tokio::test]
    async fn a_slow_server_times_out_with_no_retry() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(good().set_delay(Duration::from_secs(3)))
            .expect(1)
            .mount(&server)
            .await;
        let client = JevClient::with_endpoint(
            &format!("{}/v1/systemone", server.uri()),
            Duration::from_millis(300),
            Duration::from_millis(10),
        )
        .unwrap();
        let started = Instant::now();
        assert_eq!(
            ask(&client).await,
            Err(JevFailure::new(ErrorCode::JevTimeout))
        );
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "{:?}",
            started.elapsed()
        );
    }

    #[tokio::test]
    async fn an_unreachable_server_is_unreachable_and_the_detail_hides_the_key() {
        // Bind, read the port, and close it again, so nothing listens there.
        // Another process can take the freed port in the ~2 s refusal window
        // on Windows; this is rare and accepted.
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        // The key sits in the URL, which the error text may name. A refused
        // connection on Windows takes about 2 seconds, so the timeout is long.
        let client = JevClient::with_endpoint(
            &format!("http://127.0.0.1:{port}/v1/{KEY}"),
            Duration::from_secs(10),
            Duration::from_millis(10),
        )
        .unwrap();
        let failure = ask(&client).await.unwrap_err();
        assert_eq!(failure.code, ErrorCode::JevUnreachable);
        assert!(failure.detail.is_some());
        assert!(!format!("{failure:?}").contains(KEY), "{failure:?}");
        assert!(!failure.to_wire().message.contains(KEY));
    }

    #[test]
    fn a_bad_endpoint_is_an_error() {
        assert!(JevClient::with_endpoint("not a url", ATTEMPT_TIMEOUT, RETRY_PAUSE).is_err());
    }

    #[test]
    fn the_production_client_builds() {
        assert!(JevClient::new().is_ok());
    }

    /// A good answer padded to exactly `len` bytes.
    fn padded_answer(len: usize) -> String {
        let head = r#"{"answers": {"q": {"type": "noul", "noul": 0.5}}, "pad": ""#;
        let tail = r#""}"#;
        let body = format!("{head}{}{tail}", "x".repeat(len - head.len() - tail.len()));
        assert_eq!(body.len(), len);
        body
    }

    #[tokio::test]
    async fn the_retry_waits_for_the_pause() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(429))
            .up_to_n_times(1)
            .with_priority(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .respond_with(good())
            .mount(&server)
            .await;
        let client = JevClient::with_endpoint(
            &format!("{}/v1/systemone", server.uri()),
            Duration::from_secs(2),
            Duration::from_millis(500),
        )
        .unwrap();
        let started = Instant::now();
        assert!(ask(&client).await.is_ok());
        assert!(
            started.elapsed() >= Duration::from_millis(500),
            "{:?}",
            started.elapsed()
        );
    }

    #[tokio::test]
    async fn a_200_body_at_the_cap_is_read_and_one_byte_over_is_not() {
        for (len, ok) in [
            (8 * 1024, true),
            (MAX_RESPONSE_BYTES, true),
            (MAX_RESPONSE_BYTES + 1, false),
        ] {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .respond_with(ResponseTemplate::new(200).set_body_string(padded_answer(len)))
                .mount(&server)
                .await;
            let result = ask(&client(&server)).await;
            if ok {
                assert!(result.is_ok(), "{len}: {result:?}");
            } else {
                assert_eq!(result, Err(JevFailure::new(ErrorCode::BadAnswer)), "{len}");
            }
        }
    }

    #[tokio::test]
    async fn an_error_body_at_the_cap_gives_a_detail_and_one_byte_over_does_not() {
        for (len, has_detail) in [(MAX_ERROR_BYTES, true), (MAX_ERROR_BYTES + 1, false)] {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .respond_with(ResponseTemplate::new(500).set_body_string("x".repeat(len)))
                .mount(&server)
                .await;
            let failure = ask(&client(&server)).await.unwrap_err();
            assert_eq!(failure.status, Some(500));
            assert_eq!(failure.detail.is_some(), has_detail, "{len}");
        }
    }

    #[tokio::test]
    async fn a_dropped_connection_is_unreachable_and_not_retried() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;
        // A server that accepts each connection and closes it at once.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let accepted = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&accepted);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                counter.fetch_add(1, Ordering::SeqCst);
                drop(stream);
            }
        });
        let client = JevClient::with_endpoint(
            &format!("http://127.0.0.1:{port}/v1/systemone"),
            Duration::from_secs(5),
            Duration::from_millis(10),
        )
        .unwrap();
        let failure = ask(&client).await.unwrap_err();
        assert_eq!(failure.code, ErrorCode::JevUnreachable);
        assert_eq!(accepted.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn the_unreachable_detail_names_the_underlying_cause() {
        // Another process can take the freed port in the ~2 s refusal window
        // on Windows; this is rare and accepted.
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let client = JevClient::with_endpoint(
            &format!("http://127.0.0.1:{port}/v1/systemone"),
            Duration::from_secs(10),
            Duration::from_millis(10),
        )
        .unwrap();
        let detail = ask(&client).await.unwrap_err().detail.unwrap();
        // The top error only says the request failed; the cause is in its sources.
        assert!(detail.contains("tcp connect error"), "{detail}");
    }

    /// A raw HTTP server on 127.0.0.1. For each connection it reads the whole
    /// request, then hands the stream to `reply`. Returns the port.
    fn raw_server(reply: fn(std::net::TcpStream)) -> u16 {
        use std::io::Read;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut request = Vec::new();
                let mut buffer = [0u8; 4096];
                let complete = loop {
                    let Ok(read) = stream.read(&mut buffer) else {
                        break false;
                    };
                    if read == 0 {
                        break false;
                    }
                    request.extend_from_slice(&buffer[..read]);
                    let text = String::from_utf8_lossy(&request).to_ascii_lowercase();
                    if let Some(end) = text.find("\r\n\r\n") {
                        let length = text[..end]
                            .lines()
                            .find_map(|line| line.strip_prefix("content-length:"))
                            .and_then(|value| value.trim().parse::<usize>().ok())
                            .unwrap_or(0);
                        if request.len() >= end + 4 + length {
                            break true;
                        }
                    }
                };
                if complete {
                    std::thread::spawn(move || reply(stream));
                }
            }
        });
        port
    }

    fn raw_client(port: u16, attempt_timeout: Duration) -> JevClient {
        JevClient::with_endpoint(
            &format!("http://127.0.0.1:{port}/v1/systemone"),
            attempt_timeout,
            Duration::from_millis(10),
        )
        .unwrap()
    }

    #[tokio::test]
    async fn a_body_that_drips_times_out() {
        let port = raw_server(|mut stream| {
            use std::io::Write;
            if stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1000\r\n\r\n")
                .is_err()
            {
                return;
            }
            while stream.write_all(b"x").is_ok() {
                std::thread::sleep(Duration::from_millis(100));
            }
        });
        let client = raw_client(port, Duration::from_millis(300));
        let started = Instant::now();
        assert_eq!(
            ask(&client).await,
            Err(JevFailure::new(ErrorCode::JevTimeout))
        );
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "{:?}",
            started.elapsed()
        );
    }

    #[tokio::test]
    async fn an_error_body_cut_short_keeps_the_status() {
        let port = raw_server(|mut stream| {
            use std::io::Write;
            let _ = stream
                .write_all(b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 100\r\n\r\npartial");
            let _ = stream.flush();
            // Dropping the stream closes the connection 93 bytes short.
        });
        assert_eq!(
            ask(&raw_client(port, Duration::from_secs(10))).await,
            Err(JevFailure {
                code: ErrorCode::JevUnauthorized,
                status: Some(401),
                detail: None,
            })
        );
    }

    #[tokio::test]
    async fn an_error_body_that_drips_still_times_out() {
        let port = raw_server(|mut stream| {
            use std::io::Write;
            if stream
                .write_all(b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 1000\r\n\r\n")
                .is_err()
            {
                return;
            }
            while stream.write_all(b"x").is_ok() {
                std::thread::sleep(Duration::from_millis(100));
            }
        });
        assert_eq!(
            ask(&raw_client(port, Duration::from_millis(300))).await,
            Err(JevFailure::new(ErrorCode::JevTimeout))
        );
    }

    #[tokio::test]
    async fn a_200_body_cut_short_is_unreachable() {
        let port = raw_server(|mut stream| {
            use std::io::Write;
            let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\npartial");
            let _ = stream.flush();
        });
        let failure = ask(&raw_client(port, Duration::from_secs(10)))
            .await
            .unwrap_err();
        assert_eq!(
            (failure.code, failure.status),
            (ErrorCode::JevUnreachable, None)
        );
    }

    #[tokio::test]
    async fn a_plain_http_endpoint_that_is_not_loopback_is_refused_before_sending() {
        // `.invalid` never resolves, so this cannot reach the network; the
        // failure must be the https-only check, not a lookup failure.
        let client = JevClient::with_endpoint(
            "http://example.invalid/v1",
            Duration::from_secs(10),
            Duration::from_millis(10),
        )
        .unwrap();
        let failure = ask(&client).await.unwrap_err();
        assert_eq!(failure.code, ErrorCode::JevUnreachable);
        let detail = failure.detail.unwrap();
        assert!(detail.contains("URL scheme is not allowed"), "{detail}");
    }
}
