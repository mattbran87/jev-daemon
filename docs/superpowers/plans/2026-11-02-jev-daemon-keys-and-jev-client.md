# jev-daemon keys and Jev client — implementation plan (plan 2 of 7)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the tolerant error-code reader to `jev-proto`, and add the keyring access, the key cache, and the Jev HTTPS client to the `jevd` library, all tested with no real keyring and no real network.

**Architecture:** The spec is `docs/superpowers/specs/2026-11-02-jev-daemon-design.md`. This plan covers spec section 2 ("Key cache", and the unknown-code rule under "Messages") and section 4, steps 3, 6, and 7. Keyring access sits behind a `KeyStore` trait with a real `OsKeyring` and a `MemoryKeyring` test double, so CI never touches a credential store. The Jev client splits into pure functions (`jevd::jev`: the body, and what a response means) and the async sender (`jevd::jev_http`), which tests drive against a local fake server. Every function that depends on time takes `now`, except the HTTP timeouts, which the tests set short.

**Tech Stack:** Rust stable (edition 2021, MSRV 1.85, `resolver = "3"`), plus plan 1's crates. New: `keyring` 3.6, `zeroize` 1.9, `reqwest` 0.13 (rustls), `tokio` 1, and `wiremock` 0.6 for tests.

**Builds on:** plan 1, `docs/superpowers/plans/2026-11-02-jev-daemon-core-library.md`, merged into `main`. Plan 1's `check_key`, `redact_value`, and `valid_answers` are used here unchanged.

**Reference code:** the private Python prototype's `typesafe.py`. `jevd::jev` and `jevd::jev_http` keep its rules. Each difference has a note in its task.

**Commit rule:** gate each commit on `cargo test`'s own exit status. Never pipe the test run into another command before the commit decision. End every commit message with the line `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

**TDD rule:** run the red step for real and read the failure. After green, run a short mutation self-check: break each rule in the new code one at a time, and check that a test fails. If a mutant survives, add a test in a separate `test(...)` commit. Restore the file after each mutant.

Code blocks show each task's logic as written; Task 7 ran `cargo fmt`, so the source layout differs.

## Choices this plan makes

These come from checks run on 2026-11-02, on Windows 11 with Rust 1.99.1:

- **keyring 3.6, not 4.** keyring 4.2 needs Rust 1.88, above the MSRV. keyring 3.6.3 needs 1.75.
- **Linux keyring backend.** keyring 3.6 with `async-secret-service`, `crypto-rust`, and `async-io`. This uses the pure-Rust `zbus`, so the build needs no system `libdbus` and no OpenSSL. `cargo check --target x86_64-unknown-linux-gnu` passed with this feature set before `reqwest` came in (Task 6), and `cargo tree` showed no `dbus` or `openssl` crate. On the final tree, `aws-lc-sys` needs a C cross compiler, so a cross-check needs one, or a scratch copy with `reqwest`'s `rustls-no-provider` feature (the final review ran that: `cargo check -p jevd --lib` passed for Linux and macOS). Plan 6's CI is the real check. `async-io` keeps `zbus` on its own executor, apart from `jevd`'s tokio runtime.
- **`resolver = "3"`.** `reqwest` pulls in `url`, then `idna`, then the `icu` crates, and their newest versions need Rust 1.88. Resolver 3 picks the newest version that fits the MSRV. With it, no locked crate needs more than 1.85.
- **reqwest 0.13 with `rustls`.** Its TLS provider is `aws-lc-rs`, whose `aws-lc-sys` compiles C code. It built on Windows with no extra tools. Plan 6 checks the Linux and macOS release builds.
- **Status codes.** A 401 or a 403 is `jev-unauthorized`. Any other status that is not 200 is `jev-http-error`, and a redirect is one of them.

---

## File structure

| File | Responsibility |
| --- | --- |
| `Cargo.toml` | Workspace: `resolver = "3"` |
| `crates/jev-proto/src/error.rs` | Adds `ErrorCode::from_name` and `WireCode`. `WireError.code` becomes a `WireCode`. |
| `crates/jev-proto/src/lib.rs` | Re-exports `WireCode` |
| `crates/jev-proto/src/message.rs` | Adds one test: a reply with an unknown code parses |
| `crates/jevd/Cargo.toml` | New dependencies |
| `crates/jevd/src/lib.rs` | Module list |
| `crates/jevd/src/keystore.rs` | `KeyStore` trait, account names, `OsKeyring`, `MemoryKeyring` |
| `crates/jevd/src/keycache.rs` | `KeyCache`: read once, keep, wipe after the period |
| `crates/jevd/src/jev.rs` | The Jev body, `JevFailure`, the error detail, and what a response means |
| `crates/jevd/src/jev_http.rs` | `JevClient`: one HTTPS request with the retry, the caps, and no redirects |
| `docs/superpowers/specs/2026-11-02-jev-daemon-design.md` | Records this plan's choices (Task 7) |

---

### Task 1: The MSRV-aware resolver

**Files:**
- Modify: `Cargo.toml` (the `resolver` line)

- [ ] **Step 1: Change the resolver**

In `Cargo.toml`, change:

```toml
resolver = "2"
```

to:

```toml
resolver = "3"
```

- [ ] **Step 2: Check that the build and the tests still pass**

Run: `cargo test --workspace --locked`
Expected: PASS. jev-proto has 11 tests, jevd has 93.

- [ ] **Step 3: Check that no locked crate needs more than Rust 1.85**

Run:

```bash
cargo metadata --format-version 1 | python -c "import json,sys; d=json.load(sys.stdin); v=lambda s: tuple(int(x) for x in s.split('.')[:2]); print([(p['name'], p['rust_version']) for p in d['packages'] if p.get('rust_version') and v(p['rust_version']) > (1, 85)])"
```

Expected: `[]`

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml
git commit -m "chore: the MSRV-aware resolver" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: The tolerant error-code reader (`jev-proto`)

Spec section 2: a client reads a code it does not know as `WireCode::Unknown`, never as an unreadable reply. A raw code that does not match `[a-z0-9-]{1,64}` reads as `Unknown("malformed")`, so a bad reply cannot put control characters into CLI output. `ErrorCode` stays closed, so a new code makes the compiler flag each `match` in `jev`. `jevd` only builds a `WireError` through `WireError::new`, which takes an `ErrorCode`, so it cannot send an unknown code.

A missing `code` also reads as `malformed`, and a missing `message` reads as empty, so a failure reply with a broken `error` object still parses.

**Files:**
- Modify: `crates/jev-proto/src/error.rs`
- Modify: `crates/jev-proto/src/lib.rs`
- Modify: `crates/jev-proto/src/message.rs` (tests only)

- [ ] **Step 1: Write the failing tests**

Add these tests at the end of `mod tests` in `crates/jev-proto/src/error.rs`:

```rust
    #[test]
    fn every_known_code_reads_as_known() {
        for code in ErrorCode::ALL {
            let wire: WireCode = serde_json::from_value(serde_json::json!(code.as_str())).unwrap();
            assert_eq!(wire, WireCode::Known(code));
            assert_eq!(serde_json::to_value(&wire).unwrap(), serde_json::json!(code.as_str()));
            assert_eq!(ErrorCode::from_name(code.as_str()), Some(code));
        }
        assert_eq!(ErrorCode::from_name("no-such-code"), None);
    }

    #[test]
    fn an_unknown_code_keeps_its_raw_text() {
        let wire: WireCode = serde_json::from_value(serde_json::json!("quota-exceeded")).unwrap();
        assert_eq!(wire, WireCode::Unknown("quota-exceeded".to_string()));
        assert_eq!(wire.as_str(), "quota-exceeded");
        assert_eq!(wire.known(), None);
        assert_eq!(wire.exit_code(), 1);
        assert_eq!(serde_json::to_value(&wire).unwrap(), serde_json::json!("quota-exceeded"));
        let longest = "a1-".repeat(21) + "z";
        assert_eq!(WireCode::from_raw(&longest), WireCode::Unknown(longest.clone()));
    }

    #[test]
    fn a_bad_raw_code_reads_as_malformed() {
        let malformed = WireCode::Unknown(MALFORMED.to_string());
        for raw in [
            serde_json::json!(""),
            serde_json::json!("Not-Paired"),
            serde_json::json!("a b"),
            serde_json::json!("a_b"),
            serde_json::json!("\u{1b}[31mred"),
            serde_json::json!("a".repeat(65)),
            serde_json::json!("ü"),
            serde_json::json!(7),
            serde_json::json!(null),
            serde_json::json!({"code": "internal"}),
        ] {
            let wire: WireCode = serde_json::from_value(raw.clone()).unwrap();
            assert_eq!(wire, malformed, "{raw}");
        }
    }

    #[test]
    fn a_known_code_keeps_its_exit_code() {
        assert_eq!(WireCode::Known(ErrorCode::RateLimited).exit_code(), 5);
        assert_eq!(WireCode::from(ErrorCode::NotPaired), WireCode::Known(ErrorCode::NotPaired));
        assert_eq!(
            WireError::new(ErrorCode::Forbidden, "no").code,
            WireCode::Known(ErrorCode::Forbidden)
        );
    }
```

Add this test at the end of `mod tests` in `crates/jev-proto/src/message.rs`:

```rust
    #[test]
    fn a_reply_with_an_unknown_code_still_parses() {
        use crate::WireCode;
        let reply: Reply = serde_json::from_str(
            r#"{"ok":false,"error":{"code":"quota-exceeded","message":"try later","retry_after":30}}"#,
        )
        .unwrap();
        let error = reply.error.unwrap();
        assert_eq!(error.code, WireCode::Unknown("quota-exceeded".to_string()));
        assert_eq!(error.message, "try later");
        assert_eq!(error.retry_after, Some(30));
        let bare: Reply = serde_json::from_str(r#"{"ok":false,"error":{}}"#).unwrap();
        let error = bare.error.unwrap();
        assert_eq!(error.code, WireCode::Unknown("malformed".to_string()));
        assert_eq!(error.message, "");
    }
```

In `crates/jev-proto/src/lib.rs`, change:

```rust
pub use error::{ErrorCode, WireError};
```

to:

```rust
pub use error::{ErrorCode, WireCode, WireError};
```

- [ ] **Step 2: Run the tests to check that they fail**

Run: `cargo test -p jev-proto`
Expected: FAIL to compile, with `cannot find type WireCode` and `no function or associated item named from_name`.

- [ ] **Step 3: Write the implementation**

In `crates/jev-proto/src/error.rs`, change the first line:

```rust
use serde::{Deserialize, Serialize};
```

to:

```rust
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;
```

Add this method inside `impl ErrorCode`, after `exit_code`:

```rust
    /// The code for a wire name, or `None` for a name this version does not know.
    pub fn from_name(name: &str) -> Option<ErrorCode> {
        ErrorCode::ALL.into_iter().find(|code| code.as_str() == name)
    }
```

Replace the `WireError` struct and its `impl` with this code. It goes above `#[cfg(test)]`:

