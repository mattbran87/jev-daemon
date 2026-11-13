# The API key in an object key Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** jevd refuses an `ask` with `bad-request` when an object key in `state` or `questions` contains the API key, so the key never reaches Jev in an object key.

**Architecture:** One new check in `Daemon::ask` (`crates/jevd/src/ask.rs`), after the key read and before the rate limit. It reuses `names_key` from `crates/jevd/src/jev.rs`. The design spec's section 4 gets a new step 4, and the later steps move down by 1.

**Tech Stack:** Rust, `cargo test`, `wiremock`.

**Spec:** `docs/superpowers/specs/2026-11-13-jevd-key-in-object-keys-design.md`

**Branch:** `fix/jevd-key-in-object-keys` (the spec commit is on it).

**Commit identity for every commit:**

```bash
git -c user.name="Matthew Brandenburg" -c user.email="mattbran87@users.noreply.github.com" commit -m "<subject>" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

**Line ends:** `core.autocrlf=true`. Before and after each edit, run `git ls-files --eol <file>`. `git diff --stat` must not show a whole-file rewrite.

**Facts checked for this plan:**
- The test key is `KEY = "tsk-test-key-0123456789"` (`crates/jevd/src/daemon.rs`, in `tests`).
- The redaction marker is `REDACTED = "[redacted]"` (`crates/jevd/src/redact.rs`).
- The mock in `jev()` in the `ask.rs` tests answers only a POST with `authorization: Bearer {KEY}`. `server.received_requests().await.unwrap()` lists the requests that reached it.
- `WireError` has a `message: String` field.
- `names_key` (`crates/jevd/src/jev.rs` line 133) is private now. Its doc says that it runs only after the depth check.

---

### Task 1: The check in `ask`

**Files:**
- Modify: `crates/jevd/src/jev.rs` (`names_key` near line 130)
- Modify: `crates/jevd/src/ask.rs` (`Daemon::ask` and the `tests` module)

- [ ] **Step 1: Write the failing tests**

At the end of the `tests` module in `crates/jevd/src/ask.rs`, add:

```rust
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
        let sent = String::from_utf8(requests[0].body.clone()).unwrap();
        assert!(!sent.contains(KEY), "{sent}");
        assert!(sent.contains("[redacted]"), "{sent}");
    }
```

If `json!` does not accept the `KEY` constant as an object key, build the `criteria` map with `serde_json::Map` and `insert(KEY.to_string(), ...)`.

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test -p jevd --locked --lib ask::tests`

Expected:
- `the_key_in_a_state_object_key_is_bad_request` and `the_key_in_an_option_name_is_bad_request` fail: the reply is ok, not `bad-request`.
- `the_key_in_a_string_value_still_goes_out_redacted` passes. It guards the existing redaction.

- [ ] **Step 3: Make `names_key` visible to `ask`**

In `crates/jevd/src/jev.rs`, change:

```rust
/// True when an object key, at any level, contains the key. Such an answer is
/// refused rather than renamed, since renamed keys could collide. Called only
/// after the depth check, so the recursion is bounded.
fn names_key(value: &Value, key: &str) -> bool {
```

to:

```rust
/// True when an object key, at any level, contains the key. Such a request or
/// answer is refused rather than renamed, since renamed keys could collide.
/// Called only after the depth check, so the recursion is bounded.
pub(crate) fn names_key(value: &Value, key: &str) -> bool {
```

- [ ] **Step 4: Write the check**

In `crates/jevd/src/ask.rs`, change the import:

```rust
use crate::daemon::{failure, unix_now, Daemon};
```

to:

```rust
use crate::daemon::{failure, unix_now, Daemon};
use crate::jev::names_key;
```

Then, right after the block that ends with:

```rust
        let key = match self.key(&client.profile).await {
            Ok(key) => key,
            Err(error) => return Reply::failure(error.for_profile(&client.profile)),
        };
```

add:

```rust
        // Redaction leaves object keys as they are, so a request that names
        // the key in one never goes out. Like a bad key, it uses no slot.
        if names_key(state, &key) || names_key(questions, &key) {
            return failure(
                ErrorCode::BadRequest,
                "an object key in state or questions contains the API key",
            );
        }
```

If `failure` needs a `String`, add `.to_string()`. If `&key` does not coerce from `Zeroizing<String>` to `&str`, use `key.as_str()`.

Change the module comment at the top of `ask.rs` from:

```rust
//! One `ask` (spec section 4): shape, token, key, rate limit, then the Jev
//! call, which redacts, sends, and checks the answer.
```

to:

```rust
//! One `ask` (spec section 4): shape, token, key, the key in object keys,
//! rate limit, then the Jev call, which redacts, sends, and checks the answer.
```

