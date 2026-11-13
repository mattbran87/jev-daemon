//! One `ask` (spec section 4): shape, token, key, the key in object keys,
//! rate limit, then the Jev call, which redacts, sends, and checks the answer.

use std::sync::Arc;
use std::time::Instant;

use jev_proto::{ErrorCode, Reply, WireError};
use serde_json::{json, Value};
use zeroize::Zeroizing;

use crate::daemon::{failure, unix_now, Daemon};
use crate::jev::names_key;
use crate::keycache::{read_key, Lookup, Ticket, READ_LIMIT};
use crate::shape::check_ask;

impl Daemon {
    pub(crate) async fn ask(
        self: &Arc<Self>,
        token: &str,
        state: &Value,
        questions: &Value,
    ) -> Reply {
        if let Err(problem) = check_ask(state, questions) {
            return failure(ErrorCode::BadRequest, problem);
        }
        let client = match self.lock().pairings.authenticate(token, unix_now()) {
            Ok(client) => client,
            Err(code) => return token_failure(code),
        };
        // The key check comes before the rate limit, so a bad key uses no slot.
        let key = match self.key(&client.profile).await {
            Ok(key) => key,
            Err(error) => return Reply::failure(error.for_profile(&client.profile)),
        };
        // Redaction leaves object keys as they are, so a request that names
        // the key in one never goes out. Like a bad key, it uses no slot.
        if names_key(state, &key) || names_key(questions, &key) {
            return failure(
                ErrorCode::BadRequest,
                "an object key in state or questions contains the API key",
            );
        }
        let strict = {
            let mut state = self.lock();
            // The key read may have taken long: look the token up again, so a
            // revoke or a replacing pairing in the meantime stops this ask.
            let client = match state.pairings.current(token) {
                Ok(client) => client,
                Err(code) => return token_failure(code),
            };
            let limit = client.limit().unwrap_or(state.settings.rate_limit);
            if let Err(wait) = state.limiter.check(&client.name, limit, Instant::now()) {
                return Reply::failure(WireError {
                    retry_after: Some(wait),
                    ..WireError::new(
                        ErrorCode::RateLimited,
                        format!("rate limit reached; try again in {wait} s"),
                    )
                });
            }
            state.settings.redact_strict
        };
        match self.jev.ask(state, questions, &key, strict).await {
            Ok(answers) => Reply::success(json!({ "answers": answers })),
            Err(failure) => Reply::failure(failure.to_wire().for_profile(&client.profile)),
        }
    }

    /// The checked key of `profile`, from the cache or the keyring. One
    /// keyring read per profile runs at a time; see `keycache::Lookup`.
    async fn key(self: &Arc<Self>, profile: &str) -> Result<Zeroizing<String>, WireError> {
        loop {
            // Subscribe before the lookup, so no outcome slips between them.
            let mut outcomes = self.reads.subscribe();
            let lookup = self.lock().keys.lookup(profile, Instant::now());
            let read = match lookup {
                Lookup::Hit(key) => return Ok(key),
                Lookup::Miss(ticket) => {
                    return match self.spawn_read(ticket, profile.to_string()).await {
                        Ok(result) => result,
                        Err(_) => Err(read_failed()),
                    };
                }
                Lookup::Wait(read) => read,
            };
            let _ = tokio::time::timeout(READ_LIMIT, outcomes.changed()).await;
            let failed = match &*outcomes.borrow() {
                Some((id, Err(error))) if *id == read => Some(error.clone()),
                _ => None,
            };
            if let Some(error) = failed {
                return Err(error);
            }
        }
    }

    /// Reads the key on a blocking thread, in a task that the request does
    /// not own, so a dropped connection still fills the cache.
    fn spawn_read(
        self: &Arc<Self>,
        ticket: Ticket,
        profile: String,
    ) -> tokio::task::JoinHandle<Result<Zeroizing<String>, WireError>> {
        let daemon = Arc::clone(self);
        tokio::spawn(async move {
            let read = ticket.read_id();
            let store = Arc::clone(&daemon.store);
            let blocking = tokio::task::spawn_blocking(move || read_key(store.as_ref(), &profile));
            let result = match blocking.await {
                Ok(result) => daemon.lock().keys.finish(ticket, result, Instant::now()),
                Err(_) => {
                    daemon.lock().keys.abandon(ticket);
                    Err(read_failed())
                }
            };
            let outcome = result.as_ref().map(|_| ()).map_err(Clone::clone);
            daemon.reads.send_replace(Some((read, outcome)));
            result
        })
    }
}