```rust
/// The raw text of a code that is missing, is not a string, or has a bad shape.
const MALFORMED: &str = "malformed";

/// An error code as a client receives it (spec section 2). A code this
/// version does not know is `Unknown`, so the reply still parses.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum WireCode {
    Known(ErrorCode),
    /// Read from the wire, the raw text matches `[a-z0-9-]{1,64}`, or is `malformed`.
    Unknown(String),
}

fn valid_raw_code(raw: &str) -> bool {
    (1..=64).contains(&raw.len())
        && raw
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn malformed() -> WireCode {
    WireCode::Unknown(MALFORMED.to_string())
}

impl WireCode {
    /// Reads a received code. A server code named `malformed` reads the same as a malformed one.
    pub fn from_raw(raw: &str) -> WireCode {
        match ErrorCode::from_name(raw) {
            Some(code) => WireCode::Known(code),
            None if valid_raw_code(raw) => WireCode::Unknown(raw.to_string()),
            None => malformed(),
        }
    }

    /// The code's wire text.
    pub fn as_str(&self) -> &str {
        match self {
            WireCode::Known(code) => code.as_str(),
            WireCode::Unknown(raw) => raw,
        }
    }

    /// The known code, or `None` for an unknown one.
    pub fn known(&self) -> Option<ErrorCode> {
        match self {
            WireCode::Known(code) => Some(*code),
            WireCode::Unknown(_) => None,
        }
    }

    /// The `jev` exit code. An unknown code is an unexpected error (1).
    pub fn exit_code(&self) -> i32 {
        self.known().map_or(1, ErrorCode::exit_code)
    }
}

impl From<ErrorCode> for WireCode {
    fn from(code: ErrorCode) -> WireCode {
        WireCode::Known(code)
    }
}

impl Serialize for WireCode {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for WireCode {
    /// Any JSON value is accepted, so a bad code never fails the reply.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<WireCode, D::Error> {
        Ok(match Value::deserialize(deserializer)? {
            Value::String(raw) => WireCode::from_raw(&raw),
            _ => malformed(),
        })
    }
}

/// The `error` object of a failed reply.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireError {
    /// A missing code reads as `malformed`.
    #[serde(default = "malformed")]
    pub code: WireCode,
    #[serde(default)]
    pub message: String,
    /// Seconds until a `rate-limited` client can try again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_after: Option<u64>,
    /// The HTTP status for `jev-http-error` and `jev-unauthorized`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
}

impl WireError {
    /// The usual way to build an error to send. `jevd` sets `code` only through `new`, so it sends known codes only.
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        WireError {
            code: WireCode::Known(code),
            message: message.into(),
            retry_after: None,
            status: None,
        }
    }
}
```

- [ ] **Step 4: Run the tests to check that they pass**

Run: `cargo test -p jev-proto`
Expected: PASS, 16 tests.

Run: `cargo test --workspace`
Expected: PASS. jevd still has 93 tests.

- [ ] **Step 5: Commit**

```bash
git add crates/jev-proto/src/error.rs crates/jev-proto/src/lib.rs crates/jev-proto/src/message.rs
git commit -m "feat(proto): read an unknown error code as WireCode::Unknown" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: Keyring access (`jevd::keystore`)

Spec section 1: keys are in the OS keyring, service `jev`, account `profile:<name>`. CLI client tokens are under `client:<name>`. Plan 4's CLI writes both, so this module has `set` and `delete` as well as `get`.

The keyring's own error text never passes on: `keyring::Error::BadEncoding` holds the stored bytes. `StoreError` is a closed enum, and each kind has a fixed message, so the type itself keeps secrets out of the text. Plan 3 can tell a locked keyring (`StoreError::Locked`) apart from other failures.

keyring 3 falls back to an in-memory mock store on any OS other than Windows, macOS, and Linux, so a key would be silently lost. The module stops the build on any other OS with `compile_error!`.

`Zeroizing` wipes jevd's own copy of a secret when it is dropped. Copies made inside the OS keyring libraries are not wiped, so this is best-effort, in line with the threat model (spec section 10).

`MemoryKeyring` is public, not `#[cfg(test)]`, because the tests of plans 3 and 4 use it. It counts reads, so the key cache tests can prove that the cache works, and it can fail every call, as a locked keyring does.

**Files:**
- Modify: `crates/jevd/Cargo.toml`
- Modify: `crates/jevd/src/lib.rs`
- Create: `crates/jevd/src/keystore.rs`

- [ ] **Step 1: Add the dependencies**

In `crates/jevd/Cargo.toml`, add these 2 lines to `[dependencies]`, in alphabetical order (`keyring` after `jev-proto`, `zeroize` last):

```toml
keyring = "3.6"
zeroize = "1.9"
```

Add these tables after `[dependencies]`. Cargo merges each target's features into the base `keyring` entry:

```toml
[target.'cfg(windows)'.dependencies]
keyring = { version = "3.6", features = ["windows-native"] }

[target.'cfg(target_os = "macos")'.dependencies]
keyring = { version = "3.6", features = ["apple-native"] }

[target.'cfg(target_os = "linux")'.dependencies]
keyring = { version = "3.6", features = ["async-secret-service", "crypto-rust", "async-io"] }
```

- [ ] **Step 2: Write the failing tests**

In `crates/jevd/src/lib.rs`, add `pub mod keystore;` after `pub mod key;`.

Create `crates/jevd/src/keystore.rs` with only the tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accounts_follow_the_spec() {
        assert_eq!(SERVICE, "jev");
        assert_eq!(profile_account("work"), "profile:work");
        assert_eq!(client_account("cc-jev-adhd"), "client:cc-jev-adhd");
    }

    #[test]
    fn the_memory_keyring_stores_and_deletes() {
        let store = MemoryKeyring::new();
        assert!(store.get("profile:work").unwrap().is_none());
        store.set("profile:work", "tsk-1").unwrap();
        store.set("profile:work", "tsk-2").unwrap();
        assert_eq!(store.get("profile:work").unwrap().expect("stored").as_str(), "tsk-2");
        assert!(store.delete("profile:work").unwrap());
        assert!(!store.delete("profile:work").unwrap());
        assert!(store.get("profile:work").unwrap().is_none());
        assert_eq!(store.reads(), 3);
    }

    #[test]
    fn a_failing_memory_keyring_fails_every_call() {
        let store = MemoryKeyring::new();
        store.set("profile:work", "tsk-1").unwrap();
        store.set_failing(true);
        assert_eq!(store.get("profile:work").unwrap_err(), StoreError::Locked);
        assert_eq!(store.set("profile:work", "tsk-2").unwrap_err(), StoreError::Locked);
        assert_eq!(store.delete("profile:work").unwrap_err(), StoreError::Locked);
        store.set_failing(false);
        assert_eq!(store.get("profile:work").unwrap().expect("kept").as_str(), "tsk-1");
    }

    #[test]
    fn a_failed_read_still_counts() {
        let store = MemoryKeyring::new();
        assert_eq!(store.reads(), 0);
        store.set_failing(true);
        assert!(store.get("profile:work").is_err());
        assert_eq!(store.reads(), 1);
    }

    #[test]
    fn each_store_error_has_a_fixed_message() {
        let cases = [
            (StoreError::Locked, "the keyring could not be reached; it may be locked"),
            (StoreError::BadEncoding, "a keyring entry is not valid text"),
            (StoreError::Refused, "the keyring refused the entry name or value"),
            (StoreError::Ambiguous, "the keyring holds more than one entry for this account"),
            (StoreError::Other, "the keyring failed"),
        ];
        for (error, text) in cases {
            assert_eq!(error.to_string(), text);
            assert!(!text.contains('{') && !text.contains('}'), "{text}");
        }
    }

    #[test]
    fn keyring_errors_map_to_fixed_kinds() {
        let boxed = || -> Box<dyn std::error::Error + Send + Sync> { "tsk-secret".into() };
        let cases = [
            (keyring::Error::PlatformFailure(boxed()), StoreError::Locked),
            (keyring::Error::NoStorageAccess(boxed()), StoreError::Locked),
            (keyring::Error::BadEncoding(b"tsk-secret".to_vec()), StoreError::BadEncoding),
            (keyring::Error::TooLong("name".to_string(), 1), StoreError::Refused),
            (keyring::Error::Invalid("name".to_string(), "why".to_string()), StoreError::Refused),
            (keyring::Error::Ambiguous(Vec::new()), StoreError::Ambiguous),
            (keyring::Error::NoEntry, StoreError::Other),
        ];
        for (error, kind) in cases {
            assert_eq!(store_error(&error), kind, "{error}");
        }
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "not a valid profile name")]
    fn a_bad_profile_name_is_a_bug() {
        profile_account("Work:x");
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "not a valid client name")]
    fn a_bad_client_name_is_a_bug() {
        client_account("");
    }

    #[test]
    #[ignore = "touches the real OS keyring; run by hand with --ignored"]
    fn the_os_keyring_round_trips() {
        let account = format!("test:{}", std::process::id());
        let store = OsKeyring;
        store.set(&account, "secret-value").unwrap();
        assert_eq!(store.get(&account).unwrap().expect("stored").as_str(), "secret-value");
        assert!(store.delete(&account).unwrap());
        assert!(store.get(&account).unwrap().is_none());
        assert!(!store.delete(&account).unwrap());
    }
}
```

- [ ] **Step 3: Run the tests to check that they fail**

Run: `cargo test -p jevd keystore`
Expected: FAIL to compile, with `cannot find value SERVICE`, `cannot find function profile_account`, `cannot find type MemoryKeyring`, and `cannot find type StoreError`.

- [ ] **Step 4: Write the implementation**

Add this code above `#[cfg(test)]` in `crates/jevd/src/keystore.rs`:

