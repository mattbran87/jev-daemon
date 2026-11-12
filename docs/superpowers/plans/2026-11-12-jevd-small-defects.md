# Three small jevd defects Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix 3 small defects in jevd's pairing code: the message for an unknown code, a bad `id_sha256` in `pending.json`, and the order after the 10-minute cap.

**Architecture:** Each fix touches one function. Task 1 changes `Daemon::approve` and `Daemon::deny` in `crates/jevd/src/daemon.rs`. Tasks 2 and 3 change `Pairings::load_pending` in `crates/jevd/src/pairing.rs`. No wire format, file format, or exit code changes.

**Tech Stack:** Rust, `cargo test`, tokio tests in `daemon.rs`.

**Spec:** `docs/superpowers/specs/2026-11-12-jevd-small-defects-design.md`

**Branch:** `fix/jevd-small-defects` (the spec commit is on it).

**Commit identity for every commit:**

```bash
git -c user.name="Matthew Brandenburg" -c user.email="mattbran87@users.noreply.github.com" commit -m "<subject>" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

**Line ends:** `core.autocrlf=true`. Before and after each edit, run `git ls-files --eol <file>`. `git diff --stat` must not show a whole-file rewrite.

**Facts checked for this plan:**
- `jev allow` checks the code against the waiting list itself (`crates/jev/src/pairing.rs:143`) and prints its own `NO_SUCH_CODE`. No client matches on jevd's message text, so a new text breaks nothing.
- `CODE_ALPHABET` is `b"23456789ABCDEFGHJKMNPQRSTUVWXYZ"`, so `CODE_ALPHABET[0]` is `'2'`, and the code `22222222` sorts first.
- The daemon test `fixture()` has the profile `default`.

---

### Task 1: The message for an unknown code

**Files:**
- Modify: `crates/jevd/src/daemon.rs` (`approve` near line 280, `deny` near line 301, and the `tests` module)

- [ ] **Step 1: Write the failing tests**

In the `tests` module of `crates/jevd/src/daemon.rs`, add these 2 tests after `deny_revoke_and_set_limit_name_what_they_cannot_find`:

```rust
    #[tokio::test]
    async fn approve_and_deny_say_what_to_do_about_an_unknown_code() {
        let f = fixture();
        let message = |reply: &Reply| reply.error.as_ref().unwrap().message.clone();
        let deny = call(
            &f.daemon,
            Peer::Jev,
            Request::Deny {
                code: "ABCD2345".into(),
            },
        )
        .await;
        assert_eq!(code_of(&deny), Some(ErrorCode::BadRequest));
        assert_eq!(message(&deny), NO_SUCH_CODE);
        let approve = call(
            &f.daemon,
            Peer::Jev,
            Request::Approve {
                code: "ABCD2345".into(),
                profile: "default".into(),
            },
        )
        .await;
        assert_eq!(code_of(&approve), Some(ErrorCode::BadRequest));
        assert_eq!(message(&approve), NO_SUCH_CODE);
    }

    #[tokio::test]
    async fn approve_names_a_bad_profile_name() {
        let f = fixture();
        // Only a hand-edited settings file can hold a name like this.
        f.daemon.lock().settings.profiles.push("Bad Name".into());
        let pair = call(
            &f.daemon,
            Peer::Other,
            Request::Pair {
                name: "app".into(),
                profile: None,
            },
        )
        .await;
        let code = pair.data["code"].as_str().unwrap().to_string();
        let approve = call(
            &f.daemon,
            Peer::Jev,
            Request::Approve {
                code,
                profile: "Bad Name".into(),
            },
        )
        .await;
        assert_eq!(code_of(&approve), Some(ErrorCode::BadRequest));
        let message = &approve.error.as_ref().unwrap().message;
        assert!(message.contains("\"Bad Name\""), "{message}");
        assert!(!message.contains("code"), "{message}");
    }
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test -p jevd --locked --lib daemon::tests::approve_`
Expected: a compile error, because `NO_SUCH_CODE` does not exist yet.

- [ ] **Step 3: Write the fix**

In `crates/jevd/src/daemon.rs`, add this constant just before `pub(crate) fn failure`:

```rust
/// The message for a failed approve or deny. `jev allow` prints the same text.
const NO_SUCH_CODE: &str =
    "no waiting request has that code; run jev allow with no code to see the waiting requests";
