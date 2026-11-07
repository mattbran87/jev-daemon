# Waiting Pairing Requests Survive a Restart (0.1.1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `jevd` keeps its waiting pairing requests in `pending.json`, so a restart loses none of them. The Claude Code toast leads with `jev allow` and stays 60 seconds. `jev allow` says what to do next. The release is 0.1.1.

**Architecture:** `Pairings` keys each pending request by the SHA-256 of its request ID, and keeps a deadline `Instant`. After `load_pending`, each change to the set of waiting requests writes the whole pending file with `write_atomic`. The file holds the ID hash, the code, the name, the profile, the `cli` flag, and the deadline in Unix seconds. It never holds a token or a request ID. `Daemon::new` loads the file after the pairing file. A file that `jevd` cannot use is deleted and logged, and never stops the start.

**Tech Stack:** Rust 1.85 (MSRV), CI toolchain 1.99.1, `serde`/`serde_json`, `sha2`, `tokio`; the plugin is TypeScript, tested with `claude plugin test` from Claude Code 2.1.300.

**Spec:** `docs/superpowers/specs/2026-11-07-pairing-restart-design.md`. It changes parts of the main spec, `docs/superpowers/specs/2026-11-02-jev-daemon-design.md`.

---

## Before you start

- Work on the branch `pairing-restart`. It holds the spec commit, ba4bed4.
- The repo has `core.autocrlf=true`. Write files with LF line ends. Git stores them with LF.
- Commit as the user, and end each commit message with the trailer:

```bash
git -c user.name="Matthew Brandenburg" -c user.email="mattbran87@users.noreply.github.com" commit -m "<subject>" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

- The gates on each Rust task are, from the repo root:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
```

- The plugin tests need the `claude` CLI, version 2.1.300, as in CI: `npm install -g @anthropic-ai/claude-code@2.1.300`.

## Design notes

Read these before Task 2. They explain choices that the spec leaves open.

- **The key.** The map `Pairings::pending` uses the SHA-256 of the request ID as its key, as lowercase hex. A socket client polls with its plaintext ID, and `status` hashes it. A request loaded from the file has only the hash, so it is found the same way.
- **The ID of a request from `jev`.** `Pending::cli_id` holds the plaintext ID of a request from the `jev` binary. A repeat `start` and `approve` give it back. A socket request has `None`: only the client holds its ID. At a load, each request from `jev` gets a new random ID (spec section 4).
- **The deadline.** `Pending::since` becomes `Pending::deadline`, an `Instant`. A loaded request gets `now + <seconds left>`, so no `Instant` is ever subtracted.
- **The two clocks.** `load_pending(path, now, now_unix)` keeps `now` and `now_unix` as one moment. A write turns each deadline `Instant` into Unix seconds from that pair. The tests pass their own pair, such as `(t0, T)`.
- **No file before the load.** Until `load_pending` runs, nothing writes a pending file. `Pairings::load` alone keeps the 0.1.0 behavior, so the existing tests do not change.
- **The file shape.** The file is `{"requests": [ ... ]}`. Each entry has exactly the six fields of spec section 2. Both levels refuse unknown fields.
- **A write after the load.** A load that read the file writes it again at once. Without this, a cut deadline and a dropped entry come back at the next restart, and a clock moved back could keep a request alive for more than 10 minutes across restarts.
- **Log lines.** `load_pending` returns its problem as one line, and `Daemon::new` writes it to the log. A failed write logs from `save_pending`. Neither line holds text from the file: a parse error gives only its line and column.
- **An entry whose ID hash another entry already uses** is dropped at the load, as a repeated code is. Otherwise one would replace the other in the map.

## File map

| File | Change |
| --- | --- |
| `crates/jev-proto/src/paths.rs` | New: `Paths::pending_file`, `<state_dir>/pending.json`. |
| `crates/jevd/src/pairing.rs` | Keys by request-ID hash; `cli_id`; `deadline`; the pending file: `load_pending`, `save_pending`, writes on each change. |
| `crates/jevd/src/daemon.rs` | `Config::pending_file`; `Daemon::new` loads the pending file. |
| `crates/jev/src/bin/jevd.rs`, `crates/jev/src/link.rs`, `crates/jev/tests/jevd_client.rs`, `crates/jev-tray/tests/state.rs`, `crates/jevd/tests/serve.rs` | `pending_file` in each `Config`. |
| `crates/jev/tests/cli.rs` | `Setup::serve`; the end-to-end restart test; the new error texts. |
| `crates/jev/src/pairing.rs` | The two new `jev allow` error texts. |
| `plugins/claude-code/hooks/text.ts`, `jev.ts`, `text.test.ts`, `register.test.ts` | The new toast text; `TOAST_MS` is 60 000. |
| `docs/superpowers/specs/2026-11-02-jev-daemon-design.md`, `README.md`, `CHANGELOG.md` | Spec section 6. |
| `Cargo.toml`, `Cargo.lock`, `plugins/claude-code/.claude-plugin/plugin.json` | Version 0.1.1. |

---

### Task 1: The path of the pending file

**Files:**
- Modify: `crates/jev-proto/src/paths.rs`

- [ ] **Step 1: Write the failing test**

In `crates/jev-proto/src/paths.rs`, in the test `jev_home_moves_everything_under_it`, replace:

```rust
        assert_eq!(paths.log_file(), home.join("state").join("jevd.log"));
```

with:

```rust
        assert_eq!(paths.log_file(), home.join("state").join("jevd.log"));
        assert_eq!(
            paths.pending_file(),
            home.join("state").join("pending.json")
        );
```

- [ ] **Step 2: Run the test and see it fail**

Run: `cargo test -p jev-proto --locked --lib paths::`
Expected: FAIL to compile with `error[E0599]: no method named `pending_file` found for struct `Paths``.

- [ ] **Step 3: Add `pending_file`**

In `crates/jev-proto/src/paths.rs`, in `struct Paths`, replace:

```rust
    /// The log file.
    pub state_dir: PathBuf,
```

with:

```rust
    /// The log file, and the waiting pairing requests.
    pub state_dir: PathBuf,
```

In `impl Paths`, replace:

```rust
    pub fn log_file(&self) -> PathBuf {
        self.state_dir.join("jevd.log")
    }
```

with:

```rust
    pub fn log_file(&self) -> PathBuf {
        self.state_dir.join("jevd.log")
    }

    /// The pairing requests that wait for a person. Only `jevd` writes it.
    pub fn pending_file(&self) -> PathBuf {
        self.state_dir.join("pending.json")
    }
```

- [ ] **Step 4: Run the test and see it pass**

Run: `cargo test -p jev-proto --locked --lib paths::`
Expected: PASS, `test result: ok. 8 passed`.

- [ ] **Step 5: Commit**

```bash
git add crates/jev-proto/src/paths.rs
git -c user.name="Matthew Brandenburg" -c user.email="mattbran87@users.noreply.github.com" commit -m "feat(jev-proto): the path of the pending file" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: Key pending requests by the hash of the request ID

This task changes no behavior. It prepares `Pairings` for a request that has only the hash of its ID. The 48 existing pairing tests cover it, so this task adds no test.

**Files:**
- Modify: `crates/jevd/src/pairing.rs`

- [ ] **Step 1: See the tests pass before the change**

Run: `cargo test -p jevd --locked --lib pairing::`
Expected: PASS, `test result: ok. 48 passed`.

- [ ] **Step 2: Change `Pending`**

In `crates/jevd/src/pairing.rs`, replace:

```rust
struct Pending {
    code: String,
    name: String,
    profile: Option<String>,
    /// True when the request came from the `jev` binary.
    cli: bool,
    /// The start, or the approval once approved.
    since: Instant,
    stage: Stage,
}
```

with:

```rust
struct Pending {
    code: String,
    name: String,
    profile: Option<String>,
    /// The request ID of a request from the `jev` binary: a repeat `start`
    /// and `approve` give it back. `None` for a socket client, which alone
    /// holds its ID.
    cli_id: Option<String>,
    /// The end of the wait, or of the collection window once approved.
    deadline: Instant,
    stage: Stage,
}
```

- [ ] **Step 3: Document the key, and add `hash_request_id`**

Replace:

```rust
    revoked: BTreeSet<String>,
    pending: HashMap<String, Pending>,
}