```rust
//! The OS keyring (spec section 1, "Storage"), behind a trait so that tests
//! never touch a real credential store. `Zeroizing` wipes jevd's own copy of a
//! secret when it is dropped; copies made inside the OS keyring libraries are
//! not wiped, so this is best-effort, in line with the threat model (spec
//! section 10).

// keyring 3 falls back to an in-memory mock store on any other OS, so a key
// would be silently lost.
#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
compile_error!("jevd needs a real OS keyring: Windows, macOS, or Linux");

use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;

use zeroize::Zeroizing;

pub const SERVICE: &str = "jev";

/// The account that holds a profile's TypeSafe key.
pub fn profile_account(profile: &str) -> String {
    debug_assert!(jev_proto::valid_name(profile), "not a valid profile name");
    format!("profile:{profile}")
}

/// The account that holds a CLI client's token.
pub fn client_account(client: &str) -> String {
    debug_assert!(jev_proto::valid_name(client), "not a valid client name");
    format!("client:{client}")
}

/// A keyring failure. Each kind has a fixed message, so no secret can reach it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreError {
    /// The keyring is locked, or its service cannot be reached.
    Locked,
    /// An entry holds bytes that are not valid text.
    BadEncoding,
    /// The keyring refused the entry name or value.
    Refused,
    /// More than one entry matches the account.
    Ambiguous,
    /// Any other keyring failure.
    Other,
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            StoreError::Locked => "the keyring could not be reached; it may be locked",
            StoreError::BadEncoding => "a keyring entry is not valid text",
            StoreError::Refused => "the keyring refused the entry name or value",
            StoreError::Ambiguous => "the keyring holds more than one entry for this account",
            StoreError::Other => "the keyring failed",
        })
    }
}

impl std::error::Error for StoreError {}

pub trait KeyStore: Send + Sync {
    /// The secret for `account`, or `None` when the keyring has no entry.
    fn get(&self, account: &str) -> Result<Option<Zeroizing<String>>, StoreError>;
    fn set(&self, account: &str, secret: &str) -> Result<(), StoreError>;
    /// Deletes the entry. `false` when there was none.
    fn delete(&self, account: &str) -> Result<bool, StoreError>;
}

/// The real keyring: Windows Credential Manager, the macOS Keychain, or the
/// Secret Service on Linux. A call can block on a keyring prompt, so async
/// code runs it on a blocking thread.
pub struct OsKeyring;

fn entry(account: &str) -> Result<keyring::Entry, StoreError> {
    keyring::Entry::new(SERVICE, account).map_err(|error| store_error(&error))
}

/// The kind of each failure. The keyring's own text is never passed on,
/// because `BadEncoding` holds the stored bytes.
fn store_error(error: &keyring::Error) -> StoreError {
    match error {
        keyring::Error::PlatformFailure(_) | keyring::Error::NoStorageAccess(_) => {
            StoreError::Locked
        }
        keyring::Error::BadEncoding(_) => StoreError::BadEncoding,
        keyring::Error::TooLong(..) | keyring::Error::Invalid(..) => StoreError::Refused,
        keyring::Error::Ambiguous(_) => StoreError::Ambiguous,
        _ => StoreError::Other,
    }
}

impl KeyStore for OsKeyring {
    fn get(&self, account: &str) -> Result<Option<Zeroizing<String>>, StoreError> {
        match entry(account)?.get_password() {
            Ok(secret) => Ok(Some(Zeroizing::new(secret))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(store_error(&error)),
        }
    }

    fn set(&self, account: &str, secret: &str) -> Result<(), StoreError> {
        entry(account)?
            .set_password(secret)
            .map_err(|error| store_error(&error))
    }

    fn delete(&self, account: &str) -> Result<bool, StoreError> {
        match entry(account)?.delete_credential() {
            Ok(()) => Ok(true),
            Err(keyring::Error::NoEntry) => Ok(false),
            Err(error) => Err(store_error(&error)),
        }
    }
}

/// An in-memory keyring for the tests of `jevd` and `jev`. It counts reads,
/// and it can fail every call, as a locked keyring does.
#[derive(Default)]
pub struct MemoryKeyring {
    entries: Mutex<HashMap<String, String>>,
    failing: AtomicBool,
    reads: AtomicUsize,
}

impl MemoryKeyring {
    pub fn new() -> MemoryKeyring {
        MemoryKeyring::default()
    }

    /// While `true`, every call fails.
    pub fn set_failing(&self, failing: bool) {
        self.failing.store(failing, Ordering::SeqCst);
    }

    /// How many times `get` ran, failed calls included.
    pub fn reads(&self) -> usize {
        self.reads.load(Ordering::SeqCst)
    }

    fn check(&self) -> Result<(), StoreError> {
        if self.failing.load(Ordering::SeqCst) {
            return Err(StoreError::Locked);
        }
        Ok(())
    }

    fn entries(&self) -> std::sync::MutexGuard<'_, HashMap<String, String>> {
        self.entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl KeyStore for MemoryKeyring {
    fn get(&self, account: &str) -> Result<Option<Zeroizing<String>>, StoreError> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        self.check()?;
        Ok(self.entries().get(account).map(|secret| Zeroizing::new(secret.clone())))
    }

    fn set(&self, account: &str, secret: &str) -> Result<(), StoreError> {
        self.check()?;
        self.entries().insert(account.to_string(), secret.to_string());
        Ok(())
    }

    fn delete(&self, account: &str) -> Result<bool, StoreError> {
        self.check()?;
        Ok(self.entries().remove(account).is_some())
    }
}
```

- [ ] **Step 5: Run the tests to check that they pass**

Run: `cargo test -p jevd keystore`
Expected: PASS, 8 passed and 1 ignored. In a release build (`--release`), the 2 `should_panic` tests do not exist, because `debug_assert!` is off there.

Run once by hand, on this machine only: `cargo test -p jevd keystore -- --ignored`
Expected: PASS, 1 passed. It writes and then deletes one entry, `jev` / `test:<pid>`, in the real keyring. If it fails, report the error and do not change the test. CI never runs it.

- [ ] **Step 6: Commit**