fn token_failure(code: ErrorCode) -> Reply {
    match code {
        ErrorCode::Revoked => failure(ErrorCode::Revoked, "this token was revoked; pair again"),
        code => failure(code, "this token is not paired; pair first"),
    }
}

fn read_failed() -> WireError {
    WireError::new(ErrorCode::Internal, "the keyring read failed")
}

#[cfg(test)]
mod tests {
    use crate::daemon::tests::{
        call, code_of, finished, fixture_with, gated, pair_socket, wait_for_readers, Outcome, KEY,
    };
    use crate::daemon::Peer;
    use crate::keystore::{profile_account, KeyStore, MemoryKeyring};
    use jev_proto::{ErrorCode, Request};
    use serde_json::{json, Value};
    use std::sync::Arc;
    use wiremock::matchers::{header, method};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn questions() -> Value {
        json!({"q": {"type": "noul", "instructions": "Is it done?"}})
    }

    fn ask(token: &str) -> Request {
        Request::Ask {
            token: token.into(),
            state: json!("the state"),
            questions: questions(),
        }
    }

    async fn jev() -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(header("authorization", format!("Bearer {KEY}").as_str()))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"answers": {"q": {"type": "noul", "noul": 0.8}}})),
            )
            .mount(&server)
            .await;
        server
    }

    fn keyring_with_key() -> Arc<MemoryKeyring> {
        let store = MemoryKeyring::new();
        store.set(&profile_account("default"), KEY).unwrap();
        Arc::new(store)
    }

    #[tokio::test]
    async fn a_good_ask_gets_the_answers_and_reads_the_key_once() {
        let server = jev().await;
        let store = keyring_with_key();
        let f = fixture_with(store.clone(), &server.uri());
        let token = pair_socket(&f.daemon, "app", "default").await;
        for _ in 0..2 {
            let reply = call(&f.daemon, Peer::Other, ask(&token)).await;
            assert!(reply.ok, "{reply:?}");
            assert_eq!(reply.data["answers"]["q"]["noul"], 0.8);
        }
        assert_eq!(store.reads(), 1);
    }

    #[tokio::test]
    async fn a_bad_shape_is_refused_before_the_token_and_the_key() {
        let store = keyring_with_key();
        let f = fixture_with(store.clone(), "http://127.0.0.1:9/");
        let reply = call(
            &f.daemon,
            Peer::Other,
            Request::Ask {
                token: "jevt_unknown".into(),
                state: json!("s"),
                questions: json!({}),
            },
        )
        .await;
        assert_eq!(code_of(&reply), Some(ErrorCode::BadRequest));
        assert_eq!(store.reads(), 0);
    }

    #[tokio::test]
    async fn an_unknown_token_is_not_paired_and_a_revoked_one_is_revoked() {
        let store = keyring_with_key();
        let f = fixture_with(store.clone(), "http://127.0.0.1:9/");
        let reply = call(&f.daemon, Peer::Other, ask("jevt_unknown")).await;
        assert_eq!(code_of(&reply), Some(ErrorCode::NotPaired));
        let token = pair_socket(&f.daemon, "app", "default").await;
        call(&f.daemon, Peer::Jev, Request::Revoke { name: "app".into() }).await;
        let reply = call(&f.daemon, Peer::Other, ask(&token)).await;
        assert_eq!(code_of(&reply), Some(ErrorCode::Revoked));
        assert_eq!(store.reads(), 0);
    }

    #[tokio::test]
    async fn a_missing_key_uses_no_rate_limit_slot() {
        let server = jev().await;
        let store = Arc::new(MemoryKeyring::new());
        let f = fixture_with(store.clone(), &server.uri());
        let token = pair_socket(&f.daemon, "app", "default").await;
        let one = Request::SetLimit {
            name: "app".into(),
            limit: Some("1/60".into()),
        };
        assert!(call(&f.daemon, Peer::Jev, one).await.ok);
        let reply = call(&f.daemon, Peer::Other, ask(&token)).await;
        assert_eq!(code_of(&reply), Some(ErrorCode::NoKey));
        assert_eq!(
            reply.error.as_ref().unwrap().profile.as_deref(),
            Some("default")
        );
        store.set(&profile_account("default"), KEY).unwrap();
        let reply = call(&f.daemon, Peer::Other, ask(&token)).await;
        assert!(reply.ok, "the failed ask used no slot: {reply:?}");
        let reply = call(&f.daemon, Peer::Other, ask(&token)).await;
        assert_eq!(code_of(&reply), Some(ErrorCode::RateLimited));
        assert_eq!(reply.error.as_ref().unwrap().profile, None);
        let wait = reply.error.unwrap().retry_after.unwrap();
        assert!((1..=60).contains(&wait), "{wait}");
    }

    #[tokio::test]
    async fn the_121st_ask_in_the_window_is_refused() {
        let server = jev().await;
        let f = fixture_with(keyring_with_key(), &server.uri());
        let token = pair_socket(&f.daemon, "app", "default").await;
        for _ in 0..120 {
            assert!(call(&f.daemon, Peer::Other, ask(&token)).await.ok);
        }
        let reply = call(&f.daemon, Peer::Other, ask(&token)).await;
        assert_eq!(code_of(&reply), Some(ErrorCode::RateLimited));
    }

    #[tokio::test]
    async fn jev_refusing_the_key_is_unauthorized_and_the_reply_hides_the_key() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(401).set_body_string(format!("bad key {KEY}")))
            .mount(&server)
            .await;
        let f = fixture_with(keyring_with_key(), &server.uri());
        let token = pair_socket(&f.daemon, "app", "default").await;
        let reply = call(&f.daemon, Peer::Other, ask(&token)).await;
        assert_eq!(code_of(&reply), Some(ErrorCode::JevUnauthorized));
        assert_eq!(
            reply.error.as_ref().unwrap().profile.as_deref(),
            Some("default")
        );
        assert!(!reply.to_line().contains(KEY));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn two_asks_at_once_share_one_keyring_read() {
        let server = jev().await;
        let (store, release) = gated(Outcome::Key);
        let f = fixture_with(store.clone(), &server.uri());
        let token = pair_socket(&f.daemon, "app", "default").await;
        let first = tokio::spawn({
            let daemon = f.daemon.clone();
            let token = token.clone();
            async move { call(&daemon, Peer::Other, ask(&token)).await }
        });
        store.wait_for_calls(1).await;
        let second = tokio::spawn({
            let daemon = f.daemon.clone();
            async move { call(&daemon, Peer::Other, ask(&token)).await }
        });
        wait_for_readers(&f.daemon, 2).await;
        release.send(()).unwrap();
        drop(release);
        let (a, b) = (finished(first).await, finished(second).await);
        assert!(a.ok && b.ok, "{a:?} {b:?}");
        assert_eq!(store.calls(), 1);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_failed_read_fails_every_waiter_once() {
        let (store, release) = gated(Outcome::Locked);
        let f = fixture_with(store.clone(), "http://127.0.0.1:9/");
        let token = pair_socket(&f.daemon, "app", "default").await;
        let first = tokio::spawn({
            let daemon = f.daemon.clone();
            let token = token.clone();
            async move { call(&daemon, Peer::Other, ask(&token)).await }
        });
        store.wait_for_calls(1).await;
        let second = tokio::spawn({
            let daemon = f.daemon.clone();
            async move { call(&daemon, Peer::Other, ask(&token)).await }
        });
        wait_for_readers(&f.daemon, 2).await;
        release.send(()).unwrap();
        drop(release);
        let (a, b) = (finished(first).await, finished(second).await);
        assert_eq!(code_of(&a), Some(ErrorCode::Internal));
        assert_eq!(code_of(&b), Some(ErrorCode::Internal));
        // Only key errors name a profile; a locked keyring is internal.
        assert_eq!(a.error.as_ref().unwrap().profile, None);
        assert_eq!(b.error.as_ref().unwrap().profile, None);
        assert_eq!(store.calls(), 1);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_panic_in_the_keyring_is_internal_and_the_next_ask_reads_again() {
        let server = jev().await;
        let (store, release) = gated(Outcome::PanicFirst);
        release.send(()).unwrap();
        release.send(()).unwrap();
        drop(release);
        let f = fixture_with(store.clone(), &server.uri());
        let token = pair_socket(&f.daemon, "app", "default").await;
        let reply = call(&f.daemon, Peer::Other, ask(&token)).await;
        assert_eq!(code_of(&reply), Some(ErrorCode::Internal));
        let reply = call(&f.daemon, Peer::Other, ask(&token)).await;
        assert!(reply.ok, "{reply:?}");
        assert_eq!(store.calls(), 2);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_revoke_during_the_key_read_stops_the_ask_before_jev() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&server)
            .await;
        let (store, release) = gated(Outcome::Key);
        let f = fixture_with(store.clone(), &server.uri());
        let token = pair_socket(&f.daemon, "app", "default").await;
        let asking = tokio::spawn({
            let daemon = f.daemon.clone();
            async move { call(&daemon, Peer::Other, ask(&token)).await }
        });
        store.wait_for_calls(1).await;
        let revoke = Request::Revoke { name: "app".into() };
        assert!(call(&f.daemon, Peer::Jev, revoke).await.ok);
        release.send(()).unwrap();
        drop(release);
        let reply = finished(asking).await;
        assert_eq!(code_of(&reply), Some(ErrorCode::Revoked), "{reply:?}");
        server.verify().await;
    }

    #[tokio::test]
    async fn reload_wipes_the_cached_key() {
        let server = jev().await;
        let store = keyring_with_key();
        let f = fixture_with(store.clone(), &server.uri());
        let token = pair_socket(&f.daemon, "app", "default").await;
        assert!(call(&f.daemon, Peer::Other, ask(&token)).await.ok);
        assert!(call(&f.daemon, Peer::Jev, Request::Reload).await.ok);
        assert!(call(&f.daemon, Peer::Other, ask(&token)).await.ok);
        assert_eq!(store.reads(), 2);
    }

    /// One ask that holds `KEY` in an object key, and one good ask, with a
    /// limit of 1 per minute: the refusal must use no slot.
    async fn refused_before_the_slot(state: Value, questions: Value) {
        let server = jev().await;
        let f = fixture_with(keyring_with_key(), &server.uri());
        let token = pair_socket(&f.daemon, "app", "default").await;
        let one = Request::SetLimit {
            name: "app".into(),
            limit: Some("1/60".into()),
        };
        assert!(call(&f.daemon, Peer::Jev, one).await.ok);
        let request = Request::Ask {
            token: token.clone(),
            state,
            questions,
        };
        let reply = call(&f.daemon, Peer::Other, request).await;
        assert_eq!(code_of(&reply), Some(ErrorCode::BadRequest));
        assert_eq!(
            reply.error.as_ref().unwrap().message,
            "an object key in state or questions contains the API key"
        );
        assert!(!reply.to_line().contains(KEY));
        assert!(server.received_requests().await.unwrap().is_empty());
        let reply = call(&f.daemon, Peer::Other, ask(&token)).await;
        assert!(reply.ok, "the refused ask used no slot: {reply:?}");
    }

    #[tokio::test]
    async fn the_key_in_a_state_object_key_is_bad_request() {
        let state = json!({"notes": {format!("token {KEY}"): "x"}});
        refused_before_the_slot(state, questions()).await;
    }

    #[tokio::test]
    async fn the_key_in_an_option_name_is_bad_request() {
        let questions = json!({"c": {
            "type": "choice",
            "instructions": "Which one?",
            "criteria": {"calm": "Calm", KEY: "The key"},
        }});
        refused_before_the_slot(json!("the state"), questions).await;
    }

    #[tokio::test]
    async fn the_key_in_a_string_value_still_goes_out_redacted() {
        let server = jev().await;
        let f = fixture_with(keyring_with_key(), &server.uri());
        let token = pair_socket(&f.daemon, "app", "default").await;
        let request = Request::Ask {
            token,
            state: json!({"note": format!("token {KEY}")}),
            questions: questions(),
        };
        let reply = call(&f.daemon, Peer::Other, request).await;
        assert!(reply.ok, "{reply:?}");
        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1);
        let sent = String::from_utf8(requests[0].body.clone()).unwrap();
        assert!(!sent.contains(KEY), "{sent}");
        assert!(sent.contains("[redacted]"), "{sent}");
    }
}