/// The SHA-256 of a token, as lowercase hex.
pub fn hash_token(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}
```

with:

```rust
    revoked: BTreeSet<String>,
    /// By the SHA-256 of the request ID, so a request loaded from the pending
    /// file, which holds no request ID, is found the same way.
    pending: HashMap<String, Pending>,
}

/// The SHA-256 of a token, as lowercase hex.
pub fn hash_token(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

/// The SHA-256 of a request ID, as lowercase hex.
fn hash_request_id(request_id: &str) -> String {
    hex::encode(Sha256::digest(request_id.as_bytes()))
}
```

- [ ] **Step 4: Change `purge`, and rename `waiting_id` to `waiting_key`**

Replace:

```rust
    fn purge(&mut self, now: Instant) {
        self.pending
            .retain(|_, pending| now.duration_since(pending.since) < PENDING_EXPIRY);
    }

    fn waiting_id(&self, code: &str) -> Option<String> {
        let code = normalize_code(code);
        self.pending
            .iter()
            .find(|(_, pending)| pending.code == code && matches!(pending.stage, Stage::Waiting))
            .map(|(id, _)| id.clone())
    }
```

with:

```rust
    fn purge(&mut self, now: Instant) {
        self.pending.retain(|_, pending| now < pending.deadline);
    }

    /// The key of the waiting request with `code`.
    fn waiting_key(&self, code: &str) -> Option<String> {
        let code = normalize_code(code);
        self.pending
            .iter()
            .find(|(_, pending)| pending.code == code && matches!(pending.stage, Stage::Waiting))
            .map(|(key, _)| key.clone())
    }
```

- [ ] **Step 5: Change `start`**

In `start`, replace:

```rust
        if cli {
            let repeat = self.pending.iter().find(|(_, pending)| {
                pending.cli && pending.name == name && matches!(pending.stage, Stage::Waiting)
            });
            if let Some((id, pending)) = repeat {
                return Ok((id.clone(), pending.code.clone()));
            }
        }
```

with:

```rust
        if cli {
            let repeat = self
                .pending
                .values()
                .find_map(|pending| match &pending.cli_id {
                    Some(id) if pending.name == name && matches!(pending.stage, Stage::Waiting) => {
                        Some((id.clone(), pending.code.clone()))
                    }
                    _ => None,
                });
            if let Some(repeat) = repeat {
                return Ok(repeat);
            }
        }
```

Then replace:

```rust
        self.pending.insert(
            request_id.clone(),
            Pending {
                code: code.clone(),
                name: name.to_string(),
                profile: profile.map(str::to_string),
                cli,
                since: now,
                stage: Stage::Waiting,
            },
        );
```

with:

```rust
        self.pending.insert(
            hash_request_id(&request_id),
            Pending {
                code: code.clone(),
                name: name.to_string(),
                profile: profile.map(str::to_string),
                cli_id: cli.then(|| request_id.clone()),
                deadline: now + PENDING_EXPIRY,
                stage: Stage::Waiting,
            },
        );
```

- [ ] **Step 6: Change `status`**

In `status`, replace:

```rust
        self.purge(now);
        let Some(pending) = self.pending.get(request_id) else {
            return Ok(PairState::Expired);
        };
        let (profile, token_sha256, token) = match &pending.stage {
            Stage::Waiting => return Ok(PairState::Pending),
            Stage::Denied => {
                self.pending.remove(request_id);
                return Ok(PairState::Denied);
            }
            Stage::Approved { profile, .. } if !profiles.contains(profile) => {
                self.pending.remove(request_id);
                return Ok(PairState::Expired);
            }
```

with:

```rust
        self.purge(now);
        let key = hash_request_id(request_id);
        let Some(pending) = self.pending.get(&key) else {
            return Ok(PairState::Expired);
        };
        let (profile, token_sha256, token) = match &pending.stage {
            Stage::Waiting => return Ok(PairState::Pending),
            Stage::Denied => {
                self.pending.remove(&key);
                return Ok(PairState::Denied);
            }
            Stage::Approved { profile, .. } if !profiles.contains(profile) => {
                self.pending.remove(&key);
                return Ok(PairState::Expired);
            }
```

Then replace:

```rust
        self.save(clients, revoked)?;
        self.pending.remove(request_id);
        Ok(PairState::Approved(token))
```

with:

```rust
        self.save(clients, revoked)?;
        self.pending.remove(&key);
        Ok(PairState::Approved(token))
```

- [ ] **Step 7: Change `approve` and `deny`**

Replace:

```rust
        self.purge(now);
        let id = self.waiting_id(code).ok_or(ErrorCode::BadRequest)?;
        let name = self.pending[&id].name.clone();
        self.cancel_approvals(|pending| pending.name == name);
        let token = format!("{TOKEN_PREFIX}{}", random_hex(TOKEN_HEX_LENGTH / 2));
        let pending = self.pending.get_mut(&id).expect("found above");
        let cli = pending.cli;
        pending.stage = Stage::Approved {
            profile: profile.to_string(),
            token_sha256: hash_token(&token),
            token: (!cli).then(|| token.clone()),
        };
        pending.since = now;
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
```

with:

```rust
        self.purge(now);
        let key = self.waiting_key(code).ok_or(ErrorCode::BadRequest)?;
        let name = self.pending[&key].name.clone();
        self.cancel_approvals(|pending| pending.name == name);
        let token = format!("{TOKEN_PREFIX}{}", random_hex(TOKEN_HEX_LENGTH / 2));
        let pending = self.pending.get_mut(&key).expect("found above");
        let cli_id = pending.cli_id.clone();
        pending.stage = Stage::Approved {
            profile: profile.to_string(),
            token_sha256: hash_token(&token),
            token: cli_id.is_none().then(|| token.clone()),
        };
        pending.deadline = now + PENDING_EXPIRY;
        Ok(Approval {
            replaces: self.clients.contains_key(&name),
            name,
            cli: cli_id.map(|request_id| CliToken { request_id, token }),
        })
    }

    pub fn deny(&mut self, code: &str, now: Instant) -> Result<(), ErrorCode> {
        self.purge(now);
        let key = self.waiting_key(code).ok_or(ErrorCode::BadRequest)?;
        self.pending.get_mut(&key).expect("found above").stage = Stage::Denied;
        Ok(())
    }
```

- [ ] **Step 8: Run the tests and see them pass**

Run: `cargo test -p jevd --locked --lib`
Expected: PASS, `test result: ok. 247 passed; 0 failed; 1 ignored`.

Then run the two gates from "Before you start". Expected: both pass.

- [ ] **Step 9: Commit**

```bash
git add crates/jevd/src/pairing.rs
git -c user.name="Matthew Brandenburg" -c user.email="mattbran87@users.noreply.github.com" commit -m "refactor(jevd): key pending requests by the hash of the request ID" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: The pending file in `Pairings`

**Files:**
- Modify: `crates/jevd/src/pairing.rs`

- [ ] **Step 1: Write the failing tests**

In `crates/jevd/src/pairing.rs`, in the test `the_file_holds_hashes_and_survives_a_reload`, replace:

```rust
            "pending requests live in memory only"
```

with:

```rust
            "the pairing file holds no pending request"
```

At the end of `mod tests`, before its closing `}`, add:

```rust

    // The pending file (the pairing-restart spec, section 7).

    fn pending_path(dir: &tempfile::TempDir) -> PathBuf {
        dir.path().join("state").join("pending.json")
    }

    /// The store in `dir`, as jevd loads it at `now`, the Unix time `unix`.
    fn reload(dir: &tempfile::TempDir, now: Instant, unix: u64) -> Pairings {
        let mut pairings = Pairings::load(&dir.path().join("pairings.json")).unwrap();
        assert_eq!(pairings.load_pending(&pending_path(dir), now, unix), None);
        pairings
    }

    /// The codes in the pending file, sorted.
    fn codes_in_file(dir: &tempfile::TempDir) -> Vec<String> {
        let text = std::fs::read_to_string(pending_path(dir)).unwrap();
        let file: serde_json::Value = serde_json::from_str(&text).unwrap();
        let mut codes: Vec<String> = file["requests"]
            .as_array()
            .unwrap()
            .iter()
            .map(|request| request["code"].as_str().unwrap().to_string())
            .collect();
        codes.sort();
        codes
    }

    /// One socket request as the pending file holds it.
    fn saved(code: &str, name: &str, deadline: u64) -> serde_json::Value {
        serde_json::json!({
            "id_sha256": hash_request_id(code),
            "code": code,
            "name": name,
            "profile": null,
            "cli": false,
            "deadline": deadline,
        })
    }

    fn write_pending(dir: &tempfile::TempDir, requests: Vec<serde_json::Value>) {
        let path = pending_path(dir);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let text = serde_json::json!({ "requests": requests }).to_string();
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn a_waiting_request_survives_a_reload() {
        let dir = tempfile::tempdir().unwrap();
        let t0 = Instant::now();
        let mut pairings = reload(&dir, t0, T);
        let (_, code) = pairings.start("app", Some("work"), false, t0).unwrap();
        let t1 = t0 + Duration::from_secs(100);
        let mut reloaded = reload(&dir, t1, T + 100);
        assert_eq!(
            reloaded.pending_list(t1),
            [PendingView {
                code,
                name: "app".into(),
                profile: Some("work".into())
            }]
        );
        let almost = t0 + PENDING_EXPIRY - Duration::from_secs(1);
        assert_eq!(reloaded.pending_list(almost).len(), 1);
        assert!(
            reloaded.pending_list(t0 + PENDING_EXPIRY).is_empty(),
            "the deadline stays 10 minutes after the start"
        );
    }

    #[test]
    fn a_socket_client_polls_with_its_request_id_after_a_reload() {
        let dir = tempfile::tempdir().unwrap();
        let now = Instant::now();
        let mut pairings = reload(&dir, now, T);
        let (request_id, code) = pairings.start("app", None, false, now).unwrap();
        let mut reloaded = reload(&dir, now, T);
        assert_eq!(
            reloaded.status(&request_id, &known(), now, T),
            Ok(PairState::Pending)
        );
        reloaded.approve(&code, "default", now).unwrap();
        assert!(matches!(
            reloaded.status(&request_id, &known(), now, T),
            Ok(PairState::Approved(Some(_)))
        ));
    }

    #[test]
    fn a_repeat_cli_request_after_a_reload_gets_the_same_code_and_no_new_slot() {
        let dir = tempfile::tempdir().unwrap();
        let now = Instant::now();
        let mut pairings = reload(&dir, now, T);
        let (old_id, code) = pairings.start("script", None, true, now).unwrap();
        for i in 1..MAX_PENDING {
            pairings
                .start(&format!("app{i}"), None, false, now)
                .unwrap();
        }
        let mut reloaded = reload(&dir, now, T);
        let (new_id, again) = reloaded.start("script", None, true, now).unwrap();
        assert_eq!(again, code);
        assert_ne!(new_id, old_id, "jevd does not know the old ID after a load");
        assert_eq!(
            reloaded.start("script", None, true, now).unwrap(),
            (new_id, code),
            "the new ID stays"
        );
        assert_eq!(
            reloaded.start("other", None, false, now),
            Err(ErrorCode::RateLimited)
        );
    }

    #[test]
    fn jev_allow_approves_a_reloaded_cli_request_by_its_code() {
        let dir = tempfile::tempdir().unwrap();
        let now = Instant::now();
        let mut pairings = reload(&dir, now, T);
        let (_, code) = pairings.start("script", None, true, now).unwrap();
        let mut reloaded = reload(&dir, now, T);
        let approval = reloaded.approve(&code, "default", now).unwrap();
        let cli = approval.cli.expect("a CLI request gets the token");
        assert_eq!(
            reloaded.status(&cli.request_id, &known(), now, T),
            Ok(PairState::Approved(None))
        );
        assert_eq!(reloaded.authenticate(&cli.token, T).unwrap().name, "script");
    }

    #[test]
    fn an_approval_that_was_not_collected_is_gone_after_a_reload() {
        let dir = tempfile::tempdir().unwrap();
        let now = Instant::now();
        let mut pairings = reload(&dir, now, T);
        pair(&mut pairings, "other", now);
        let pairing_file = dir.path().join("pairings.json");
        let before = std::fs::read(&pairing_file).unwrap();
        let (request_id, code) = pairings.start("app", None, false, now).unwrap();
        pairings.approve(&code, "default", now).unwrap();
        let mut reloaded = reload(&dir, now, T);
        assert_eq!(
            reloaded.status(&request_id, &known(), now, T),
            Ok(PairState::Expired)
        );
        assert!(reloaded.pending_list(now).is_empty());
        assert_eq!(
            std::fs::read(&pairing_file).unwrap(),
            before,
            "the pairings file has no new entry"
        );
    }

    #[test]
    fn a_load_drops_an_expired_entry_and_cuts_a_far_deadline() {
        let dir = tempfile::tempdir().unwrap();
        write_pending(
            &dir,
            vec![
                saved("AAAAAAAA", "old", T),
                saved("BBBBBBBB", "far", T + 3600),
            ],
        );
        let t0 = Instant::now();
        let mut pairings = reload(&dir, t0, T);
        assert_eq!(
            codes_in_file(&dir),
            ["BBBBBBBB"],
            "the load writes the file"
        );
        let text = std::fs::read_to_string(pending_path(&dir)).unwrap();
        let file: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(file["requests"][0]["deadline"], T + 600);
        let almost = t0 + PENDING_EXPIRY - Duration::from_secs(1);
        assert_eq!(pairings.pending_list(almost).len(), 1);
        assert!(pairings.pending_list(t0 + PENDING_EXPIRY).is_empty());
    }

    #[test]
    fn a_load_keeps_the_16_latest_valid_entries() {
        let dir = tempfile::tempdir().unwrap();
        let code = |i: usize| (CODE_ALPHABET[i] as char).to_string().repeat(CODE_LENGTH);
        let mut requests: Vec<serde_json::Value> = (0..MAX_PENDING + 2)
            .map(|i| saved(&code(i), &format!("app{i}"), T + 100 + i as u64))
            .collect();
        requests.push(saved("XXXXXXXX", "Bad Name", T + 500));
        let mut bad_profile = saved("YYYYYYYY", "bad-profile", T + 500);
        bad_profile["profile"] = "Bad".into();
        requests.push(bad_profile);
        requests.push(saved("ABC", "short-code", T + 500));
        requests.push(saved("ABCDEFG0", "zero-in-code", T + 500));
        requests.push(saved(&code(MAX_PENDING + 1), "copy", T + 590));
        write_pending(&dir, requests);
        let now = Instant::now();
        let mut pairings = reload(&dir, now, T);
        let mut names: Vec<String> = pairings
            .pending_list(now)
            .into_iter()
            .map(|view| view.name)
            .collect();
        names.sort();
        let mut expected: Vec<String> = (2..MAX_PENDING + 2).map(|i| format!("app{i}")).collect();
        expected.sort();
        assert_eq!(names, expected);
    }

    #[test]
    fn a_file_that_jevd_cannot_use_is_deleted_and_logged() {
        let unknown_field = r#"{"requests": [{"id_sha256": "aa", "code": "AAAAAAAA", "name": "a", "profile": null, "cli": false, "deadline": 1900000000, "token": "x"}]}"#;
        let wrong_type = r#"{"requests": [{"id_sha256": "aa", "code": "AAAAAAAA", "name": "a", "profile": null, "cli": false, "deadline": "soon"}]}"#;
        let cases: [&[u8]; 5] = [
            b"not json",
            br#"{"requests": [], "extra": 1}"#,
            unknown_field.as_bytes(),
            wrong_type.as_bytes(),
            // Not UTF-8, so it cannot be read as text.
            &[0xff, 0xfe, 0x00],
        ];
        for contents in cases {
            let dir = tempfile::tempdir().unwrap();
            let path = pending_path(&dir);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, contents).unwrap();
            let now = Instant::now();
            let mut pairings = Pairings::load(&dir.path().join("pairings.json")).unwrap();
            let problem = pairings
                .load_pending(&path, now, T)
                .expect("one line for the log");
            assert!(problem.contains("pending.json"), "{problem}");
            assert!(!problem.contains('\n'), "{problem}");
            assert!(
                !problem.contains("soon"),
                "no text from the file: {problem}"
            );
            assert!(pairings.pending_list(now).is_empty());
            assert!(!path.exists(), "the file is deleted: {problem}");
            assert!(pairings.start("app", None, false, now).is_ok());
        }
    }

    #[test]
    fn the_pending_file_never_holds_a_request_id_or_a_token() {
        let dir = tempfile::tempdir().unwrap();
        let now = Instant::now();
        let mut pairings = reload(&dir, now, T);
        let (socket_id, socket_code) = pairings.start("app", None, false, now).unwrap();
        let (cli_id, cli_code) = pairings.start("script", None, true, now).unwrap();
        let text = std::fs::read_to_string(pending_path(&dir)).unwrap();
        assert!(!text.contains(&socket_id) && !text.contains(&cli_id));
        assert!(text.contains(&hash_request_id(&socket_id)));
        pairings.approve(&socket_code, "default", now).unwrap();
        let approval = pairings.approve(&cli_code, "default", now).unwrap();
        let cli = approval.cli.unwrap();
        pairings.start("waits", None, false, now).unwrap();
        let text = std::fs::read_to_string(pending_path(&dir)).unwrap();
        assert!(!text.contains(&socket_id) && !text.contains(&cli.request_id));
        assert!(!text.contains(&cli.token) && !text.contains(TOKEN_PREFIX));
    }

    #[test]
    fn a_failed_write_of_the_pending_file_fails_no_request() {
        let dir = tempfile::tempdir().unwrap();
        let now = Instant::now();
        let mut pairings = reload(&dir, now, T);
        let path = pending_path(&dir);
        // A folder that is not empty, where the file goes, so every write fails.
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(path.join("inside"), "x").unwrap();
        let (request_id, _) = pairings.start("app", None, false, now).unwrap();
        assert_eq!(
            pairings.status(&request_id, &known(), now, T),
            Ok(PairState::Pending)
        );
        std::fs::remove_dir_all(&path).unwrap();
        pairings.start("other", None, false, now).unwrap();
        let mut reloaded = reload(&dir, now, T);
        assert_eq!(
            reloaded.pending_list(now).len(),
            2,
            "the next change writes the whole file"
        );
    }

    #[test]
    fn each_change_to_the_waiting_requests_writes_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let t0 = Instant::now();
        let mut pairings = reload(&dir, t0, T);
        let (_, a) = pairings.start("a", None, false, t0).unwrap();
        let (_, b) = pairings.start("b", None, false, t0).unwrap();
        let later = t0 + Duration::from_secs(60);
        let (_, c) = pairings.start("c", None, false, later).unwrap();
        let mut all = vec![a.clone(), b.clone(), c.clone()];
        all.sort();
        assert_eq!(codes_in_file(&dir), all);
        pairings.approve(&a, "default", t0).unwrap();
        let mut left = vec![b.clone(), c.clone()];
        left.sort();
        assert_eq!(codes_in_file(&dir), left);
        pairings.deny(&b, t0).unwrap();
        assert_eq!(codes_in_file(&dir), [c]);
        pairings.pending_list(later + PENDING_EXPIRY);
        assert!(codes_in_file(&dir).is_empty(), "an expiry writes too");
    }

    #[test]
    fn a_repeat_cli_request_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let now = Instant::now();
        let mut pairings = reload(&dir, now, T);
        let first = pairings.start("script", None, true, now).unwrap();
        std::fs::remove_file(pending_path(&dir)).unwrap();
        assert_eq!(pairings.start("script", None, true, now).unwrap(), first);
        assert!(!pending_path(&dir).exists());
    }
```

These tests map to spec section 7 as follows: 1 `a_waiting_request_survives_a_reload`, 2 `a_socket_client_polls_with_its_request_id_after_a_reload`, 3 `a_repeat_cli_request_after_a_reload_gets_the_same_code_and_no_new_slot`, 4 `jev_allow_approves_a_reloaded_cli_request_by_its_code`, 5 `an_approval_that_was_not_collected_is_gone_after_a_reload`, 6 `a_load_drops_an_expired_entry_and_cuts_a_far_deadline`, 7 `a_load_keeps_the_16_latest_valid_entries`, 8 `a_file_that_jevd_cannot_use_is_deleted_and_logged`, 9 `the_pending_file_never_holds_a_request_id_or_a_token`, 10 `a_failed_write_of_the_pending_file_fails_no_request`. The last two tests check the write rules of spec section 3.

In test 8, the "one log line" is the one line that `load_pending` returns. Task 4 writes it to the log. The file that "cannot be read" holds bytes that are not UTF-8, so `read_to_string` fails, and the file can still be deleted.

- [ ] **Step 2: Run the tests and see them fail**

Run: `cargo test -p jevd --locked --lib pairing::`
Expected: FAIL to compile with `error[E0599]: no method named `load_pending` found for struct `pairing::Pairings``.

- [ ] **Step 3: Extend the module doc**

Replace:

```rust
//! Pairing (spec section 3): pending requests in memory, paired clients in a
//! JSON file that holds token hashes, never tokens. Only jevd writes the file.
//!
```

with:

```rust
//! Pairing (spec section 3): pending requests in memory, paired clients in a
//! JSON file that holds token hashes, never tokens. Only jevd writes the file.
//!
//! The requests that wait for a person also go to a pending file, so they
//! survive a restart (the pairing-restart spec). It holds no token and no
//! request ID. An approved request that is not committed stays in memory only.
//!
```

- [ ] **Step 4: Add the file types and the clock pair**

Replace:

```rust
    #[serde(default)]
    revoked: Vec<String>,
}

/// What `pair-status` reports.
```

with:

```rust
    #[serde(default)]
    revoked: Vec<String>,
}

/// The pending file: the requests that wait for a person.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PendingFile {
    #[serde(default)]
    requests: Vec<SavedRequest>,
}

/// One waiting request in the pending file.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedRequest {
    /// The SHA-256 of the request ID, as lowercase hex. Never the ID itself.
    id_sha256: String,
    code: String,
    name: String,
    profile: Option<String>,
    /// True when the request came from the `jev` binary.
    cli: bool,
    /// The end of the wait, in Unix seconds.
    deadline: u64,
}

/// Where the waiting requests go, and the two clocks at the load, so that a
/// deadline in memory can be written in Unix seconds.
struct PendingStore {
    path: PathBuf,
    loaded: Instant,
    loaded_unix: u64,
}

impl PendingStore {
    /// The Unix time of `at`.
    fn unix_at(&self, at: Instant) -> u64 {
        match at.checked_duration_since(self.loaded) {
            Some(after) => self.loaded_unix.saturating_add(after.as_secs()),
            None => self
                .loaded_unix
                .saturating_sub(self.loaded.duration_since(at).as_secs()),
        }
    }
}

/// What `pair-status` reports.
```

- [ ] **Step 5: Add the store to `Pairings`**

Replace:

```rust
    /// file, which holds no request ID, is found the same way.
    pending: HashMap<String, Pending>,
}
```

with:

```rust
    /// file, which holds no request ID, is found the same way.
    pending: HashMap<String, Pending>,
    /// `None` until `load_pending`: until then, nothing writes a pending file.
    pending_store: Option<PendingStore>,
}
```

In `Pairings::load`, replace:

```rust
            revoked: file.revoked.into_iter().collect(),
            pending: HashMap::new(),
        })
    }
```

with:

```rust
            revoked: file.revoked.into_iter().collect(),
            pending: HashMap::new(),
            pending_store: None,
        })
    }
```

- [ ] **Step 6: Add `valid_code` and `discard`**

Replace:

```rust
/// A typed code, ready to compare: spaces trimmed, letters uppercased.
pub fn normalize_code(code: &str) -> String {
    code.trim().to_ascii_uppercase()
}
```

with:

```rust
/// A typed code, ready to compare: spaces trimmed, letters uppercased.
pub fn normalize_code(code: &str) -> String {
    code.trim().to_ascii_uppercase()
}

fn valid_code(code: &str) -> bool {
    code.len() == CODE_LENGTH && code.bytes().all(|b| CODE_ALPHABET.contains(&b))
}

/// Deletes a pending file that jevd cannot use, and returns the line for the log.
fn discard(path: &Path, problem: &str) -> String {
    let _ = std::fs::remove_file(path);
    format!(
        "cannot use {}: {problem}; jevd deletes it and starts with no waiting pairing requests",
        path.display()
    )
}
```

- [ ] **Step 7: Add `load_pending`, `save_pending`, and `waiting_count`**

In `impl Pairings`, directly after the end of `fn load` (the lines `pending_store: None,`, `})`, `}` from Step 5), add:

```rust

    /// Reads the pending file, once, at start, and writes the waiting
    /// requests to it from now on. `now` and `now_unix` are the same moment
    /// on the two clocks. A file that jevd cannot use never stops it: the
    /// file is deleted, and the returned line goes to the log.
    pub fn load_pending(&mut self, path: &Path, now: Instant, now_unix: u64) -> Option<String> {
        self.pending_store = Some(PendingStore {
            path: path.to_path_buf(),
            loaded: now,
            loaded_unix: now_unix,
        });
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
            Err(error) => return Some(discard(path, &error.to_string())),
        };
        let file: PendingFile = match serde_json::from_str(&text) {
            Ok(file) => file,
            // The place only: serde's own text can quote the file.
            Err(error) => {
                let problem = format!(
                    "not a pending file (line {}, column {})",
                    error.line(),
                    error.column()
                );
                return Some(discard(path, &problem));
            }
        };
        let mut codes = BTreeSet::new();
        // Each entry with the seconds it has left, at most 10 minutes, so a
        // clock moved back cannot keep a request alive longer.
        let mut kept: Vec<(u64, SavedRequest)> = Vec::new();
        for saved in file.requests {
            if saved.deadline <= now_unix {
                continue;
            }
            let valid = valid_name(&saved.name)
                && saved.profile.as_deref().is_none_or(valid_name)
                && valid_code(&saved.code);
            if !valid || !codes.insert(saved.code.clone()) {
                continue;
            }
            let left = (saved.deadline - now_unix).min(PENDING_EXPIRY.as_secs());
            kept.push((left, saved));
        }
        // The latest deadlines stay, and the earliest go first.
        kept.sort_by_key(|(left, _)| std::cmp::Reverse(*left));
        kept.truncate(MAX_PENDING);
        for (left, saved) in kept {
            // jevd does not know the ID of a request from `jev` any more, and
            // `jev` never polls with it, so it gets a new one.
            let cli_id = saved.cli.then(|| random_hex(REQUEST_ID_HEX_LENGTH / 2));
            let key = cli_id.as_deref().map_or(saved.id_sha256, hash_request_id);
            if self.pending.contains_key(&key) {
                continue;
            }
            self.pending.insert(
                key,
                Pending {
                    code: saved.code,
                    name: saved.name,
                    profile: saved.profile,
                    cli_id,
                    deadline: now + Duration::from_secs(left),
                    stage: Stage::Waiting,
                },
            );
        }
        // Write at once, so a dropped entry and a cut deadline stay so at the
        // next start.
        self.save_pending();
        None
    }

    /// Writes the waiting requests to the pending file, after a load. A
    /// failed write fails nothing: it goes to the log, and the next change
    /// writes the whole file again.
    fn save_pending(&self) {
        let Some(store) = &self.pending_store else {
            return;
        };
        let mut requests: Vec<SavedRequest> = self
            .pending
            .iter()
            .filter(|(_, pending)| matches!(pending.stage, Stage::Waiting))
            .map(|(key, pending)| SavedRequest {
                id_sha256: key.clone(),
                code: pending.code.clone(),
                name: pending.name.clone(),
                profile: pending.profile.clone(),
                cli: pending.cli_id.is_some(),
                deadline: store.unix_at(pending.deadline),
            })
            .collect();
        requests.sort_by(|a, b| a.code.cmp(&b.code));
        let written = serde_json::to_string_pretty(&PendingFile { requests })
            .map_err(|error| error.to_string())
            .and_then(|text| write_atomic(&store.path, &text).map_err(|error| error.to_string()));
        if let Err(problem) = written {
            crate::log::event(&format!("cannot save {}: {problem}", store.path.display()));
        }
    }

    fn waiting_count(&self) -> usize {
        self.pending
            .values()
            .filter(|pending| matches!(pending.stage, Stage::Waiting))
            .count()
    }
```

- [ ] **Step 8: Write on each change**

Replace:

```rust
    fn purge(&mut self, now: Instant) {
        self.pending.retain(|_, pending| now < pending.deadline);
    }
```

with:

```rust
    /// Drops expired requests. Writes the pending file when a waiting
    /// request was among them.
    fn purge(&mut self, now: Instant) {
        let waiting = self.waiting_count();
        self.pending.retain(|_, pending| now < pending.deadline);
        if self.waiting_count() != waiting {
            self.save_pending();
        }
    }
```

In `start`, replace:

```rust
                deadline: now + PENDING_EXPIRY,
                stage: Stage::Waiting,
            },
        );
        Ok((request_id, code))
```

with:

```rust
                deadline: now + PENDING_EXPIRY,
                stage: Stage::Waiting,
            },
        );
        self.save_pending();
        Ok((request_id, code))
```

The repeat of a request from `jev` returns before this line, so it writes nothing.

In `approve`, replace:

```rust
        pending.deadline = now + PENDING_EXPIRY;
        Ok(Approval {
```

with:

```rust
        pending.deadline = now + PENDING_EXPIRY;
        // The request stops waiting. Its token never reaches the file.
        self.save_pending();
        Ok(Approval {
```

In `deny`, replace:

```rust
        self.pending.get_mut(&key).expect("found above").stage = Stage::Denied;
        Ok(())
```

with:

```rust
        self.pending.get_mut(&key).expect("found above").stage = Stage::Denied;
        self.save_pending();
        Ok(())
```

- [ ] **Step 9: Run the tests and see them pass**

Run: `cargo test -p jevd --locked --lib pairing::`
Expected: PASS, `test result: ok. 60 passed`.

Run: `cargo test -p jevd --locked --lib`
Expected: PASS, `test result: ok. 259 passed; 0 failed; 1 ignored`.

Then run the two gates from "Before you start". Expected: both pass. `Option::is_none_or` needs Rust 1.82, so the MSRV of 1.85 holds.

- [ ] **Step 10: Commit**

```bash
git add crates/jevd/src/pairing.rs
git -c user.name="Matthew Brandenburg" -c user.email="mattbran87@users.noreply.github.com" commit -m "feat(jevd): save waiting pairing requests to a pending file" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: `jevd` loads the pending file at start

**Files:**
- Modify: `crates/jevd/src/daemon.rs`
- Modify: `crates/jev/src/bin/jevd.rs`, `crates/jev/src/link.rs`, `crates/jev/tests/jevd_client.rs`, `crates/jev-tray/tests/state.rs`, `crates/jevd/tests/serve.rs`
- Test: `crates/jevd/src/daemon.rs`, `crates/jev/tests/cli.rs`

- [ ] **Step 1: Add `pending_file` to `Config`**

In `crates/jevd/src/daemon.rs`, replace:

```rust
    pub pairing_file: PathBuf,
    pub store: Arc<dyn KeyStore>,
```

with:

```rust
    pub pairing_file: PathBuf,
    /// The waiting pairing requests, so they survive a restart.
    pub pending_file: PathBuf,
    pub store: Arc<dyn KeyStore>,
```

In `crates/jev/src/bin/jevd.rs`, `crates/jev/tests/jevd_client.rs`, `crates/jev-tray/tests/state.rs`, and `crates/jevd/tests/serve.rs`, replace:

```rust
        pairing_file: paths.pairing_file(),
```

with:

```rust
        pairing_file: paths.pairing_file(),
        pending_file: paths.pending_file(),
```

In `crates/jev/src/link.rs`, in the test helper `start_daemon`, replace:

```rust
            pairing_file: paths.pairing_file(),
```

with:

```rust
            pairing_file: paths.pairing_file(),
            pending_file: paths.pending_file(),
```

In `crates/jev/tests/cli.rs`, in `running`, replace:

```rust
        pairing_file: setup.paths.pairing_file(),
```

with:

```rust
        pairing_file: setup.paths.pairing_file(),
        pending_file: setup.paths.pending_file(),
```

In `crates/jevd/src/daemon.rs`, in `mod tests`, replace:

```rust
        pub(crate) settings_file: PathBuf,
        pub(crate) pairing_file: PathBuf,
    }
```

with:

```rust
        pub(crate) settings_file: PathBuf,
        pub(crate) pairing_file: PathBuf,
        pub(crate) pending_file: PathBuf,
    }