- [ ] **Step 5: Run the tests**

Run: `cargo test -p jevd --locked --lib`
Expected: all pass.

- [ ] **Step 6: Format, lint, and commit**

Run: `cargo fmt --all` and `cargo clippy -p jevd --all-targets --locked -- -D warnings`
Expected: no warnings.

```bash
git add crates/jevd/src/jev.rs crates/jevd/src/ask.rs
git -c user.name="Matthew Brandenburg" -c user.email="mattbran87@users.noreply.github.com" commit -m "fix(jevd): refuse an ask with the API key in an object key (#18)" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: The design spec and the step numbers

**Files:**
- Modify: `docs/superpowers/specs/2026-11-02-jev-daemon-design.md` (section 4, near lines 198-212)
- Modify: the first comment lines of `crates/jevd/src/ratelimit.rs`, `redact.rs`, `jev_http.rs`, `jev.rs`, and `answer.rs`

- [ ] **Step 1: Add step 4 and renumber**

In section 4 of `docs/superpowers/specs/2026-11-02-jev-daemon-design.md`, after the line that starts `3. **Key.**`, add:

```markdown
4. **Object keys.** If an object key in `state` or `questions` contains the key, at any depth, the request is `bad-request`. Jev gets nothing, and the request uses no rate-limit slot.
```

Then renumber the next steps: **Rate limit** 4 → 5, **Redaction** 5 → 6, **Send** 6 → 7, **Answer check** 7 → 8.

In the **Redaction** step, change:

```markdown
`jevd` leaves dict keys as they are, because 2 keys can become the same text.
```

to:

```markdown
`jevd` leaves dict keys as they are, because 2 keys can become the same text; step 4 refuses a dict key that contains the key.
```

Below the steps, change `The values in steps 1, 3, 5, 6, and 7` to `The values in steps 1, 3, 6, 7, and 8`.

- [ ] **Step 2: Change the step numbers in the code comments**

Change only the step numbers in these comments:

| File | Old | New |
|------|-----|-----|
| `crates/jevd/src/ratelimit.rs:1` | `(spec section 4, step 4)` | `(spec section 4, step 5)` |
| `crates/jevd/src/redact.rs:2` | `(spec section 4, step 5)` | `(spec section 4, step 6)` |
| `crates/jevd/src/jev_http.rs:1` | `(spec section 4, step 6)` | `(spec section 4, step 7)` |
| `crates/jevd/src/jev.rs:2` | `(spec section 4, steps 6 and 7)` | `(spec section 4, steps 7 and 8)` |
| `crates/jevd/src/answer.rs:1` | `(spec section 4, step 7)` | `(spec section 4, step 8)` |

Steps 1 to 3 keep their numbers, so `shape.rs`, `key.rs`, and `keycache.rs` do not change. Then run `grep -rn "spec section 4, step" crates/jevd/src` and check each hit against the new list. If a comment wraps across 2 lines, cargo fmt does not change it, so keep its line breaks.

- [ ] **Step 3: Check and commit**

Run: `cargo fmt --all --check` and `cargo test -p jevd --locked --lib`
Expected: no changes, all pass.

```bash
git add docs/superpowers/specs/2026-11-02-jev-daemon-design.md crates/jevd/src
git -c user.name="Matthew Brandenburg" -c user.email="mattbran87@users.noreply.github.com" commit -m "docs: the object key check is step 4 of the ask checks" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: CHANGELOG

**Files:**
- Modify: `CHANGELOG.md`

- [ ] **Step 1: Add the section**

Between `# Changelog` and `## 0.1.5`, add:

```markdown
## Unreleased

- jevd refuses an `ask` with `bad-request` when an object key in `state` or `questions` contains the API key, such as an option name of a `choice` question. Before, jevd redacted the key only in string values, so such a request reached Jev with the key in it ([#18](https://github.com/mattbran87/jev-daemon/issues/18)). The refused request uses no rate-limit slot.
```

Keep one blank line before and after the new section. Keep the file's line ends: check `git ls-files --eol CHANGELOG.md` before and after.

- [ ] **Step 2: Commit**

```bash
git add CHANGELOG.md
git -c user.name="Matthew Brandenburg" -c user.email="mattbran87@users.noreply.github.com" commit -m "docs: the object key refusal in the changelog" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: Final check

- [ ] **Step 1: Run the CI commands**

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

Expected: no format changes, no warnings, every test passes, and 1 test is ignored (the real-keyring test).

- [ ] **Step 2: Check the branch**

Run: `git log --oneline main..HEAD` and `git status --short`
Expected: 5 commits (the spec, the plan, then Tasks 1 to 3) and a clean tree.

Push, the pull request, the merge, and closing #18 wait for the person's approval.
