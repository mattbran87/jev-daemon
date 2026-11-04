# jev-daemon daemon and transport — implementation plan (plan 3 of 7)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `jevd` a working daemon: it binds a private channel, answers every protocol-1 message, runs each `ask` through the checks, commits a client only when its token reaches its storage, and stops cleanly. Add the small blocking client in `jev-proto` that finds the channel, connects, and starts `jevd` on demand.

**Architecture:** The spec is `docs/superpowers/specs/2026-11-02-jev-daemon-design.md`. This plan covers spec section 2 (lifecycle, key cache use, transport, messages), section 3 (pairing, with decision 18), and the order of section 4. `jevd::daemon` holds the state behind one `std::sync::Mutex` and turns each `Request` into a `Reply`; no code holds that lock across an `await`. `jevd::conn` frames one connection, `jevd::listen` owns the Unix socket or the Windows pipe, `jevd::peer` decides which peer is `jev`, and `jevd::run` drives the accept loop, idle exit, the key sweep, and the stop. `jev_proto::paths` and `jev_proto::client` give every caller the same channel and the same connect rules.

**Tech Stack:** Rust stable (edition 2021, MSRV 1.85, `resolver = "3"`), plus plans 1 and 2's crates. New: tokio's `rt-multi-thread`, `net`, `io-util`, `signal`, `sync`, and `macros` features (and `test-util` for tests), `windows-sys` 0.59 on Windows, `libc` 0.2 on Unix. No `interprocess` crate.

**Builds on:** plans 1 and 2, merged into `main` at `c064c02`. This plan uses plan 2's `KeyCache`, `read_key`, `KeyStore`, and `JevClient` unchanged. Plan 1's `Pairings` changes in Task 1.

**Commit rule:** gate each commit on `cargo test`'s own exit status. Never pipe the test run into another command before the commit decision. End every commit message with the line `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

**TDD rule:** run the red step for real and read the failure. After green, run a short mutation self-check: break each rule in the new code one at a time, and check that a test fails. If a mutant survives, add a test in a separate `test(...)` commit. Restore the file after each mutant.

**Platform rule:** every test in this plan ran on Windows 11 only. Code under `#[cfg(unix)]`, `#[cfg(target_os = "linux")]`, and `#[cfg(target_os = "macos")]` did not compile here. Copy it exactly. Plan 6's CI compiles and tests it on Linux and macOS. If you run on Linux or macOS, run every step there too, and report any failure.

The code blocks are the files as tested, after `cargo fmt`.

## Choices this plan makes

These come from checks run on 2026-11-03, on Windows 11 with Rust 1.99.1:

- **Pipe access list without `interprocess`** (spec section 12). tokio's `ServerOptions::create_with_security_attributes_raw` takes a security descriptor built from the SDDL `O:<SID>D:P(A;;GA;;;<SID>)`: this user owns the pipe, and only this user has access. `first_pipe_instance(true)` makes a second creator fail with error 5. A scratch program checked each call: the owner check, `GetNamedPipeClientProcessId`, and `QueryFullProcessImageNameW`.
- **The client checks the pipe owner,** not the server process. A pipe that another user created first has another owner, so the client refuses it. The SDDL sets the owner on purpose: without `O:`, an elevated `jevd` makes the Administrators group the owner.
- **macOS peer process ID.** tokio's `peer_cred().pid()` uses `LOCAL_PEEREPID` on macOS, so jevd needs no extra code.
- **One thread per client connection.** On Windows, a blocking read on a pipe blocks a write on a copy of the same handle. The client's worker thread does each write, then its read.
- **Single instance on Unix.** `jevd` takes an exclusive `flock` on `jevd.pid` before it binds. The lock makes the check and the bind one step, so a stale socket file can go safely.
- **`JEV_HOME`.** When set, every file and the channel go under that folder. On Windows the pipe name gets a 16-digit hash of the folder. Tests use it, so they never meet the person's daemon.
- **Values the spec left to the plan.** Request line cap 1 MiB. Reply line cap 2 MiB in the client. Stop drain limit 45 s. Client reply timeout 120 s. Start wait 5 s. A connection over `max-connections` gets `daemon-unavailable`, after jevd reads its first line.
- **`jev key remove --force`.** The protocol has no message for it. `jev` revokes each client by name, and `revoke` also cancels that name's approvals that are not committed. Then `jev` sends `reload`, which wipes every key. An approval whose profile is no longer in the settings expires at its commit.

Code reviews during Tasks 1 to 9 changed the design after this plan was written (for example, the start wait is now 10 s). The spec is the current record.

---

## File structure

| File | Responsibility |
| --- | --- |
| `crates/jevd/src/pairing.rs` | Changed: commit at collection, CLI requests, slots, cancels |
| `crates/jev-proto/Cargo.toml` | `libc` on Unix, `windows-sys` on Windows, `tempfile` for tests |
| `crates/jev-proto/src/os.rs` | Windows only: the user's SID, and the owner of a kernel object |
| `crates/jev-proto/src/paths.rs` | `Paths`: config, state, and runtime folders, the socket or pipe, `JEV_HOME` |
| `crates/jev-proto/src/client.rs` | `Connection`, `start_daemon`, `connect_or_start` |
| `crates/jev-proto/src/lib.rs` | Module list and re-exports |
| `crates/jevd/Cargo.toml` | tokio features, `windows-sys`, `libc` |
| `crates/jevd/src/daemon.rs` | `Daemon`, `Config`, `Peer`: the state, and the reply to each message |
| `crates/jevd/src/ask.rs` | The `ask` path: checks in order, the key read off the lock |
| `crates/jevd/src/conn.rs` | One connection: framing, `hello`, idle limit |
| `crates/jevd/src/peer.rs` | `AdminCheck`: which peer runs the `jev` next to `jevd` |
| `crates/jevd/src/listen.rs` | `bind` and `Listener`: the Unix socket or the Windows pipe |
| `crates/jevd/src/log.rs` | The log file |
| `crates/jevd/src/run.rs` | The accept loop, `max-connections`, idle exit, key sweep, signals, stop |
| `crates/jevd/src/main.rs` | The `jevd` binary |
| `crates/jevd/src/lib.rs` | Module list |
| `crates/jevd/tests/serve.rs` | The daemon over its real channel, in process |
| `crates/jevd/tests/binary.rs` | The real `jevd` binary |
| `docs/superpowers/specs/2026-11-02-jev-daemon-design.md` | Records this plan's choices (Task 10) |

---

### Task 1: Commit a client at collection (`jevd::pairing`)

Spec decision 18 and section 3. Plan 1's `approve` saved the client at once. Now `approve` saves nothing:

- A socket client's `pair-status` after the approval commits the client and returns the token once.
- For a request from `jev` (a CLI request), `approve` returns the request ID and the token. `jev` writes the token to the keyring, then sends `pair-status`, which commits.
- The commit revokes an old client of the same name. Until then, the old client keeps working.
- A failed save at the commit is `internal`, and the request stays, so the client can poll again.
- Waiting requests and approvals that are not committed both hold one of the 16 slots. A denied request holds none.
- A repeat `pair` from `jev` for a name with a waiting request from `jev` gets the same request ID and code. Its expiry does not restart. A request from a socket client is never shared, because its request ID must reach only that client.
- A second approval for a name cancels the first approval that is not committed. `revoke` and `revoke_profile` cancel approvals that are not committed, for that name or that profile.
- `status` takes the profile names from the settings. An approval whose profile left the settings expires, and commits nothing.

`start` gets a `cli: bool` argument. `approve` loses `now_unix` and returns an `Approval`. `status` gets `profiles` and `now_unix`, and returns `Result<PairState, ErrorCode>`. `PairState::Approved` carries `Option<String>`: the token for a socket client, `None` for a CLI client.

**Files:**
- Modify: `crates/jevd/src/pairing.rs` (the whole file)

- [ ] **Step 1: Replace the test module**