```

In `fixture_with`, replace:

```rust
        let pairing_file = dir.path().join("pairings.json");
        let mut settings = Settings::default();
```

with:

```rust
        let pairing_file = dir.path().join("pairings.json");
        let pending_file = dir.path().join("pending.json");
        let mut settings = Settings::default();
```

Then replace:

```rust
            pairing_file: pairing_file.clone(),
            store,
```

with:

```rust
            pairing_file: pairing_file.clone(),
            pending_file: pending_file.clone(),
            store,
```

and replace:

```rust
            settings_file,
            pairing_file,
        }
    }
```

with:

```rust
            settings_file,
            pairing_file,
            pending_file,
        }
    }
```

Run: `cargo check --workspace --all-targets --locked`
Expected: it compiles, with one warning: `field `pending_file` is never read`. The tests of Step 2 read it.

- [ ] **Step 2: Write the failing daemon tests**

In `crates/jevd/src/daemon.rs`, in `mod tests`, before the test `a_failed_save_at_collection_is_internal_and_a_later_poll_works`, add:

```rust
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
```

- [ ] **Step 3: Write the failing end-to-end test**

The test runs `jevd` in the test process, on its real channel, as the other tests in `crates/jev/tests/cli.rs` do. The real `jevd` binary cannot serve here: `jev allow` needs a terminal, and only the in-process `jev` can take a scripted one.

In `crates/jev/tests/cli.rs`, replace:

```rust
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
        pending_file: setup.paths.pending_file(),
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
```

with:

```rust
/// A home with a daemon running in this process.
async fn running() -> Setup {
    let mut setup = setup().await;
    setup.serve();
    setup
}