```bash
git add Cargo.lock crates/jevd/Cargo.toml crates/jevd/src/lib.rs crates/jevd/src/keystore.rs
git commit -m "feat(jevd): keyring access behind a trait, with an in-memory double" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: The key cache (`jevd::keycache`)

Spec section 2, "Key cache": `jevd` reads a profile's key at that profile's first request and keeps it in memory. It wipes the key after `key-cache` with no use of that key, and on `reload`, `jev key remove`, and exit. Plan 3 calls `reload`, `forget`, and `clear` at those points.

Rules:

- Each use restarts the period. A key whose last use is a full period ago goes.
- The keyring read runs outside the cache. `lookup` claims the read and gives a `Ticket`. The caller runs `read_key` with no lock held, then calls `finish`. Plan 3 keeps the cache behind a `std::sync::Mutex` and runs `read_key` on a blocking thread, so a keyring prompt (a macOS Keychain dialog, or a locked Secret Service) stalls only the requests of that one profile. The module stays synchronous: no tokio in `keycache.rs`.
- In the daemon, a request never calls `get`. On `Miss`, the daemon runs `read_key` and `finish` in a detached task that the request does not own, so a cancelled request still fills the cache. If the blocking read panics (a `JoinError`), the detached task calls `abandon`.
- The daemon's `tokio::sync::watch` carries the outcome of each read, such as `(read_id, Result<(), WireError>)`. The detached task publishes its ticket's `read_id()` and the outcome after `finish`. The daemon also raises the watch after `abandon`, `forget`, `clear`, and `reload`. On `Wait(read)`, the request remembers `read` and awaits `tokio::time::timeout(READ_LIMIT, rx.changed())`. If that read failed, the request replies with the same error and does not call `lookup` again, so one failed read prompts once. In every other case, a timeout included, it calls `lookup` again: `sweep` dropping an expired mark does not raise the watch, and the watch keeps only its newest value. A request that arrives after the failure gets `Miss` and reads again.
- One read per profile at a time. While a read is in flight, `lookup` gives `Wait(id)` with that read's id. Each read has its own id, which its `Ticket` (`read_id()`) and its in-flight mark both hold.
- A caller whose read was cancelled calls `abandon`. A read that started `READ_LIMIT` (10 minutes) ago or more counts as lost, and `sweep` drops its mark. The limit is long because a person can leave a keyring prompt open for minutes.
- `finish` and `abandon` act only if the profile's mark is still the ticket's read. `forget`, `clear`, and `reload` drop the marks, so a key removed during a read does not come back, and a late read never clears or overwrites a newer read. `forget` of one profile does not touch a read of another. `finish` gives back its result unchanged in all cases.
- `reload` sets the period and clears every key.
- The cache keeps the trimmed key that `check_key` returns, so the key that is sent and the key that is redacted are the same text.
- A key that fails `check_key` is not kept. The next request reads the keyring again, so a fixed key works at once.
- A keyring failure is `internal`, with the keyring's message. It is not kept.
- Keys sit in `Zeroizing<String>`, which wipes the memory when a key leaves the cache. Its `Debug` prints no contents. Each hit is a separate `Zeroizing` copy. The copy in the HTTP header is not wiped.
- A key is wiped only when something calls the cache, so the daemon must call `sweep` from a timer, set from `next_expiry`.

**Files:**
- Modify: `crates/jevd/src/lib.rs`
- Create: `crates/jevd/src/keycache.rs`

- [ ] **Step 1: Write the failing tests**

In `crates/jevd/src/lib.rs`, add `pub mod keycache;` after `pub mod key;`.

Create `crates/jevd/src/keycache.rs` with only the tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::keystore::MemoryKeyring;
    use jev_proto::WireCode;

    const SECOND: Duration = Duration::from_secs(1);
    const MINUTE: Duration = Duration::from_secs(60);
    const HOUR: Duration = Duration::from_secs(3600);

    fn store_with(key: &str) -> MemoryKeyring {
        let store = MemoryKeyring::new();
        store.set(&profile_account("work"), key).unwrap();
        store
    }

    fn key(text: &str) -> Result<Zeroizing<String>, WireError> {
        Ok(Zeroizing::new(text.to_string()))
    }

    fn ticket(lookup: Lookup) -> Ticket {
        match lookup {
            Lookup::Miss(ticket) => ticket,
            Lookup::Hit(_) => panic!("expected a miss, got a hit"),
            Lookup::Wait(_) => panic!("expected a miss, got a wait"),
        }
    }

    fn hit(lookup: Lookup) -> Zeroizing<String> {
        match lookup {
            Lookup::Hit(key) => key,
            Lookup::Miss(_) => panic!("expected a hit, got a miss"),
            Lookup::Wait(_) => panic!("expected a hit, got a wait"),
        }
    }

    #[test]
    fn the_first_request_reads_the_keyring_and_later_ones_do_not() {
        let store = store_with("tsk-work");
        let mut cache = KeyCache::new(2 * HOUR);
        let t0 = Instant::now();
        assert_eq!(cache.get("work", &store, t0).unwrap().as_str(), "tsk-work");
        assert_eq!(cache.get("work", &store, t0 + HOUR).unwrap().as_str(), "tsk-work");
        assert_eq!(store.reads(), 1);
    }

    #[test]
    fn a_key_unused_for_the_whole_period_is_read_again() {
        let store = store_with("tsk-work");
        let mut cache = KeyCache::new(2 * HOUR);
        let t0 = Instant::now();
        cache.get("work", &store, t0).unwrap();
        cache.get("work", &store, t0 + 2 * HOUR).unwrap();
        assert_eq!(store.reads(), 2);
    }

    #[test]
    fn each_use_restarts_the_period() {
        let store = store_with("tsk-work");
        let mut cache = KeyCache::new(2 * HOUR);
        let t0 = Instant::now();
        cache.get("work", &store, t0).unwrap();
        cache.get("work", &store, t0 + 90 * MINUTE).unwrap();
        cache.get("work", &store, t0 + 180 * MINUTE).unwrap();
        assert_eq!(store.reads(), 1);
        cache.get("work", &store, t0 + 300 * MINUTE).unwrap();
        assert_eq!(store.reads(), 2);
    }

    #[test]
    fn the_stored_key_is_trimmed() {
        let store = store_with("  tsk-work\n");
        assert_eq!(read_key(&store, "work").unwrap().as_str(), "tsk-work");
        let mut cache = KeyCache::new(2 * HOUR);
        assert_eq!(cache.get("work", &store, Instant::now()).unwrap().as_str(), "tsk-work");
    }

    #[test]
    fn a_missing_or_blank_key_is_no_key() {
        let empty = MemoryKeyring::new();
        let error = KeyCache::new(2 * HOUR)
            .get("work", &empty, Instant::now())
            .unwrap_err();
        assert_eq!(error.code, WireCode::Known(ErrorCode::NoKey));
        assert!(error.message.contains("jev key add work"), "{}", error.message);
        let blank = store_with("   ");
        let error = KeyCache::new(2 * HOUR)
            .get("work", &blank, Instant::now())
            .unwrap_err();
        assert_eq!(error.code, WireCode::Known(ErrorCode::NoKey));
    }

    #[test]
    fn a_bad_key_is_not_kept() {
        let store = store_with("a b");
        let mut cache = KeyCache::new(2 * HOUR);
        let t0 = Instant::now();
        let error = cache.get("work", &store, t0).unwrap_err();
        assert_eq!(error.code, WireCode::Known(ErrorCode::BadKey));
        assert!(!error.message.contains("a b"), "{}", error.message);
        assert!(cache.is_empty());
        store.set(&profile_account("work"), "tsk-fixed").unwrap();
        assert_eq!(cache.get("work", &store, t0).unwrap().as_str(), "tsk-fixed");
    }

    #[test]
    fn a_keyring_failure_is_internal_and_not_kept() {
        let store = store_with("tsk-work");
        store.set_failing(true);
        let mut cache = KeyCache::new(2 * HOUR);
        let t0 = Instant::now();
        let error = cache.get("work", &store, t0).unwrap_err();
        assert_eq!(error.code, WireCode::Known(ErrorCode::Internal));
        assert!(error.message.contains("locked"), "{}", error.message);
        assert!(cache.is_empty());
        store.set_failing(false);
        assert_eq!(cache.get("work", &store, t0).unwrap().as_str(), "tsk-work");
    }

    #[test]
    fn forget_and_clear_wipe_keys() {
        let store = store_with("tsk-work");
        store.set(&profile_account("home"), "tsk-home").unwrap();
        let mut cache = KeyCache::new(2 * HOUR);
        let t0 = Instant::now();
        assert_eq!(cache.get("home", &store, t0).unwrap().as_str(), "tsk-home");
        assert_eq!(cache.get("work", &store, t0).unwrap().as_str(), "tsk-work");
        assert_eq!(cache.len(), 2);
        cache.forget("work");
        assert_eq!(cache.len(), 1);
        cache.get("work", &store, t0).unwrap();
        assert_eq!(store.reads(), 3);
        cache.clear();
        assert!(cache.is_empty());
    }

    #[test]
    fn sweep_wipes_only_keys_past_the_period() {
        let store = store_with("tsk-work");
        store.set(&profile_account("home"), "tsk-home").unwrap();
        let mut cache = KeyCache::new(2 * HOUR);
        let t0 = Instant::now();
        cache.get("work", &store, t0).unwrap();
        cache.get("home", &store, t0 + HOUR).unwrap();
        cache.sweep(t0 + 2 * HOUR);
        assert_eq!(cache.len(), 1);
        cache.get("home", &store, t0 + 2 * HOUR).unwrap();
        assert_eq!(store.reads(), 2);
        // `reload` sets the period and also clears every key.
        cache.reload(MINUTE);
        assert!(cache.is_empty());
        let t1 = t0 + 2 * HOUR;
        cache.get("home", &store, t1).unwrap();
        assert_eq!(store.reads(), 3);
        cache.sweep(t1 + MINUTE - SECOND);
        assert_eq!(cache.len(), 1);
        cache.sweep(t1 + MINUTE);
        assert!(cache.is_empty());
    }

    #[test]
    fn a_miss_marks_the_profile_so_a_second_lookup_waits() {
        let mut cache = KeyCache::new(2 * HOUR);
        let t0 = Instant::now();
        let _ticket = ticket(cache.lookup("work", t0));
        assert!(matches!(cache.lookup("work", t0 + MINUTE), Lookup::Wait(_)));
        let _other = ticket(cache.lookup("home", t0 + MINUTE));
        assert!(cache.is_empty());
    }

    #[test]
    fn a_wait_carries_the_id_of_the_read_in_flight() {
        let mut cache = KeyCache::new(2 * HOUR);
        let t0 = Instant::now();
        let home = ticket(cache.lookup("home", t0));
        let old = ticket(cache.lookup("work", t0));
        assert!(matches!(cache.lookup("home", t0), Lookup::Wait(id) if id == home.read_id()));
        assert!(matches!(cache.lookup("work", t0), Lookup::Wait(id) if id == old.read_id()));
        cache.forget("work");
        let new = ticket(cache.lookup("work", t0));
        assert_ne!(new.read_id(), old.read_id());
        assert!(matches!(cache.lookup("work", t0), Lookup::Wait(id) if id == new.read_id()));
        cache.abandon(home);
        cache.abandon(old);
        cache.abandon(new);
    }

    #[test]
    fn after_finish_the_next_lookup_is_a_hit() {
        let mut cache = KeyCache::new(2 * HOUR);
        let t0 = Instant::now();
        let claim = ticket(cache.lookup("work", t0));
        let returned = cache.finish(claim, key("tsk-work"), t0 + MINUTE).unwrap();
        assert_eq!(returned.as_str(), "tsk-work");
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.next_expiry(), Some(t0 + MINUTE + 2 * HOUR));
        assert_eq!(hit(cache.lookup("work", t0 + MINUTE)).as_str(), "tsk-work");
    }

    #[test]
    fn a_wait_turns_into_a_miss_after_the_read_limit() {
        let mut cache = KeyCache::new(2 * HOUR);
        let t0 = Instant::now();
        let _lost = ticket(cache.lookup("work", t0));
        assert!(matches!(cache.lookup("work", t0 + READ_LIMIT - SECOND), Lookup::Wait(_)));
        let _retry = ticket(cache.lookup("work", t0 + READ_LIMIT));
        assert!(matches!(cache.lookup("work", t0 + READ_LIMIT + SECOND), Lookup::Wait(_)));
    }

    #[test]
    fn a_read_open_for_minutes_still_holds_the_profile() {
        // A person can leave a keyring prompt open for minutes.
        let mut cache = KeyCache::new(2 * HOUR);
        let t0 = Instant::now();
        let _claim = ticket(cache.lookup("work", t0));
        assert!(matches!(cache.lookup("work", t0 + 9 * MINUTE), Lookup::Wait(_)));
        let _retry = ticket(cache.lookup("work", t0 + 10 * MINUTE));
    }

    #[test]
    fn a_failed_finish_stores_nothing_and_the_next_lookup_is_a_miss() {
        let mut cache = KeyCache::new(2 * HOUR);
        let t0 = Instant::now();
        let claim = ticket(cache.lookup("work", t0));
        let failure = WireError::new(ErrorCode::Internal, "the keyring is locked".to_string());
        let error = cache.finish(claim, Err(failure), t0).unwrap_err();
        assert_eq!(error.code, WireCode::Known(ErrorCode::Internal));
        assert_eq!(error.message, "the keyring is locked");
        assert!(cache.is_empty());
        let _retry = ticket(cache.lookup("work", t0));
    }

    #[test]
    fn forget_between_miss_and_finish_keeps_the_key_out() {
        let mut cache = KeyCache::new(2 * HOUR);
        let t0 = Instant::now();
        let old = ticket(cache.lookup("work", t0));
        cache.forget("work");
        let new = ticket(cache.lookup("work", t0));
        assert_eq!(cache.finish(old, key("tsk-old"), t0).unwrap().as_str(), "tsk-old");
        assert!(cache.is_empty());
        // The stale ticket leaves the newer read's mark in place.
        assert!(matches!(cache.lookup("work", t0), Lookup::Wait(_)));
        cache.finish(new, key("tsk-new"), t0).unwrap();
        assert_eq!(hit(cache.lookup("work", t0)).as_str(), "tsk-new");
    }

    #[test]
    fn clear_between_miss_and_finish_keeps_the_key_out() {
        let mut cache = KeyCache::new(2 * HOUR);
        let t0 = Instant::now();
        let old = ticket(cache.lookup("work", t0));
        cache.clear();
        let new = ticket(cache.lookup("work", t0));
        assert_eq!(cache.finish(old, key("tsk-old"), t0).unwrap().as_str(), "tsk-old");
        assert!(cache.is_empty());
        assert!(matches!(cache.lookup("work", t0), Lookup::Wait(_)));
        cache.finish(new, key("tsk-new"), t0).unwrap();
        assert_eq!(hit(cache.lookup("work", t0)).as_str(), "tsk-new");
    }

    #[test]
    fn reload_between_miss_and_finish_keeps_the_key_out() {
        let mut cache = KeyCache::new(2 * HOUR);
        let t0 = Instant::now();
        let old = ticket(cache.lookup("work", t0));
        cache.reload(HOUR);
        let _new = ticket(cache.lookup("work", t0));
        cache.finish(old, key("tsk-old"), t0).unwrap();
        assert!(cache.is_empty());
    }

    #[test]
    fn forgetting_one_profile_leaves_another_profiles_read_alone() {
        let mut cache = KeyCache::new(MINUTE);
        let t0 = Instant::now();
        let claim = ticket(cache.lookup("home", t0));
        cache.forget("work");
        cache.finish(claim, key("tsk-home"), t0).unwrap();
        assert_eq!(hit(cache.lookup("home", t0)).as_str(), "tsk-home");
        // The key goes after the period, and no mark is left behind.
        let _retry = ticket(cache.lookup("home", t0 + MINUTE));
    }

    #[test]
    fn a_ticket_past_the_read_limit_does_not_touch_the_newer_read() {
        let mut cache = KeyCache::new(2 * HOUR);
        let t0 = Instant::now();
        let old = ticket(cache.lookup("work", t0));
        let new = ticket(cache.lookup("work", t0 + READ_LIMIT));
        let late = t0 + READ_LIMIT + SECOND;
        assert_eq!(cache.finish(old, key("tsk-old"), late).unwrap().as_str(), "tsk-old");
        assert!(cache.is_empty());
        assert!(matches!(cache.lookup("work", late), Lookup::Wait(_)));
        cache.finish(new, key("tsk-new"), late).unwrap();
        assert_eq!(hit(cache.lookup("work", late)).as_str(), "tsk-new");
    }

    #[test]
    fn abandon_clears_only_its_own_mark() {
        let mut cache = KeyCache::new(2 * HOUR);
        let t0 = Instant::now();
        let claim = ticket(cache.lookup("work", t0));
        cache.abandon(claim);
        assert!(cache.is_empty());
        let old = ticket(cache.lookup("work", t0));
        cache.forget("work");
        let _new = ticket(cache.lookup("work", t0));
        cache.abandon(old);
        assert!(matches!(cache.lookup("work", t0), Lookup::Wait(_)));
    }

    #[test]
    fn sweep_drops_marks_at_the_read_limit() {
        let mut cache = KeyCache::new(2 * HOUR);
        let t0 = Instant::now();
        let work = ticket(cache.lookup("work", t0));
        let home = ticket(cache.lookup("home", t0 + SECOND));
        let late = t0 + READ_LIMIT;
        cache.sweep(late);
        // The work mark is gone, so its read is not stored.
        cache.finish(work, key("tsk-work"), late).unwrap();
        cache.finish(home, key("tsk-home"), late).unwrap();
        assert_eq!(cache.len(), 1);
        assert_eq!(hit(cache.lookup("home", late)).as_str(), "tsk-home");
    }

    #[test]
    fn next_expiry_is_the_oldest_key_plus_the_period() {
        let store = store_with("tsk-work");
        store.set(&profile_account("home"), "tsk-home").unwrap();
        let mut cache = KeyCache::new(2 * HOUR);
        assert_eq!(cache.next_expiry(), None);
        let t0 = Instant::now();
        cache.get("work", &store, t0).unwrap();
        cache.get("home", &store, t0 + HOUR).unwrap();
        let _in_flight = ticket(cache.lookup("other", t0 + HOUR));
        assert_eq!(cache.next_expiry(), Some(t0 + 2 * HOUR));
        cache.forget("work");
        assert_eq!(cache.next_expiry(), Some(t0 + 3 * HOUR));
    }

    #[test]
    fn get_on_wait_reads_the_key_and_stores_nothing() {
        let store = store_with("tsk-work");
        let mut cache = KeyCache::new(2 * HOUR);
        let t0 = Instant::now();
        let claim = ticket(cache.lookup("work", t0));
        assert_eq!(cache.get("work", &store, t0).unwrap().as_str(), "tsk-work");
        assert_eq!(store.reads(), 1);
        assert!(cache.is_empty());
        assert!(matches!(cache.lookup("work", t0), Lookup::Wait(_)));
        cache.finish(claim, key("tsk-work"), t0).unwrap();
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn forgetting_an_unknown_profile_is_fine() {
        let store = store_with("tsk-work");
        let mut cache = KeyCache::new(2 * HOUR);
        let t0 = Instant::now();
        cache.get("work", &store, t0).unwrap();
        cache.forget("nobody");
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn a_now_earlier_than_the_last_use_does_not_panic() {
        let store = store_with("tsk-work");
        let mut cache = KeyCache::new(2 * HOUR);
        let t0 = Instant::now();
        cache.get("work", &store, t0 + HOUR).unwrap();
        assert_eq!(cache.get("work", &store, t0).unwrap().as_str(), "tsk-work");
        cache.sweep(t0);
        assert_eq!(cache.len(), 1);
        assert_eq!(store.reads(), 1);
        let _claim = ticket(cache.lookup("home", t0 + HOUR));
        assert!(matches!(cache.lookup("home", t0), Lookup::Wait(_)));
    }
}
```