Replace everything from `#[cfg(test)]` to the end of `crates/jevd/src/pairing.rs` with:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const T: u64 = 1_800_000_000;

    fn store() -> (tempfile::TempDir, Pairings) {
        let dir = tempfile::tempdir().unwrap();
        let pairings = Pairings::load(&dir.path().join("pairings.json")).unwrap();
        (dir, pairings)
    }

    /// The profiles that the settings file knows, in these tests.
    fn known() -> Vec<String> {
        vec!["default".to_string(), "work".to_string()]
    }

    /// Pair a socket client `name` on `profile` all the way, and return its token.
    fn pair_on(pairings: &mut Pairings, name: &str, profile: &str, now: Instant) -> String {
        let (request_id, code) = pairings.start(name, None, false, now).unwrap();
        pairings.approve(&code, profile, now).unwrap();
        match pairings.status(&request_id, &known(), now, T).unwrap() {
            PairState::Approved(Some(token)) => token,
            other => panic!("expected approved, got {other:?}"),
        }
    }

    fn pair(pairings: &mut Pairings, name: &str, now: Instant) -> String {
        pair_on(pairings, name, "default", now)
    }

    #[test]
    fn codes_and_tokens_have_the_spec_shape() {
        let (_dir, mut pairings) = store();
        let now = Instant::now();
        let (request_id, code) = pairings.start("app", None, false, now).unwrap();
        assert_eq!(request_id.len(), 32);
        assert_eq!(code.len(), CODE_LENGTH);
        assert!(code.bytes().all(|b| CODE_ALPHABET.contains(&b)));
        for look_alike in ["0", "O", "1", "I", "L"] {
            assert!(!std::str::from_utf8(CODE_ALPHABET)
                .unwrap()
                .contains(look_alike));
        }
        let token = pair(&mut pairings, "other", now);
        assert!(token.starts_with(TOKEN_PREFIX));
        assert_eq!(token.len(), TOKEN_PREFIX.len() + 64);
    }

    #[test]
    fn the_token_comes_back_once_only() {
        let (_dir, mut pairings) = store();
        let now = Instant::now();
        let (request_id, code) = pairings.start("app", Some("work"), false, now).unwrap();
        assert_eq!(
            pairings.status(&request_id, &known(), now, T),
            Ok(PairState::Pending)
        );
        assert_eq!(
            pairings.pending_list(now),
            [PendingView {
                code: code.clone(),
                name: "app".into(),
                profile: Some("work".into())
            }]
        );
        let approval = pairings.approve(&code.to_lowercase(), "work", now).unwrap();
        assert_eq!(approval.name, "app");
        assert!(!approval.replaces);
        assert!(approval.cli.is_none(), "a socket client collects its token");
        assert!(matches!(
            pairings.status(&request_id, &known(), now, T),
            Ok(PairState::Approved(Some(_)))
        ));
        assert_eq!(
            pairings.status(&request_id, &known(), now, T),
            Ok(PairState::Expired)
        );
        assert!(pairings.pending_list(now).is_empty());
    }

    #[test]
    fn a_wrong_request_id_reports_expired() {
        let (_dir, mut pairings) = store();
        let now = Instant::now();
        pairings.start("app", None, false, now).unwrap();
        assert_eq!(
            pairings.status("0123456789abcdef0123456789abcdef", &known(), now, T),
            Ok(PairState::Expired)
        );
    }

    #[test]
    fn deny_ends_the_request() {
        let (_dir, mut pairings) = store();
        let now = Instant::now();
        let (request_id, code) = pairings.start("app", None, false, now).unwrap();
        pairings.deny(&code, now).unwrap();
        assert_eq!(
            pairings.status(&request_id, &known(), now, T),
            Ok(PairState::Denied)
        );
        assert_eq!(
            pairings.status(&request_id, &known(), now, T),
            Ok(PairState::Expired)
        );
        assert_eq!(pairings.deny(&code, now), Err(ErrorCode::BadRequest));
    }

    #[test]
    fn the_17th_pending_request_is_refused() {
        let (_dir, mut pairings) = store();
        let now = Instant::now();
        for i in 0..MAX_PENDING {
            pairings
                .start(&format!("app{i}"), None, false, now)
                .unwrap();
        }
        assert_eq!(
            pairings.start("one-more", None, false, now),
            Err(ErrorCode::RateLimited)
        );
    }

    #[test]
    fn a_pending_request_expires_after_ten_minutes() {
        let (_dir, mut pairings) = store();
        let t0 = Instant::now();
        let (request_id, code) = pairings.start("app", None, false, t0).unwrap();
        let almost = t0 + PENDING_EXPIRY - Duration::from_secs(1);
        assert_eq!(
            pairings.status(&request_id, &known(), almost, T),
            Ok(PairState::Pending)
        );
        let late = t0 + PENDING_EXPIRY;
        assert_eq!(
            pairings.status(&request_id, &known(), late, T),
            Ok(PairState::Expired)
        );
        assert_eq!(
            pairings.approve(&code, "default", late).err(),
            Some(ErrorCode::BadRequest)
        );
    }

    #[test]
    fn bad_names_are_refused() {
        let (_dir, mut pairings) = store();
        let now = Instant::now();
        assert_eq!(
            pairings.start("Bad Name", None, false, now),
            Err(ErrorCode::BadRequest)
        );
        assert_eq!(
            pairings.start("app", Some("Bad"), false, now),
            Err(ErrorCode::BadRequest)
        );
        let (_, code) = pairings.start("app", None, false, now).unwrap();
        assert_eq!(
            pairings.approve(&code, "Bad", now).err(),
            Some(ErrorCode::BadRequest)
        );
    }

    #[test]
    fn a_token_authenticates_its_client() {
        let (_dir, mut pairings) = store();
        let now = Instant::now();
        let token = pair(&mut pairings, "app", now);
        let client = pairings.authenticate(&token, T).unwrap();
        assert_eq!(
            (client.name.as_str(), client.profile.as_str()),
            ("app", "default")
        );
        assert_eq!(
            pairings.authenticate("jevt_wrong", T),
            Err(ErrorCode::NotPaired)
        );
    }

    #[test]
    fn an_approval_saves_nothing_until_the_commit() {
        let (dir, mut pairings) = store();
        let now = Instant::now();
        let (request_id, code) = pairings.start("app", None, false, now).unwrap();
        pairings.approve(&code, "default", now).unwrap();
        assert!(pairings.clients().is_empty());
        assert!(!dir.path().join("pairings.json").exists());
        let Ok(PairState::Approved(Some(token))) = pairings.status(&request_id, &known(), now, T)
        else {
            panic!("expected the token");
        };
        assert_eq!(pairings.authenticate(&token, T).unwrap().name, "app");
        assert!(dir.path().join("pairings.json").exists());
    }

    #[test]
    fn the_old_client_works_until_the_commit_and_is_revoked_after_it() {
        let (_dir, mut pairings) = store();
        let now = Instant::now();
        let old = pair(&mut pairings, "app", now);
        let (request_id, code) = pairings.start("app", None, false, now).unwrap();
        let approval = pairings.approve(&code, "default", now).unwrap();
        assert!(approval.replaces);
        assert_eq!(pairings.authenticate(&old, T).unwrap().name, "app");
        let Ok(PairState::Approved(Some(new))) = pairings.status(&request_id, &known(), now, T)
        else {
            panic!("expected the token");
        };
        assert_eq!(pairings.authenticate(&old, T), Err(ErrorCode::Revoked));
        assert_eq!(pairings.authenticate(&new, T).unwrap().name, "app");
        assert_eq!(pairings.clients().len(), 1);
    }

    #[test]
    fn a_cli_request_gets_its_token_at_approval() {
        let (_dir, mut pairings) = store();
        let now = Instant::now();
        let (request_id, code) = pairings.start("script", None, true, now).unwrap();
        let approval = pairings.approve(&code, "default", now).unwrap();
        let cli = approval.cli.expect("a CLI request gets the token");
        assert_eq!(cli.request_id, request_id);
        assert!(cli.token.starts_with(TOKEN_PREFIX));
        assert_eq!(
            pairings.authenticate(&cli.token, T),
            Err(ErrorCode::NotPaired),
            "not committed yet"
        );
        assert_eq!(
            pairings.status(&request_id, &known(), now, T),
            Ok(PairState::Approved(None)),
            "the commit does not send the token again"
        );
        assert_eq!(pairings.authenticate(&cli.token, T).unwrap().name, "script");
    }

    #[test]
    fn a_repeat_cli_request_gets_the_same_request() {
        let (_dir, mut pairings) = store();
        let t0 = Instant::now();
        let first = pairings.start("script", None, true, t0).unwrap();
        let later = t0 + Duration::from_secs(300);
        assert_eq!(
            pairings.start("script", Some("work"), true, later).unwrap(),
            first
        );
        assert_eq!(pairings.pending_list(later).len(), 1);
        assert_eq!(
            pairings.status(&first.0, &known(), t0 + PENDING_EXPIRY, T),
            Ok(PairState::Expired),
            "the expiry does not restart"
        );
    }

    #[test]
    fn only_a_waiting_cli_request_is_shared() {
        let (_dir, mut pairings) = store();
        let now = Instant::now();
        let socket = pairings.start("app", None, false, now).unwrap();
        let cli = pairings.start("app", None, true, now).unwrap();
        assert_ne!(cli, socket, "a socket request is never given to jev");
        assert_ne!(pairings.start("app", None, false, now).unwrap(), socket);
        pairings.approve(&cli.1, "default", now).unwrap();
        assert_ne!(
            pairings.start("app", None, true, now).unwrap(),
            cli,
            "an approved request is not waiting"
        );
    }

    #[test]
    fn an_approval_holds_its_slot_until_the_commit() {
        let (_dir, mut pairings) = store();
        let now = Instant::now();
        let mut started = Vec::new();
        for i in 0..MAX_PENDING {
            started.push(
                pairings
                    .start(&format!("app{i}"), None, false, now)
                    .unwrap(),
            );
        }
        pairings.approve(&started[0].1, "default", now).unwrap();
        assert!(pairings
            .pending_list(now)
            .iter()
            .all(|view| view.code != started[0].1));
        assert_eq!(
            pairings.start("one-more", None, false, now),
            Err(ErrorCode::RateLimited)
        );
        pairings.status(&started[0].0, &known(), now, T).unwrap();
        assert!(pairings.start("one-more", None, false, now).is_ok());
    }

    #[test]
    fn a_denied_request_frees_its_slot() {
        let (_dir, mut pairings) = store();
        let now = Instant::now();
        let mut codes = Vec::new();
        for i in 0..MAX_PENDING {
            codes.push(
                pairings
                    .start(&format!("app{i}"), None, false, now)
                    .unwrap()
                    .1,
            );
        }
        pairings.deny(&codes[0], now).unwrap();
        assert!(pairings.start("one-more", None, false, now).is_ok());
    }

    #[test]
    fn a_second_approval_for_a_name_cancels_the_first() {
        let (_dir, mut pairings) = store();
        let now = Instant::now();
        let first = pairings.start("app", None, false, now).unwrap();
        let second = pairings.start("app", None, false, now).unwrap();
        pairings.approve(&first.1, "default", now).unwrap();
        pairings.approve(&second.1, "work", now).unwrap();
        assert_eq!(
            pairings.status(&first.0, &known(), now, T),
            Ok(PairState::Expired)
        );
        assert!(matches!(
            pairings.status(&second.0, &known(), now, T),
            Ok(PairState::Approved(Some(_)))
        ));
        assert_eq!(pairings.clients()[0].profile, "work");
    }

    #[test]
    fn revoke_cancels_an_approval_that_is_not_committed() {
        let (_dir, mut pairings) = store();
        let now = Instant::now();
        let (request_id, code) = pairings.start("app", None, false, now).unwrap();
        pairings.approve(&code, "default", now).unwrap();
        assert_eq!(pairings.revoke("app"), Ok(()));
        assert_eq!(
            pairings.status(&request_id, &known(), now, T),
            Ok(PairState::Expired)
        );
        assert!(pairings.clients().is_empty());
        assert_eq!(pairings.revoke("app"), Err(ErrorCode::BadRequest));
    }

    #[test]
    fn revoke_profile_cancels_approvals_on_it() {
        let (_dir, mut pairings) = store();
        let now = Instant::now();
        let on_it = pairings.start("a", None, false, now).unwrap();
        let other = pairings.start("b", None, false, now).unwrap();
        pairings.approve(&on_it.1, "default", now).unwrap();
        pairings.approve(&other.1, "work", now).unwrap();
        assert!(pairings.revoke_profile("default").unwrap().is_empty());
        assert_eq!(
            pairings.status(&on_it.0, &known(), now, T),
            Ok(PairState::Expired)
        );
        assert!(matches!(
            pairings.status(&other.0, &known(), now, T),
            Ok(PairState::Approved(Some(_)))
        ));
    }

    #[test]
    fn an_approval_whose_profile_went_away_expires() {
        let (_dir, mut pairings) = store();
        let now = Instant::now();
        let (request_id, code) = pairings.start("app", None, false, now).unwrap();
        pairings.approve(&code, "default", now).unwrap();
        assert_eq!(
            pairings.status(&request_id, &["work".to_string()], now, T),
            Ok(PairState::Expired)
        );
        assert!(pairings.clients().is_empty());
    }

    #[test]
    fn a_failed_save_at_the_commit_keeps_the_request() {
        let (dir, mut pairings) = store();
        let path = dir.path().join("pairings.json");
        let now = Instant::now();
        let old = pair(&mut pairings, "app", now);
        let (request_id, code) = pairings.start("app", None, false, now).unwrap();
        pairings.approve(&code, "default", now).unwrap();
        break_saves(&path);
        assert_eq!(
            pairings.status(&request_id, &known(), now, T),
            Err(ErrorCode::Internal)
        );
        assert_eq!(pairings.authenticate(&old, T).unwrap().name, "app");
        std::fs::remove_dir_all(&path).unwrap();
        assert!(matches!(
            pairings.status(&request_id, &known(), now, T),
            Ok(PairState::Approved(Some(_)))
        ));
        assert_eq!(pairings.authenticate(&old, T), Err(ErrorCode::Revoked));
    }

    #[test]
    fn revoke_stops_a_token() {
        let (_dir, mut pairings) = store();
        let now = Instant::now();
        let token = pair(&mut pairings, "app", now);
        pairings.revoke("app").unwrap();
        assert_eq!(pairings.authenticate(&token, T), Err(ErrorCode::Revoked));
        assert_eq!(pairings.revoke("app"), Err(ErrorCode::BadRequest));
    }

    #[test]
    fn revoke_profile_revokes_every_client_on_it() {
        let (_dir, mut pairings) = store();
        let now = Instant::now();
        let a = pair(&mut pairings, "a", now);
        pair(&mut pairings, "b", now);
        pair_on(&mut pairings, "c", "work", now);
        assert_eq!(pairings.revoke_profile("default").unwrap(), ["a", "b"]);
        assert_eq!(pairings.authenticate(&a, T), Err(ErrorCode::Revoked));
        assert_eq!(pairings.clients().len(), 1);
    }

    #[test]
    fn limits_set_and_clear() {
        let (_dir, mut pairings) = store();
        let now = Instant::now();
        pair(&mut pairings, "app", now);
        let limit: Limit = "10/60".parse().unwrap();
        pairings.set_limit("app", Some(limit)).unwrap();
        assert_eq!(pairings.clients()[0].limit(), Some(limit));
        pairings.set_limit("app", None).unwrap();
        assert_eq!(pairings.clients()[0].limit(), None);
        assert_eq!(pairings.set_limit("nope", None), Err(ErrorCode::BadRequest));
    }

    #[test]
    fn last_used_moves_at_most_once_a_minute() {
        let (_dir, mut pairings) = store();
        let now = Instant::now();
        let token = pair(&mut pairings, "app", now);
        assert_eq!(pairings.authenticate(&token, T).unwrap().last_used, Some(T));
        assert_eq!(
            pairings.authenticate(&token, T + 59).unwrap().last_used,
            Some(T)
        );
        assert_eq!(
            pairings.authenticate(&token, T + 60).unwrap().last_used,
            Some(T + 60)
        );
    }

    #[test]
    fn the_file_holds_hashes_and_survives_a_reload() {
        let (dir, mut pairings) = store();
        let now = Instant::now();
        let token = pair(&mut pairings, "app", now);
        let path = dir.path().join("pairings.json");
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains(&token));
        assert!(text.contains(&hash_token(&token)));
        let mut reloaded = Pairings::load(&path).unwrap();
        assert_eq!(reloaded.authenticate(&token, T).unwrap().name, "app");
        assert!(
            reloaded.pending_list(now).is_empty(),
            "pending requests live in memory only"
        );
    }

    #[test]
    fn the_constants_match_the_spec() {
        assert_eq!(CODE_LENGTH, 8);
        assert_eq!(CODE_ALPHABET, b"23456789ABCDEFGHJKMNPQRSTUVWXYZ");
        assert_eq!(TOKEN_PREFIX, "jevt_");
        assert_eq!(PENDING_EXPIRY, Duration::from_secs(600));
        assert_eq!(MAX_PENDING, 16);
    }

    #[test]
    fn codes_use_every_letter_of_the_alphabet() {
        let mut seen = std::collections::BTreeSet::new();
        for _ in 0..1000 {
            seen.extend(new_code().bytes());
        }
        assert_eq!(seen.len(), CODE_ALPHABET.len());
    }

    #[test]
    fn an_expired_code_cannot_be_approved_or_denied() {
        // Two stores: each call purges, and that would hide a call that does not.
        let t0 = Instant::now();
        let late = t0 + PENDING_EXPIRY;
        let (_dir, mut pairings) = store();
        let (_, code) = pairings.start("a", None, false, t0).unwrap();
        assert_eq!(
            pairings.approve(&code, "default", late).err(),
            Some(ErrorCode::BadRequest)
        );
        let (_dir, mut pairings) = store();
        let (_, code) = pairings.start("b", None, false, t0).unwrap();
        assert_eq!(pairings.deny(&code, late), Err(ErrorCode::BadRequest));
    }

    #[test]
    fn expired_requests_leave_the_list_and_free_their_slots() {
        let (_dir, mut pairings) = store();
        let t0 = Instant::now();
        for i in 0..MAX_PENDING {
            pairings.start(&format!("app{i}"), None, false, t0).unwrap();
        }
        let late = t0 + PENDING_EXPIRY;
        assert!(pairings.pending_list(late).is_empty());
        let (_dir, mut pairings) = store();
        for i in 0..MAX_PENDING {
            pairings.start(&format!("app{i}"), None, false, t0).unwrap();
        }
        assert!(pairings.start("one-more", None, false, late).is_ok());
    }

    #[test]
    fn a_code_works_once_only() {
        let (_dir, mut pairings) = store();
        let now = Instant::now();
        let (_, code) = pairings.start("a", None, false, now).unwrap();
        pairings.approve(&code, "default", now).unwrap();
        assert_eq!(
            pairings.approve(&code, "default", now).err(),
            Some(ErrorCode::BadRequest)
        );
        assert_eq!(pairings.deny(&code, now), Err(ErrorCode::BadRequest));
        let (_, code) = pairings.start("b", None, false, now).unwrap();
        pairings.deny(&code, now).unwrap();
        assert_eq!(
            pairings.approve(&code, "default", now).err(),
            Some(ErrorCode::BadRequest)
        );
    }

    #[test]
    fn a_typed_code_may_have_spaces_around_it() {
        let (_dir, mut pairings) = store();
        let now = Instant::now();
        let (_, code) = pairings.start("app", None, false, now).unwrap();
        assert!(pairings
            .approve(&format!("  {code}\n"), "default", now)
            .is_ok());
    }

    #[test]
    fn the_pending_list_is_sorted_by_code() {
        let (_dir, mut pairings) = store();
        let now = Instant::now();
        for i in 0..MAX_PENDING {
            pairings
                .start(&format!("app{i}"), None, false, now)
                .unwrap();
        }
        let codes: Vec<String> = pairings
            .pending_list(now)
            .into_iter()
            .map(|view| view.code)
            .collect();
        let mut sorted = codes.clone();
        sorted.sort();
        assert_eq!(codes, sorted);
    }

    #[test]
    fn the_commit_records_the_created_time() {
        let (_dir, mut pairings) = store();
        pair(&mut pairings, "app", Instant::now());
        assert_eq!(pairings.clients()[0].created, T);
    }

    #[test]
    fn changes_survive_a_reload() {
        let (dir, mut pairings) = store();
        let path = dir.path().join("pairings.json");
        let now = Instant::now();
        let gone = pair(&mut pairings, "gone", now);
        let a = pair(&mut pairings, "a", now);
        pair_on(&mut pairings, "app", "work", now);
        // Reload after each change: every save writes the whole store, so a
        // later save would hide an earlier change that did not save.
        let limit: Limit = "10/60".parse().unwrap();
        pairings.set_limit("app", Some(limit)).unwrap();
        let mut pairings = Pairings::load(&path).unwrap();
        assert_eq!(
            pairings
                .clients()
                .iter()
                .find(|c| c.name == "app")
                .unwrap()
                .limit(),
            Some(limit)
        );
        pairings.revoke("gone").unwrap();
        let mut pairings = Pairings::load(&path).unwrap();
        assert_eq!(pairings.authenticate(&gone, T), Err(ErrorCode::Revoked));
        pairings.revoke_profile("default").unwrap();
        let mut pairings = Pairings::load(&path).unwrap();
        assert_eq!(pairings.authenticate(&a, T), Err(ErrorCode::Revoked));
    }

    #[test]
    fn last_used_survives_a_reload() {
        let (dir, mut pairings) = store();
        let token = pair(&mut pairings, "app", Instant::now());
        pairings.authenticate(&token, T + 5).unwrap();
        let reloaded = Pairings::load(&dir.path().join("pairings.json")).unwrap();
        assert_eq!(reloaded.clients()[0].last_used, Some(T + 5));
    }

    #[test]
    fn a_bad_file_is_an_error_not_an_empty_store() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pairings.json");
        std::fs::write(&path, "not json").unwrap();
        assert!(Pairings::load(&path).is_err());
        assert!(
            Pairings::load(dir.path()).is_err(),
            "a folder cannot be read as the file"
        );
    }

    #[test]
    fn an_approved_token_can_be_collected_for_a_full_window_after_approval() {
        let (_dir, mut pairings) = store();
        let t0 = Instant::now();
        let (late_id, late_code) = pairings.start("late", None, false, t0).unwrap();
        let (gone_id, gone_code) = pairings.start("gone", None, false, t0).unwrap();
        let approved_at = t0 + Duration::from_secs(599);
        pairings
            .approve(&late_code, "default", approved_at)
            .unwrap();
        pairings
            .approve(&gone_code, "default", approved_at)
            .unwrap();
        let collect = t0 + Duration::from_secs(601);
        assert!(matches!(
            pairings.status(&late_id, &known(), collect, T),
            Ok(PairState::Approved(_))
        ));
        assert_eq!(
            pairings.status(&gone_id, &known(), approved_at + PENDING_EXPIRY, T),
            Ok(PairState::Expired)
        );
        assert_eq!(
            pairings.clients().len(),
            1,
            "nothing commits after the window"
        );
    }

    /// Put a folder that is not empty where the pairing file goes, so every save fails.
    fn break_saves(path: &Path) {
        std::fs::remove_file(path).unwrap();
        std::fs::create_dir(path).unwrap();
        std::fs::write(path.join("inside"), "x").unwrap();
    }

    #[test]
    fn a_failed_save_changes_nothing() {
        let (dir, mut pairings) = store();
        let path = dir.path().join("pairings.json");
        let now = Instant::now();
        let old = pair(&mut pairings, "app", now);
        pairings
            .set_limit("app", Some("10/60".parse().unwrap()))
            .unwrap();
        let before = pairings.clients();
        break_saves(&path);

        assert!(pairings.revoke("app").is_err());
        assert_eq!(pairings.authenticate(&old, T).unwrap().name, "app");

        assert!(pairings.revoke_profile("default").is_err());
        assert_eq!(pairings.authenticate(&old, T).unwrap().name, "app");

        assert!(pairings.set_limit("app", None).is_err());
        assert_eq!(
            pairings.clients(),
            before,
            "no limit or last-used change without a save"
        );
        assert_eq!(
            pairings.authenticate(&old, T + 600).unwrap().last_used,
            before[0].last_used
        );
        assert_eq!(pairings.clients(), before);
    }

    #[test]
    fn debug_output_never_holds_a_token() {
        let (_dir, mut pairings) = store();
        let now = Instant::now();
        let (request_id, code) = pairings.start("app", None, false, now).unwrap();
        pairings.approve(&code, "default", now).unwrap();
        let state = pairings.status(&request_id, &known(), now, T).unwrap();
        assert!(matches!(state, PairState::Approved(Some(_))));
        assert_eq!(format!("{state:?}"), "Approved(<redacted>)");
        assert!(!format!("{state:#?}").contains(TOKEN_PREFIX));
        assert_eq!(format!("{:?}", PairState::Pending), "Pending");
        assert_eq!(format!("{:?}", PairState::Denied), "Denied");
        assert_eq!(format!("{:?}", PairState::Expired), "Expired");
        let (_, code) = pairings.start("script", None, true, now).unwrap();
        let approval = pairings.approve(&code, "default", now).unwrap();
        assert!(!format!("{approval:?}").contains(TOKEN_PREFIX));
        assert!(format!("{approval:?}").contains("CliToken(<redacted>)"));
    }

    #[test]
    fn the_file_refuses_unknown_keys() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pairings.json");
        std::fs::write(&path, r#"{"clients": [], "revoked": [], "extra": 1}"#).unwrap();
        assert!(Pairings::load(&path).is_err());
    }

    #[test]
    fn the_file_refuses_a_limit_it_cannot_read() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pairings.json");
        let file = |limit: &str| {
            format!(
                r#"{{"clients": [{{"name": "a", "profile": "default", "token_sha256": "aa", "created": 1, "limit": "{limit}"}}]}}"#
            )
        };
        std::fs::write(&path, file("10/60")).unwrap();
        assert!(Pairings::load(&path).is_ok());
        std::fs::write(&path, file("fast")).unwrap();
        let error = Pairings::load(&path).err().expect("a bad limit is refused");
        assert!(error.contains("the client a"));
    }

    #[test]
    fn the_file_refuses_a_name_or_a_hash_twice() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pairings.json");
        let client = |name: &str, hash: &str| {
            format!(
                r#"{{"name": "{name}", "profile": "default", "token_sha256": "{hash}", "created": 1}}"#
            )
        };
        let good = format!(
            r#"{{"clients": [{}, {}]}}"#,
            client("a", "aa"),
            client("b", "bb")
        );
        std::fs::write(&path, good).unwrap();
        assert!(Pairings::load(&path).is_ok());
        let same_name = format!(
            r#"{{"clients": [{}, {}]}}"#,
            client("a", "aa"),
            client("a", "bb")
        );
        std::fs::write(&path, same_name).unwrap();
        assert!(Pairings::load(&path).is_err());
        let same_hash = format!(
            r#"{{"clients": [{}, {}]}}"#,
            client("a", "aa"),
            client("b", "aa")
        );
        std::fs::write(&path, same_hash).unwrap();
        assert!(Pairings::load(&path).is_err());
    }

    #[test]
    fn a_client_with_an_unknown_key_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pairings.json");
        let text = r#"{"clients": [{"name": "a", "profile": "default", "token_sha256": "aa", "created": 1, "limt": "1/60"}]}"#;
        std::fs::write(&path, text).unwrap();
        assert!(
            Pairings::load(&path).is_err(),
            "a typo such as limt must not drop a limit silently"
        );
    }
}
```

- [ ] **Step 2: Run the tests and read the failure**

Run: `cargo test -p jevd --lib pairing`
Expected: FAIL to compile. The errors name the new arguments of `start`, `approve`, and `status`, `Approval`, and `PairState::Approved(Some(..))`.

- [ ] **Step 3: Replace the code above the test module**

Replace everything above `#[cfg(test)]` in `crates/jevd/src/pairing.rs` with:

```rust
//! Pairing (spec section 3): pending requests in memory, paired clients in a
//! JSON file that holds token hashes, never tokens. Only jevd writes the file.
//!
//! A client is committed (saved, and able to ask) only when its token reaches
//! its storage (spec decision 18). For a socket client, that is the first
//! `pair-status` after the approval. For a CLI client, `approve` gives the
//! token to `jev`, which writes it to the keyring and then sends `pair-status`.

use crate::files::write_atomic;
use crate::units::Limit;
use jev_proto::{valid_name, ErrorCode};
use rand::rngs::OsRng;
use rand::{Rng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// No 0, O, 1, I, or L, so a code reads aloud and types without mistakes.
pub const CODE_ALPHABET: &[u8] = b"23456789ABCDEFGHJKMNPQRSTUVWXYZ";
pub const CODE_LENGTH: usize = 8;
/// How long a request waits for a person, and how long an approval waits for
/// its commit. The second window starts at the approval.
pub const PENDING_EXPIRY: Duration = Duration::from_secs(600);
/// Waiting requests and approvals that are not committed both hold a slot.
pub const MAX_PENDING: usize = 16;
pub const TOKEN_PREFIX: &str = "jevt_";
/// The last-used time moves at most once per this many seconds per client.
const LAST_USED_STEP: u64 = 60;

/// One paired client, as stored in the pairing file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Client {
    pub name: String,
    pub profile: String,
    pub token_sha256: String,
    /// Unix seconds.
    pub created: u64,
    /// Unix seconds.
    #[serde(default)]
    pub last_used: Option<u64>,
    /// A per-client rate limit, `<count>/<seconds>`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<String>,
}

impl Client {
    /// The per-client override, if one is set and readable.
    pub fn limit(&self) -> Option<Limit> {
        self.limit.as_deref().and_then(|text| text.parse().ok())
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    #[serde(default)]
    clients: Vec<Client>,
    /// Hashes of revoked and replaced tokens, so jevd can answer `revoked`.
    #[serde(default)]
    revoked: Vec<String>,
}

/// What `pair-status` reports.
#[derive(Clone, PartialEq, Eq)]
pub enum PairState {
    Pending,
    /// The client is committed. A socket client gets its token here, once
    /// only. A CLI client gets `None`: `approve` gave its token to `jev`.
    Approved(Option<String>),
    Denied,
    /// Also the answer for an unknown request ID.
    Expired,
}

/// Written by hand so the token never reaches a log or a panic message.
impl std::fmt::Debug for PairState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PairState::Pending => f.write_str("Pending"),
            PairState::Approved(_) => f.write_str("Approved(<redacted>)"),
            PairState::Denied => f.write_str("Denied"),
            PairState::Expired => f.write_str("Expired"),
        }
    }
}

impl PairState {
    pub fn as_str(&self) -> &'static str {
        match self {
            PairState::Pending => "pending",
            PairState::Approved(_) => "approved",
            PairState::Denied => "denied",
            PairState::Expired => "expired",
        }
    }
}

/// A pending request, as `jev allow` lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingView {
    pub code: String,
    pub name: String,
    pub profile: Option<String>,
}

/// What `approve` returns. Nothing is saved yet.
#[derive(Debug)]
pub struct Approval {
    pub name: String,
    /// True when a client of that name is committed now. The commit replaces it.
    pub replaces: bool,
    /// For a request from `jev`: the request ID and the token, for `jev` to store.
    pub cli: Option<CliToken>,
}

/// The token of an approved CLI request, and the request ID that commits it.
pub struct CliToken {
    pub request_id: String,
    pub token: String,
}

/// Written by hand so the token never reaches a log or a panic message.
impl std::fmt::Debug for CliToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CliToken(<redacted>)")
    }
}

enum Stage {
    Waiting,
    /// Approved, not committed. `token` is `None` for a CLI request.
    Approved {
        profile: String,
        token_sha256: String,
        token: Option<String>,
    },
    Denied,
}

struct Pending {
    code: String,
    name: String,
    profile: Option<String>,
    /// True when the request came from the `jev` binary.
    cli: bool,
    /// The start, or the approval once approved.
    created: Instant,
    stage: Stage,
}

impl Pending {
    fn holds_slot(&self) -> bool {
        !matches!(self.stage, Stage::Denied)
    }

    fn approved_profile(&self) -> Option<&str> {
        match &self.stage {
            Stage::Approved { profile, .. } => Some(profile),
            _ => None,
        }
    }
}

pub struct Pairings {
    path: PathBuf,
    clients: BTreeMap<String, Client>,
    revoked: BTreeSet<String>,
    pending: HashMap<String, Pending>,
}

/// The SHA-256 of a token, as lowercase hex.
pub fn hash_token(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

fn random_hex(bytes: usize) -> String {
    let mut buffer = vec![0u8; bytes];
    OsRng.fill_bytes(&mut buffer);
    hex::encode(buffer)
}

fn new_code() -> String {
    (0..CODE_LENGTH)
        .map(|_| CODE_ALPHABET[OsRng.gen_range(0..CODE_ALPHABET.len())] as char)
        .collect()
}

/// A typed code, ready to compare: spaces trimmed, letters uppercased.
pub fn normalize_code(code: &str) -> String {
    code.trim().to_ascii_uppercase()
}

impl Pairings {
    /// Read the pairing file, or start empty when it does not exist.
    pub fn load(path: &Path) -> Result<Pairings, String> {
        let file: File = match std::fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text)
                .map_err(|error| format!("{}: {error}", path.display()))?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => File::default(),
            Err(error) => return Err(format!("cannot read {}: {error}", path.display())),
        };
        // Fail closed: a name or a token hash that appears twice is a damaged file.
        let mut clients = BTreeMap::new();
        let mut hashes = BTreeSet::new();
        for client in file.clients {
            if !hashes.insert(client.token_sha256.clone()) {
                return Err(format!("{}: a token hash appears twice", path.display()));
            }
            if clients.contains_key(&client.name) {
                return Err(format!(
                    "{}: the client {} appears twice",
                    path.display(),
                    client.name
                ));
            }
            // Fail closed: a limit that cannot be read must not fall back to the global default.
            if let Some(text) = &client.limit {
                if text.parse::<Limit>().is_err() {
                    return Err(format!(
                        "{}: the client {} has a limit that cannot be read",
                        path.display(),
                        client.name
                    ));
                }
            }
            clients.insert(client.name.clone(), client);
        }
        Ok(Pairings {
            path: path.to_path_buf(),
            clients,
            revoked: file.revoked.into_iter().collect(),
            pending: HashMap::new(),
        })
    }

    /// Save a new client list and revoked list, and only then put them in
    /// memory. A failed save leaves memory as it was, so memory and disk agree.
    fn commit(
        &mut self,
        clients: BTreeMap<String, Client>,
        revoked: BTreeSet<String>,
    ) -> Result<(), ErrorCode> {
        let file = File {
            clients: clients.values().cloned().collect(),
            revoked: revoked.iter().cloned().collect(),
        };
        let text = serde_json::to_string_pretty(&file).map_err(|_| ErrorCode::Internal)?;
        write_atomic(&self.path, &text).map_err(|_| ErrorCode::Internal)?;
        self.clients = clients;
        self.revoked = revoked;
        Ok(())
    }

    fn purge(&mut self, now: Instant) {
        self.pending
            .retain(|_, pending| now.duration_since(pending.created) < PENDING_EXPIRY);
    }

    fn waiting_id(&self, code: &str) -> Option<String> {
        let code = normalize_code(code);
        self.pending
            .iter()
            .find(|(_, pending)| pending.code == code && matches!(pending.stage, Stage::Waiting))
            .map(|(id, _)| id.clone())
    }

    /// Drops every approval that is not committed and that `which` picks.
    /// Returns how many it dropped.
    fn cancel(&mut self, which: impl Fn(&Pending) -> bool) -> usize {
        let before = self.pending.len();
        self.pending
            .retain(|_, pending| pending.approved_profile().is_none() || !which(pending));
        before - self.pending.len()
    }

    /// Start a pairing. Returns the request ID and the code. `cli` is true
    /// when the request came from the `jev` binary. A request from `jev` for
    /// a name that already has a waiting request from `jev` gets that
    /// request's ID and code back, and its expiry does not restart.
    pub fn start(
        &mut self,
        name: &str,
        profile: Option<&str>,
        cli: bool,
        now: Instant,
    ) -> Result<(String, String), ErrorCode> {
        if !valid_name(name) || profile.is_some_and(|profile| !valid_name(profile)) {
            return Err(ErrorCode::BadRequest);
        }
        self.purge(now);
        if cli {
            let repeat = self.pending.iter().find(|(_, pending)| {
                pending.cli && pending.name == name && matches!(pending.stage, Stage::Waiting)
            });
            if let Some((id, pending)) = repeat {
                return Ok((id.clone(), pending.code.clone()));
            }
        }
        let held = self
            .pending
            .values()
            .filter(|pending| pending.holds_slot())
            .count();
        if held >= MAX_PENDING {
            return Err(ErrorCode::RateLimited);
        }
        let mut code = new_code();
        while self.pending.values().any(|pending| pending.code == code) {
            code = new_code();
        }
        let request_id = random_hex(16);
        self.pending.insert(
            request_id.clone(),
            Pending {
                code: code.clone(),
                name: name.to_string(),
                profile: profile.map(str::to_string),
                cli,
                created: now,
                stage: Stage::Waiting,
            },
        );
        Ok((request_id, code))
    }

    /// The state of a request. `Approved` commits the client: it saves the
    /// client, and revokes an old client of the same name. `Approved` and
    /// `Denied` end the request. A failed save is `internal` and keeps the
    /// request, so the client can poll again. An approval whose profile is
    /// not in `profiles` any more is dropped, and reports `expired`.
    pub fn status(
        &mut self,
        request_id: &str,
        profiles: &[String],
        now: Instant,
        now_unix: u64,
    ) -> Result<PairState, ErrorCode> {
        self.purge(now);
        let Some(pending) = self.pending.get(request_id) else {
            return Ok(PairState::Expired);
        };
        let (profile, token_sha256) = match &pending.stage {
            Stage::Waiting => return Ok(PairState::Pending),
            Stage::Denied => {
                self.pending.remove(request_id);
                return Ok(PairState::Denied);
            }
            Stage::Approved { profile, .. } if !profiles.contains(profile) => {
                self.pending.remove(request_id);
                return Ok(PairState::Expired);
            }
            Stage::Approved {
                profile,
                token_sha256,
                ..
            } => (profile.clone(), token_sha256.clone()),
        };
        let name = pending.name.clone();
        let mut clients = self.clients.clone();
        let mut revoked = self.revoked.clone();
        if let Some(old) = clients.remove(&name) {
            revoked.insert(old.token_sha256);
        }
        clients.insert(
            name.clone(),
            Client {
                name,
                profile,
                token_sha256,
                created: now_unix,
                last_used: None,
                limit: None,
            },
        );
        self.commit(clients, revoked)?;
        match self.pending.remove(request_id).map(|pending| pending.stage) {
            Some(Stage::Approved { token, .. }) => Ok(PairState::Approved(token)),
            _ => unreachable!("approved above"),
        }
    }

    /// The requests that wait for a person, sorted by code.
    pub fn pending_list(&mut self, now: Instant) -> Vec<PendingView> {
        self.purge(now);
        let mut list: Vec<PendingView> = self
            .pending
            .values()
            .filter(|pending| matches!(pending.stage, Stage::Waiting))
            .map(|pending| PendingView {
                code: pending.code.clone(),
                name: pending.name.clone(),
                profile: pending.profile.clone(),
            })
            .collect();
        list.sort_by(|a, b| a.code.cmp(&b.code));
        list
    }

    /// Approve the request with `code`, bound to `profile`. Saves nothing:
    /// `status` commits the client. An approval of the same name that is not
    /// committed gets cancelled. The collection window starts now.
    pub fn approve(
        &mut self,
        code: &str,
        profile: &str,
        now: Instant,
    ) -> Result<Approval, ErrorCode> {
        if !valid_name(profile) {
            return Err(ErrorCode::BadRequest);
        }
        self.purge(now);
        let id = self.waiting_id(code).ok_or(ErrorCode::BadRequest)?;
        let name = self.pending[&id].name.clone();
        self.cancel(|pending| pending.name == name);
        let token = format!("{TOKEN_PREFIX}{}", random_hex(32));
        let pending = self.pending.get_mut(&id).expect("found above");
        let cli = pending.cli;
        pending.stage = Stage::Approved {
            profile: profile.to_string(),
            token_sha256: hash_token(&token),
            token: (!cli).then(|| token.clone()),
        };
        pending.created = now;
        Ok(Approval {
            replaces: self.clients.contains_key(&name),
            name,
            cli: cli.then_some(CliToken {
                request_id: id,
                token,
            }),
        })
    }

    pub fn deny(&mut self, code: &str, now: Instant) -> Result<(), ErrorCode> {
        self.purge(now);
        let id = self.waiting_id(code).ok_or(ErrorCode::BadRequest)?;
        self.pending.get_mut(&id).expect("found above").stage = Stage::Denied;
        Ok(())
    }

    /// The client that owns `token`. Moves its last-used time at most once a minute.
    pub fn authenticate(&mut self, token: &str, now_unix: u64) -> Result<Client, ErrorCode> {
        let hash = hash_token(token);
        if self.revoked.contains(&hash) {
            return Err(ErrorCode::Revoked);
        }
        let client = self
            .clients
            .values()
            .find(|client| client.token_sha256 == hash)
            .ok_or(ErrorCode::NotPaired)?;
        let fresh =
            matches!(client.last_used, Some(t) if now_unix.saturating_sub(t) < LAST_USED_STEP);
        if fresh {
            return Ok(client.clone());
        }
        let name = client.name.clone();
        let mut clients = self.clients.clone();
        clients.get_mut(&name).expect("found above").last_used = Some(now_unix);
        // The last-used time is a convenience. A failed write must not fail the
        // request. It leaves the old time in memory, as on disk.
        let _ = self.commit(clients, self.revoked.clone());
        Ok(self.clients[&name].clone())
    }

    /// Revoke the client `name`, and cancel its approvals that are not
    /// committed. `bad-request` when there is neither.
    pub fn revoke(&mut self, name: &str) -> Result<(), ErrorCode> {
        let cancelled = self.cancel(|pending| pending.name == name);
        let mut clients = self.clients.clone();
        let mut revoked = self.revoked.clone();
        let Some(client) = clients.remove(name) else {
            return if cancelled > 0 {
                Ok(())
            } else {
                Err(ErrorCode::BadRequest)
            };
        };
        revoked.insert(client.token_sha256);
        self.commit(clients, revoked)
    }

    /// Revoke every client bound to `profile`, and cancel the approvals on it
    /// that are not committed. Returns the names of the revoked clients, sorted.
    pub fn revoke_profile(&mut self, profile: &str) -> Result<Vec<String>, ErrorCode> {
        self.cancel(|pending| pending.approved_profile() == Some(profile));
        let names: Vec<String> = self
            .clients
            .values()
            .filter(|client| client.profile == profile)
            .map(|client| client.name.clone())
            .collect();
        let mut clients = self.clients.clone();
        let mut revoked = self.revoked.clone();
        for name in &names {
            let client = clients.remove(name).expect("listed above");
            revoked.insert(client.token_sha256);
        }
        self.commit(clients, revoked)?;
        Ok(names)
    }

    /// Set a client's rate limit, or clear it with None.
    pub fn set_limit(&mut self, name: &str, limit: Option<Limit>) -> Result<(), ErrorCode> {
        let mut clients = self.clients.clone();
        let client = clients.get_mut(name).ok_or(ErrorCode::BadRequest)?;
        client.limit = limit.map(|limit| limit.to_string());
        self.commit(clients, self.revoked.clone())
    }

    /// Every paired client, sorted by name.
    pub fn clients(&self) -> Vec<Client> {
        self.clients.values().cloned().collect()
    }
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p jevd --lib pairing`
Expected: PASS, 43 tests.