impl Setup {
    /// Starts a daemon in this process, on the files of this home.
    fn serve(&mut self) {
        let jev = JevClient::with_endpoint(
            &self.jev_url,
            Duration::from_secs(5),
            Duration::from_millis(10),
        )
        .unwrap();
        let daemon = Daemon::new(Config {
            settings_file: self.paths.settings_file(),
            pairing_file: self.paths.pairing_file(),
            pending_file: self.paths.pending_file(),
            store: self.store.clone(),
            jev,
        })
        .unwrap();
        let listener = bind(&self.paths).unwrap();
        let admin = AdminCheck::new(&std::env::current_exe().unwrap());
        // run() installs signal handlers in this test process; every wait here has a limit.
        self.daemon = Some(tokio::spawn(jevd::run::run(daemon, listener, admin)));
    }

```

Before the test `key_add_from_stdin_stores_the_key_and_reloads_the_daemon`, add:

```rust
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_waiting_request_survives_a_jevd_restart() {
    let mut setup = running().await;
    setup.add_key("default").await;
    let pending = setup
        .quiet(&["pair", "--name", "script", "--no-prompt", "--json"])
        .await;
    assert_eq!(pending.code, 3, "{}", pending.out);
    let code = pending.json()["pair_code"].as_str().unwrap().to_string();
    let stop = setup.quiet(&["stop"]).await;
    assert_eq!(stop.code, 0, "{}", stop.err);
    let task = setup.daemon.take().unwrap();
    tokio::time::timeout(Duration::from_secs(10), task)
        .await
        .expect("jevd stopped")
        .unwrap();
    setup.serve();
    let allow = setup
        .jev(
            &["allow", &code],
            ScriptedTerminal::answering(&["", "y"]),
            Some(""),
        )
        .await;
    assert_eq!(allow.code, 0, "{} {:?}", allow.err, allow.shown);
    assert!(setup.token("script").is_some(), "jev wrote the token");
    let clients = setup.quiet(&["clients", "--json"]).await;
    assert_eq!(clients.json()["clients"][0]["name"], "script");
}

```

- [ ] **Step 4: Run the tests and see them fail**

Run: `cargo test -p jevd --locked --lib daemon::`
Expected: FAIL. `a_waiting_socket_request_survives_a_restart` fails with `assertion `left == right` failed` (the state is `expired`). `a_pending_file_that_cannot_be_used_does_not_stop_the_start` fails: jevd does not delete the file yet.

Run: `cargo test -p jev --locked --test cli a_waiting_request_survives_a_jevd_restart`
Expected: FAIL with `jev: bad-request: no waiting request has that code`, `left: 9`, `right: 0`.

- [ ] **Step 5: Load the pending file in `Daemon::new`**

In `crates/jevd/src/daemon.rs`, replace:

```rust
    /// Reads the settings file and the pairing file. Reads no key.
    pub fn new(config: Config) -> Result<Arc<Daemon>, String> {
        let settings = Settings::load(&config.settings_file)?;
        let pairings = Pairings::load(&config.pairing_file)?;
        let keys = KeyCache::new(settings.key_cache);
        let now = Instant::now();
```

with:

```rust
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
```

The `started: now,` line further down stays as it is.

In the module doc, replace:

```rust
//! with an fsync. For a per-user daemon that is a short, rare wait, and it
//! keeps memory and disk in one order.
```

with:

```rust
//! with an fsync. For a per-user daemon that is a short, rare wait, and it
//! keeps memory and disk in one order. The pending file is too, at each
//! change to the waiting pairing requests.
```

- [ ] **Step 6: Run the tests and see them pass**

Run: `cargo test -p jevd --locked --lib daemon::`
Expected: PASS, `test result: ok. 14 passed`.

Run: `cargo test -p jev --locked --test cli`
Expected: PASS, `test result: ok. 32 passed`.

Then run the two gates from "Before you start", and `cargo test --workspace --locked`. Expected: all pass.

- [ ] **Step 7: Commit**

```bash
git add crates/jevd/src/daemon.rs crates/jev/src/bin/jevd.rs crates/jev/src/link.rs crates/jev/tests/cli.rs crates/jev/tests/jevd_client.rs crates/jev-tray/tests/state.rs crates/jevd/tests/serve.rs
git -c user.name="Matthew Brandenburg" -c user.email="mattbran87@users.noreply.github.com" commit -m "feat(jevd): waiting pairing requests survive a restart" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: `jev allow` says what to do next

**Files:**
- Modify: `crates/jev/src/pairing.rs`
- Test: `crates/jev/tests/cli.rs`

- [ ] **Step 1: Write the failing test**

In `crates/jev/tests/cli.rs`, in the test `allow_refuses_no_request_a_wrong_code_and_an_unknown_profile`, replace:

```rust
    assert!(
        none.err.contains("no pairing request waits"),
        "{}",
        none.err
    );
```

with:

```rust
    assert!(
        none.err.contains(
            "no pairing request waits; a request lasts 10 minutes, so ask the client to try again"
        ),
        "{}",
        none.err
    );
```

and replace:

```rust
    assert!(wrong.err.contains("no waiting request"), "{}", wrong.err);
```

with:

```rust
    assert!(
        wrong.err.contains(
            "no waiting request has that code; run jev allow with no code to see the waiting requests"
        ),
        "{}",
        wrong.err
    );
```

The exit code checks (`9`) stay. The exit codes do not change.

- [ ] **Step 2: Run the test and see it fail**

Run: `cargo test -p jev --locked --test cli allow_refuses`
Expected: FAIL with `jev: bad-request: no pairing request waits`.

- [ ] **Step 3: Change the texts**

In `crates/jev/src/pairing.rs`, replace:

```rust
/// `jev allow [<code>]`: show the waiting requests, and approve one.
```

with:

```rust
/// `jev allow` with no code, when no request waits. Each error says what to do next.
const NONE_WAITS: &str =
    "no pairing request waits; a request lasts 10 minutes, so ask the client to try again";
/// `jev allow <code>`, when no waiting request has the code.
const NO_SUCH_CODE: &str =
    "no waiting request has that code; run jev allow with no code to see the waiting requests";

/// `jev allow [<code>]`: show the waiting requests, and approve one.
```

Replace:

```rust
                return Err(Failure::new(
                    ErrorCode::BadRequest,
                    "no pairing request waits",
                ));
```

with:

```rust
                return Err(Failure::new(ErrorCode::BadRequest, NONE_WAITS));
```

Replace:

```rust
        .ok_or_else(|| Failure::new(ErrorCode::BadRequest, "no waiting request has that code"))?;
```

with:

```rust
        .ok_or_else(|| Failure::new(ErrorCode::BadRequest, NO_SUCH_CODE))?;
```

The daemon's own `approve` and `deny` replies keep the text `no waiting request has that code`. The spec changes only the `jev allow` errors.

- [ ] **Step 4: Run the test and see it pass**

Run: `cargo test -p jev --locked --test cli allow_refuses`
Expected: PASS, `test result: ok. 1 passed`.

Then run the two gates from "Before you start". Expected: both pass.

- [ ] **Step 5: Commit**

```bash
git add crates/jev/src/pairing.rs crates/jev/tests/cli.rs
git -c user.name="Matthew Brandenburg" -c user.email="mattbran87@users.noreply.github.com" commit -m "feat(jev): jev allow errors say what to do next" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: The pairing toast leads with `jev allow` and stays 60 seconds

The host's `ui.toast` event carries `{ text, timeoutMs }`, so a test hook can read both.

**Files:**
- Modify: `plugins/claude-code/hooks/text.ts`, `plugins/claude-code/hooks/jev.ts`
- Test: `plugins/claude-code/hooks/text.test.ts`, `plugins/claude-code/hooks/register.test.ts`

- [ ] **Step 1: Write the failing tests**

In `plugins/claude-code/hooks/text.test.ts`, replace:

```ts
test('each toast holds its argument', () => {
  expect(pendingToast('acme', 'ABCD2345')).toContain('jev allow ABCD2345')
```

with:

```ts
test('the pending toast leads with jev allow, and the code comes last', () => {
  expect(pendingToast('acme', 'ABCD2345')).toBe('jev: the plugin acme asks to use Jev. In a terminal, run jev allow and approve code ABCD2345.')
})

test('each toast holds its argument', () => {
  expect(pendingToast('acme', 'ABCD2345')).toContain('approve code ABCD2345')
```

In `plugins/claude-code/hooks/register.test.ts`, in `engine`, replace:

```ts
  const runs: Run[] = []
  const toasts: string[] = []
  on('ui.toast', (_$, e) => {
    toasts.push(String((e as unknown as { text?: unknown }).text ?? e))
    return ok(undefined)
  })
```

with:

```ts
  const runs: Run[] = []
  const toasts: string[] = []
  const toastTimeouts: unknown[] = []
  on('ui.toast', (_$, e) => {
    toasts.push(String((e as unknown as { text?: unknown }).text ?? e))
    toastTimeouts.push((e as unknown as { timeoutMs?: unknown }).timeoutMs)
    return ok(undefined)
  })
```

and replace:

```ts
    return done(world.jev?.(args, init?.stdin) ?? line({ ok: true }))
  })
  return { runs, toasts }
}
```

with:

```ts
    return done(world.jev?.(args, init?.stdin) ?? line({ ok: true }))
  })
  return { runs, toasts, toastTimeouts }
}
```

In the test `not-paired starts a pairing; the toast shows once per code`, replace:

```ts
  expect(seen.toasts.length).toBe(1)
  expect(seen.toasts[0]?.includes('jev allow ABCD2345')).toBe(true)
})
```

with:

```ts
  expect(seen.toasts).toEqual(['jev: the plugin acme-review asks to use Jev. In a terminal, run jev allow and approve code ABCD2345.'])
  expect(seen.toastTimeouts).toEqual([60_000])
})
```

- [ ] **Step 2: Run the tests and see them fail**

Run: `claude plugin test plugins/claude-code`
Expected: FAIL, `36 pass`, `3 fail`. The failures are `not-paired starts a pairing; the toast shows once per code`, `the pending toast leads with jev allow, and the code comes last`, and `each toast holds its argument`. Each receives `jev: the plugin ... asks to use Jev. Run jev allow ABCD2345 in a terminal to approve it.`

- [ ] **Step 3: Change the toast and its timeout**

In `plugins/claude-code/hooks/text.ts`, replace:

```ts
// Only a `pairCode` that `readReply` checked may go in.
export function pendingToast(plugin: string, code: string): string {
  return `jev: the plugin ${plugin} asks to use Jev. Run jev allow ${code} in a terminal to approve it.`
}
```

with:

```ts
// Only a `pairCode` that `readReply` checked may go in. The code comes last,
// so the person can match it to a line of the `jev allow` list.
export function pendingToast(plugin: string, code: string): string {
  return `jev: the plugin ${plugin} asks to use Jev. In a terminal, run jev allow and approve code ${code}.`
}
```

In `plugins/claude-code/hooks/jev.ts`, replace:

```ts
const TOAST_MS = 15_000
```

with:

```ts
/** The host's toast has only a timeout, so it cannot stay until someone closes it. */
const TOAST_MS = 60_000
```

Every toast of the plugin uses `TOAST_MS`, so each now stays 60 seconds. The toast with no code does not change.

- [ ] **Step 4: Run the tests and see them pass**

Run: `claude plugin test plugins/claude-code`
Expected: PASS, `39 pass`, `0 fail`.

- [ ] **Step 5: Commit**

```bash
git add plugins/claude-code/hooks/text.ts plugins/claude-code/hooks/jev.ts plugins/claude-code/hooks/text.test.ts plugins/claude-code/hooks/register.test.ts
git -c user.name="Matthew Brandenburg" -c user.email="mattbran87@users.noreply.github.com" commit -m "feat(plugin): the pairing toast leads with jev allow and stays 60 seconds" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 7: Documents, version 0.1.1, gates, and the pull request