- [ ] **Step 2: Run the tests to check that they fail**

Run: `cargo test -p jevd keycache`
Expected: FAIL to compile, with `cannot find type KeyCache`, `cannot find type Lookup`, and `cannot find type Duration`.

- [ ] **Step 3: Write the implementation**

Add this code above `#[cfg(test)]` in `crates/jevd/src/keycache.rs`:

```rust
//! The key cache (spec section 2, "Key cache"). A profile's key stays in
//! memory after its first request, and goes after `key-cache` with no use.
//!
//! The keyring read runs outside the cache: `lookup` claims the read, the
//! caller runs `read_key` with no lock held, and `finish` stores the result.
//! A keyring prompt then stalls only the requests of that one profile.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use jev_proto::{ErrorCode, WireError};
use zeroize::Zeroizing;

use crate::key::check_key;
use crate::keystore::{profile_account, KeyStore};

/// An in-flight read older than this counts as abandoned, so a lost ticket
/// cannot block a profile forever. It is long because a person can leave a
/// keyring prompt open for minutes. A caller whose read was cancelled calls
/// `abandon`, so this limit covers only tickets that are lost.
pub const READ_LIMIT: Duration = Duration::from_secs(600);

/// What `lookup` found.
///
/// In the daemon (plan 3), a request never calls `get`. On `Miss`, the daemon
/// runs `read_key` and `finish` in a detached task that the request does not
/// own, so a cancelled request still fills the cache. If the blocking read
/// panics (a `JoinError`), the detached task calls `abandon`.
///
/// The daemon owns a `tokio::sync::watch` that carries the outcome of each
/// read, such as `(read_id, Result<(), WireError>)`. The detached task
/// publishes its ticket's `read_id` and the outcome after `finish`, and the
/// daemon also raises the watch after `abandon`, `forget`, `clear`, and
/// `reload`. On `Wait(read)`, the request remembers `read` and awaits
/// `tokio::time::timeout(READ_LIMIT, rx.changed())`. If the watch shows that
/// `read` failed, the request replies with the same error and does not call
/// `lookup` again, so one failed read prompts once. In every other case,
/// a timeout included, it calls `lookup` again: `sweep` dropping an expired
/// mark does not raise the watch, and the watch keeps only its newest value.
/// A request that arrives after the failure gets `Miss` and reads again.
pub enum Lookup {
    /// The cached key. Its period restarts. Each hit is a separate
    /// `Zeroizing` copy, wiped when the caller drops it. A copy that the
    /// caller makes from it, such as the HTTP header value, is not wiped.
    Hit(Zeroizing<String>),
    /// No key: read it with `read_key` off the lock, then call `finish`.
    Miss(Ticket),
    /// Another read of this profile is in flight, with this id. Wait for its
    /// outcome, as above.
    Wait(u64),
}

/// Proof of a claimed read. It names the profile and the read.
#[must_use = "pass the ticket to finish or abandon"]
pub struct Ticket {
    profile: String,
    read: u64,
}

impl Ticket {
    /// The read's id, which a `Wait` for this read carries.
    pub fn read_id(&self) -> u64 {
        self.read
    }
}

/// Reads and checks a profile's key, with no cache access, so the caller can
/// run it on a blocking thread with no lock held. The result is the trimmed
/// key that `check_key` returns. A missing or blank key is `no-key`, a key of
/// a bad shape is `bad-key`, and a keyring failure is `internal`, with the
/// keyring's message. No error message holds key text.
///
/// The key check runs here, so the caller has its result before it takes a
/// rate-limit slot (spec section 4, step 3). `profile` must be a name that
/// passed `jev_proto::valid_name`.
pub fn read_key(store: &dyn KeyStore, profile: &str) -> Result<Zeroizing<String>, WireError> {
    let no_key = || {
        WireError::new(
            ErrorCode::NoKey,
            format!("profile {profile} has no key; run jev key add {profile}"),
        )
    };
    let raw = store
        .get(&profile_account(profile))
        .map_err(|error| {
            WireError::new(
                ErrorCode::Internal,
                format!("cannot read the key of profile {profile}: {error}"),
            )
        })?
        .ok_or_else(no_key)?;
    let key = check_key(&raw).map_err(|code| match code {
        ErrorCode::NoKey => no_key(),
        code => WireError::new(
            code,
            format!("the key of profile {profile} is not a valid key; run jev key add {profile}"),
        ),
    })?;
    Ok(Zeroizing::new(key.to_string()))
}

struct Cached {
    key: Zeroizing<String>,
    last_used: Instant,
}

/// Keys in memory. A key is wiped from memory when it leaves the cache.
///
/// A key leaves only when something calls the cache, so the daemon must call
/// `sweep` from a timer, and set the timer from `next_expiry`.
pub struct KeyCache {
    period: Duration,
    entries: HashMap<String, Cached>,
    /// The profiles with a read in flight: the read's id, and when it started.
    pending: HashMap<String, (u64, Instant)>,
    /// The id of the next read.
    next_read: u64,
}

impl KeyCache {
    pub fn new(period: Duration) -> KeyCache {
        KeyCache {
            period,
            entries: HashMap::new(),
            pending: HashMap::new(),
            next_read: 0,
        }
    }

    /// Sweeps, then returns a hit (and restarts its period), a miss with a
    /// ticket (and marks the profile in flight), or `Wait` when a read of the
    /// profile started less than `READ_LIMIT` ago. See `Lookup` for how the
    /// daemon uses each answer.
    pub fn lookup(&mut self, profile: &str, now: Instant) -> Lookup {
        self.sweep(now);
        if let Some(cached) = self.entries.get_mut(profile) {
            cached.last_used = now;
            return Lookup::Hit(cached.key.clone());
        }
        if let Some(&(read, _)) = self.pending.get(profile) {
            return Lookup::Wait(read);
        }
        let read = self.next_read;
        self.next_read = self.next_read.wrapping_add(1);
        self.pending.insert(profile.to_string(), (read, now));
        Lookup::Miss(Ticket {
            profile: profile.to_string(),
            read,
        })
    }

    /// Whether the in-flight mark of the ticket's profile is this read's.
    fn owns(&self, ticket: &Ticket) -> bool {
        self.pending.get(&ticket.profile).map(|pending| pending.0) == Some(ticket.read)
    }

    /// Ends a read. If the profile's in-flight mark is still this read's, it
    /// clears the mark and stores an `Ok` key. A failure is never stored.
    /// Otherwise it changes nothing: a `forget`, `clear`, or `reload` of the
    /// profile ran since the claim, or the read passed `READ_LIMIT`, and a mark
    /// there now belongs to a newer read. A read that ends after its mark
    /// expired and was dropped is not stored, but its result still goes back
    /// to the caller. Returns the result unchanged in all cases, so the caller
    /// can reply with it.
    pub fn finish(
        &mut self,
        ticket: Ticket,
        result: Result<Zeroizing<String>, WireError>,
        now: Instant,
    ) -> Result<Zeroizing<String>, WireError> {
        if !self.owns(&ticket) {
            return result;
        }
        self.pending.remove(&ticket.profile);
        if let Ok(key) = &result {
            self.entries.insert(
                ticket.profile,
                Cached {
                    key: key.clone(),
                    last_used: now,
                },
            );
        }
        result
    }

    /// Ends a read with no result, for a caller whose read was cancelled. It
    /// clears the in-flight mark only if the mark is still this read's.
    pub fn abandon(&mut self, ticket: Ticket) {
        if self.owns(&ticket) {
            self.pending.remove(&ticket.profile);
        }
    }

    /// `lookup`, `read_key`, and `finish` in one call, for single-threaded
    /// callers (tests, the CLI). The daemon does not call it (see `Lookup`).
    /// On `Wait` it reads without caching. Each key it returns is a separate
    /// `Zeroizing` copy; a copy that the caller makes from it, such as the
    /// HTTP header value, is not wiped.
    ///
    /// As with `read_key`, the key check runs before the caller takes a
    /// rate-limit slot (spec section 4, step 3), and `profile` must be a name
    /// that passed `jev_proto::valid_name`.
    pub fn get(
        &mut self,
        profile: &str,
        store: &dyn KeyStore,
        now: Instant,
    ) -> Result<Zeroizing<String>, WireError> {
        match self.lookup(profile, now) {
            Lookup::Hit(key) => Ok(key),
            Lookup::Miss(ticket) => {
                let result = read_key(store, profile);
                self.finish(ticket, result, now)
            }
            Lookup::Wait(_) => read_key(store, profile),
        }
    }

    /// Wipes every key whose last use is a full period ago, and drops every
    /// in-flight mark that started `READ_LIMIT` or more ago. A key is wiped
    /// only when something calls the cache, so the daemon must call this from
    /// a timer, set from `next_expiry`.
    pub fn sweep(&mut self, now: Instant) {
        let period = self.period;
        self.entries
            .retain(|_, cached| now.saturating_duration_since(cached.last_used) < period);
        self.pending
            .retain(|_, pending| now.saturating_duration_since(pending.1) < READ_LIMIT);
    }

    /// The earliest instant a cached key expires, so the daemon can sleep
    /// until then. `None` when no key is cached.
    pub fn next_expiry(&self) -> Option<Instant> {
        self.entries
            .values()
            .filter_map(|cached| cached.last_used.checked_add(self.period))
            .min()
    }

    /// Wipes one profile's key and its in-flight mark, for `jev key remove`.
    pub fn forget(&mut self, profile: &str) {
        self.entries.remove(profile);
        self.pending.remove(profile);
    }

    /// Wipes every key and every in-flight mark, for exit.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.pending.clear();
    }

    /// Sets the period and clears every key, for `reload`.
    pub fn reload(&mut self, period: Duration) {
        self.period = period;
        self.clear();
    }

    /// The number of cached keys. In-flight reads do not count.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
```