Run: `cargo test --workspace --locked`
Expected: PASS. No other module calls `Pairings` yet.

- [ ] **Step 5: Mutation self-check**

Break each rule once, run `cargo test -p jevd --lib pairing`, and check that a test fails. Restore the file after each one:

1. In `start`, drop the `pending.cli &&` check from the repeat search.
2. In `start`, count only `Stage::Waiting` requests as slots.
3. In `approve`, delete the `self.cancel(...)` line.
4. In `status`, skip the `!profiles.contains(profile)` arm.
5. In `status`, remove the request before `self.commit(...)?`.
6. In `revoke`, return `Err(ErrorCode::BadRequest)` when no client exists, even after a cancel.
7. In `revoke_profile`, delete the `self.cancel(...)` line.

- [ ] **Step 6: Commit**

```bash
git add crates/jevd/src/pairing.rs
git commit -m "feat(jevd): commit a client only when its token reaches its storage" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: Where the files and the channel are (`jev_proto::paths`)

Spec section 1, "Storage", and section 2, "Transport". `jevd`, `jev`, and any other client find the channel through `Paths`, so they always agree.

| Item | Windows | macOS | Linux |
| --- | --- | --- | --- |
| Settings and pairing files | `%APPDATA%\jev` | `~/Library/Application Support/jev` | `$XDG_CONFIG_HOME/jev`, or `~/.config/jev` |
| Log | `%LOCALAPPDATA%\jev` | `~/Library/Logs/jev` | `$XDG_STATE_HOME/jev`, or `~/.local/state/jev` |
| PID file | `%LOCALAPPDATA%\jev\run` | `$TMPDIR/jev` | `$XDG_RUNTIME_DIR/jev`, or `/tmp/jev-<uid>` |
| Channel | `\\.\pipe\jev-<SID>` | `$TMPDIR/jev/jevd.sock` | `$XDG_RUNTIME_DIR/jev/jevd.sock` |

File names: `config.toml`, `pairings.json`, `jevd.log`, `jevd.pid`, `jevd.sock`. With `JEV_HOME` set, the folders are `config`, `state`, and `run` under it.

On Unix, `check_runtime_dir` refuses a runtime folder that another user owns, or that has any group or other access. That matters most for the `/tmp` fallback.

**Files:**
- Modify: `crates/jev-proto/Cargo.toml`
- Create: `crates/jev-proto/src/os.rs`
- Create: `crates/jev-proto/src/paths.rs`
- Modify: `crates/jev-proto/src/lib.rs`

- [ ] **Step 1: Add the dependencies**

Append to `crates/jev-proto/Cargo.toml`:

```toml

[target.'cfg(unix)'.dependencies]
libc = "0.2"

[target.'cfg(windows)'.dependencies]
windows-sys = { version = "0.59", features = ["Win32_Foundation", "Win32_Security", "Win32_Security_Authorization", "Win32_System_Threading"] }

[dev-dependencies]
tempfile = "3"
```

`windows-sys` 0.59 is already in `Cargo.lock` through other crates, and it needs Rust 1.60.

- [ ] **Step 2: Write the tests**

Create `crates/jev-proto/src/os.rs` with only its test module for now:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sid_has_the_text_form() {
        let sid = user_sid().unwrap();
        assert!(sid.starts_with("S-1-"), "{sid}");
        assert!(sid
            .bytes()
            .all(|b| b.is_ascii_digit() || b == b'-' || b == b'S'));
    }

    #[test]
    fn this_user_owns_its_own_test_binary() {
        let file = std::fs::File::open(std::env::current_exe().unwrap()).unwrap();
        assert!(owned_by_current_user(file.as_raw_handle()).unwrap());
    }
}
```

Create `crates/jev-proto/src/paths.rs` with only its test module for now:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn vars(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let map: HashMap<String, OsString> = pairs
            .iter()
            .map(|(name, value)| (name.to_string(), OsString::from(value)))
            .collect();
        move |name| map.get(name).cloned()
    }

    #[test]
    fn jev_home_moves_everything_under_it() {
        let home = std::env::temp_dir().join("jev-home-test");
        let paths =
            Paths::from_vars(vars(&[(HOME_VAR, home.to_str().unwrap()), ("HOME", "/x")])).unwrap();
        assert_eq!(
            paths.settings_file(),
            home.join("config").join("config.toml")
        );
        assert_eq!(
            paths.pairing_file(),
            home.join("config").join("pairings.json")
        );
        assert_eq!(paths.log_file(), home.join("state").join("jevd.log"));
        assert_eq!(paths.pid_file(), home.join("run").join("jevd.pid"));
        #[cfg(unix)]
        assert_eq!(paths.socket, home.join("run").join("jevd.sock"));
        #[cfg(windows)]
        {
            let name = paths.socket.to_str().unwrap();
            let prefix = format!(r"\\.\pipe\jev-{}-", crate::os::user_sid().unwrap());
            assert!(name.starts_with(&prefix), "{name}");
            assert_eq!(name.len(), prefix.len() + 16);
        }
    }

    #[test]
    fn an_empty_jev_home_counts_as_unset() {
        let with_empty = Paths::from_vars(vars(&[
            (HOME_VAR, ""),
            ("HOME", "/home/u"),
            ("APPDATA", r"C:\r"),
            ("LOCALAPPDATA", r"C:\l"),
        ]))
        .unwrap();
        let without = Paths::from_vars(vars(&[
            ("HOME", "/home/u"),
            ("APPDATA", r"C:\r"),
            ("LOCALAPPDATA", r"C:\l"),
        ]))
        .unwrap();
        assert_eq!(with_empty, without);
    }

    #[test]
    fn two_homes_get_two_channels() {
        let a = Paths::under(Path::new("/a")).unwrap();
        let b = Paths::under(Path::new("/b")).unwrap();
        assert_ne!(a.socket, b.socket);
    }

    #[cfg(windows)]
    #[test]
    fn windows_uses_appdata_and_the_user_pipe() {
        let paths =
            Paths::from_vars(vars(&[("APPDATA", r"C:\r"), ("LOCALAPPDATA", r"C:\l")])).unwrap();
        assert_eq!(paths.config_dir, PathBuf::from(r"C:\r\jev"));
        assert_eq!(paths.state_dir, PathBuf::from(r"C:\l\jev"));
        assert_eq!(paths.runtime_dir, PathBuf::from(r"C:\l\jev\run"));
        assert_eq!(
            paths.socket,
            PathBuf::from(format!(r"\\.\pipe\jev-{}", crate::os::user_sid().unwrap()))
        );
        assert!(Paths::from_vars(vars(&[("APPDATA", r"C:\r")])).is_err());
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn linux_uses_the_xdg_folders() {
        let paths = Paths::from_vars(vars(&[
            ("HOME", "/home/u"),
            ("XDG_RUNTIME_DIR", "/run/user/1000"),
        ]))
        .unwrap();
        assert_eq!(paths.config_dir, PathBuf::from("/home/u/.config/jev"));
        assert_eq!(paths.state_dir, PathBuf::from("/home/u/.local/state/jev"));
        assert_eq!(paths.socket, PathBuf::from("/run/user/1000/jev/jevd.sock"));
        let bare = Paths::from_vars(vars(&[("HOME", "/home/u")])).unwrap();
        assert!(bare.runtime_dir.starts_with("/tmp"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_uses_library_and_tmpdir() {
        let paths = Paths::from_vars(vars(&[
            ("HOME", "/Users/u"),
            ("TMPDIR", "/var/folders/x/T/"),
        ]))
        .unwrap();
        assert_eq!(
            paths.config_dir,
            PathBuf::from("/Users/u/Library/Application Support/jev")
        );
        assert_eq!(
            paths.socket,
            PathBuf::from("/var/folders/x/T/jev/jevd.sock")
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_runtime_folder_must_be_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::under(dir.path()).unwrap();
        paths.prepare_runtime_dir().unwrap();
        let mode = std::fs::metadata(&paths.runtime_dir)
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o700);
        std::fs::set_permissions(&paths.runtime_dir, std::fs::Permissions::from_mode(0o755))
            .unwrap();
        assert!(paths.check_runtime_dir().is_err());
        assert!(paths.prepare_runtime_dir().is_err());
    }

    #[test]
    fn prepare_creates_the_runtime_folder() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::under(dir.path()).unwrap();
        assert!(paths.check_runtime_dir().is_err() || cfg!(windows));
        paths.prepare_runtime_dir().unwrap();
        assert!(paths.runtime_dir.is_dir());
        paths.check_runtime_dir().unwrap();
    }
}
```

Change the module list in `crates/jev-proto/src/lib.rs` to:

```rust
//! Wire types shared by jevd, the jev CLI, and third-party clients.

mod error;
mod message;
mod names;
#[cfg(windows)]
pub mod os;
pub mod paths;

pub use error::{ErrorCode, WireCode, WireError};
pub use message::{Reply, Request, PROTOCOL_VERSION};
pub use names::valid_name;
pub use paths::Paths;
```

- [ ] **Step 3: Run the tests and read the failure**

Run: `cargo test -p jev-proto`
Expected: FAIL to compile: `Paths`, `HOME_VAR`, `user_sid`, and `owned_by_current_user` are not found.

- [ ] **Step 4: Write the code**

Put this above the test module in `crates/jev-proto/src/os.rs`:

```rust
//! Windows calls: the current user's SID, and the owner of a kernel object
//! such as a named pipe.

use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};

use windows_sys::Win32::Foundation::{LocalFree, ERROR_SUCCESS, HANDLE};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, GetSecurityInfo, SE_KERNEL_OBJECT,
};
use windows_sys::Win32::Security::{
    EqualSid, GetTokenInformation, TokenUser, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
    PSID, TOKEN_QUERY, TOKEN_USER,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

/// The `TOKEN_USER` of this process. `u64` keeps the buffer aligned.
struct CurrentUser(Vec<u64>);

impl CurrentUser {
    fn get() -> io::Result<CurrentUser> {
        // SAFETY: each call gets valid pointers, and the token handle is
        // closed when `token` drops.
        unsafe {
            let mut raw: HANDLE = std::ptr::null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw) == 0 {
                return Err(io::Error::last_os_error());
            }
            let token = OwnedHandle::from_raw_handle(raw as RawHandle);
            let mut len = 0u32;
            GetTokenInformation(
                token.as_raw_handle() as HANDLE,
                TokenUser,
                std::ptr::null_mut(),
                0,
                &mut len,
            );
            let mut buffer = vec![0u64; (len as usize).div_ceil(8)];
            if GetTokenInformation(
                token.as_raw_handle() as HANDLE,
                TokenUser,
                buffer.as_mut_ptr().cast(),
                len,
                &mut len,
            ) == 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(CurrentUser(buffer))
        }
    }

    fn sid(&self) -> PSID {
        // SAFETY: the buffer holds a TOKEN_USER that GetTokenInformation wrote.
        unsafe { (*(self.0.as_ptr() as *const TOKEN_USER)).User.Sid }
    }
}

/// The current user's SID as text, such as `S-1-5-21-...-1001`.
pub fn user_sid() -> io::Result<String> {
    let user = CurrentUser::get()?;
    // SAFETY: `text` gets a LocalAlloc string that is freed below.
    unsafe {
        let mut text: *mut u16 = std::ptr::null_mut();
        if ConvertSidToStringSidW(user.sid(), &mut text) == 0 {
            return Err(io::Error::last_os_error());
        }
        let len = (0..).take_while(|&i| *text.add(i) != 0).count();
        let sid = String::from_utf16_lossy(std::slice::from_raw_parts(text, len));
        LocalFree(text.cast());
        Ok(sid)
    }
}

/// True when the current user owns the kernel object behind `handle`.
pub fn owned_by_current_user(handle: RawHandle) -> io::Result<bool> {
    let user = CurrentUser::get()?;
    // SAFETY: `owner` points into `descriptor`, which is freed after the compare.
    unsafe {
        let mut owner: PSID = std::ptr::null_mut();
        let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
        let status = GetSecurityInfo(
            handle as HANDLE,
            SE_KERNEL_OBJECT,
            OWNER_SECURITY_INFORMATION,
            &mut owner,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut descriptor,
        );
        if status != ERROR_SUCCESS {
            return Err(io::Error::from_raw_os_error(status as i32));
        }
        let same = EqualSid(owner, user.sid()) != 0;
        LocalFree(descriptor.cast());
        Ok(same)
    }
}
```

Put this above the test module in `crates/jev-proto/src/paths.rs`:

```rust
//! Where jev keeps its files and its channel (spec section 1, "Storage", and
//! section 2, "Transport"). `jevd`, `jev`, and other clients all find the
//! channel here.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

/// When set, every jev file and the channel go under this folder. Tests use
/// it, so they never meet a real daemon.
pub const HOME_VAR: &str = "JEV_HOME";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    /// The settings file and the pairing file.
    pub config_dir: PathBuf,
    /// The log file.
    pub state_dir: PathBuf,
    /// The PID file, and on Unix the socket.
    pub runtime_dir: PathBuf,
    /// The Unix socket file, or the Windows pipe name.
    pub socket: PathBuf,
}

impl Paths {
    /// The paths for this user, from the environment.
    pub fn from_env() -> io::Result<Paths> {
        Paths::from_vars(|name| std::env::var_os(name))
    }

    /// The paths, with `var` in place of the environment.
    pub fn from_vars(var: impl Fn(&str) -> Option<OsString>) -> io::Result<Paths> {
        let set = |name: &str| var(name).filter(|value| !value.is_empty());
        if let Some(home) = set(HOME_VAR) {
            return Paths::under(Path::new(&home));
        }
        platform(&set)
    }

    /// Every path under `home`.
    pub fn under(home: &Path) -> io::Result<Paths> {
        let runtime_dir = home.join("run");
        #[cfg(unix)]
        let socket = runtime_dir.join("jevd.sock");
        #[cfg(windows)]
        let socket = PathBuf::from(format!(
            r"\\.\pipe\jev-{}-{:016x}",
            crate::os::user_sid()?,
            fnv1a(home.as_os_str().as_encoded_bytes())
        ));
        Ok(Paths {
            config_dir: home.join("config"),
            state_dir: home.join("state"),
            runtime_dir,
            socket,
        })
    }

    pub fn settings_file(&self) -> PathBuf {
        self.config_dir.join("config.toml")
    }

    pub fn pairing_file(&self) -> PathBuf {
        self.config_dir.join("pairings.json")
    }

    pub fn log_file(&self) -> PathBuf {
        self.state_dir.join("jevd.log")
    }

    pub fn pid_file(&self) -> PathBuf {
        self.runtime_dir.join("jevd.pid")
    }

    /// Creates the runtime folder when it is missing, then checks it as
    /// `check_runtime_dir` does. `jevd` calls this before it binds.
    pub fn prepare_runtime_dir(&self) -> io::Result<()> {
        let mut builder = std::fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
        builder.create(&self.runtime_dir)?;
        self.check_runtime_dir()
    }

    /// On Unix: the runtime folder must belong to this user, with no access
    /// for the group or others, so no other user can put a socket there.
    /// On Windows the pipe has its own access list, so this checks nothing.
    pub fn check_runtime_dir(&self) -> io::Result<()> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let meta = std::fs::symlink_metadata(&self.runtime_dir)?;
            // SAFETY: geteuid has no preconditions.
            let me = unsafe { libc::geteuid() };
            if !meta.is_dir() || meta.uid() != me || meta.mode() & 0o077 != 0 {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!(
                        "{} must be a folder of this user with mode 0700",
                        self.runtime_dir.display()
                    ),
                ));
            }
        }
        Ok(())
    }
}

#[cfg(windows)]
fn platform(set: &dyn Fn(&str) -> Option<OsString>) -> io::Result<Paths> {
    let missing =
        |name: &str| io::Error::new(io::ErrorKind::NotFound, format!("{name} is not set"));
    let roaming = PathBuf::from(set("APPDATA").ok_or_else(|| missing("APPDATA"))?);
    let local = PathBuf::from(set("LOCALAPPDATA").ok_or_else(|| missing("LOCALAPPDATA"))?);
    Ok(Paths {
        config_dir: roaming.join("jev"),
        state_dir: local.join("jev"),
        runtime_dir: local.join("jev").join("run"),
        socket: PathBuf::from(format!(r"\\.\pipe\jev-{}", crate::os::user_sid()?)),
    })
}