**Files:**
- Modify: `docs/superpowers/specs/2026-11-02-jev-daemon-design.md`
- Modify: `README.md`, `CHANGELOG.md`
- Modify: `Cargo.toml`, `Cargo.lock`, `plugins/claude-code/.claude-plugin/plugin.json`

- [ ] **Step 1: The main spec**

Make the 3 changes of spec section 6 in `docs/superpowers/specs/2026-11-02-jev-daemon-design.md`.

1. In section 1, in the Storage table, after the Pairings row:

```markdown
| Pairings | JSON file in the platform configuration directory. Written only by `jevd`. Holds token hashes, never tokens. |
```

add the row:

```markdown
| Waiting pairing requests | `pending.json` in the platform state directory. Written only by `jevd`. Holds no token and no request ID. See `2026-11-07-pairing-restart-design.md`. |
```

2. In section 3, in the Values table, replace:

```markdown
| Pending expiry | 10 minutes. Pending requests live in memory only. |
```

with:

```markdown
| Pending expiry | 10 minutes. Waiting requests are also saved to `pending.json`, and survive a restart. Approved requests that are not collected live in memory only. |
```

3. In section 7, in the Toasts table, replace:

```markdown
| `pending` | The code, and "Run `jev allow` in a terminal". With no valid code, only "Run `jev allow` in a terminal". | Once per code |
```