- [ ] **Step 4: Run the tests to check that they pass**

Run: `cargo test -p jevd keycache`
Expected: PASS, 26 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/jevd/src/lib.rs crates/jevd/src/keycache.rs
git commit -m "feat(jevd): the key cache, with wiped memory and a sliding period" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: The Jev body and what a response means (`jevd::jev`)

Spec section 4, steps 6 and 7. These are pure functions, so their tests need no server.

Rules:

- The body is `{"model": "jev-latest", "state": ..., "questions": ...}`. State and question text both get redaction, because question text is caller content too. `redact-strict` reaches both.
- A 200 needs a JSON object with `answers` that is at most 64 levels deep (`shape::MAX_DEPTH`), passes `valid_answers`, and has no object key, at any level, that contains the key. Anything else, or a body over its cap, is `bad-answer`. Extra fields and extra question ids in the answer pass, as in the prototype. An object key that names the key is refused, not renamed, because renamed keys could collide.
- After the check, the key and each of its encoded forms (the same forms as for the error detail, below) are replaced with `[redacted]` in every string value of the answers, longest first. The shape rules do not run on answers.
- A 401 or a 403 is `jev-unauthorized`. Any other status, a 2xx that is not 200 included, is `jev-http-error`, with the status.
- An error detail is built in this order:
  1. Encoded echoes of the key become `[redacted]`: its JSON-escaped form, the same form with `/` as `\/`, and its percent-encoded form (every byte except ASCII letters, digits, `-`, `.`, `_`, `~` as uppercase `%XX`). Each form is used only when it differs from the literal key, and the longest is replaced first.
  2. `redact` runs, with the key.
  3. Every control character, the bidi format characters U+202A to U+202E and U+2066 to U+2069, and the bidi marks U+200E, U+200F, and U+061C become a space. An error page can carry ANSI escapes, OSC sequences, BEL, and bidi overrides, and the detail reaches CLI output. This comes after redaction, because the PEM rule needs the newlines.
  4. The text is trimmed and cut to 300 characters. A cut first could leave part of a token too short to match. The prototype cuts after redaction too.
- An error body that is not valid UTF-8 is read lossily.
- A 429 or 529 reaches `read_response` only after the retry, so here it is final.

**Files:**
- Modify: `crates/jevd/src/lib.rs`
- Create: `crates/jevd/src/jev.rs`

- [ ] **Step 1: Write the failing tests**

In `crates/jevd/src/lib.rs`, add `pub mod jev;` after `pub mod files;`.