#[cfg(unix)]
fn platform(set: &dyn Fn(&str) -> Option<OsString>) -> io::Result<Paths> {
    let home = set("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME is not set"))?;
    // SAFETY: geteuid has no preconditions.
    let uid = unsafe { libc::geteuid() };
    let fallback = || PathBuf::from(format!("/tmp/jev-{uid}"));
    #[cfg(target_os = "macos")]
    let (config_dir, state_dir, runtime_dir) = (
        home.join("Library/Application Support/jev"),
        home.join("Library/Logs/jev"),
        set("TMPDIR")
            .map(|dir| PathBuf::from(dir).join("jev"))
            .unwrap_or_else(fallback),
    );
    #[cfg(not(target_os = "macos"))]
    let (config_dir, state_dir, runtime_dir) = (
        set("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config"))
            .join("jev"),
        set("XDG_STATE_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local/state"))
            .join("jev"),
        set("XDG_RUNTIME_DIR")
            .map(|dir| PathBuf::from(dir).join("jev"))
            .unwrap_or_else(fallback),
    );
    let socket = runtime_dir.join("jevd.sock");
    Ok(Paths {
        config_dir,
        state_dir,
        runtime_dir,
        socket,
    })
}

/// FNV-1a, 64 bits: a short, stable name for a `JEV_HOME` folder. Not a
/// security measure; the pipe's access list is.
#[cfg(windows)]
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}
```

- [ ] **Step 5: Run the tests**

Run: `cargo test -p jev-proto`
Expected: PASS. On Windows: 23 tests. The Unix and macOS tests compile only there.

- [ ] **Step 6: Mutation self-check**

1. In `from_vars`, drop `.filter(|value| !value.is_empty())`.
2. In `under`, leave the hash out of the Windows pipe name.
3. On Unix: in `check_runtime_dir`, drop the `meta.mode() & 0o077 != 0` test.

- [ ] **Step 7: Commit**

```bash
git add Cargo.lock crates/jev-proto
git commit -m "feat(jev-proto): find the jev folders and the channel, with JEV_HOME for tests" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: The daemon's state and messages (`jevd::daemon`, `jevd::ask`)

Spec section 2, "Messages", and section 4. `Daemon::handle` turns one `Request` into one `Reply`. The transport (Task 4) parses lines and calls it, so these tests need no socket.

Rules in `handle`:

- An admin message from a peer that is not `jev` gets `forbidden`, and does nothing.
- `hello` belongs to the transport. Here it gets `bad-request`.
- `approve` refuses a profile that the settings file does not list.
- The `approve` reply carries `request_id` and `token` for a CLI request only. The `pair-status` reply carries `token` for a socket client only. Neither reply is ever logged.
- `revoke` also drops the client's rate-limit counts.
- `reload` reads the settings file again, then wipes every key. A bad file leaves the old settings.
- `stop` replies, then starts the stop. Task 7 does the rest.

The `ask` path, in the spec's order:

1. Shape (`check_ask`): `bad-request`.
2. Token (`authenticate`): `not-paired` or `revoked`.
3. Key: from the cache, or one keyring read per profile on a blocking thread. `no-key`, `bad-key`, or `internal`. A failure here uses no rate-limit slot.
4. Rate limit: the client's override, or the setting. `rate-limited` with `retry_after`.
5. to 7. `JevClient::ask`: redaction, the send, and the answer check.

The key read follows the protocol in plan 2's `keycache::Lookup` doc. On `Miss`, a detached task runs the read and calls `finish`, so a dropped connection still fills the cache. It publishes `(read_id, outcome)` on a `watch`. On `Wait(read)`, the request waits on the watch for up to `READ_LIMIT`. If the watch shows that `read` failed, the request replies with that error. Otherwise it looks up again. A request subscribes before its lookup, so no outcome slips between them.

`Busy` guards mark requests in flight, for idle exit (Task 7). `last_active` is a `tokio::time::Instant`, so tests with paused time can move it.

**Files:**
- Modify: `crates/jevd/Cargo.toml`
- Create: `crates/jevd/src/daemon.rs`
- Create: `crates/jevd/src/ask.rs`
- Modify: `crates/jevd/src/lib.rs`

- [ ] **Step 1: Add the tokio features**

In `crates/jevd/Cargo.toml`, change the tokio line under `[dependencies]` to:

```toml
tokio = { version = "1", features = ["io-util", "macros", "net", "rt-multi-thread", "signal", "sync", "time"] }
```

Change the tokio line under `[dev-dependencies]` to:

```toml
tokio = { version = "1", features = ["test-util"] }
```

The main line now has `macros` and the runtime. `test-util` adds paused time for Tasks 4 and 7.

- [ ] **Step 2: Write the daemon tests**

Create `crates/jevd/src/daemon.rs` with only this test module for now. Other test modules use its `fixture_with`, `call`, `code_of`, and `pair_socket`, so it is `pub(crate)`:

```rust
#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::keystore::MemoryKeyring;
    use crate::pairing::TOKEN_PREFIX;
    use std::time::Duration;

    pub(crate) struct Fixture {
        pub(crate) _dir: tempfile::TempDir,
        pub(crate) daemon: Arc<Daemon>,
        pub(crate) settings_file: PathBuf,
        pub(crate) pairing_file: PathBuf,
    }

    /// A daemon with the profiles `default` and `work`, a keyring of
    /// `store`, and a Jev client that sends to `endpoint`.
    pub(crate) fn fixture_with(store: Arc<dyn KeyStore>, endpoint: &str) -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let settings_file = dir.path().join("config.toml");
        let pairing_file = dir.path().join("pairings.json");
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
            store,
            jev,
        })
        .unwrap();
        Fixture {
            _dir: dir,
            daemon,
            settings_file,
            pairing_file,
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
```

Add to `crates/jevd/src/lib.rs`, in name order:

```rust
mod ask;
pub mod daemon;
```

Create `crates/jevd/src/ask.rs` with only its test module for now:

```rust
#[cfg(test)]
mod tests {
    use crate::daemon::tests::{call, code_of, fixture_with, pair_socket};
    use crate::daemon::Peer;
    use crate::keystore::{profile_account, KeyStore, MemoryKeyring, StoreError};
    use jev_proto::{ErrorCode, Request};
    use serde_json::{json, Value};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;
    use wiremock::matchers::{header, method};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    use zeroize::Zeroizing;

    const KEY: &str = "tsk-test-key-0123456789";

    fn questions() -> Value {
        json!({"q": {"type": "noul", "question": "Is it done?"}})
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
        store.set(&profile_account("default"), KEY).unwrap();
        let reply = call(&f.daemon, Peer::Other, ask(&token)).await;
        assert!(reply.ok, "the failed ask used no slot: {reply:?}");
        let reply = call(&f.daemon, Peer::Other, ask(&token)).await;
        assert_eq!(code_of(&reply), Some(ErrorCode::RateLimited));
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
        assert!(!reply.to_line().contains(KEY));
    }

    /// A keyring whose reads take a while and get counted.
    struct SlowKeyring {
        reads: AtomicUsize,
        fail: bool,
    }

    impl KeyStore for SlowKeyring {
        fn get(&self, _account: &str) -> Result<Option<Zeroizing<String>>, StoreError> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(300));
            if self.fail {
                return Err(StoreError::Locked);
            }
            Ok(Some(Zeroizing::new(KEY.to_string())))
        }
        fn set(&self, _account: &str, _secret: &str) -> Result<(), StoreError> {
            Ok(())
        }
        fn delete(&self, _account: &str) -> Result<bool, StoreError> {
            Ok(false)
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn two_asks_at_once_share_one_keyring_read() {
        let server = jev().await;
        let store = Arc::new(SlowKeyring {
            reads: AtomicUsize::new(0),
            fail: false,
        });
        let f = fixture_with(store.clone(), &server.uri());
        let token = pair_socket(&f.daemon, "app", "default").await;
        let (a, b) = tokio::join!(
            call(&f.daemon, Peer::Other, ask(&token)),
            call(&f.daemon, Peer::Other, ask(&token))
        );
        assert!(a.ok && b.ok, "{a:?} {b:?}");
        assert_eq!(store.reads.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_failed_read_fails_every_waiter_once() {
        let store = Arc::new(SlowKeyring {
            reads: AtomicUsize::new(0),
            fail: true,
        });
        let f = fixture_with(store.clone(), "http://127.0.0.1:9/");
        let token = pair_socket(&f.daemon, "app", "default").await;
        let (a, b) = tokio::join!(
            call(&f.daemon, Peer::Other, ask(&token)),
            call(&f.daemon, Peer::Other, ask(&token))
        );
        assert_eq!(code_of(&a), Some(ErrorCode::Internal));
        assert_eq!(code_of(&b), Some(ErrorCode::Internal));
        assert_eq!(store.reads.load(Ordering::SeqCst), 1);
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
}
```

- [ ] **Step 3: Run the tests and read the failure**

Run: `cargo test -p jevd --lib daemon::`
Expected: FAIL to compile: `Daemon`, `Config`, `Peer`, and `Busy` are not found.

- [ ] **Step 4: Write the daemon**

Put this above the test module in `crates/jevd/src/daemon.rs`:

```rust
//! The daemon's state, and its reply to each message (spec section 2,
//! "Messages"). The transport parses each line and calls `handle`.
//!
//! The state sits behind one `std::sync::Mutex`. No lock is held across an
//! `await`: the keyring read and the Jev call run with the lock released.

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

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

const SAVE_FAILED: &str = "jevd cannot save the pairing file";

impl Daemon {
    /// Reads the settings file and the pairing file. Reads no key.
    pub fn new(config: Config) -> Result<Arc<Daemon>, String> {
        let settings = Settings::load(&config.settings_file)?;
        let pairings = Pairings::load(&config.pairing_file)?;
        let keys = KeyCache::new(settings.key_cache);
        let now = Instant::now();
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

    /// When the last request ended, or `None` while a request runs.
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

    pub(crate) fn unix_now(&self) -> u64 {
        unix_now()
    }
}
```

Put this above the test module in `crates/jevd/src/ask.rs`:

```rust
//! One `ask` (spec section 4): shape, token, key, rate limit, then the Jev
//! call, which redacts, sends, and checks the answer.

use std::sync::Arc;
use std::time::Instant;

use jev_proto::{ErrorCode, Reply, WireError};
use serde_json::{json, Value};
use zeroize::Zeroizing;

use crate::daemon::{failure, Daemon};
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
        let client = match self.lock().pairings.authenticate(token, self.unix_now()) {
            Ok(client) => client,
            Err(ErrorCode::Revoked) => {
                return failure(ErrorCode::Revoked, "this token was revoked; pair again")
            }
            Err(code) => return failure(code, "this token is not paired; pair first"),
        };
        // The key check comes before the rate limit, so a bad key uses no slot.
        let key = match self.key(&client.profile).await {
            Ok(key) => key,
            Err(error) => return Reply::failure(error),
        };
        let strict = {
            let mut state = self.lock();
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
            Err(failure) => Reply::failure(failure.to_wire()),
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

fn read_failed() -> WireError {
    WireError::new(ErrorCode::Internal, "the keyring read failed")
}
```

- [ ] **Step 5: Run the tests**

Run: `cargo test -p jevd --lib daemon::`
Expected: PASS, 12 tests.

Run: `cargo test -p jevd --lib ask::`
Expected: PASS, 9 tests. `two_asks_at_once_share_one_keyring_read` and `a_failed_read_fails_every_waiter_once` take about 0.3 s each.

- [ ] **Step 6: Mutation self-check**

1. In `handle`, change `peer != Peer::Jev` to `false`.
2. In `approve` (daemon), drop the profile check.
3. In `pair`, pass `false` for `cli` always.
4. In `ask`, swap the key step and the rate-limit step.
5. In `key`, on `Wait`, return `read_failed()` at once.
6. In `spawn_read`, skip `send_replace`. Check that `a_failed_read_fails_every_waiter_once` fails within `READ_LIMIT`: it hangs. Stop it after 30 s; a hang counts as a caught mutant.
7. In `reload`, skip `state.keys.reload(...)`.

- [ ] **Step 7: Commit**

```bash
git add Cargo.lock crates/jevd/Cargo.toml crates/jevd/src/lib.rs crates/jevd/src/daemon.rs crates/jevd/src/ask.rs
git commit -m "feat(jevd): the daemon state, every protocol-1 message, and the ask path" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: One connection (`jevd::conn`)

Spec section 2, "Transport":

- One JSON object per line. A request line holds at most `MAX_LINE` (1 MiB) bytes, newline not counted. A longer line gets `too-large`, and the connection closes. The daemon keeps running.
- The first message must be `hello` with protocol 1. Another version gets `protocol-unsupported`. Any other first message, or a line that does not parse, gets `bad-request`. In each case the connection closes.
- After `hello`, a line that does not parse gets `bad-request`, and the connection stays open.
- One request in flight per connection: jevd reads the next line only after it sends the reply.
- `IDLE_LIMIT` (60 s) applies only to the wait for the next message.
- A stop closes the connection between requests. A request in flight finishes and gets its reply.

**Files:**
- Create: `crates/jevd/src/conn.rs`
- Modify: `crates/jevd/src/lib.rs`

- [ ] **Step 1: Write the tests**

Create `crates/jevd/src/conn.rs` with only its test module for now:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::daemon::tests::fixture_with;
    use crate::keystore::MemoryKeyring;
    use tokio::io::{DuplexStream, ReadHalf, WriteHalf};

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
        let (ours, theirs) = tokio::io::duplex(4 * MAX_LINE);
        tokio::spawn(serve(theirs, peer, Arc::clone(daemon)));
        let (read, write) = tokio::io::split(ours);
        Client {
            lines: BufReader::new(read).lines(),
            write,
        }
    }

    fn code(reply: &Reply) -> Option<ErrorCode> {
        reply.error.as_ref().and_then(|error| error.code.known())
    }

    fn daemon() -> crate::daemon::tests::Fixture {
        fixture_with(Arc::new(MemoryKeyring::new()), "http://127.0.0.1:9/")
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
        assert!(client.reply().await.is_none());
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
        assert!(client.reply().await.is_none());
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
        assert!(client.reply().await.is_none());
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
}
```

Add `pub mod conn;` to `crates/jevd/src/lib.rs`, in name order.

- [ ] **Step 2: Run the tests and read the failure**

Run: `cargo test -p jevd --lib conn::`
Expected: FAIL to compile: `serve`, `read_line`, `Line`, and `MAX_LINE` are not found.

- [ ] **Step 3: Write the code**

Put this above the test module in `crates/jevd/src/conn.rs`:

```rust
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

/// The longest request line, newline not counted.
pub const MAX_LINE: usize = 1 << 20;
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
            line = tokio::time::timeout(IDLE_LIMIT, read_line(&mut reader, MAX_LINE)) => line,
            _ = Daemon::stopped(&mut stopping) => return,
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
                if send(&mut write, &reply).await.is_err() || !greeted {
                    return;
                }
                continue;
            }
        };
        if !greeted {
            let (reply, keep) = greet(&request);
            if send(&mut write, &reply).await.is_err() || !keep {
                return;
            }
            greeted = true;
            continue;
        }
        let reply = daemon.handle(request, peer).await;
        if send(&mut write, &reply).await.is_err() {
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

async fn send<W: AsyncWrite + Unpin>(write: &mut W, reply: &Reply) -> std::io::Result<()> {
    write.write_all(reply.to_line().as_bytes()).await?;
    write.flush().await
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p jevd --lib conn::`
Expected: PASS, 10 tests. The idle test uses paused time, so it takes no real minute.

- [ ] **Step 5: Mutation self-check**

1. In `read_line`, use `take(max as u64)` in place of `max as u64 + 1`.
2. In `serve`, `continue` after a bad line before `hello`, in place of `return`.
3. In `greet`, accept any protocol.
4. In `serve`, drop the `Daemon::stopped` branch of the `select!`.

- [ ] **Step 6: Commit**

```bash
git add crates/jevd/src/lib.rs crates/jevd/src/conn.rs
git commit -m "feat(jevd): one connection: line cap, hello first, idle limit, stop between requests" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: The peer check (`jevd::peer`)

Spec section 2, "Transport": jevd takes the full path of the `jev` next to it at start, and accepts admin messages only from a peer process with that exact path. The OS gives the peer's process ID; `exe_of` turns it into a path:

| OS | Process ID | Path |
| --- | --- | --- |
| Linux | `SO_PEERCRED`, through tokio | `readlink /proc/<pid>/exe` |
| macOS | `LOCAL_PEEREPID`, through tokio | `proc_pidpath` |
| Windows | `GetNamedPipeClientProcessId` | `QueryFullProcessImageNameW` |

Both sides go through `canonicalize`, so a path with `..` or a link compares as the file it names. Any failure gives `Peer::Other`: a missing `jev`, a process that ended, or no process ID.

The OS can reuse a process ID after its process ends, so a race of a few milliseconds can happen. The threat model (spec section 10) already excludes malware that runs as the same user.

**Files:**
- Modify: `crates/jevd/Cargo.toml`
- Create: `crates/jevd/src/peer.rs`
- Modify: `crates/jevd/src/lib.rs`

- [ ] **Step 1: Add the OS dependencies**

In `crates/jevd/Cargo.toml`, add this line under `[target.'cfg(windows)'.dependencies]`:

```toml
windows-sys = { version = "0.59", features = ["Win32_Foundation", "Win32_Security", "Win32_Security_Authorization", "Win32_System_Pipes", "Win32_System_Threading"] }
```

Add this section before `[target.'cfg(target_os = "macos")'.dependencies]`:

```toml
[target.'cfg(unix)'.dependencies]
libc = "0.2"