with:

```markdown
| `pending` | "In a terminal, run `jev allow` and approve code `<CODE>`": the action first, the code last. With no valid code, only "Run `jev allow` in a terminal". It stays 60 seconds, as every toast does. | Once per code |
```

- [ ] **Step 2: The README**

In `README.md`, replace:

```markdown
**Status:** 0.1.0, the first release. See [CHANGELOG.md](CHANGELOG.md).
```

with:

```markdown
**Status:** 0.1.1. See [CHANGELOG.md](CHANGELOG.md).
```

Then, after the `## Build from source` section and before `## jev-tray`, add:

````markdown
## Pair a client

A program must pair once before it can ask Jev. A Claude Code plugin pairs as the client `cc-<plugin>`.

1. The client asks to pair. The Claude Code plugin shows a toast: "In a terminal, run jev allow and approve code `<CODE>`."
2. In a terminal, run `jev allow`. It lists each waiting request: its code, and the name of the client.
3. Type the code from the list. Then pick a profile, and type `y`.

A request waits 10 minutes, also across a restart of `jevd`. When no request waits, `jev allow` says so: ask the client to try again.

````

The section ends with one blank line before `## jev-tray`.

- [ ] **Step 3: The CHANGELOG**

In `CHANGELOG.md`, replace:

```markdown
# Changelog

## 0.1.0
```