Create `crates/jevd/src/jev.rs` with only the tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use jev_proto::WireCode;

    /// A key that matches no redaction shape, so only the key rule can catch it.
    const KEY: &str = "plainkey123";

    fn questions() -> Value {
        json!({"q": {"type": "noul", "text": "is it?"}})
    }

    #[test]
    fn the_body_names_the_model_and_redacts_the_key() {
        let body = request_body(
            &json!({"note": "my key is plainkey123"}),
            &json!({"q": {"type": "noul", "text": "is plainkey123 ok?"}}),
            KEY,
            false,
        );
        assert_eq!(body["model"], "jev-latest");
        assert_eq!(body["state"]["note"], "my key is [redacted]");
        assert_eq!(body["questions"]["q"]["text"], "is [redacted] ok?");
        assert_eq!(body["questions"]["q"]["type"], "noul");
        assert!(!body.to_string().contains(KEY));
    }

    #[test]
    fn strict_redaction_reaches_the_body() {
        let state = json!({"password": "hunter2 has spaces"});
        assert_eq!(
            request_body(&state, &questions(), KEY, false)["state"]["password"],
            "hunter2 has spaces"
        );
        assert_eq!(
            request_body(&state, &questions(), KEY, true)["state"]["password"],
            "[redacted]"
        );
    }

    #[test]
    fn a_good_answer_passes() {
        let body = br#"{"answers": {"q": {"type": "noul", "noul": 0.8}}, "extra": 1}"#;
        assert_eq!(
            read_response(200, Some(body), &questions(), KEY),
            Ok(json!({"q": {"type": "noul", "noul": 0.8}}))
        );
    }

    #[test]
    fn a_bad_or_oversized_answer_is_bad_answer() {
        let bodies: [Option<&[u8]>; 6] = [
            Some(b"not json"),
            Some(br#"{"answers": {"q": {"type": "score"}}}"#),
            Some(br#"{"answers": {"q": {"type": "noul", "noul": 2}}}"#),
            Some(br#"{"other": 1}"#),
            Some(b"[1, 2]"),
            None,
        ];
        for body in bodies {
            assert_eq!(
                read_response(200, body, &questions(), KEY),
                Err(JevFailure::new(ErrorCode::BadAnswer))
            );
        }
    }

    #[test]
    fn statuses_map_to_codes() {
        assert_eq!(
            read_response(401, Some(b"unknown key plainkey123"), &questions(), KEY),
            Err(JevFailure {
                code: ErrorCode::JevUnauthorized,
                status: Some(401),
                detail: Some("unknown key [redacted]".to_string()),
            })
        );
        assert_eq!(
            read_response(403, None, &questions(), KEY).unwrap_err().code,
            ErrorCode::JevUnauthorized
        );
        for status in [302, 400, 429, 500, 529] {
            assert_eq!(
                read_response(status, Some(b""), &questions(), KEY),
                Err(JevFailure {
                    code: ErrorCode::JevHttpError,
                    status: Some(status),
                    detail: None,
                })
            );
        }
    }

    #[test]
    fn the_detail_is_redacted_before_it_is_cut() {
        // The token starts 10 characters before the cut. Cut first, its stub
        // "sk-ABCDEFG" would be too short for the sk- rule and would leak.
        let text = format!("{} sk-ABCDEFGHIJKLMNOPQRSTUVWX", "x".repeat(289));
        let detail = make_detail(&text, KEY).unwrap();
        assert!(!detail.contains("sk-"), "{detail}");
        assert!(detail.ends_with("[redacted]"), "{detail}");
        assert_eq!(detail.chars().count(), DETAIL_LIMIT);
    }

    #[test]
    fn the_detail_counts_characters_not_bytes() {
        let detail = make_detail(&"é".repeat(400), KEY).unwrap();
        assert_eq!(detail.chars().count(), DETAIL_LIMIT);
    }

    #[test]
    fn an_empty_detail_is_none() {
        assert_eq!(make_detail("  \n", KEY), None);
        assert_eq!(make_detail("", KEY), None);
    }

    #[test]
    fn failures_become_wire_errors() {
        let wire = JevFailure {
            code: ErrorCode::JevHttpError,
            status: Some(502),
            detail: Some("bad gateway".to_string()),
        }
        .to_wire();
        assert_eq!(wire.code, WireCode::Known(ErrorCode::JevHttpError));
        assert_eq!(wire.status, Some(502));
        assert_eq!(wire.message, "Jev returned HTTP 502: bad gateway");
        let timeout = JevFailure::new(ErrorCode::JevTimeout).to_wire();
        assert_eq!(timeout.message, "Jev did not answer in time");
        assert_eq!(timeout.status, None);
    }

    #[test]
    fn strict_redaction_reaches_the_questions() {
        let questions = json!({"q": {"type": "noul", "text": "is it?", "password": "hunter2 has spaces"}});
        assert_eq!(
            request_body(&json!({}), &questions, KEY, false)["questions"]["q"]["password"],
            "hunter2 has spaces"
        );
        assert_eq!(
            request_body(&json!({}), &questions, KEY, true)["questions"]["q"]["password"],
            "[redacted]"
        );
    }

    #[test]
    fn only_200_is_an_answer() {
        let body = br#"{"answers": {"q": {"type": "noul", "noul": 0.8}}}"#;
        for status in [201, 204] {
            assert_eq!(
                read_response(status, Some(body), &questions(), KEY).unwrap_err().code,
                ErrorCode::JevHttpError
            );
        }
    }

    #[test]
    fn each_failure_has_its_summary() {
        let cases = [
            (ErrorCode::JevUnauthorized, "Jev refused the key"),
            (ErrorCode::JevHttpError, "Jev returned an HTTP error"),
            (ErrorCode::JevUnreachable, "Jev could not be reached"),
            (ErrorCode::BadAnswer, "Jev's answer failed the answer check"),
            (ErrorCode::Internal, "the request to Jev failed (internal)"),
        ];
        for (code, message) in cases {
            assert_eq!(JevFailure::new(code).to_wire().message, message);
        }
    }

    #[test]
    fn control_characters_leave_the_detail() {
        let text = "a\x1b[2Jb\x1b]0;x\x07c\rd\ne\tf\u{202e}g";
        let detail = make_detail(text, KEY).unwrap();
        assert_eq!(detail, "a [2Jb ]0;x c d e f g");
        for c in ['\x1b', '\x07', '\r', '\n', '\t', '\u{202e}'] {
            assert!(!detail.contains(c), "{detail:?}");
        }
    }

    #[test]
    fn bidi_controls_leave_the_detail_and_their_neighbours_stay() {
        let marks = ['\u{200e}', '\u{200f}', '\u{061c}'];
        for c in ('\u{202a}'..='\u{202e}').chain('\u{2066}'..='\u{2069}').chain(marks) {
            assert_eq!(make_detail(&format!("a{c}b"), KEY).as_deref(), Some("a b"));
        }
        for c in ['\u{2029}', '\u{202f}', '\u{2065}', '\u{206a}', '\u{200d}', '\u{2010}', '\u{061b}', '\u{061d}'] {
            let text = format!("a{c}b");
            assert_eq!(make_detail(&text, KEY), Some(text));
        }
    }

    #[test]
    fn encoded_echoes_of_the_key_leave_the_detail() {
        let key = "tsk/ab+cd\"ef";
        for text in [
            "bad key tsk\\/ab+cd\\\"ef",
            "bad key tsk/ab+cd\\\"ef",
            "bad key tsk%2Fab%2Bcd%22ef",
            "bad key tsk/ab+cd\"ef",
        ] {
            assert_eq!(make_detail(text, key).as_deref(), Some("bad key [redacted]"), "{text}");
        }
    }

    #[test]
    fn a_private_key_is_redacted_before_its_newlines_go() {
        let text = "-----BEGIN RSA PRIVATE KEY-----\nQUJD\nREVG\n- done";
        assert_eq!(make_detail(text, KEY).as_deref(), Some("[redacted] - done"));
    }

    #[test]
    fn the_longest_key_form_is_replaced_first() {
        // The JSON form `/ab\"cd` sits inside the `\/` form `\/ab\"cd`.
        let key = "/ab\"cd";
        assert_eq!(
            make_detail("bad key \\/ab\\\"cd", key).as_deref(),
            Some("bad key [redacted]")
        );
    }

    #[test]
    fn the_percent_form_keeps_the_unreserved_characters() {
        let key = "Az09-._~/ é";
        assert_eq!(percent_encode(key), "Az09-._~%2F%20%C3%A9");
        assert_eq!(
            make_detail("bad key Az09-._~%2F%20%C3%A9", key).as_deref(),
            Some("bad key [redacted]")
        );
    }

    #[test]
    fn the_encoded_key_forms_leave_the_answers() {
        let key = "tsk/ab+cd\"ef";
        let body = serde_json::to_vec(&json!({"answers": {"q": {
            "type": "noul", "noul": 0.8,
            "why": "tsk%2Fab%2Bcd%22ef",
            "notes": ["tsk/ab+cd\\\"ef", "tsk\\/ab+cd\\\"ef", "x tsk/ab+cd\"ef y"],
        }}}))
        .unwrap();
        assert_eq!(
            read_response(200, Some(&body), &questions(), key),
            Ok(json!({"q": {"type": "noul", "noul": 0.8, "why": "[redacted]",
                "notes": ["[redacted]", "[redacted]", "x [redacted] y"]}}))
        );
        // The key sits inside its JSON-escaped form, so the longer form goes first.
        let key = "\\tsk\\";
        let body = serde_json::to_vec(&json!({"answers": {"q": {
            "type": "noul", "noul": 0.8, "why": "\\\\tsk\\\\",
        }}}))
        .unwrap();
        assert_eq!(
            read_response(200, Some(&body), &questions(), key),
            Ok(json!({"q": {"type": "noul", "noul": 0.8, "why": "[redacted]"}}))
        );
    }

    #[test]
    fn the_key_leaves_the_answers() {
        let body = br#"{"answers": {"q": {"type": "noul", "noul": 0.8, "why": "your key plainkey123", "notes": ["plainkey123", {"deep": "x plainkey123 y"}]}}}"#;
        assert_eq!(
            read_response(200, Some(body), &questions(), KEY),
            Ok(json!({"q": {"type": "noul", "noul": 0.8, "why": "your key [redacted]",
                "notes": ["[redacted]", {"deep": "x [redacted] y"}]}}))
        );
        let clean = br#"{"answers": {"q": {"type": "noul", "noul": 0.8, "why": "sk-ABCDEFGHIJKLMNOPQRSTUVWX"}}}"#;
        assert_eq!(
            read_response(200, Some(clean), &questions(), KEY),
            Ok(json!({"q": {"type": "noul", "noul": 0.8, "why": "sk-ABCDEFGHIJKLMNOPQRSTUVWX"}}))
        );
        // Only the key goes; the shape rules do not run on answers.
        let both = br#"{"answers": {"q": {"type": "noul", "noul": 0.8, "why": "plainkey123 sk-ABCDEFGHIJKLMNOPQRSTUVWX"}}}"#;
        assert_eq!(
            read_response(200, Some(both), &questions(), KEY),
            Ok(json!({"q": {"type": "noul", "noul": 0.8, "why": "[redacted] sk-ABCDEFGHIJKLMNOPQRSTUVWX"}}))
        );
    }

    #[test]
    fn an_answer_that_names_the_key_is_bad_answer() {
        let bodies: [&[u8]; 3] = [
            br#"{"answers": {"q": {"type": "noul", "noul": 0.8, "why": {"plainkey123": 1}}}}"#,
            br#"{"answers": {"q": {"type": "noul", "noul": 0.8}, "plainkey123": {}}}"#,
            br#"{"answers": {"q": {"type": "noul", "noul": 0.8, "why": [{"x plainkey123 y": 1}]}}}"#,
        ];
        for body in bodies {
            assert_eq!(
                read_response(200, Some(body), &questions(), KEY),
                Err(JevFailure::new(ErrorCode::BadAnswer)),
                "{}",
                String::from_utf8_lossy(body)
            );
        }
        let fine = br#"{"answers": {"q": {"type": "noul", "noul": 0.8, "why": {"plainkey12": [{"note": 1}]}}}}"#;
        assert_eq!(
            read_response(200, Some(fine), &questions(), KEY),
            Ok(json!({"q": {"type": "noul", "noul": 0.8, "why": {"plainkey12": [{"note": 1}]}}}))
        );
    }

    /// An answer body whose `answers` value is `depth` levels deep.
    fn deep_answer(depth: usize) -> Vec<u8> {
        // `answers` and `q` are two levels; the arrays in `deep` are the rest.
        let mut deep = json!([]);
        for _ in 1..depth - 2 {
            deep = json!([deep]);
        }
        let answers = json!({"q": {"type": "noul", "noul": 0.8, "deep": deep}});
        assert_eq!(crate::shape::depth(&answers), depth);
        serde_json::to_vec(&json!({"answers": answers})).unwrap()
    }

    #[test]
    fn answers_past_the_depth_cap_are_bad_answer() {
        assert!(read_response(200, Some(&deep_answer(64)), &questions(), KEY).is_ok());
        assert_eq!(
            read_response(200, Some(&deep_answer(65)), &questions(), KEY),
            Err(JevFailure::new(ErrorCode::BadAnswer))
        );
    }

    #[test]
    fn an_error_body_that_is_not_utf8_still_gives_a_detail() {
        assert_eq!(
            read_response(502, Some(b"bad \xff\xfe gateway"), &questions(), KEY)
                .unwrap_err()
                .detail
                .as_deref(),
            Some("bad \u{fffd}\u{fffd} gateway")
        );
    }

    #[test]
    fn an_extra_question_id_in_the_answers_passes() {
        let body = br#"{"answers": {"q": {"type": "noul", "noul": 0.8}, "extra": {"type": "noul", "noul": 0.1}}}"#;
        assert_eq!(
            read_response(200, Some(body), &questions(), KEY),
            Ok(json!({"q": {"type": "noul", "noul": 0.8}, "extra": {"type": "noul", "noul": 0.1}}))
        );
    }
}
```

- [ ] **Step 2: Run the tests to check that they fail**

Run: `cargo test -p jevd jev::`
Expected: FAIL to compile, with `cannot find function request_body`, `cannot find function percent_encode`, `cannot find type JevFailure`, and `cannot find macro json`.

- [ ] **Step 3: Write the implementation**

Add this code above `#[cfg(test)]` in `crates/jevd/src/jev.rs`:

```rust
//! One request to TypeSafe's Jev model: the body, and what a response means
//! (spec section 4, steps 6 and 7). `jev_http` does the sending.

use jev_proto::{ErrorCode, WireError};
use serde_json::{json, Value};

use crate::answer::valid_answers;
use crate::redact::{redact, redact_value, REDACTED};
use crate::shape::{depth, MAX_DEPTH};

pub const ENDPOINT: &str = "https://api.typesafe.ai/v1/systemone";
pub const MODEL: &str = "jev-latest";
/// Characters, counted after redaction.
pub const DETAIL_LIMIT: usize = 300;
pub const MAX_RESPONSE_BYTES: usize = 1 << 20;
pub const MAX_ERROR_BYTES: usize = 4096;

/// Why a request to Jev failed. `detail` is already redacted and cut.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JevFailure {
    pub code: ErrorCode,
    pub status: Option<u16>,
    pub detail: Option<String>,
}

impl JevFailure {
    pub fn new(code: ErrorCode) -> JevFailure {
        JevFailure {
            code,
            status: None,
            detail: None,
        }
    }

    /// The wire error for this failure. The detail goes into the message.
    pub fn to_wire(&self) -> WireError {
        let summary = match self.code {
            ErrorCode::JevUnauthorized => "Jev refused the key".to_string(),
            ErrorCode::JevHttpError => match self.status {
                Some(status) => format!("Jev returned HTTP {status}"),
                None => "Jev returned an HTTP error".to_string(),
            },
            ErrorCode::JevTimeout => "Jev did not answer in time".to_string(),
            ErrorCode::JevUnreachable => "Jev could not be reached".to_string(),
            ErrorCode::BadAnswer => "Jev's answer failed the answer check".to_string(),
            other => format!("the request to Jev failed ({})", other.as_str()),
        };
        let message = match &self.detail {
            Some(detail) => format!("{summary}: {detail}"),
            None => summary,
        };
        WireError {
            status: self.status,
            ..WireError::new(self.code, message)
        }
    }
}

/// The body for Jev. Question text is caller content too, so it gets the same
/// redaction as state. `key` must come from `check_key`.
pub fn request_body(state: &Value, questions: &Value, key: &str, strict: bool) -> Value {
    json!({
        "model": MODEL,
        "state": redact_value(state, Some(key), strict),
        "questions": redact_value(questions, Some(key), strict),
    })
}

/// Redacted first, then cleaned, then cut. A cut first could leave part of a
/// token too short to match, and the PEM rule needs the newlines that the
/// cleaning removes. `None` when nothing is left.
pub fn make_detail(text: &str, key: &str) -> Option<String> {
    let mut text = text.to_string();
    for form in key_forms(key) {
        text = text.replace(&form, REDACTED);
    }
    let cleaned: String = redact(&text, Some(key))
        .chars()
        .map(|c| if is_unsafe(c) { ' ' } else { c })
        .collect();
    let detail: String = cleaned.trim().chars().take(DETAIL_LIMIT).collect();
    if detail.is_empty() {
        None
    } else {
        Some(detail)
    }
}

/// A character that could move the cursor, change the terminal, or reorder
/// text when the detail is printed.
fn is_unsafe(c: char) -> bool {
    c.is_control()
        || ('\u{202a}'..='\u{202e}').contains(&c)
        || ('\u{2066}'..='\u{2069}').contains(&c)
        || matches!(c, '\u{200e}' | '\u{200f}' | '\u{061c}')
}

/// The encoded forms of the key that an error page might echo: JSON-escaped,
/// JSON-escaped with `\/`, and percent-encoded. Longest first, so a shorter
/// form never cuts into a longer one. Forms equal to the literal key are left
/// to `redact`.
fn key_forms(key: &str) -> Vec<String> {
    let quoted = serde_json::to_string(key).expect("a string always serializes");
    let json = quoted[1..quoted.len() - 1].to_string();
    let slashed = json.replace('/', "\\/");
    let mut forms: Vec<String> = Vec::new();
    for form in [json, slashed, percent_encode(key)] {
        if form != key && !forms.contains(&form) {
            forms.push(form);
        }
    }
    forms.sort_by_key(|form| std::cmp::Reverse(form.len()));
    forms
}

/// Every byte that is not an ASCII letter, digit, `-`, `.`, `_`, or `~`
/// becomes `%XX`, uppercase.
fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// True when an object key, at any level, contains the key. Such an answer is
/// refused rather than renamed, since renamed keys could collide. Called only
/// after the depth check, so the recursion is bounded.
fn names_key(value: &Value, key: &str) -> bool {
    match value {
        Value::Array(items) => items.iter().any(|item| names_key(item, key)),
        Value::Object(map) => map
            .iter()
            .any(|(name, item)| name.contains(key) || names_key(item, key)),
        _ => false,
    }
}

/// The encoded forms of the key and then the literal key, longest first, as
/// `make_detail` replaces them. Escaping only adds characters, so every form
/// that differs from the key is longer than it, and the key goes last.
fn scrub_forms(key: &str) -> Vec<String> {
    let mut forms = key_forms(key);
    forms.push(key.to_string());
    forms
}

/// Replaces the key and each of its encoded `forms` in every string value.
/// Object keys are checked by `names_key` instead. Called only after the
/// depth check, so the recursion is bounded.
fn scrub_key(value: &mut Value, forms: &[String]) {
    match value {
        Value::String(text) => {
            for form in forms {
                if text.contains(form.as_str()) {
                    *text = text.replace(form.as_str(), REDACTED);
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(|item| scrub_key(item, forms)),
        Value::Object(map) => map.values_mut().for_each(|item| scrub_key(item, forms)),
        _ => {}
    }
}

/// What one final response means. `body` is `None` when it went over its cap.
/// Retries happen before this, so a 429 or a 529 here is final.
pub fn read_response(
    status: u16,
    body: Option<&[u8]>,
    questions: &Value,
    key: &str,
) -> Result<Value, JevFailure> {
    if status == 200 {
        let answers = body
            .and_then(|bytes| serde_json::from_slice::<Value>(bytes).ok())
            .and_then(|mut value| value.get_mut("answers").map(Value::take));
        return match answers {
            Some(mut answers)
                if depth(&answers) <= MAX_DEPTH
                    && valid_answers(&answers, questions)
                    && !names_key(&answers, key) =>
            {
                scrub_key(&mut answers, &scrub_forms(key));
                Ok(answers)
            }
            _ => Err(JevFailure::new(ErrorCode::BadAnswer)),
        };
    }
    let code = if status == 401 || status == 403 {
        ErrorCode::JevUnauthorized
    } else {
        ErrorCode::JevHttpError
    };
    let detail = body.and_then(|bytes| make_detail(&String::from_utf8_lossy(bytes), key));
    Err(JevFailure {
        code,
        status: Some(status),
        detail,
    })
}
```

- [ ] **Step 4: Run the tests to check that they pass**

Run: `cargo test -p jevd jev::`
Expected: PASS, 24 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/jevd/src/lib.rs crates/jevd/src/jev.rs
git commit -m "feat(jevd): the Jev body, the error detail, and the response check" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: The Jev HTTPS client (`jevd::jev_http`)

Spec section 4, step 6: 2 attempts of 15 seconds each, with a 1-second pause. Retry only on 429 and 529. Never follow a redirect, because a redirect would resend the `Authorization` header to the new host. Response capped at 1 MiB, error body at 4 KiB.

Differences from the prototype:

- `reqwest`'s client timeout covers the connection, the headers, and the whole body. The prototype needed a worker thread and a deadline check between reads to get the same bound.
- A timeout or a network failure is never retried, as in the prototype.
- The `Authorization` header is marked sensitive, so `reqwest` leaves it out of its `Debug` output.
- A network failure's text can hold the URL, so it goes through `make_detail`.
- A loopback endpoint skips any proxy, so the tests work when `HTTP_PROXY` is set. The prototype does the same for `127.0.0.1`.
- Only a loopback endpoint may use plain `http`. Any other endpoint must be `https` (`https_only`), so the `Authorization` header never goes over plain `http` to a real host. reqwest refuses such a request before any network I/O, and the result is `jev-unreachable`.
- If a non-200 error body fails partway with an error that is not a timeout, the status still counts: the body is treated as missing, as when it goes over its cap. So a 401 whose body is cut short stays `jev-unauthorized`, and a 429 is still retried. A timeout is still `jev-timeout`, and a 200 body that fails is still `jev-unreachable`.

The tests use `wiremock`, a fake HTTP server on `127.0.0.1`. For a body that drips or is cut short, they use a raw `TcpListener` on `127.0.0.1`, because `wiremock`'s `set_delay` only delays the whole response. They use short timeouts and a 10 ms pause.

**Files:**
- Modify: `crates/jevd/Cargo.toml`
- Modify: `crates/jevd/src/lib.rs`
- Create: `crates/jevd/src/jev_http.rs`

- [ ] **Step 1: Add the dependencies**

In `crates/jevd/Cargo.toml`, add these 2 lines to `[dependencies]`, in alphabetical order:

```toml
reqwest = { version = "0.13", default-features = false, features = ["rustls", "system-proxy"] }
tokio = { version = "1", features = ["time"] }
```

Add this table after the target tables:

```toml
[dev-dependencies]
tokio = { version = "1", features = ["macros", "rt"] }
wiremock = "0.6"
```

Run: `cargo build -p jevd`
Expected: PASS. The first build compiles `aws-lc-sys`, which takes a few minutes.

Run the Task 1 Step 3 MSRV check again.
Expected: `[]`

- [ ] **Step 2: Write the failing tests**

In `crates/jevd/src/lib.rs`, add `pub mod jev_http;` after `pub mod jev;`.

Create `crates/jevd/src/jev_http.rs` with only the tests:

```rust
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
        assert_eq!((failure.code, failure.status), (ErrorCode::JevHttpError, Some(429)));
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
        assert_eq!((failure.code, failure.status), (ErrorCode::JevHttpError, Some(500)));
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
        assert_eq!((failure.code, failure.status), (ErrorCode::JevHttpError, Some(302)));
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
        assert!(started.elapsed() < Duration::from_secs(2), "{:?}", started.elapsed());
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
        assert!(started.elapsed() >= Duration::from_millis(500), "{:?}", started.elapsed());
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
                    let Ok(read) = stream.read(&mut buffer) else { break false };
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
        assert!(started.elapsed() < Duration::from_secs(2), "{:?}", started.elapsed());
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
        assert_eq!((failure.code, failure.status), (ErrorCode::JevUnreachable, None));
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
```

- [ ] **Step 3: Run the tests to check that they fail**

Run: `cargo test -p jevd jev_http`
Expected: FAIL to compile, with `cannot find type JevClient` and `cannot find type Duration`.

- [ ] **Step 4: Write the implementation**

Add this code above `#[cfg(test)]` in `crates/jevd/src/jev_http.rs`:

```rust
//! Sends one request to Jev over HTTPS (spec section 4, step 6).

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
```

- [ ] **Step 5: Run the tests to check that they pass**

Run: `cargo test -p jevd jev_http`
Expected: PASS, 24 passed. The unreachable-server tests take about 2 seconds on Windows.

Run: `cargo test --workspace`
Expected: PASS. jev-proto has 16 tests. jevd has 175 passed and 1 ignored.

- [ ] **Step 6: Commit**

```bash
git add Cargo.lock crates/jevd/Cargo.toml crates/jevd/src/lib.rs crates/jevd/src/jev_http.rs
git commit -m "feat(jevd): the Jev HTTPS client, with one retry, size caps, and no redirects" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 7: Lints, full test run, and spec updates

**Files:**
- Modify: `docs/superpowers/specs/2026-11-02-jev-daemon-design.md`
- Modify: this plan, if a test count changed

- [ ] **Step 1: Format and lint**

Run: `cargo fmt --all`
Then: `cargo fmt --all -- --check`
Expected: no output, exit 0.

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: no warnings. Fix each warning in the code. Do not add `#[allow]`.

- [ ] **Step 2: Full test run**

Run: `cargo test --workspace --locked`
Expected: PASS. jev-proto has 16 tests. jevd has 175 passed and 1 ignored on Windows, and 176 passed and 1 ignored on Unix (plan 1's file-mode test). These counts include the tests that the mutation self-checks and the reviews added.

- [ ] **Step 3: Record this plan's choices in the spec**

In section 4, step 6 ("Send"), add this sentence at the end of the step:

```markdown
A 401 or a 403 is `jev-unauthorized`. Any other status that is not 200, a redirect included, is `jev-http-error` with the status. A 200 body over its cap is `bad-answer`; an error body over its cap gives no detail. Only an `https` endpoint is used. When an error body cannot be read for a reason other than a timeout, the status still counts.
```

In section 4, step 7 ("Answer check"), add at the end of the step:

```markdown
An answer deeper than 64 levels, or one with an object key that contains the key, is `bad-answer`. `jevd` replaces the key and its encoded forms in the answer's string values.
```

In section 4, replace the line "`jevd` redacts every error `detail` and cuts it to 300 characters." with:

```markdown
`jevd` redacts every error `detail`, including the JSON-escaped and percent-encoded forms of the key, turns control and bidi characters into spaces, and then cuts it to 300 characters.
```

In section 2, "Key cache", add this bullet:

```markdown
- `jevd` reads a key off the cache lock, on a blocking thread, and only one read per profile runs at a time.
```

In section 10, "Limits to document", add:

```markdown
- On a domain-joined Windows machine with roaming profiles, Credential Manager can copy jev's keys and client tokens to other machines where the same user signs in. The `keyring` crate stores them with `CRED_PERSIST_ENTERPRISE`.
- `jevd` uses the system and environment proxy settings. A proxy cannot read the key over HTTPS, but a TLS-inspecting proxy whose root certificate the OS trusts can.
```

In section 12, "Checks for the plan", replace the "Linux keyring backend" item with:

```markdown
- **Linux keyring backend.** Done in plan 2: `keyring` 3.6 with `async-secret-service`, `crypto-rust`, and `async-io`. It uses the pure-Rust `zbus`, so the build needs no system `libdbus` and no OpenSSL. `keyring` 4 needs Rust 1.88, above the MSRV. TLS uses `reqwest`'s `rustls` with `aws-lc-rs`, whose `aws-lc-sys` compiles C code, so plan 6 still picks between a `gnu` and a static `musl` build. The Linux and macOS cross-check passed for the keyring code before `reqwest` came in; on the final tree, a cross-check needs a C cross compiler or a scratch copy with `reqwest`'s `rustls-no-provider` feature, and plan 6's CI is the real check.
```

Replace the "Crate names" item with:

```markdown
- **Crate names.** Checked on 2026-11-02: `jevd` and `jev-proto` are free on crates.io, and `jev` is taken. Plan 6 decides the published name of the CLI crate. The binary can stay `jev`.
```

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "chore: format, lint, and record plan 2 choices in the spec" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

## Not tested by this plan

- The real OS keyring runs only in the ignored test, by hand. CI never touches it.
- Linux and macOS: no test ran there. The keyring feature set passed `cargo check` for both targets before `reqwest` came in (Task 6). Since then, `aws-lc-sys` needs a C cross compiler, so on Windows a plain cross-check fails in its build script. The final review ran `cargo check -p jevd --lib` for both targets in a scratch copy with `reqwest`'s `rustls-no-provider` feature, and it passed. Plan 6's CI, which runs the suite on all 3, is the real check.
- The real Jev endpoint. No test sends a request to `api.typesafe.ai`.
- The proxy path with a real proxy.
- The `compile_error!` for an unsupported OS was not compiled on an unsupported target.
- Plan 3 wires the key cache and the client into the daemon: the order of checks, `spawn_blocking` for keyring calls, and when `forget` and `clear` run.

## Notes for plan 3

- Runtime shutdown can hang on an open keyring prompt. Call `clear()`, then `Runtime::shutdown_timeout`.
- "Stop lets requests in flight finish" needs a limit for requests that wait on a keyring read.
- One `ask` can take about 31 s in Jev alone, plus any keyring wait. So the 60 s idle-connection timer must not run during a request, idle exit must not fire while a read or a Jev call runs, and the CLI and the mod need a reply timeout longer than that budget.
- Build `JevClient` once and share it. Map an `Err` from `new()` to a start-up failure. Rebuild it on `reload`, or document that a proxy change needs a restart.
- A Jev 401 or 403 needs no `forget`: `jev key add` sends `reload`, which clears every key.
- Pass the key cache's key to `JevClient::ask` only. Consider a `CheckedKey` newtype.
- Keep `with_endpoint` test-only. A loopback endpoint turns off the https-only rule and the proxy.
- Plan 4 must clean control and bidi characters from `error.message` before it prints it.
- A locked or denied keyring is `internal` (exit 1). Plan 4 may want to treat it apart.
- `MemoryKeyring` could move behind a `test-util` feature.
- `jevd`'s tokio needs `rt-multi-thread`, `net`, `sync`, `signal`, and `io-util`.