```

Task 6 uses the `Pipes` and `Security` features, and `flock` from `libc`.

- [ ] **Step 2: Write the tests**

Create `crates/jevd/src/peer.rs` with only its test module for now:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn me() -> PathBuf {
        std::fs::canonicalize(std::env::current_exe().unwrap()).unwrap()
    }

    #[test]
    fn exe_of_finds_this_process() {
        let found = std::fs::canonicalize(exe_of(std::process::id()).unwrap()).unwrap();
        assert_eq!(found, me());
    }

    #[test]
    fn only_the_exact_binary_counts_as_jev() {
        let pid = Some(std::process::id());
        assert_eq!(AdminCheck::new(&me()).peer(pid), Peer::Jev);
        let elsewhere = tempfile::tempdir().unwrap();
        let copy = elsewhere.path().join(me().file_name().unwrap());
        std::fs::copy(me(), &copy).unwrap();
        assert_eq!(
            AdminCheck::new(&copy).peer(pid),
            Peer::Other,
            "a copy in another folder is not this process"
        );
        assert_eq!(AdminCheck::new(&me()).peer(None), Peer::Other);
    }

    #[test]
    fn a_missing_jev_makes_every_peer_other() {
        let check = AdminCheck::new(Path::new("/no/such/jev"));
        assert_eq!(check.peer(Some(std::process::id())), Peer::Other);
    }

    #[test]
    fn jev_sits_next_to_the_running_binary() {
        let jev = jev_next_to_me().unwrap();
        assert_eq!(jev.parent(), std::env::current_exe().unwrap().parent());
        assert_eq!(
            jev.file_name().unwrap().to_str().unwrap(),
            format!("jev{}", std::env::consts::EXE_SUFFIX)
        );
    }
}
```

Add `pub mod peer;` to `crates/jevd/src/lib.rs`, in name order.

- [ ] **Step 3: Run the tests and read the failure**

Run: `cargo test -p jevd --lib peer::`
Expected: FAIL to compile: `exe_of`, `AdminCheck`, and `jev_next_to_me` are not found.

- [ ] **Step 4: Write the code**

Put this above the test module in `crates/jevd/src/peer.rs`:

```rust
//! The peer check (spec section 2, "Transport"): admin messages count only
//! from a peer process whose full path is exactly the `jev` next to `jevd`.

use std::io;
use std::path::{Path, PathBuf};

use crate::daemon::Peer;

/// The `jev` binary in the same folder as the running `jevd`.
pub fn jev_next_to_me() -> io::Result<PathBuf> {
    let me = std::env::current_exe()?;
    let folder = me
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "jevd has no folder"))?;
    Ok(folder.join(format!("jev{}", std::env::consts::EXE_SUFFIX)))
}

/// Decides which peers count as `jev`.
#[derive(Debug, Clone)]
pub struct AdminCheck {
    /// The canonical path of `jev`, or `None` when it does not exist, so no
    /// peer counts as `jev`.
    jev: Option<PathBuf>,
}

impl AdminCheck {
    pub fn new(jev: &Path) -> AdminCheck {
        AdminCheck {
            jev: std::fs::canonicalize(jev).ok(),
        }
    }

    /// `Peer::Jev` only when the process `pid` runs the exact `jev` binary.
    /// Any failure, such as a process that ended, gives `Peer::Other`.
    pub fn peer(&self, pid: Option<u32>) -> Peer {
        let (Some(jev), Some(pid)) = (&self.jev, pid) else {
            return Peer::Other;
        };
        match exe_of(pid).and_then(std::fs::canonicalize) {
            Ok(path) if &path == jev => Peer::Jev,
            _ => Peer::Other,
        }
    }
}

/// The full path of the executable that process `pid` runs.
#[cfg(target_os = "linux")]
pub fn exe_of(pid: u32) -> io::Result<PathBuf> {
    std::fs::read_link(format!("/proc/{pid}/exe"))
}

#[cfg(target_os = "macos")]
pub fn exe_of(pid: u32) -> io::Result<PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    let mut buffer = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    let pid = libc::c_int::try_from(pid)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "pid out of range"))?;
    // SAFETY: the buffer is writable and its length is passed.
    let len = unsafe { libc::proc_pidpath(pid, buffer.as_mut_ptr().cast(), buffer.len() as u32) };
    if len <= 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(PathBuf::from(std::ffi::OsStr::from_bytes(
        &buffer[..len as usize],
    )))
}

#[cfg(windows)]
pub fn exe_of(pid: u32) -> io::Result<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use std::os::windows::io::{FromRawHandle, OwnedHandle};
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    // SAFETY: the handle is checked, then closed when `process` drops; the
    // buffer is writable and its length is passed.
    unsafe {
        let raw = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if raw.is_null() {
            return Err(io::Error::last_os_error());
        }
        let process = OwnedHandle::from_raw_handle(raw);
        let mut buffer = vec![0u16; 32_768];
        let mut len = buffer.len() as u32;
        let ok = QueryFullProcessImageNameW(
            std::os::windows::io::AsRawHandle::as_raw_handle(&process),
            PROCESS_NAME_WIN32,
            buffer.as_mut_ptr(),
            &mut len,
        );
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(PathBuf::from(std::ffi::OsString::from_wide(
            &buffer[..len as usize],
        )))
    }
}
```

- [ ] **Step 5: Run the tests**

Run: `cargo test -p jevd --lib peer::`
Expected: PASS, 4 tests. `only_the_exact_binary_counts_as_jev` copies the test binary, so it takes a moment.

- [ ] **Step 6: Mutation self-check**

1. In `peer`, compare only the file names.
2. In `peer`, return `Peer::Jev` when `pid` is `None`.

- [ ] **Step 7: Commit**

```bash
git add Cargo.lock crates/jevd/Cargo.toml crates/jevd/src/lib.rs crates/jevd/src/peer.rs
git commit -m "feat(jevd): the peer check for admin messages" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: The channel (`jevd::listen`)

Spec section 2, "Lifecycle", step 2, and "Transport".

**Unix.** `bind` creates the runtime folder with mode 0700 and checks it. It opens `jevd.pid` with mode 0600 and takes an exclusive, non-blocking `flock`. If another process holds the lock, another daemon runs: `BindError::Running`. With the lock, it writes its process ID, deletes any stale socket file, and binds `jevd.sock`. `close` deletes the socket and the PID file.

**Windows.** `bind` builds the security descriptor once, and creates the first pipe instance with `first_pipe_instance(true)` and `reject_remote_clients(true)`. Error 5 (access denied) or 231 (pipe busy) means another process holds the name: `BindError::Running`. After each `connect`, `accept` creates the next instance at once, so a client never finds the pipe gone. A failed `connect` replaces the waiting instance. `close` deletes the PID file; the pipe goes away with its last handle.

**Files:**
- Create: `crates/jevd/src/listen.rs`
- Modify: `crates/jevd/src/lib.rs`

- [ ] **Step 1: Write the tests**

Create `crates/jevd/src/listen.rs` with only its test module for now:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[cfg(unix)]
    async fn dial(paths: &Paths) -> io::Result<impl tokio::io::AsyncRead + tokio::io::AsyncWrite> {
        tokio::net::UnixStream::connect(&paths.socket).await
    }

    #[cfg(windows)]
    async fn dial(paths: &Paths) -> io::Result<impl tokio::io::AsyncRead + tokio::io::AsyncWrite> {
        tokio::net::windows::named_pipe::ClientOptions::new().open(&paths.socket)
    }

    #[tokio::test]
    async fn a_second_bind_finds_the_first_daemon() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::under(dir.path()).unwrap();
        let first = bind(&paths).unwrap();
        assert!(matches!(bind(&paths), Err(BindError::Running)));
        first.close();
        let again = bind(&paths).expect("the channel is free after close");
        again.close();
    }

    #[tokio::test]
    async fn accept_gives_the_stream_and_this_process_id() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::under(dir.path()).unwrap();
        let mut listener = bind(&paths).unwrap();
        let mut client = dial(&paths).await.unwrap();
        let (mut stream, pid) = listener.accept().await.unwrap();
        assert_eq!(pid, Some(std::process::id()));
        client.write_all(b"ping\n").await.unwrap();
        let mut buffer = [0u8; 5];
        stream.read_exact(&mut buffer).await.unwrap();
        assert_eq!(&buffer, b"ping\n");
        let mut second = dial(&paths).await.expect("a new instance waits");
        let (_stream, _) = listener.accept().await.unwrap();
        second.write_all(b"x").await.unwrap();
        listener.close();
    }

    #[tokio::test]
    async fn the_pid_file_names_this_process() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::under(dir.path()).unwrap();
        let listener = bind(&paths).unwrap();
        let text = std::fs::read_to_string(paths.pid_file()).unwrap();
        assert_eq!(text.trim(), std::process::id().to_string());
        listener.close();
        assert!(!paths.pid_file().exists());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_stale_socket_file_is_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::under(dir.path()).unwrap();
        paths.prepare_runtime_dir().unwrap();
        std::fs::write(&paths.socket, "stale").unwrap();
        let listener = bind(&paths).unwrap();
        dial(&paths).await.unwrap();
        listener.close();
        assert!(!paths.socket.exists());
    }
}
```

Add `pub mod listen;` to `crates/jevd/src/lib.rs`, in name order.

- [ ] **Step 2: Run the tests and read the failure**

Run: `cargo test -p jevd --lib listen::`
Expected: FAIL to compile: `bind`, `BindError`, and `Listener` are not found.

- [ ] **Step 3: Write the code**

Put this above the test module in `crates/jevd/src/listen.rs`:

```rust
//! The channel (spec section 2, "Transport"): a Unix socket in a private
//! folder, or a Windows named pipe that only this user can open. Binding
//! fails with `Running` when another daemon holds the channel.

use std::io;
use std::path::PathBuf;

use jev_proto::Paths;

/// Why `bind` failed.
#[derive(Debug)]
pub enum BindError {
    /// Another daemon holds the channel.
    Running,
    Io(io::Error),
}

impl std::fmt::Display for BindError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BindError::Running => f.write_str("another jevd holds the channel"),
            BindError::Io(error) => write!(f, "cannot open the channel: {error}"),
        }
    }
}

impl From<io::Error> for BindError {
    fn from(error: io::Error) -> BindError {
        BindError::Io(error)
    }
}

#[cfg(unix)]
pub use unix::{bind, Listener, Stream};
#[cfg(windows)]
pub use windows::{bind, Listener, Stream};

#[cfg(unix)]
mod unix {
    use super::*;
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    use std::os::unix::io::AsRawFd;

    pub type Stream = tokio::net::UnixStream;

    pub struct Listener {
        inner: tokio::net::UnixListener,
        socket: PathBuf,
        pid_file: PathBuf,
        /// Holds the lock for the life of the daemon.
        _lock: std::fs::File,
    }

    /// Takes the lock on the PID file, then binds the socket. The lock makes
    /// the check and the bind one step: of two daemons, only one gets it.
    pub fn bind(paths: &Paths) -> Result<Listener, BindError> {
        paths.prepare_runtime_dir()?;
        let pid_file = paths.pid_file();
        let mut lock = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(&pid_file)?;
        // SAFETY: the descriptor is open for the call.
        if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            let error = io::Error::last_os_error();
            return Err(if error.kind() == io::ErrorKind::WouldBlock {
                BindError::Running
            } else {
                BindError::Io(error)
            });
        }
        lock.set_len(0)?;
        writeln!(lock, "{}", std::process::id())?;
        // A socket file left by a daemon that crashed: the lock shows that no
        // daemon uses it.
        match std::fs::remove_file(&paths.socket) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error.into()),
            _ => {}
        }
        let inner = tokio::net::UnixListener::bind(&paths.socket)?;
        Ok(Listener {
            inner,
            socket: paths.socket.clone(),
            pid_file,
            _lock: lock,
        })
    }

    impl Listener {
        /// The next connection, and the peer's process ID when the OS gives it.
        pub async fn accept(&mut self) -> io::Result<(Stream, Option<u32>)> {
            let (stream, _) = self.inner.accept().await?;
            let pid = stream
                .peer_cred()
                .ok()
                .and_then(|cred| cred.pid())
                .and_then(|pid| u32::try_from(pid).ok());
            Ok((stream, pid))
        }

        /// Deletes the socket file and the PID file, then lets the lock go.
        pub fn close(self) {
            let _ = std::fs::remove_file(&self.socket);
            let _ = std::fs::remove_file(&self.pid_file);
        }
    }
}

#[cfg(windows)]
mod windows {
    use super::*;
    use std::ffi::c_void;
    use std::os::windows::io::AsRawHandle;
    use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
    use windows_sys::Win32::Foundation::{LocalFree, ERROR_ACCESS_DENIED, ERROR_PIPE_BUSY, HANDLE};
    use windows_sys::Win32::Security::Authorization::{
        ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
    };
    use windows_sys::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
    use windows_sys::Win32::System::Pipes::GetNamedPipeClientProcessId;

    pub type Stream = NamedPipeServer;

    /// A security descriptor that names this user as owner and gives only
    /// this user access. No inherited entries.
    struct PipeSecurity(PSECURITY_DESCRIPTOR);

    // SAFETY: the descriptor is never changed after it is built, and it is
    // freed once, in `drop`.
    unsafe impl Send for PipeSecurity {}
    unsafe impl Sync for PipeSecurity {}

    impl PipeSecurity {
        fn for_current_user() -> io::Result<PipeSecurity> {
            let sid = jev_proto::os::user_sid()?;
            let sddl: Vec<u16> = format!("O:{sid}D:P(A;;GA;;;{sid})")
                .encode_utf16()
                .chain(Some(0))
                .collect();
            let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
            // SAFETY: `sddl` ends in a 0, and `descriptor` gets a LocalAlloc
            // pointer that `drop` frees.
            let ok = unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    sddl.as_ptr(),
                    SDDL_REVISION_1,
                    &mut descriptor,
                    std::ptr::null_mut(),
                )
            };
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(PipeSecurity(descriptor))
        }

        fn create(&self, name: &PathBuf, first: bool) -> io::Result<NamedPipeServer> {
            let mut attributes = SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: self.0,
                bInheritHandle: 0,
            };
            // SAFETY: `attributes` and the descriptor outlive the call.
            unsafe {
                ServerOptions::new()
                    .first_pipe_instance(first)
                    .reject_remote_clients(true)
                    .create_with_security_attributes_raw(
                        name,
                        (&mut attributes as *mut SECURITY_ATTRIBUTES).cast::<c_void>(),
                    )
            }
        }
    }

    impl Drop for PipeSecurity {
        fn drop(&mut self) {
            // SAFETY: the pointer came from LocalAlloc, and this is its only free.
            unsafe { LocalFree(self.0.cast()) };
        }
    }

    pub struct Listener {
        name: PathBuf,
        security: PipeSecurity,
        /// The instance that waits for the next client.
        next: NamedPipeServer,
        pid_file: PathBuf,
    }

    /// Creates the first pipe instance. `FILE_FLAG_FIRST_PIPE_INSTANCE`
    /// makes this fail when any process holds the name, so no other process
    /// can squat the pipe first, and of two daemons only one binds.
    pub fn bind(paths: &Paths) -> Result<Listener, BindError> {
        paths.prepare_runtime_dir()?;
        let security = PipeSecurity::for_current_user()?;
        let next = match security.create(&paths.socket, true) {
            Ok(next) => next,
            Err(error)
                if [ERROR_ACCESS_DENIED, ERROR_PIPE_BUSY]
                    .map(|code| code as i32)
                    .contains(&error.raw_os_error().unwrap_or(0)) =>
            {
                return Err(BindError::Running)
            }
            Err(error) => return Err(error.into()),
        };
        let pid_file = paths.pid_file();
        write_pid(&pid_file)?;
        Ok(Listener {
            name: paths.socket.clone(),
            security,
            next,
            pid_file,
        })
    }

    fn write_pid(pid_file: &std::path::Path) -> io::Result<()> {
        crate::files::write_atomic(pid_file, &format!("{}\n", std::process::id()))
    }

    impl Listener {
        /// The next connection, and the peer's process ID.
        pub async fn accept(&mut self) -> io::Result<(Stream, Option<u32>)> {
            if let Err(error) = self.next.connect().await {
                // That instance may be broken now; wait on a new one.
                self.next = self.security.create(&self.name, false)?;
                return Err(error);
            }
            let fresh = self.security.create(&self.name, false)?;
            let stream = std::mem::replace(&mut self.next, fresh);
            let mut pid = 0u32;
            // SAFETY: the handle is a connected pipe server for the call.
            let ok =
                unsafe { GetNamedPipeClientProcessId(stream.as_raw_handle() as HANDLE, &mut pid) };
            Ok((stream, (ok != 0).then_some(pid)))
        }

        /// Deletes the PID file. The pipe goes away with its last handle.
        pub fn close(self) {
            let _ = std::fs::remove_file(&self.pid_file);
        }
    }
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p jevd --lib listen::`
Expected: PASS. On Windows: 3 tests. On Unix: 4.