```

Change the start of `approve` from:

```rust
    fn approve(&self, code: &str, profile: &str) -> Reply {
        let mut state = self.lock();
        if !state.settings.profiles.iter().any(|known| known == profile) {
```

to:

```rust
    fn approve(&self, code: &str, profile: &str) -> Reply {
        // `Pairings::approve` refuses this name too, but with no reason, and
        // the message below would then blame the code.
        if !jev_proto::valid_name(profile) {
            return failure(
                ErrorCode::BadRequest,
                format!("{profile:?} is not a valid profile name"),
            );
        }
        let mut state = self.lock();
        if !state.settings.profiles.iter().any(|known| known == profile) {
```

In `approve` and in `deny`, change both lines:

```rust
            Err(code) => failure(code, "no waiting request has that code"),
```

to:

```rust
            Err(code) => failure(code, NO_SUCH_CODE),
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p jevd --locked --lib daemon::tests`
Expected: all pass, including the 2 new tests.

- [ ] **Step 5: Format, lint, and commit**

Run: `cargo fmt --all` and `cargo clippy -p jevd --all-targets --locked -- -D warnings`
Expected: no warnings.

```bash
git add crates/jevd/src/daemon.rs
git -c user.name="Matthew Brandenburg" -c user.email="mattbran87@users.noreply.github.com" commit -m "fix(jevd): say what to do about an unknown pairing code" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: A bad `id_sha256` in `pending.json`

**Files:**
- Modify: `crates/jevd/src/pairing.rs` (`valid_code` near line 258, `load_pending` near line 360, and the `tests` module)

- [ ] **Step 1: Write the failing test**

In the `tests` module of `crates/jevd/src/pairing.rs`, add this test after `a_load_keeps_the_16_latest_valid_entries`:

```rust
    #[test]
    fn a_load_drops_a_socket_entry_with_a_bad_id_hash() {
        let dir = tempfile::tempdir().unwrap();
        let mut empty = saved("AAAAAAAA", "empty", T + 300);
        empty["id_sha256"] = "".into();
        let mut short = saved("BBBBBBBB", "short", T + 300);
        short["id_sha256"] = "a".repeat(63).into();
        let mut upper = saved("CCCCCCCC", "upper", T + 300);
        upper["id_sha256"] = hash_request_id("CCCCCCCC").to_ascii_uppercase().into();
        // jevd gives a `cli` entry a new ID at the load, so its hash does not matter.
        let mut cli = saved("DDDDDDDD", "cli", T + 300);
        cli["id_sha256"] = "".into();
        cli["cli"] = true.into();
        write_pending(
            &dir,
            vec![empty, short, upper, cli, saved("EEEEEEEE", "good", T + 300)],
        );
        let now = Instant::now();
        let mut pairings = reload(&dir, now, T);
        let mut names: Vec<String> = pairings
            .pending_list(now)
            .into_iter()
            .map(|view| view.name)
            .collect();
        names.sort();
        assert_eq!(names, ["cli", "good"]);
    }
```

- [ ] **Step 2: Run the test to see it fail**

Run: `cargo test -p jevd --locked --lib pairing::tests::a_load_drops_a_socket_entry_with_a_bad_id_hash`
Expected: FAIL. The list also holds `empty`, `short`, and `upper`.

- [ ] **Step 3: Write the fix**

In `crates/jevd/src/pairing.rs`, add this function after `valid_code`:

```rust
/// A request ID hash as `hash_request_id` writes it: 64 lowercase hex digits.
fn valid_id_sha256(hash: &str) -> bool {
    hash.len() == 64 && hash.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}
```

In `load_pending`, change:

```rust
            let valid = valid_name(&saved.name)
                && saved.profile.as_deref().is_none_or(valid_name)
                && valid_code(&saved.code);
```

to:

```rust
            // A `cli` entry gets a new ID below, so its saved hash is not used.
            let valid = valid_name(&saved.name)
                && saved.profile.as_deref().is_none_or(valid_name)
                && valid_code(&saved.code)
                && (saved.cli || valid_id_sha256(&saved.id_sha256));
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p jevd --locked --lib pairing::tests`
Expected: all pass.

- [ ] **Step 5: Format, lint, and commit**

Run: `cargo fmt --all` and `cargo clippy -p jevd --all-targets --locked -- -D warnings`
Expected: no warnings.

```bash
git add crates/jevd/src/pairing.rs
git -c user.name="Matthew Brandenburg" -c user.email="mattbran87@users.noreply.github.com" commit -m "fix(jevd): drop a pending entry with a bad id_sha256" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: The order after the 10-minute cap

**Files:**
- Modify: `crates/jevd/src/pairing.rs` (`load_pending` near line 369, and the `tests` module)

- [ ] **Step 1: Write the failing test**

In the `tests` module of `crates/jevd/src/pairing.rs`, add this test after `a_load_drops_a_socket_entry_with_a_bad_id_hash`:

```rust
    #[test]
    fn after_the_cap_a_load_keeps_the_latest_saved_deadlines() {
        let dir = tempfile::tempdir().unwrap();
        let code = |i: usize| (CODE_ALPHABET[i] as char).to_string().repeat(CODE_LENGTH);
        // Every deadline is past the 10-minute cap, so every entry gets 600
        // seconds. The earliest deadline comes first in the file, and its
        // code, 22222222, sorts first.
        let requests = (0..MAX_PENDING + 1)
            .map(|i| saved(&code(i), &format!("app{i}"), T + 700 + i as u64))
            .collect();
        write_pending(&dir, requests);
        let now = Instant::now();
        let mut pairings = reload(&dir, now, T);
        let mut names: Vec<String> = pairings
            .pending_list(now)
            .into_iter()
            .map(|view| view.name)
            .collect();
        names.sort();
        let mut expected: Vec<String> = (1..MAX_PENDING + 1).map(|i| format!("app{i}")).collect();
        expected.sort();
        assert_eq!(names, expected);
    }
```

- [ ] **Step 2: Run the test to see it fail**

Run: `cargo test -p jevd --locked --lib pairing::tests::after_the_cap_a_load_keeps_the_latest_saved_deadlines`
Expected: FAIL. The list holds `app0` and not `app16`.

- [ ] **Step 3: Write the fix**

In `load_pending`, change:

```rust
        // The latest deadlines stay, and the earliest go first.
        kept.sort_by_key(|(left, _)| std::cmp::Reverse(*left));
```

to:

```rust
        // The latest deadlines stay, and the earliest go first. Sort by the
        // saved deadline, not by the time left: after the cap, many entries
        // can have the same time left, and the newest of them must stay.
        kept.sort_by_key(|(_, saved)| std::cmp::Reverse(saved.deadline));
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p jevd --locked --lib pairing::tests`
Expected: all pass, including `a_load_keeps_the_16_latest_valid_entries`.

- [ ] **Step 5: Format, lint, and commit**

Run: `cargo fmt --all` and `cargo clippy -p jevd --all-targets --locked -- -D warnings`
Expected: no warnings.

```bash
git add crates/jevd/src/pairing.rs
git -c user.name="Matthew Brandenburg" -c user.email="mattbran87@users.noreply.github.com" commit -m "fix(jevd): after the 10-minute cap, keep the newest pending requests" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: CHANGELOG

**Files:**
- Modify: `CHANGELOG.md`

- [ ] **Step 1: Add the section**

`CHANGELOG.md` has no `## Unreleased` section, because release 0.1.4 renamed it. Insert this section directly above the line `## 0.1.4`, with one blank line after it:

```markdown
## Unreleased

- jevd's message for an unknown pairing code says to run `jev allow` with no code. jevd also names a bad profile name, where it blamed the code before.
- jevd drops a waiting socket request in `pending.json` whose `id_sha256` is not a SHA-256. No client could collect such a request.
- After a clock change, jevd keeps the 16 newest waiting requests, not the 16 whose codes sort first.
```

Keep the file's line ends: check `git ls-files --eol CHANGELOG.md` before and after.

- [ ] **Step 2: Commit**

```bash
git add CHANGELOG.md
git -c user.name="Matthew Brandenburg" -c user.email="mattbran87@users.noreply.github.com" commit -m "docs: the three jevd fixes in the changelog" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: Final check

- [ ] **Step 1: Run the CI commands**

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

Expected: no format changes, no warnings, every test passes, and 1 test is ignored (the real-keyring test), as before.

- [ ] **Step 2: Check the branch**

Run: `git log --oneline main..HEAD` and `git status --short`
Expected: 6 commits (the spec, the plan, then Tasks 1 to 4) and a clean tree.

Push, the pull request, and the merge wait for the person's approval.