with:

```markdown
# Changelog

## 0.1.1

- Waiting pairing requests survive a `jevd` restart ([#5](https://github.com/mattbran87/jev-daemon/issues/5)). `jevd` saves them to `pending.json` in the jev state folder. The file holds no token and no request ID. A request still lasts 10 minutes.
- The pairing toast of the Claude Code plugin leads with `jev allow`, and it ends with the code. It stays 60 seconds, up from 15 ([#6](https://github.com/mattbran87/jev-daemon/issues/6)). Each toast of the plugin now stays 60 seconds.
- `jev allow` says what to do next when no request waits, and when no waiting request has the code.

Known limits:

- Nobody has checked `jev-tray` by hand on macOS or on a Linux desktop yet. CI builds and tests both systems, but it has no real tray.
- The macOS binaries are not signed or notarized.
- **Stop jevd** does not last: a plugin's next request starts `jevd` again.
- `jev ask --score` has no way to give `criteria`. The plugin's types list `criteria` as needed on a score question. To ask a score question, give the whole request as JSON on stdin or with `--request`.

## 0.1.0
```

The 0.1.0 entry stays as it is. It records that release.

- [ ] **Step 4: The version**

In `Cargo.toml`, under `[workspace.package]`, replace:

```toml
version = "0.1.0"
```