- [ ] **Step 5: Mutation self-check**

1. On Windows: in `bind`, call `security.create(&paths.socket, false)`. `a_second_bind_finds_the_first_daemon` must fail.
2. On Unix: drop `LOCK_NB`. The same test hangs; a hang counts as caught.

- [ ] **Step 6: Commit**

```bash
git add crates/jevd/src/lib.rs crates/jevd/src/listen.rs
git commit -m "feat(jevd): a private channel that one daemon holds at a time" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 7: The daemon's life (`jevd::log`, `jevd::run`, the binary)

Spec section 2, "Lifecycle", and plan 2's notes for plan 3:

- `run` accepts connections and serves each one in a `JoinSet` task. A connection over `max-connections` gets `daemon-unavailable` after jevd reads its first line, then closes. Reading the line first lets the client's `hello` write finish on Windows, so the client reads the reason.
- `idle_exit` stops the daemon after `idle-timeout` with no request, never while a `Busy` guard exists. With `off`, never.
- `sweep_keys` wipes expired keys at most 60 s late. Each lookup also sweeps, so a key past its period is never used.
- A stop (the `stop` message, idle exit, or a signal) ends the accept loop. Requests in flight get `DRAIN_LIMIT` (45 s) to finish; then jevd cuts the rest. Then jevd wipes every key and closes the channel.
- `main` binds first, then loads the settings and the pairing file, as the spec's lifecycle orders it. Another daemon on the channel: exit 0 at once. A failure at start: exit 1, with the reason in the log and on stderr.
- `main` builds `JevClient` once. A change of proxy settings needs a restart.
- After `run`, `main` calls `Runtime::shutdown_timeout(1 s)`, because a keyring prompt can hold a blocking thread. The keys are already wiped by then.
- Signals: SIGTERM, SIGINT, and SIGHUP on Unix; Ctrl+C on Windows. A detached jevd on Windows has no console, so it gets no console events, and it ends with the session.
- The log holds one line per event: start, stop, idle exit, signal, and start failures. Each line holds fixed text plus paths or names, and control characters become spaces. A log over 1 MiB moves to `jevd.log.old`.

**Files:**
- Create: `crates/jevd/src/log.rs`
- Create: `crates/jevd/src/run.rs`
- Modify: `crates/jevd/src/main.rs` (the whole file)
- Modify: `crates/jevd/src/lib.rs`

- [ ] **Step 1: Write the tests**

Create `crates/jevd/src/log.rs` with only its test module for now:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_writes_one_clean_line() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state").join("jevd.log");
        append(&path, "start\nforged line").unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text.lines().count(), 1);
        assert!(text.trim_end().ends_with("start forged line"));
    }

    #[test]
    fn a_full_log_moves_aside() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jevd.log");
        std::fs::write(&path, vec![b'x'; MAX_LOG_BYTES as usize + 1]).unwrap();
        append(&path, "next").unwrap();
        assert!(dir.path().join("jevd.log.old").exists());
        assert!(std::fs::metadata(&path).unwrap().len() < 100);
    }
}
```

Create `crates/jevd/src/run.rs` with only its test module for now:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::daemon::tests::fixture_with;
    use crate::keystore::MemoryKeyring;
    use crate::settings::Settings;
    use jev_proto::Request;

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
}
```

Add `pub mod log;` and `pub mod run;` to `crates/jevd/src/lib.rs`, in name order. The list is now:

```rust
//! The jev daemon's library: checks, settings, pairing, keys, and the Jev client.

pub mod answer;
mod ask;
pub mod conn;
pub mod daemon;
pub mod files;
pub mod jev;
pub mod jev_http;
pub mod key;
pub mod keycache;
pub mod keystore;
pub mod listen;
pub mod log;
pub mod pairing;
pub mod peer;
pub mod ratelimit;
pub mod redact;
pub mod run;
pub mod settings;
pub mod shape;
pub mod units;
```

- [ ] **Step 2: Run the tests and read the failure**

Run: `cargo test -p jevd --lib run::`
Expected: FAIL to compile: `idle_exit`, `append`, and `MAX_LOG_BYTES` are not found.

- [ ] **Step 3: Write the code**

Put this above the test module in `crates/jevd/src/log.rs`:

```rust
//! The log file (spec section 1, "Storage"). One line per daemon event:
//! start, stop, and failures. It never holds a key, a token, or request text,
//! so callers pass fixed text, names, and paths only.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

/// A log over this size moves to `<name>.old`, and a new one starts.
pub const MAX_LOG_BYTES: u64 = 1 << 20;

static FILE: OnceLock<PathBuf> = OnceLock::new();

/// Sets the log file. Before this runs, and in tests, `event` does nothing.
pub fn init(path: &Path) {
    let _ = FILE.set(path.to_path_buf());
}

/// Adds one line. A failed write is dropped: the log must never stop jevd.
pub fn event(text: &str) {
    if let Some(path) = FILE.get() {
        let _ = append(path, text);
    }
}

fn append(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if std::fs::metadata(path).is_ok_and(|meta| meta.len() > MAX_LOG_BYTES) {
        let mut old = path.as_os_str().to_owned();
        old.push(".old");
        std::fs::rename(path, old)?;
    }
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let line: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(file, "{seconds} {line}")
}
```

Put this above the test module in `crates/jevd/src/run.rs`:

```rust
//! The daemon's life (spec section 2, "Lifecycle"): serve connections until
//! a stop, an idle exit, or a signal, then let requests in flight finish,
//! wipe the keys, and close the channel.

use std::sync::Arc;
use std::time::{Duration, Instant};

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

/// Serves until the daemon stops, then drains and cleans up.
pub async fn run(daemon: Arc<Daemon>, mut listener: Listener, admin: AdminCheck) {
    let watchers = [
        tokio::spawn(idle_exit(Arc::clone(&daemon))),
        tokio::spawn(sweep_keys(Arc::clone(&daemon))),
        tokio::spawn(signals(Arc::clone(&daemon))),
    ];
    let mut stopping = daemon.stopping();
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let Ok((stream, pid)) = accepted else {
                    // A failed accept, such as a client that left at once.
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    continue;
                };
                if connections.len() >= daemon.max_connections() {
                    tokio::spawn(refuse(stream));
                    continue;
                }
                connections.spawn(conn::serve(stream, admin.peer(pid), Arc::clone(&daemon)));
            }
            Some(_) = connections.join_next(), if !connections.is_empty() => {}
            _ = Daemon::stopped(&mut stopping) => break,
        }
    }
    let drained = tokio::time::timeout(DRAIN_LIMIT, async {
        while connections.join_next().await.is_some() {}
    })
    .await;
    if drained.is_err() {
        connections.abort_all();
    }
    for watcher in watchers {
        watcher.abort();
    }
    daemon.clear_keys();
    listener.close();
}

/// Answers the first line of a client over `max-connections` with the
/// reason, then closes. Reading the line first lets the client's `hello`
/// write finish, so the client reads the reason.
async fn refuse(stream: crate::listen::Stream) {
    let (read, mut write) = tokio::io::split(stream);
    let mut reader = tokio::io::BufReader::new(read);
    let first = conn::read_line(&mut reader, conn::MAX_LINE);
    let _ = tokio::time::timeout(REFUSE_WAIT, first).await;
    let reply = failure(
        ErrorCode::DaemonUnavailable,
        "jevd has too many connections open; see max-connections",
    );
    let _ = write.write_all(reply.to_line().as_bytes()).await;
    let _ = write.flush().await;
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
        let now = Instant::now();
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
        // A detached jevd has no console, so these fail and the daemon
        // ends with the session. Run from a terminal, Ctrl+C stops it.
        if tokio::signal::ctrl_c().await.is_err() {
            return;
        }
    }
    crate::log::event("signal");
    daemon.request_stop();
}
```

Replace `crates/jevd/src/main.rs` with:

```rust
//! jevd: binds the channel, loads the files, and serves (spec section 2,
//! "Lifecycle"). Clients start it detached; it also runs in a terminal.

use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use jev_proto::Paths;
use jevd::daemon::{Config, Daemon};
use jevd::jev_http::JevClient;
use jevd::keystore::OsKeyring;
use jevd::listen::{bind, BindError};
use jevd::peer::{jev_next_to_me, AdminCheck};
use jevd::{log, run};

fn main() -> ExitCode {
    if std::env::args().nth(1).as_deref() == Some("--version") {
        println!("jevd {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    let paths = match Paths::from_env() {
        Ok(paths) => paths,
        Err(error) => return fail(&format!("cannot find the jev folders: {error}")),
    };
    log::init(&paths.log_file());
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => return fail(&format!("cannot start the runtime: {error}")),
    };
    let code = runtime.block_on(serve(paths));
    // A keyring prompt can hold a blocking thread. The keys are already
    // wiped, so do not wait long for it.
    runtime.shutdown_timeout(Duration::from_secs(1));
    code
}

async fn serve(paths: Paths) -> ExitCode {
    let listener = match bind(&paths) {
        Ok(listener) => listener,
        Err(BindError::Running) => {
            log::event("another jevd holds the channel; this one exits");
            return ExitCode::SUCCESS;
        }
        Err(error) => return fail(&error.to_string()),
    };
    let jev = match JevClient::new() {
        Ok(jev) => jev,
        Err(problem) => {
            listener.close();
            return fail(&problem);
        }
    };
    let config = Config {
        settings_file: paths.settings_file(),
        pairing_file: paths.pairing_file(),
        store: Arc::new(OsKeyring),
        jev,
    };
    let daemon = match Daemon::new(config) {
        Ok(daemon) => daemon,
        Err(problem) => {
            listener.close();
            return fail(&problem);
        }
    };
    let admin = AdminCheck::new(&jev_next_to_me().unwrap_or_default());
    log::event("start");
    run::run(daemon, listener, admin).await;
    log::event("stop");
    ExitCode::SUCCESS
}

fn fail(problem: &str) -> ExitCode {
    log::event(&format!("start failed: {problem}"));
    eprintln!("jevd: {problem}");
    ExitCode::FAILURE
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p jevd --lib run::`
Expected: PASS, 3 tests, in paused time.

Run: `cargo test -p jevd --lib log::`
Expected: PASS, 2 tests.

Run: `cargo run -p jevd -- --version`
Expected: `jevd 0.1.0`.

- [ ] **Step 5: Mutation self-check**

1. In `Daemon::idle_since`, return `Some(since)` even while busy. `idle_exit_waits_while_a_request_runs` must fail.
2. In `append`, drop the control-character map. `append_writes_one_clean_line` must fail.

- [ ] **Step 6: Commit**

```bash
git add crates/jevd/src/lib.rs crates/jevd/src/log.rs crates/jevd/src/run.rs crates/jevd/src/main.rs
git commit -m "feat(jevd): the daemon's life: accept loop, idle exit, key sweep, stop with a drain" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 8: The client (`jev_proto::client`)

Spec section 1 ("a small client that connects and starts `jevd` when needed") and section 2, "Lifecycle", step 2. It uses only `std`, so `jev-proto` still has no tokio.

- `Connection::connect` opens the channel, checks it, sends `hello`, and checks that the reply names protocol 1. On Unix it checks the runtime folder first. On Windows it waits up to 2 s while every pipe instance is busy, then checks that this user owns the pipe.
- One worker thread owns the stream, and does each write and then its read. See "Choices this plan makes" for why.
- `call` waits up to `REPLY_TIMEOUT` (120 s). After a timeout the connection refuses more calls, because a late reply would answer the wrong call.
- A reply line over `MAX_REPLY` (2 MiB) is `too-large`.
- Every local failure is a `WireError`: `daemon-unavailable` for no daemon, a closed connection, or a timeout; `protocol-unsupported`; and `internal` for a reply that does not parse.
- `start_daemon` starts `jevd` with no standard streams. On Unix it calls `setsid` in the child. On Windows it uses `DETACHED_PROCESS` and `CREATE_NEW_PROCESS_GROUP`, and tries `CREATE_BREAKAWAY_FROM_JOB` first, so the daemon outlives a terminal that closes its job.
- `connect_or_start` starts `jevd` only on `daemon-unavailable`, then retries for up to `START_WAIT` (5 s).

**Files:**
- Create: `crates/jev-proto/src/client.rs`
- Modify: `crates/jev-proto/src/lib.rs`

- [ ] **Step 1: Write the tests**

Create `crates/jev-proto/src/client.rs` with only its test module for now:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_daemon_is_daemon_unavailable() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::under(dir.path()).unwrap();
        paths.prepare_runtime_dir().unwrap();
        let error = Connection::connect(&paths).err().unwrap();
        assert_eq!(error.code.known(), Some(ErrorCode::DaemonUnavailable));
    }

    #[test]
    fn a_missing_jevd_cannot_start() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::under(dir.path()).unwrap();
        paths.prepare_runtime_dir().unwrap();
        let error = connect_or_start(&paths, &dir.path().join("no-such-jevd"))
            .err()
            .unwrap();
        assert_eq!(error.code.known(), Some(ErrorCode::DaemonUnavailable));
        assert!(
            error.message.contains("cannot start jevd"),
            "{}",
            error.message
        );
    }
}
```

Add `pub mod client;` as the first module line of `crates/jev-proto/src/lib.rs`. The file is now:

```rust
//! Wire types shared by jevd, the jev CLI, and third-party clients.

pub mod client;
mod error;
mod message;
mod names;
#[cfg(windows)]
pub mod os;
pub mod paths;

pub use error::{ErrorCode, WireCode, WireError};
pub use message::{Reply, Request, PROTOCOL_VERSION};
pub use names::valid_name;
pub use paths::Paths;
```

- [ ] **Step 2: Run the tests and read the failure**

Run: `cargo test -p jev-proto client::`
Expected: FAIL to compile: `Connection` and `connect_or_start` are not found.

- [ ] **Step 3: Write the code**

Put this above the test module in `crates/jev-proto/src/client.rs`:

```rust
//! A small blocking client for jevd (spec section 1, "Repo layout"): it
//! connects, checks the channel, sends `hello`, then sends one request at a
//! time. `connect_or_start` starts `jevd` when no daemon runs.
//!
//! Every failure is a `WireError`, so a caller handles a local failure and a
//! daemon's reply the same way. The client never logs a reply, because a
//! `pair-status` or `approve` reply can carry a token.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::{ErrorCode, Paths, Reply, Request, WireError, PROTOCOL_VERSION};

/// How long `connect_or_start` waits for a daemon it started.
pub const START_WAIT: Duration = Duration::from_secs(5);
/// How long a call waits for its reply. One `ask` can take about 31 s in Jev
/// alone, plus a keyring prompt, so this is well above that.
pub const REPLY_TIMEOUT: Duration = Duration::from_secs(120);
/// The longest reply line, newline not counted.
pub const MAX_REPLY: usize = 2 << 20;

#[cfg(unix)]
type Stream = std::os::unix::net::UnixStream;
#[cfg(windows)]
type Stream = std::fs::File;

fn unavailable(message: impl Into<String>) -> WireError {
    WireError::new(ErrorCode::DaemonUnavailable, message)
}

type Outcome = Result<Vec<u8>, WireError>;

/// One open connection, past `hello`. A worker thread owns the stream and
/// does each write and then its read. On Windows, a blocking read on a pipe
/// blocks a write on a copy of the same handle, so one thread does both.
pub struct Connection {
    jobs: mpsc::Sender<Vec<u8>>,
    replies: mpsc::Receiver<Outcome>,
    timeout: Duration,
    /// Set after a timeout: a late reply would answer the wrong call.
    broken: bool,
}

impl Connection {
    /// Connects to a running daemon, checks the channel, and sends `hello`.
    /// No daemon gives `daemon-unavailable`.
    pub fn connect(paths: &Paths) -> Result<Connection, WireError> {
        let stream = open(paths)?;
        let (jobs, inbox) = mpsc::channel();
        let (outbox, replies) = mpsc::channel();
        std::thread::spawn(move || work(stream, inbox, outbox));
        let mut connection = Connection {
            jobs,
            replies,
            timeout: REPLY_TIMEOUT,
            broken: false,
        };
        let reply = connection.call(&Request::Hello {
            protocol: PROTOCOL_VERSION,
        })?;
        if let Some(error) = reply.error {
            return Err(error);
        }
        if reply.data.get("protocol").and_then(|value| value.as_u64())
            != Some(u64::from(PROTOCOL_VERSION))
        {
            return Err(WireError::new(
                ErrorCode::ProtocolUnsupported,
                "jevd answered hello with another protocol",
            ));
        }
        Ok(connection)
    }

    /// Changes how long each call waits for its reply.
    pub fn set_timeout(&mut self, timeout: Duration) {
        self.timeout = timeout;
    }

    /// Sends one request and waits for its reply. A reply with `ok: false`
    /// comes back as `Ok`, so the caller sees the daemon's error as sent.
    pub fn call(&mut self, request: &Request) -> Result<Reply, WireError> {
        if self.broken {
            return Err(unavailable("this connection timed out; open a new one"));
        }
        let mut line = serde_json::to_vec(request).expect("a request always serializes");
        line.push(b'\n');
        if self.jobs.send(line).is_err() {
            return Err(unavailable("jevd closed the connection"));
        }
        let bytes = match self.replies.recv_timeout(self.timeout) {
            Ok(outcome) => outcome?,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                self.broken = true;
                return Err(unavailable("jevd did not answer in time"));
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(unavailable("jevd closed the connection"))
            }
        };
        serde_json::from_slice(&bytes).map_err(|_| {
            WireError::new(ErrorCode::Internal, "jevd sent a reply that cannot be read")
        })
    }
}

