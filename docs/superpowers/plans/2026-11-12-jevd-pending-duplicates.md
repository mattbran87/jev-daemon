# Duplicates in jevd's pending file Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** When 2 entries in `pending.json` share a code or a key, jevd keeps the newer one and still loads up to 16 requests.

**Architecture:** One change in `Pairings::load_pending` (`crates/jevd/src/pairing.rs`). The duplicate checks move from before the sort (code) and after the cut (key) into one walk over the sorted entries, which stops when 16 entries load.

**Tech Stack:** Rust, `cargo test`.

**Spec:** `docs/superpowers/specs/2026-11-12-jevd-pending-duplicates-design.md`

**Branch:** `fix/jevd-pending-duplicates` (the spec commit is on it).

**Commit identity for every commit:**

```bash
git -c user.name="Matthew Brandenburg" -c user.email="mattbran87@users.noreply.github.com" commit -m "<subject>" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

**Line ends:** `core.autocrlf=true`. Before and after each edit, run `git ls-files --eol <file>`. `git diff --stat` must not show a whole-file rewrite.

**Facts checked for this plan:**
- The test helper `saved(code, name, deadline)` sets `id_sha256` to `hash_request_id(code)`. So 2 entries with the same code also share a key, unless the test changes one.
- `CODE_ALPHABET` has 31 characters, so `code(i)` works for `i` up to 30.

---

### Task 1: One duplicate walk after the sort

**Files:**
- Modify: `crates/jevd/src/pairing.rs` (`load_pending` near lines 356-399, and the `tests` module)

- [ ] **Step 1: Write the failing tests**

In the `tests` module of `crates/jevd/src/pairing.rs`, add these 2 tests after `after_the_cap_a_load_keeps_the_latest_saved_deadlines`:

```rust
    #[test]
    fn a_load_keeps_the_newer_of_two_entries_with_one_code() {
        let dir = tempfile::tempdir().unwrap();
        let older = saved("AAAAAAAA", "older", T + 100);
        // A different key, so only the code is shared.
        let mut newer = saved("AAAAAAAA", "newer", T + 200);
        newer["id_sha256"] = hash_request_id("another request").into();
        write_pending(&dir, vec![older, newer]);
        let now = Instant::now();
        let mut pairings = reload(&dir, now, T);
        let names: Vec<String> = pairings
            .pending_list(now)
            .into_iter()
            .map(|view| view.name)
            .collect();
        assert_eq!(names, ["newer"]);
    }

    #[test]
    fn a_shared_key_does_not_take_one_of_the_16_places() {
        let dir = tempfile::tempdir().unwrap();
        let code = |i: usize| (CODE_ALPHABET[i] as char).to_string().repeat(CODE_LENGTH);
        let mut requests: Vec<serde_json::Value> = (0..MAX_PENDING + 1)
            .map(|i| saved(&code(i), &format!("app{i}"), T + 100 + i as u64))
            .collect();
        // app15 and app16 are the 2 newest, and they share a key.
        requests[MAX_PENDING - 1]["id_sha256"] = requests[MAX_PENDING]["id_sha256"].clone();
        write_pending(&dir, requests);
        let now = Instant::now();
        let mut pairings = reload(&dir, now, T);
        let mut names: Vec<String> = pairings
            .pending_list(now)
            .into_iter()
            .map(|view| view.name)
            .collect();
        names.sort();
        let mut expected: Vec<String> = (0..MAX_PENDING - 1).map(|i| format!("app{i}")).collect();
        expected.push(format!("app{MAX_PENDING}"));
        expected.sort();
        assert_eq!(names, expected);
    }
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test -p jevd --locked --lib pairing::tests::a_load_keeps_the_newer_of_two_entries_with_one_code pairing::tests::a_shared_key_does_not_take_one_of_the_16_places`

If cargo accepts only one filter, run each test by name in its own command.

Expected:
- The code test fails: the list holds `older`, not `newer`.
- The key test fails: the list holds 15 names, without `app0`.

- [ ] **Step 3: Write the fix**

In `load_pending`, replace this block:

```rust
        let mut codes = BTreeSet::new();
        // Each entry with the seconds it has left, at most 10 minutes, so a
        // clock moved back cannot keep a request alive longer.
        let mut kept: Vec<(u64, SavedRequest)> = Vec::new();
        for saved in file.requests {
            if saved.deadline <= now_unix {
                continue;
            }
            // A `cli` entry gets a new ID below, so its saved hash is not used.
            let valid = valid_name(&saved.name)
                && saved.profile.as_deref().is_none_or(valid_name)
                && valid_code(&saved.code)
                && (saved.cli || valid_id_sha256(&saved.id_sha256));
            if !valid || !codes.insert(saved.code.clone()) {
                continue;
            }
            let left = (saved.deadline - now_unix).min(PENDING_EXPIRY.as_secs());
            kept.push((left, saved));
        }
        // The latest deadlines stay; the earliest are dropped first. Sort by the
        // saved deadline, not by the time left: after the cap, many entries
        // can have the same time left, and the newest of them must stay.
        kept.sort_by_key(|(_, saved)| std::cmp::Reverse(saved.deadline));
        kept.truncate(MAX_PENDING);
        for (left, saved) in kept {
            // jevd does not know the ID of a request from `jev` any more, and
            // `jev` never polls with it, so it gets a new one.
            let cli_id = saved.cli.then(|| random_hex(REQUEST_ID_HEX_LENGTH / 2));
            let key = cli_id.as_deref().map_or(saved.id_sha256, hash_request_id);
            if self.pending.contains_key(&key) {
                continue;
            }
```

with:

```rust
        // Each entry with the seconds it has left, at most 10 minutes, so a
        // clock moved back cannot keep a request alive longer.
        let mut kept: Vec<(u64, SavedRequest)> = Vec::new();
        for saved in file.requests {
            if saved.deadline <= now_unix {
                continue;
            }
            // A `cli` entry gets a new ID below, so its saved hash is not used.
            let valid = valid_name(&saved.name)
                && saved.profile.as_deref().is_none_or(valid_name)
                && valid_code(&saved.code)
                && (saved.cli || valid_id_sha256(&saved.id_sha256));
            if !valid {
                continue;
            }
            let left = (saved.deadline - now_unix).min(PENDING_EXPIRY.as_secs());
            kept.push((left, saved));
        }
        // The latest deadlines stay; the earliest are dropped first. Sort by the
        // saved deadline, not by the time left: after the cap, many entries
        // can have the same time left, and the newest of them must stay.
        kept.sort_by_key(|(_, saved)| std::cmp::Reverse(saved.deadline));
        // Walk the newest first, so of 2 entries with one code or one key the
        // newer stays, and a skipped entry does not take one of the places.
        let mut codes = BTreeSet::new();
        let mut loaded = 0;
        for (left, saved) in kept {
            if loaded == MAX_PENDING {
                break;
            }
            if codes.contains(&saved.code) {
                continue;
            }
            // jevd does not know the ID of a request from `jev` any more, and
            // `jev` never polls with it, so it gets a new one.
            let cli_id = saved.cli.then(|| random_hex(REQUEST_ID_HEX_LENGTH / 2));
            let key = cli_id.as_deref().map_or(saved.id_sha256, hash_request_id);
            if self.pending.contains_key(&key) {
                continue;
            }
            codes.insert(saved.code.clone());
            loaded += 1;
```

The `self.pending.insert(...)` call after this block does not change.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p jevd --locked --lib pairing::tests`
Expected: all pass, including `a_load_keeps_the_16_latest_valid_entries` and `after_the_cap_a_load_keeps_the_latest_saved_deadlines`.

- [ ] **Step 5: Format, lint, and commit**

Run: `cargo fmt --all` and `cargo clippy -p jevd --all-targets --locked -- -D warnings`
Expected: no warnings.

```bash
git add crates/jevd/src/pairing.rs
git -c user.name="Matthew Brandenburg" -c user.email="mattbran87@users.noreply.github.com" commit -m "fix(jevd): of 2 pending entries with one code or key, keep the newer" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: CHANGELOG

**Files:**
- Modify: `CHANGELOG.md`

- [ ] **Step 1: Add the line**

Under `## Unreleased`, after its last bullet (the one that starts "- After the clock is set back"), add:

```markdown
- When 2 waiting requests in `pending.json` share a code or a key, jevd keeps the newer one. A skipped duplicate no longer takes one of the 16 places.
```

Keep the file's line ends: check `git ls-files --eol CHANGELOG.md` before and after.

- [ ] **Step 2: Commit**

```bash
git add CHANGELOG.md
git -c user.name="Matthew Brandenburg" -c user.email="mattbran87@users.noreply.github.com" commit -m "docs: the pending duplicates fix in the changelog" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: Final check

- [ ] **Step 1: Run the CI commands**

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

Expected: no format changes, no warnings, every test passes, and 1 test is ignored (the real-keyring test).

- [ ] **Step 2: Check the branch**

Run: `git log --oneline main..HEAD` and `git status --short`
Expected: 4 commits (the spec, the plan, then Tasks 1 and 2) and a clean tree.

Push, the pull request, and the merge wait for the person's approval.