with:

```toml
version = "0.1.1"
```

In `Cargo.lock`, change the version of the 4 workspace crates, and nothing else. For each `name` below, the next line `version = "0.1.0"` becomes `version = "0.1.1"`:

```toml
name = "jev"
version = "0.1.1"
```

```toml
name = "jev-proto"
version = "0.1.1"
```

```toml
name = "jev-tray"
version = "0.1.1"
```

```toml
name = "jevd"
version = "0.1.1"
```

In `plugins/claude-code/.claude-plugin/plugin.json`, replace:

```json
  "version": "0.1.0",
```

with:

```json
  "version": "0.1.1",
```

The path dependencies in the crate manifests say `version = "0.1.0"`. That requirement accepts 0.1.1, so they stay. `MIN_JEV` in `plugins/claude-code/hooks/text.ts` stays `[0, 1, 0]`: the plugin needs no newer `jev`.

Run: `cargo test -p jev --locked --test versions`
Expected: PASS, `test result: ok. 3 passed`. A wrong `Cargo.lock` fails here with `the lock file ... needs to be updated but --locked was passed`.

- [ ] **Step 5: All gates**

From the repo root:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

Expected: all pass. On Windows, `cargo test --workspace --locked` reports 449 tests: 448 passed and 1 ignored.

MSRV, with a separate target folder:

```bash
CARGO_TARGET_DIR=target/msrv cargo +1.85 check --workspace --locked
CARGO_TARGET_DIR=target/msrv cargo +1.85 check -p jev --no-default-features --locked
```

Expected: both pass.

The plugin, as CI runs it:

```bash
claude plugin validate --strict .
claude plugin validate --strict plugins/claude-code
claude plugin test plugins/claude-code
```

Expected: `Validation passed` twice, then `39 pass`, `0 fail`.

- [ ] **Step 6: Commit**

```bash
git add docs/superpowers/specs/2026-11-02-jev-daemon-design.md README.md CHANGELOG.md Cargo.toml Cargo.lock plugins/claude-code/.claude-plugin/plugin.json
git -c user.name="Matthew Brandenburg" -c user.email="mattbran87@users.noreply.github.com" commit -m "docs: 0.1.1, waiting pairing requests survive a restart" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

- [ ] **Step 7: Check by hand, with the person**

1. Install the branch: `jev stop`, then `cargo install --locked --path crates/jev --force`.
2. Run `jev pair --name hand-check --no-prompt --json`. Note the `pair_code`.
3. Open `pending.json` in the jev state folder (on Windows, `%LOCALAPPDATA%\jev`). It holds the code from step 2 and an `id_sha256`. It holds no `jevt_` text.
4. Run `jev stop`. Then run `jev start`.
5. Run `jev allow`. The list shows the code from step 2. Approve it on a profile.
6. Run `jev clients`. It lists `hand-check`. Run `jev revoke hand-check`.

- [ ] **Step 8: Push and open the pull request, after the person approves**

Do not push before the person approves. Then push the branch `pairing-restart` and open a pull request into `main`. The description names issues #5 and #6, and ends with:

```markdown
🤖 Generated with [Claude Code](https://claude.com/claude-code)
```

The tag `v0.1.1` is not a step of this plan. It follows the merge, and only after the person approves the release.

---

## Self-review against the spec

| Spec | Where |
| --- | --- |
| 2. The file, its fields, no token or ID, approved requests in memory | Task 3, Steps 4 and 7; test 9 |
| 3. Writes: new request, approval, denial, expiry; no write for a repeat | Task 3, Step 8; `each_change_to_the_waiting_requests_writes_the_file`, `a_repeat_cli_request_writes_nothing` |
| 3. A failed write fails nothing, one log line | Task 3, Step 7 (`save_pending`); test 10 |
| 3. Reads: expiry, 10-minute cap, 16 entries, bad entries, repeated codes | Task 3, Step 7 (`load_pending`); tests 6 and 7 |
| 3. A file that cannot be used: one log line, deleted, start goes on | Task 3, Step 7; Task 4, Step 5; test 8; `a_pending_file_that_cannot_be_used_does_not_stop_the_start` |
| 4. Socket client, `jev` request with a new ID, repeat `jev pair`, `jev allow` by code | Task 2; Task 3 tests 2, 3, 4; Task 4 |
| 5. The toast text, 60 seconds | Task 6 |
| 5. The two `jev allow` errors, same exit codes | Task 5 |
| 6. Main spec, README, CHANGELOG | Task 7, Steps 1 to 3 |
| 7. End to end in `crates/jev/tests` | Task 4, `a_waiting_request_survives_a_jevd_restart` |
| 8. Version 0.1.1; tag after the person approves | Task 7, Steps 4 and 8 |