/// For each job: write the line, then read one reply line. Ends when the
/// connection or the channel closes.
fn work(stream: Stream, jobs: mpsc::Receiver<Vec<u8>>, replies: mpsc::Sender<Outcome>) {
    let mut reader = BufReader::new(stream);
    for line in jobs {
        let sent = reader
            .get_mut()
            .write_all(&line)
            .and_then(|()| reader.get_mut().flush());
        let outcome = match sent {
            Ok(()) => read_reply(&mut reader),
            Err(error) => Err(unavailable(format!("cannot send to jevd: {error}"))),
        };
        let stop = outcome.is_err();
        if replies.send(outcome).is_err() || stop {
            return;
        }
    }
}

fn read_reply(reader: &mut BufReader<Stream>) -> Outcome {
    let mut line = Vec::new();
    let limit = MAX_REPLY as u64 + 1;
    match reader.take(limit).read_until(b'\n', &mut line) {
        Ok(0) => return Err(unavailable("jevd closed the connection")),
        Ok(_) => {}
        Err(error) => return Err(unavailable(format!("cannot read from jevd: {error}"))),
    }
    if line.last() == Some(&b'\n') {
        line.pop();
        return Ok(line);
    }
    if line.len() > MAX_REPLY {
        return Err(WireError::new(
            ErrorCode::TooLarge,
            "jevd sent a reply over the cap",
        ));
    }
    Ok(line)
}

#[cfg(unix)]
fn open(paths: &Paths) -> Result<Stream, WireError> {
    paths
        .check_runtime_dir()
        .map_err(|error| unavailable(format!("jevd is not running: {error}")))?;
    Stream::connect(&paths.socket)
        .map_err(|error| unavailable(format!("jevd is not running: {error}")))
}

#[cfg(windows)]
fn open(paths: &Paths) -> Result<Stream, WireError> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::ERROR_PIPE_BUSY;
    // Every pipe instance can be busy for a moment, between two accepts.
    let deadline = Instant::now() + Duration::from_secs(2);
    let file = loop {
        match std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&paths.socket)
        {
            Ok(file) => break file,
            Err(error)
                if error.raw_os_error() == Some(ERROR_PIPE_BUSY as i32)
                    && Instant::now() < deadline =>
            {
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(error) => return Err(unavailable(format!("jevd is not running: {error}"))),
        }
    };
    // A pipe that another user created first is a squatter.
    match crate::os::owned_by_current_user(file.as_raw_handle()) {
        Ok(true) => Ok(file),
        Ok(false) => Err(unavailable("the jevd pipe belongs to another user")),
        Err(error) => Err(unavailable(format!("cannot check the jevd pipe: {error}"))),
    }
}

/// Starts `jevd` detached: no console window on Windows, a new session on
/// Unix, and no standard streams. It does not wait for the daemon.
pub fn start_daemon(jevd: &Path) -> std::io::Result<()> {
    use std::process::{Command, Stdio};
    let mut command = Command::new(jevd);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: setsid is async-signal-safe, so it may run between fork and exec.
        unsafe {
            command.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
        command.spawn().map(|_| ())
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use windows_sys::Win32::System::Threading::{
            CREATE_BREAKAWAY_FROM_JOB, CREATE_NEW_PROCESS_GROUP, DETACHED_PROCESS,
        };
        let detached = DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP;
        // Leave the caller's job, so the daemon outlives a terminal that
        // closes its job. A job that forbids this refuses the flag.
        command.creation_flags(detached | CREATE_BREAKAWAY_FROM_JOB);
        if command.spawn().is_ok() {
            return Ok(());
        }
        command.creation_flags(detached);
        command.spawn().map(|_| ())
    }
}

/// Connects, and starts `jevd` first when no daemon runs. Waits up to
/// `START_WAIT` for the new daemon. Only `daemon-unavailable` starts one.
pub fn connect_or_start(paths: &Paths, jevd: &Path) -> Result<Connection, WireError> {
    match Connection::connect(paths) {
        Err(error) if error.code.known() == Some(ErrorCode::DaemonUnavailable) => {}
        other => return other,
    }
    start_daemon(jevd).map_err(|error| unavailable(format!("cannot start jevd: {error}")))?;
    let deadline = Instant::now() + START_WAIT;
    loop {
        std::thread::sleep(Duration::from_millis(50));
        match Connection::connect(paths) {
            Err(error)
                if error.code.known() == Some(ErrorCode::DaemonUnavailable)
                    && Instant::now() < deadline => {}
            other => return other,
        }
    }
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p jev-proto`
Expected: PASS, 25 tests on Windows.

These 2 tests check only the failure paths. Task 9 tests the client against a real daemon.

- [ ] **Step 5: Commit**

```bash
git add crates/jev-proto/src/lib.rs crates/jev-proto/src/client.rs
git commit -m "feat(jev-proto): a blocking client that connects, checks the channel, and starts jevd" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 9: End-to-end tests

Spec section 11, "Daemon and transport" and "Pairing". Two test files:

- `tests/serve.rs` runs the daemon in the test process, on its real channel, with a fake Jev and `MemoryKeyring`. `AdminCheck::new(current_exe)` makes the test binary count as `jev`, so each `pair` here is a CLI request. The socket-client flow stays covered by Task 3's tests with `Peer::Other`.
- `tests/binary.rs` runs the real `jevd` binary under a new `JEV_HOME`. It never sends an `ask`, so the OS keyring and the real Jev endpoint stay untouched. The test binary is not `jev` here, so `stop` gets `forbidden`, and the test ends the daemon by hand.

`JevClient::with_endpoint` and `MemoryKeyring` must stay `pub` for these tests.

**Files:**
- Create: `crates/jevd/tests/serve.rs`
- Create: `crates/jevd/tests/binary.rs`

- [ ] **Step 1: Write the in-process tests**

Create `crates/jevd/tests/serve.rs`:

```rust
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
    let answer = call(&daemon.paths, ask(&stored)).await;
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
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(call(&daemon.paths, Request::Stop).await.ok);
    let answer = in_flight.await.unwrap();
    assert!(answer.ok, "the ask in flight finished: {answer:?}");
    tokio::time::timeout(Duration::from_secs(5), daemon.task)
        .await
        .expect("the daemon stops after the drain")
        .unwrap();
    assert!(!daemon.paths.pid_file().exists());
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
```

- [ ] **Step 2: Write the binary tests**

Create `crates/jevd/tests/binary.rs`:

```rust
//! The real `jevd` binary, under a new `JEV_HOME`. These tests never send an
//! `ask`, so the OS keyring and the real Jev endpoint stay untouched.

use std::path::Path;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use jev_proto::client::{connect_or_start, Connection};
use jev_proto::paths::HOME_VAR;
use jev_proto::{ErrorCode, Paths, Request};

const JEVD: &str = env!("CARGO_BIN_EXE_jevd");

fn spawn(home: &Path) -> Child {
    Command::new(JEVD).env(HOME_VAR, home).spawn().unwrap()
}

/// Waits until `child` exits, for at most `limit`.
fn exited(child: &mut Child, limit: Duration) -> Option<std::process::ExitStatus> {
    let deadline = Instant::now() + limit;
    while Instant::now() < deadline {
        if let Some(status) = child.try_wait().unwrap() {
            return Some(status);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    None
}

fn connect(paths: &Paths) -> Connection {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match Connection::connect(paths) {
            Ok(connection) => return connection,
            Err(_) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            Err(error) => panic!("no daemon: {error:?}"),
        }
    }
}

#[test]
fn version_prints_and_exits() {
    let output = Command::new(JEVD).arg("--version").output().unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert_eq!(text.trim(), format!("jevd {}", env!("CARGO_PKG_VERSION")));
}

#[test]
fn of_two_daemons_started_at_once_exactly_one_survives() {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::under(dir.path()).unwrap();
    let mut a = spawn(dir.path());
    let mut b = spawn(dir.path());
    let mut connection = connect(&paths);
    let status = connection.call(&Request::Status).unwrap();
    assert!(status.ok);
    let a_done = exited(&mut a, Duration::from_secs(5));
    let b_done = exited(&mut b, Duration::from_millis(200));
    let (loser, mut winner) = match (a_done, b_done) {
        (Some(status), None) => (status, b),
        (None, Some(status)) => (status, a),
        other => panic!("expected exactly one exit, got {other:?}"),
    };
    assert!(loser.success(), "the second daemon exits with success");
    assert!(connection.call(&Request::Status).unwrap().ok);
    // The test binary is not jev, so stop is forbidden; end the winner by hand.
    let stop = connection.call(&Request::Stop).unwrap();
    assert_eq!(
        stop.error.and_then(|error| error.code.known()),
        Some(ErrorCode::Forbidden)
    );
    winner.kill().unwrap();
    winner.wait().unwrap();
}

#[test]
fn connect_or_start_starts_a_detached_daemon() {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::under(dir.path()).unwrap();
    // The started daemon finds JEV_HOME through the environment it inherits.
    std::env::set_var(HOME_VAR, dir.path());
    let mut connection = connect_or_start(&paths, Path::new(JEVD)).unwrap();
    std::env::remove_var(HOME_VAR);
    assert!(connection.call(&Request::Status).unwrap().ok);
    let pid: u32 = std::fs::read_to_string(paths.pid_file())
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    kill(pid);
}

#[cfg(unix)]
fn kill(pid: u32) {
    // SAFETY: a plain signal to a process this test started.
    unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) };
}

#[cfg(windows)]
fn kill(pid: u32) {
    let _ = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/F"])
        .output();
}
```

- [ ] **Step 3: Run the tests**

Run: `cargo test -p jevd --test serve`
Expected: PASS, 5 tests, in about 1 s.

Run: `cargo test -p jevd --test binary`
Expected: PASS, 3 tests, in about 5 s. The loser of the two daemons exits with success.

These tests pass with the code from Tasks 1 to 8, so they have no red step. Step 4 checks that they can fail.

- [ ] **Step 4: Mutation self-check**

1. In `run::refuse`, drop the read of the first line. On Windows, `a_connection_over_max_connections_is_refused` must fail with `cannot send to jevd`.
2. In `run::run`, skip the drain (`connections.abort_all()` at once). `a_stop_lets_the_request_in_flight_finish_then_cleans_up` must fail.
3. In `main.rs`, return `ExitCode::FAILURE` for `BindError::Running`. `of_two_daemons_started_at_once_exactly_one_survives` must fail.

After the check, look for a `jevd` left running by a test: `tasklist | findstr jevd` on Windows, `pgrep jevd` on Unix. Stop any you find.

- [ ] **Step 5: Commit**

```bash
git add crates/jevd/tests
git commit -m "test(jevd): the daemon over its real channel, and the real binary" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 10: Lints, full test run, and spec updates

**Files:**
- Modify: `docs/superpowers/specs/2026-11-02-jev-daemon-design.md`

- [ ] **Step 1: Format and lint**

Run: `cargo fmt --all --check`
Expected: no output.

Run: `cargo clippy --workspace --all-targets --locked -- -D warnings`
Expected: no warnings.

- [ ] **Step 2: Run the full suite 3 times**

Run: `cargo test --workspace --locked` three times.
Expected: PASS each time. On Windows: `jev-proto` 25; `jevd` library 228 and 1 ignored; `binary` 3; `serve` 5.

- [ ] **Step 3: Update the spec**

In section 2, "Transport", replace the Framing row with:

```markdown
| Framing | One JSON object per line. A request line holds at most 1 MiB, newline not counted; a longer line gets `too-large`, and the connection closes. The client reads reply lines of at most 2 MiB. |
```

Replace the Pipe squatting row with:

```markdown
| Pipe squatting | `jevd` creates the pipe with `FILE_FLAG_FIRST_PIPE_INSTANCE`, and with an access list that names the user as owner and gives only that user access. The client checks that the current user owns the pipe. |
```

Replace the Concurrency row with:

```markdown
| Concurrency | One request in flight per connection. `max-connections` caps connections; a connection over the cap gets `daemon-unavailable` and closes. |
```

Add these rows at the end of the table:

```markdown
| Single instance | Unix: an exclusive lock on `jevd.pid` before the bind. Windows: the first pipe instance. A second daemon exits with success. |
| `JEV_HOME` | When set, every file and the channel go under this folder. Tests use it. |
| Client timeouts | The client waits 120 seconds for a reply, and 5 seconds for a daemon that it started. |
```

In section 2, "Lifecycle", replace step 6 with:

```markdown
6. **Stop.** On `jev stop`, idle exit, or a termination signal, `jevd` refuses new connections and lets requests in flight finish, for at most 45 seconds. Then it wipes all cached keys, deletes the socket file and the PID file, and exits. A detached `jevd` on Windows gets no console events, so it ends with the session.
```

Add this item to section 2, "Key cache":

```markdown
- A timer wipes a key at most 60 seconds after its period ends. Each lookup also checks the period, so `jevd` never uses a key past it.
```

In section 3, "Values", replace the Repeat CLI request row with:

```markdown
| Repeat CLI request | A `pair` from `jev` for a name that already has a waiting request from `jev` gets that request's `request_id` and `code`. Its expiry does not restart. A request from a socket client is never shared. |
```

In section 3, "Values", replace the Approval not committed row with:

```markdown
| Approval not committed | A second approval for the same name cancels the first. `jev revoke <name>` cancels approvals that are not committed, for that name. `jev key remove --force` revokes each client of the profile by name, then sends `reload`. An approval whose profile is no longer in the settings expires at its commit. |
```

In section 12, replace the "Windows pipe access list" item with:

```markdown
- **Windows pipe access list.** Done in plan 3: tokio's `ServerOptions::create_with_security_attributes_raw` with a descriptor from the SDDL `O:<SID>D:P(A;;GA;;;<SID>)`, built with `windows-sys`. No `interprocess` crate.
```

Add this item to section 10, "Limits to document":

```markdown
- `jevd` matches a peer by its process ID. An ID that the OS reuses at once after its process ends could match the wrong process, for a few milliseconds.
- `jevd` builds its HTTPS client once. A change to the proxy settings takes effect after a restart.
```

- [ ] **Step 4: Check the spec style**

Run: `python "<the simple-english-docs skill>/scripts/ste_lint.py" docs/superpowers/specs/2026-11-02-jev-daemon-design.md`. That is the `simple-english-docs` skill's linter on this machine; on another machine, use that skill's `scripts/ste_lint.py`. Fix each ERROR in the lines this task added.

- [ ] **Step 5: Commit**

```bash
git add docs/superpowers/specs/2026-11-02-jev-daemon-design.md
git commit -m "docs: record plan 3 choices in the spec" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

## Not tested by this plan

- Linux and macOS: no test ran there. Plan 6's CI runs the suite on both.
- A Windows pipe that another user creates first. A test cannot create a pipe as another user, so the client's owner check has a test only for the pipe of this user.
- A real `jev` binary as the admin peer. Plan 4 builds `jev`; its tests must send `approve` from the real binary, and from a copy in another folder.
- `clear_keys` at exit. The cache is private, and the process ends right after.
- An `ask` that runs past `IDLE_LIMIT`. A test would need a Jev answer slower than 60 s.
- Idle exit with the real binary. The `run::` tests check the timer in paused time.
- Logoff and shutdown on each OS.
- The Windows owner test passes even without `O:` in the SDDL, unless `jevd` runs elevated.
- On macOS, check once on a real Mac that `proc_pidpath` and `realpath` give the same form of a path (firmlinks). If they do not, admin messages from `jev` get `forbidden`.
- The client branch that reports a `jevd` exit after a closed connection has no deterministic test.

## Notes for plan 4

- `jev` finds the channel with `Paths::from_env()`, and `jevd` with `std::env::current_exe()`'s folder. Use `connect_or_start` for every command except `jev stop` and `jev status`, which use `Connection::connect`.
- The CLI flow: `approve` returns `request_id` and `token`. Read the old `client:<name>` entry, write the new token, then send `pair-status` with the `request_id`. If that fails, write the old entry back, or delete the new one when there was none.
- `jev pair` without a terminal prints the code and exits 3. A repeat from `jev` gets the same code while the request waits.
- `jev key remove --force`: revoke each client of the profile with `revoke`, delete the keyring entry, remove the profile from the settings, then send `reload`.
- Never print or log an `approve` or `pair-status` reply as a whole.
- Clean control and bidi characters from `error.message` before `jev` prints it (plan 2's note).
- `stop` and the other admin messages need the real `jev` next to `jevd`. A `cargo run -p jev` binary sits in `target/debug`, next to `target/debug/jevd`, so the peer check passes there.
- With two `jev allow` runs at once for one CLI name, the second approval cancels the first. When `jev` restores the old keyring entry after a failed commit, it must restore only if the keyring still holds the token that this run wrote.
- `jevd` closes a long-lived `Connection` after 60 s with no message. Reconnect, and do not send again a request that changes state.
