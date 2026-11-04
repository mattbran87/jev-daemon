# jev-daemon core library — implementation plan (plan 1 of 7)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the Cargo workspace, the `jev-proto` wire types, and the pure `jevd` library modules (units, shape check, answer check, key check, redaction, rate limit, settings, pairing store), all tested, with no network and no IPC.

**Architecture:** A 3-crate workspace from the spec (`docs/superpowers/specs/2026-11-02-jev-daemon-design.md`, section 1). This plan fills `crates/jev-proto` and the library part of `crates/jevd`. `crates/jev` and the `jevd` binary stay stubs until plans 3 and 4. Every function that depends on time takes `now` as an argument, so tests use fixed instants and never sleep.

**Tech Stack:** Rust stable (edition 2021), `serde` 1, `serde_json` 1, `regex` 1, `sha2` 0.10, `hex` 0.4, `rand` 0.8, `toml` 0.8, `tempfile` 3.

**Later plans:** 2 keys and the Jev client, 3 daemon and transport, 4 CLI, 5 Claude Code mod, 6 distribution and CI, 7 docs.

**Reference code:** the private Python prototype's `jevservice` package (`names.py`, `redact.py`, `ratelimit.py`, `typesafe.py`). The ports below keep its rules. Each port notes where Rust differs.

**Commit rule:** gate each commit on `cargo test`'s own exit status. Never pipe the test run into another command before the commit decision. End every commit message with the line `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

Code blocks show each task's logic as written; Task 11 ran `cargo fmt`, so the source layout differs.

---

## File structure

| File | Responsibility |
| --- | --- |
| `Cargo.toml` | Workspace: members, shared package fields |
| `.gitignore` | Ignore `/target` |
| `LICENSE` | Apache-2.0 text, copied from the prototype |
| `crates/jev-proto/src/lib.rs` | Re-exports |
| `crates/jev-proto/src/error.rs` | `ErrorCode`, exit codes, `WireError` |
| `crates/jev-proto/src/message.rs` | `Request`, `Reply`, `PROTOCOL_VERSION` |
| `crates/jev-proto/src/names.rs` | `valid_name` for client and profile names |
| `crates/jevd/src/lib.rs` | Module list |
| `crates/jevd/src/main.rs` | Stub binary (plan 3 fills it) |
| `crates/jevd/src/units.rs` | `Period` (`2h`, `30m`, `off`) and `Limit` (`120/60`), and their "loosens" comparisons |
| `crates/jevd/src/shape.rs` | Request shape check |
| `crates/jevd/src/answer.rs` | Answer check |
| `crates/jevd/src/key.rs` | Key shape check |
| `crates/jevd/src/redact.rs` | Redaction, normal and strict |
| `crates/jevd/src/ratelimit.rs` | Sliding-window rate limit per client |
| `crates/jevd/src/files.rs` | Atomic file write |
| `crates/jevd/src/settings.rs` | Settings file, defaults, changes, and the loosening rule |
| `crates/jevd/src/pairing.rs` | Pending requests, codes, tokens, the pairing file |
| `crates/jev/src/main.rs` | Stub binary (plan 4 fills it) |

---

### Task 0: Install the Rust toolchain (person, not agent)

This machine has no `cargo` or `rustc`. An agent cannot finish this task, because the installer and the Visual Studio Build Tools need a person.

- [ ] **Step 1: Install the MSVC build tools, if missing.** Rust on Windows needs the "Desktop development with C++" workload from Visual Studio or the Visual Studio Build Tools.

- [ ] **Step 2: Install Rust with rustup.** Download and run `rustup-init.exe` from https://rustup.rs. Accept the default (stable, `x86_64-pc-windows-msvc`).

- [ ] **Step 3: Open a new terminal, and check the install**

Run: `cargo --version` and `rustc --version`
Expected: both print a version. No "command not found".

- [ ] **Step 4: Add the lint and format tools**

Run: `rustup component add clippy rustfmt`
Expected: both components install, or rustup reports them as up to date.

---

### Task 1: Workspace scaffold

**Files:**
- Create: `Cargo.toml`, `.gitignore`, `LICENSE`
- Create: `crates/jev-proto/Cargo.toml`, `crates/jev-proto/src/lib.rs`
- Create: `crates/jevd/Cargo.toml`, `crates/jevd/src/lib.rs`, `crates/jevd/src/main.rs`
- Create: `crates/jev/Cargo.toml`, `crates/jev/src/main.rs`

- [ ] **Step 1: Create the workspace manifest**

`Cargo.toml`:

```toml
[workspace]
resolver = "2"
members = ["crates/jev-proto", "crates/jevd", "crates/jev"]

[workspace.package]
version = "0.1.0"
edition = "2021"
license = "Apache-2.0"
rust-version = "1.85"
repository = "https://github.com/mattbran87/jev-daemon"
```

`.gitignore`:

```gitignore
/target
```

- [ ] **Step 2: Copy the license**

Copy the private prototype's `LICENSE` file (Apache-2.0) to `LICENSE`.
Expected: `LICENSE` exists and starts with "Apache License".

- [ ] **Step 3: Create the 3 crate manifests**

`crates/jev-proto/Cargo.toml`:

```toml
[package]
name = "jev-proto"
description = "Wire types for talking to jevd, the jev daemon"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true
repository.workspace = true

[dependencies]
```

`crates/jevd/Cargo.toml`:

```toml
[package]
name = "jevd"
description = "The jev daemon: holds TypeSafe keys and sends checked requests to Jev"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true
repository.workspace = true

[dependencies]
```

`crates/jev/Cargo.toml`:

```toml
[package]
name = "jev"
description = "The jev command line"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true
repository.workspace = true

[dependencies]
```

- [ ] **Step 4: Create the stub sources**

`crates/jev-proto/src/lib.rs`:

```rust
//! Wire types shared by jevd, the jev CLI, and third-party clients.
```

`crates/jevd/src/lib.rs`:

```rust
//! The jev daemon's library: checks, settings, and pairing.
```

`crates/jevd/src/main.rs`:

```rust
fn main() {
    println!("jevd {}", env!("CARGO_PKG_VERSION"));
}
```

`crates/jev/src/main.rs`:

```rust
fn main() {
    println!("jev {}", env!("CARGO_PKG_VERSION"));
}
```

- [ ] **Step 5: Add the dependencies**

Run each command from the repo root:

```bash
cargo add -p jev-proto serde@1 --features derive
cargo add -p jev-proto serde_json@1
cargo add -p jevd serde@1 --features derive
cargo add -p jevd serde_json@1 regex@1 sha2@0.10 hex@0.4 rand@0.8 toml@0.8
cargo add -p jevd --path crates/jev-proto
cargo add -p jevd tempfile@3
```

Expected: each command prints `Adding ...` lines and no error.

- [ ] **Step 6: Build**

Run: `cargo build --workspace`
Expected: `Finished` with no error. `Cargo.lock` now exists.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock .gitignore LICENSE crates
git commit -m "chore: Cargo workspace with jev-proto, jevd, and jev" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: Error codes and exit codes (`jev-proto`)

**Files:**
- Create: `crates/jev-proto/src/error.rs`
- Modify: `crates/jev-proto/src/lib.rs`

- [ ] **Step 1: Write the failing tests**

`crates/jev-proto/src/error.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_code_serializes_as_its_kebab_name() {
        for code in ErrorCode::ALL {
            let json = serde_json::to_string(&code).unwrap();
            assert_eq!(json, format!("\"{}\"", code.as_str()));
            let back: ErrorCode = serde_json::from_str(&json).unwrap();
            assert_eq!(back, code);
        }
    }

    #[test]
    fn exit_codes_match_the_spec_table() {
        let table = [
            (ErrorCode::Internal, 1),
            (ErrorCode::Pending, 3),
            (ErrorCode::NotPaired, 4),
            (ErrorCode::Revoked, 4),
            (ErrorCode::RateLimited, 5),
            (ErrorCode::NoKey, 6),
            (ErrorCode::BadKey, 6),
            (ErrorCode::JevUnauthorized, 6),
            (ErrorCode::JevHttpError, 7),
            (ErrorCode::JevTimeout, 7),
            (ErrorCode::JevUnreachable, 7),
            (ErrorCode::BadAnswer, 7),
            (ErrorCode::DaemonUnavailable, 8),
            (ErrorCode::ProtocolUnsupported, 8),
            (ErrorCode::Forbidden, 9),
            (ErrorCode::BadRequest, 9),
            (ErrorCode::TooLarge, 9),
        ];
        assert_eq!(table.len(), ErrorCode::ALL.len());
        for (code, exit) in table {
            assert_eq!(code.exit_code(), exit, "{}", code.as_str());
        }
    }

    #[test]
    fn wire_error_leaves_out_empty_fields() {
        let error = WireError::new(ErrorCode::NotPaired, "pair first");
        assert_eq!(
            serde_json::to_value(&error).unwrap(),
            serde_json::json!({"code": "not-paired", "message": "pair first"})
        );
        let limited = WireError {
            retry_after: Some(12),
            ..WireError::new(ErrorCode::RateLimited, "slow down")
        };
        assert_eq!(serde_json::to_value(&limited).unwrap()["retry_after"], 12);
    }
}
```

`crates/jev-proto/src/lib.rs`:

```rust
//! Wire types shared by jevd, the jev CLI, and third-party clients.

mod error;

pub use error::{ErrorCode, WireError};
```

- [ ] **Step 2: Run the tests and see them fail**

Run: `cargo test -p jev-proto`
Expected: compile errors: `cannot find type ErrorCode`, `cannot find type WireError`.

- [ ] **Step 3: Write the implementation**

Put this above the `#[cfg(test)]` block in `crates/jev-proto/src/error.rs`:

```rust
use serde::{Deserialize, Serialize};

/// Every error a client can get. Clients branch on these, never on message text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ErrorCode {
    Internal,
    Pending,
    NotPaired,
    Revoked,
    RateLimited,
    NoKey,
    BadKey,
    JevUnauthorized,
    JevHttpError,
    JevTimeout,
    JevUnreachable,
    BadAnswer,
    DaemonUnavailable,
    ProtocolUnsupported,
    Forbidden,
    BadRequest,
    TooLarge,
}

impl ErrorCode {
    pub const ALL: [ErrorCode; 17] = [
        ErrorCode::Internal,
        ErrorCode::Pending,
        ErrorCode::NotPaired,
        ErrorCode::Revoked,
        ErrorCode::RateLimited,
        ErrorCode::NoKey,
        ErrorCode::BadKey,
        ErrorCode::JevUnauthorized,
        ErrorCode::JevHttpError,
        ErrorCode::JevTimeout,
        ErrorCode::JevUnreachable,
        ErrorCode::BadAnswer,
        ErrorCode::DaemonUnavailable,
        ErrorCode::ProtocolUnsupported,
        ErrorCode::Forbidden,
        ErrorCode::BadRequest,
        ErrorCode::TooLarge,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            ErrorCode::Internal => "internal",
            ErrorCode::Pending => "pending",
            ErrorCode::NotPaired => "not-paired",
            ErrorCode::Revoked => "revoked",
            ErrorCode::RateLimited => "rate-limited",
            ErrorCode::NoKey => "no-key",
            ErrorCode::BadKey => "bad-key",
            ErrorCode::JevUnauthorized => "jev-unauthorized",
            ErrorCode::JevHttpError => "jev-http-error",
            ErrorCode::JevTimeout => "jev-timeout",
            ErrorCode::JevUnreachable => "jev-unreachable",
            ErrorCode::BadAnswer => "bad-answer",
            ErrorCode::DaemonUnavailable => "daemon-unavailable",
            ErrorCode::ProtocolUnsupported => "protocol-unsupported",
            ErrorCode::Forbidden => "forbidden",
            ErrorCode::BadRequest => "bad-request",
            ErrorCode::TooLarge => "too-large",
        }
    }

    /// The `jev` CLI's exit code for this error (spec section 6).
    pub fn exit_code(self) -> i32 {
        match self {
            ErrorCode::Internal => 1,
            ErrorCode::Pending => 3,
            ErrorCode::NotPaired | ErrorCode::Revoked => 4,
            ErrorCode::RateLimited => 5,
            ErrorCode::NoKey | ErrorCode::BadKey | ErrorCode::JevUnauthorized => 6,
            ErrorCode::JevHttpError
            | ErrorCode::JevTimeout
            | ErrorCode::JevUnreachable
            | ErrorCode::BadAnswer => 7,
            ErrorCode::DaemonUnavailable | ErrorCode::ProtocolUnsupported => 8,
            ErrorCode::Forbidden | ErrorCode::BadRequest | ErrorCode::TooLarge => 9,
        }
    }
}

/// The `error` object of a failed reply.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireError {
    pub code: ErrorCode,
    pub message: String,
    /// Seconds until a `rate-limited` client can try again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_after: Option<u64>,
    /// The HTTP status for `jev-http-error`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
}

impl WireError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        WireError {
            code,
            message: message.into(),
            retry_after: None,
            status: None,
        }
    }
}
```

- [ ] **Step 4: Run the tests and see them pass**

Run: `cargo test -p jev-proto`
Expected: `test result: ok. 3 passed`.

- [ ] **Step 5: Commit**

```bash
git add crates/jev-proto
git commit -m "feat(proto): error codes, exit codes, and the wire error" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: Messages and names (`jev-proto`)

The spec's message table lacks one message that `jev allow` needs: a list of pending requests. This task adds the admin message `pending`. Task 11 adds it to the spec.

**Files:**
- Create: `crates/jev-proto/src/message.rs`, `crates/jev-proto/src/names.rs`
- Modify: `crates/jev-proto/src/lib.rs`

- [ ] **Step 1: Write the failing tests**

`crates/jev-proto/src/names.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_follow_the_prototype_rule() {
        assert!(valid_name("commit-scope"));
        assert!(valid_name("a1"));
        assert!(valid_name("cc-my-plugin"));
        assert!(valid_name(&"a".repeat(64)));
        assert!(!valid_name(""));
        assert!(!valid_name("Commit"));
        assert!(!valid_name("-x"));
        assert!(!valid_name("a b"));
        assert!(!valid_name(&"a".repeat(65)));
    }
}
```

`crates/jev-proto/src/message.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ErrorCode, WireError};
    use serde_json::json;

    fn wire(request: &Request) -> Value {
        serde_json::to_value(request).unwrap()
    }

    #[test]
    fn requests_use_a_type_tag() {
        assert_eq!(wire(&Request::Hello { protocol: 1 }), json!({"type": "hello", "protocol": 1}));
        assert_eq!(wire(&Request::Status), json!({"type": "status"}));
        assert_eq!(
            wire(&Request::PairStatus { request_id: "ab".into() }),
            json!({"type": "pair-status", "request_id": "ab"})
        );
        assert_eq!(
            wire(&Request::Pair { name: "x".into(), profile: None }),
            json!({"type": "pair", "name": "x"})
        );
        assert_eq!(
            wire(&Request::SetLimit { name: "x".into(), limit: Some("10/60".into()) }),
            json!({"type": "set-limit", "name": "x", "limit": "10/60"})
        );
    }

    #[test]
    fn requests_read_back() {
        let line = r#"{"type":"ask","token":"jevt_1","state":"s","questions":{"q":{"type":"noul"}}}"#;
        let request: Request = serde_json::from_str(line).unwrap();
        assert_eq!(
            request,
            Request::Ask {
                token: "jevt_1".into(),
                state: json!("s"),
                questions: json!({"q": {"type": "noul"}}),
            }
        );
        assert!(serde_json::from_str::<Request>(r#"{"type":"nope"}"#).is_err());
    }

    #[test]
    fn only_admin_messages_are_admin() {
        let admin = [
            Request::Pending,
            Request::Approve { code: "C".into(), profile: "default".into() },
            Request::Deny { code: "C".into() },
            Request::Revoke { name: "x".into() },
            Request::Clients,
            Request::SetLimit { name: "x".into(), limit: None },
            Request::Reload,
            Request::Stop,
        ];
        assert!(admin.iter().all(Request::is_admin));
        let open = [
            Request::Hello { protocol: 1 },
            Request::Status,
            Request::Pair { name: "x".into(), profile: None },
            Request::PairStatus { request_id: "r".into() },
            Request::Ask { token: "t".into(), state: json!(null), questions: json!({}) },
        ];
        assert!(!open.iter().any(Request::is_admin));
    }

    #[test]
    fn debug_hides_ask_secrets() {
        let ask = Request::Ask { token: "jevt_secret".into(), state: json!("private text"), questions: json!({}) };
        let shown = format!("{:?}", ask);
        assert!(!shown.contains("jevt_secret"));
        assert!(!shown.contains("private text"));
        assert!(shown.contains("Ask"));
        assert_eq!(format!("{:?}", Request::Status), "Status");
    }

    #[test]
    fn debug_hides_pair_status_request_id_and_approve_code() {
        let status = Request::PairStatus { request_id: "req-sample-id".into() };
        let approve = Request::Approve { code: "CODE2345".into(), profile: "default".into() };
        let shown = format!("{:?} {:?}", status, approve);
        assert!(!shown.contains("req-sample-id"));
        assert!(!shown.contains("CODE2345"));
        assert!(shown.contains("PairStatus"));
        assert!(shown.contains("Approve"));
    }

    #[test]
    fn replies_carry_ok_and_flat_data() {
        let reply = Reply::success(json!({"code": "ABCD2345"}));
        assert_eq!(serde_json::to_value(&reply).unwrap(), json!({"ok": true, "code": "ABCD2345"}));
        let failure = Reply::failure(WireError::new(ErrorCode::Forbidden, "admin only"));
        assert_eq!(
            serde_json::to_value(&failure).unwrap(),
            json!({"ok": false, "error": {"code": "forbidden", "message": "admin only"}})
        );
        let back: Reply = serde_json::from_str(r#"{"ok":true,"state":"pending"}"#).unwrap();
        assert!(back.ok);
        assert_eq!(back.data["state"], "pending");
        assert!(failure.to_line().ends_with('\n'));
        assert_eq!(failure.to_line().matches('\n').count(), 1);
    }
}
```

`crates/jev-proto/src/lib.rs`:

```rust
//! Wire types shared by jevd, the jev CLI, and third-party clients.

mod error;
mod message;
mod names;

pub use error::{ErrorCode, WireError};
pub use message::{Reply, Request, PROTOCOL_VERSION};
pub use names::valid_name;
```

- [ ] **Step 2: Run the tests and see them fail**

Run: `cargo test -p jev-proto`
Expected: compile errors: `cannot find function valid_name`, `cannot find type Request`.

- [ ] **Step 3: Write the implementation**

Put this above the tests in `crates/jev-proto/src/names.rs`:

```rust
/// A client or profile name: lowercase letters, digits, and hyphens,
/// 1 to 64 characters, not starting with a hyphen.
pub fn valid_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 64
        && bytes[0] != b'-'
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
}
```

Put this above the tests in `crates/jev-proto/src/message.rs`:

```rust
use crate::error::WireError;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// The protocol version that `hello` carries.
pub const PROTOCOL_VERSION: u32 = 1;

/// One request line from a client to jevd.
///
/// `Debug` is written by hand: it must not print the token, state, or questions
/// of `Ask`, the `request_id` of `PairStatus` (the bearer capability that
/// collects a token), or the `code` of `Approve`.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Request {
    Hello {
        protocol: u32,
    },
    Status,
    Pair {
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        profile: Option<String>,
    },
    PairStatus {
        request_id: String,
    },
    Ask {
        token: String,
        state: Value,
        questions: Value,
    },
    /// Admin: list the pending pairing requests, for `jev allow`.
    Pending,
    Approve {
        code: String,
        profile: String,
    },
    Deny {
        code: String,
    },
    Revoke {
        name: String,
    },
    Clients,
    /// `limit` is `<count>/<seconds>`, or absent for the default.
    SetLimit {
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        limit: Option<String>,
    },
    Reload,
    Stop,
}

impl std::fmt::Debug for Request {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Request::Hello { protocol } => f.debug_struct("Hello").field("protocol", protocol).finish(),
            Request::Status => f.write_str("Status"),
            Request::Pair { name, profile } => {
                f.debug_struct("Pair").field("name", name).field("profile", profile).finish()
            }
            Request::PairStatus { .. } => {
                f.debug_struct("PairStatus").field("request_id", &"<redacted>").finish()
            }
            Request::Ask { .. } => f
                .debug_struct("Ask")
                .field("token", &"<redacted>")
                .field("state", &"<redacted>")
                .field("questions", &"<redacted>")
                .finish(),
            Request::Pending => f.write_str("Pending"),
            Request::Approve { profile, .. } => f
                .debug_struct("Approve")
                .field("code", &"<redacted>")
                .field("profile", profile)
                .finish(),
            Request::Deny { code } => f.debug_struct("Deny").field("code", code).finish(),
            Request::Revoke { name } => f.debug_struct("Revoke").field("name", name).finish(),
            Request::Clients => f.write_str("Clients"),
            Request::SetLimit { name, limit } => {
                f.debug_struct("SetLimit").field("name", name).field("limit", limit).finish()
            }
            Request::Reload => f.write_str("Reload"),
            Request::Stop => f.write_str("Stop"),
        }
    }
}

impl Request {
    /// True for the messages that jevd accepts only from its own `jev` binary.
    pub fn is_admin(&self) -> bool {
        matches!(
            self,
            Request::Pending
                | Request::Approve { .. }
                | Request::Deny { .. }
                | Request::Revoke { .. }
                | Request::Clients
                | Request::SetLimit { .. }
                | Request::Reload
                | Request::Stop
        )
    }
}

/// One reply line from jevd. Success data sits beside `ok`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Reply {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<WireError>,
    #[serde(flatten)]
    pub data: Map<String, Value>,
}

impl Reply {
    /// A success reply. `data` must be a JSON object; anything else gives no data.
    pub fn success(data: Value) -> Reply {
        let data = match data {
            Value::Object(map) => map,
            _ => Map::new(),
        };
        Reply { ok: true, error: None, data }
    }

    pub fn failure(error: WireError) -> Reply {
        Reply {
            ok: false,
            error: Some(error),
            data: Map::new(),
        }
    }

    /// The reply as one line of JSON, with its newline.
    pub fn to_line(&self) -> String {
        let mut line = serde_json::to_string(self).expect("a reply always serializes");
        line.push('\n');
        line
    }
}
```

- [ ] **Step 4: Run the tests and see them pass**

Run: `cargo test -p jev-proto`
Expected: `test result: ok. 8 passed`.

- [ ] **Step 5: Commit**

```bash
git add crates/jev-proto
git commit -m "feat(proto): request and reply messages, and the name rule" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: Periods and limits (`jevd::units`)

**Files:**
- Create: `crates/jevd/src/units.rs`
- Modify: `crates/jevd/src/lib.rs`

- [ ] **Step 1: Write the failing tests**

`crates/jevd/src/units.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn period(text: &str) -> Period {
        text.parse().unwrap()
    }

    fn limit(text: &str) -> Limit {
        text.parse().unwrap()
    }

    #[test]
    fn periods_parse_and_print() {
        assert_eq!(period("2h"), Period::After(Duration::from_secs(7200)));
        assert_eq!(period("30m"), Period::After(Duration::from_secs(1800)));
        assert_eq!(period("off"), Period::Off);
        assert_eq!(period("120m").to_string(), "2h");
        assert_eq!(period("90m").to_string(), "90m");
        assert_eq!(period("off").to_string(), "off");
        for bad in ["", "2", "h", "0h", "-1h", "+1h", "2d", "2 h", "1.5h", "2hé", "10001h"] {
            assert!(bad.parse::<Period>().is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_longer_period_or_off_loosens() {
        assert!(period("3h").looser_than(period("2h")));
        assert!(period("off").looser_than(period("2h")));
        assert!(!period("1h").looser_than(period("2h")));
        assert!(!period("2h").looser_than(period("2h")));
        assert!(!period("2h").looser_than(period("off")));
        assert!(!period("off").looser_than(period("off")));
    }

    #[test]
    fn limits_parse_and_print() {
        assert_eq!(limit("120/60"), DEFAULT_RATE_LIMIT);
        assert_eq!(limit("5/1").to_string(), "5/1");
        for bad in ["", "120", "/60", "120/", "0/60", "120/0", "a/60", "1000000/60", "120/10000000", "120/60/1"] {
            assert!(bad.parse::<Limit>().is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_higher_count_or_rate_loosens() {
        assert!(limit("200/60").looser_than(limit("120/60")));
        assert!(limit("120/30").looser_than(limit("120/60")));
        assert!(limit("130/600").looser_than(limit("120/60")));
        assert!(!limit("60/60").looser_than(limit("120/60")));
        assert!(!limit("120/120").looser_than(limit("120/60")));
        assert!(!limit("120/60").looser_than(limit("120/60")));
    }
}
```

`crates/jevd/src/lib.rs`:

```rust
//! The jev daemon's library: checks, settings, and pairing.

pub mod units;
```

- [ ] **Step 2: Run the tests and see them fail**

Run: `cargo test -p jevd units`
Expected: compile errors: `cannot find type Period`, `cannot find type Limit`.

- [ ] **Step 3: Write the implementation**

Put this above the tests in `crates/jevd/src/units.rs`:

```rust
//! Setting values: periods such as `2h`, and rate limits such as `120/60`.

use std::fmt;
use std::str::FromStr;
use std::time::Duration;

/// A period in minutes or hours, or `off`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Period {
    Off,
    After(Duration),
}

const MAX_PERIOD_NUMBER: u64 = 10_000;

impl FromStr for Period {
    type Err = String;

    fn from_str(text: &str) -> Result<Period, String> {
        if text == "off" {
            return Ok(Period::Off);
        }
        let problem = || format!("{text:?}: use minutes or hours, such as 30m or 2h, or off");
        let (number, seconds_each) = if let Some(number) = text.strip_suffix('h') {
            (number, 3600)
        } else if let Some(number) = text.strip_suffix('m') {
            (number, 60)
        } else {
            return Err(problem());
        };
        if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
            return Err(problem());
        }
        let count: u64 = number.parse().map_err(|_| problem())?;
        if count == 0 || count > MAX_PERIOD_NUMBER {
            return Err(problem());
        }
        Ok(Period::After(Duration::from_secs(count * seconds_each)))
    }
}

impl fmt::Display for Period {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Period::Off => write!(f, "off"),
            Period::After(duration) if duration.as_secs() % 3600 == 0 => {
                write!(f, "{}h", duration.as_secs() / 3600)
            }
            Period::After(duration) => write!(f, "{}m", duration.as_secs() / 60),
        }
    }
}

impl Period {
    /// True when `self` keeps jevd or a key around longer than `other`.
    pub fn looser_than(self, other: Period) -> bool {
        match (self, other) {
            (Period::Off, Period::Off) => false,
            (Period::Off, Period::After(_)) => true,
            (Period::After(_), Period::Off) => false,
            (Period::After(new), Period::After(old)) => new > old,
        }
    }
}

/// A rate limit: at most `count` requests in any `window`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limit {
    pub count: u32,
    pub window: Duration,
}

pub const DEFAULT_RATE_LIMIT: Limit = Limit {
    count: 120,
    window: Duration::from_secs(60),
};

const MAX_LIMIT_COUNT: u32 = 999_999;
const MAX_LIMIT_SECONDS: u64 = 9_999_999;

impl FromStr for Limit {
    type Err = String;

    fn from_str(text: &str) -> Result<Limit, String> {
        let problem = || format!("{text:?}: use <count>/<seconds>, such as 120/60");
        let (count, seconds) = text.split_once('/').ok_or_else(problem)?;
        let digits = |part: &str| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit());
        if !digits(count) || !digits(seconds) {
            return Err(problem());
        }
        let count: u32 = count.parse().map_err(|_| problem())?;
        let seconds: u64 = seconds.parse().map_err(|_| problem())?;
        if count == 0 || count > MAX_LIMIT_COUNT || seconds == 0 || seconds > MAX_LIMIT_SECONDS {
            return Err(problem());
        }
        Ok(Limit {
            count,
            window: Duration::from_secs(seconds),
        })
    }
}

impl fmt::Display for Limit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.count, self.window.as_secs())
    }
}

impl Limit {
    /// True when `self` allows a bigger burst or a higher average rate than `other`.
    pub fn looser_than(self, other: Limit) -> bool {
        let new_rate = u64::from(self.count) * other.window.as_secs();
        let old_rate = u64::from(other.count) * self.window.as_secs();
        self.count > other.count || new_rate > old_rate
    }
}
```

- [ ] **Step 4: Run the tests and see them pass**

Run: `cargo test -p jevd units`
Expected: `test result: ok. 4 passed`.

- [ ] **Step 5: Commit**

```bash
git add crates/jevd
git commit -m "feat(jevd): periods and rate limits, with the loosening rule" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: Shape check (`jevd::shape`)

Ported from `valid_questions` in `typesafe.py` and `check_ask` in `service.py`. The prototype caught deep nesting through Python's `RecursionError`. Rust uses an explicit limit, `MAX_DEPTH = 64`. `serde_json` refuses input nested deeper than 128 levels while it parses, so `depth` never recurses past that.

**Files:**
- Create: `crates/jevd/src/shape.rs`
- Modify: `crates/jevd/src/lib.rs`

- [ ] **Step 1: Write the failing tests**

`crates/jevd/src/shape.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn nested(levels: usize) -> Value {
        let mut value = json!("leaf");
        for _ in 0..levels {
            value = json!([value]);
        }
        value
    }

    #[test]
    fn questions_follow_the_prototype_rules() {
        assert!(valid_questions(&json!({"q1": {"type": "noul", "text": "Is this a test?"}})));
        assert!(valid_questions(&json!({"s": {"type": "score"}})));
        assert!(!valid_questions(&json!({})));
        assert!(!valid_questions(&json!([])));
        assert!(!valid_questions(&json!({"bad id!": {"type": "noul"}})));
        assert!(!valid_questions(&json!({"q": {"type": "other"}})));
        assert!(!valid_questions(&json!({"q": "noul"})));
        let mut long_id = serde_json::Map::new();
        long_id.insert("a".repeat(65), json!({"type": "noul"}));
        assert!(!valid_questions(&Value::Object(long_id)));
        let twenty: serde_json::Map<String, Value> =
            (0..20).map(|i| (format!("q{i}"), json!({"type": "noul"}))).collect();
        assert!(valid_questions(&Value::Object(twenty.clone())));
        let mut twenty_one = twenty;
        twenty_one.insert("q20".into(), json!({"type": "noul"}));
        assert!(!valid_questions(&Value::Object(twenty_one)));
    }

    #[test]
    fn depth_counts_arrays_and_objects() {
        assert_eq!(depth(&json!("text")), 0);
        assert_eq!(depth(&json!({"a": [1]})), 2);
        assert_eq!(depth(&nested(64)), 64);
    }

    #[test]
    fn check_ask_refuses_deep_state_and_bad_questions() {
        let questions = json!({"q": {"type": "noul"}});
        assert_eq!(check_ask(&nested(64), &questions), Ok(()));
        assert_eq!(check_ask(&nested(65), &questions), Err("state is nested too deeply"));
        assert!(check_ask(&json!("s"), &json!({})).is_err());
    }
}
```

Add `pub mod shape;` to `crates/jevd/src/lib.rs`, in sorted order:

```rust
//! The jev daemon's library: checks, settings, and pairing.

pub mod shape;
pub mod units;
```

- [ ] **Step 2: Run the tests and see them fail**

Run: `cargo test -p jevd shape`
Expected: compile errors: `cannot find function valid_questions`.

- [ ] **Step 3: Write the implementation**

Put this above the tests in `crates/jevd/src/shape.rs`:

```rust
//! The shape check on each `ask` (spec section 4, step 1).

use serde_json::Value;

pub const MAX_QUESTIONS: usize = 20;
pub const MAX_DEPTH: usize = 64;
pub const QUESTION_TYPES: [&str; 2] = ["noul", "score"];

/// 1 to 64 characters from `[A-Za-z0-9_-]`.
pub fn valid_question_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// 1 to 20 ids, each mapped to an object whose `type` is `noul` or `score`.
pub fn valid_questions(questions: &Value) -> bool {
    let Some(map) = questions.as_object() else {
        return false;
    };
    if map.is_empty() || map.len() > MAX_QUESTIONS {
        return false;
    }
    map.iter().all(|(id, question)| {
        valid_question_id(id)
            && question
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(|kind| QUESTION_TYPES.contains(&kind))
    })
}

/// How many arrays and objects sit inside each other at the deepest point.
pub fn depth(value: &Value) -> usize {
    match value {
        Value::Array(items) => 1 + items.iter().map(depth).max().unwrap_or(0),
        Value::Object(map) => 1 + map.values().map(depth).max().unwrap_or(0),
        _ => 0,
    }
}

/// Ok when an `ask` has the right shape, else the problem in a few words.
pub fn check_ask(state: &Value, questions: &Value) -> Result<(), &'static str> {
    if depth(state) > MAX_DEPTH {
        return Err("state is nested too deeply");
    }
    if !valid_questions(questions) {
        return Err("questions must map 1 to 20 ids to objects with type noul or score");
    }
    Ok(())
}
```

- [ ] **Step 4: Run the tests and see them pass**

Run: `cargo test -p jevd shape`
Expected: `test result: ok. 3 passed`.

- [ ] **Step 5: Commit**

```bash
git add crates/jevd
git commit -m "feat(jevd): the request shape check" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: Answer check and key check (`jevd::answer`, `jevd::key`)

Ported from `valid_answers` and `key_problem` in `typesafe.py`. In Rust the key check is `check_key(raw) -> Result<&str, ErrorCode>`: it returns the trimmed key, and a caller sends and redacts that value, never `raw`. `serde_json` never holds a non-finite number, and `as_f64` returns `None` for a boolean, so the prototype's `_finite` checks reduce to `as_f64` plus `is_finite`.

**Files:**
- Create: `crates/jevd/src/answer.rs`, `crates/jevd/src/key.rs`
- Modify: `crates/jevd/src/lib.rs`

- [ ] **Step 1: Write the failing tests**

`crates/jevd/src/answer.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn answers_match_their_questions() {
        let noul = json!({"q1": {"type": "noul"}});
        assert!(valid_answers(&json!({"q1": {"type": "noul", "noul": 0.8}}), &noul));
        assert!(valid_answers(&json!({"q1": {"type": "noul", "noul": 0}}), &noul));
        assert!(valid_answers(&json!({"q1": {"type": "noul", "noul": 1}}), &noul));
        assert!(!valid_answers(&json!({"q1": {"type": "noul", "noul": 1.5}}), &noul));
        assert!(!valid_answers(&json!({"q1": {"type": "noul", "noul": true}}), &noul));
        assert!(!valid_answers(&json!({"q1": {"type": "score", "score": 1, "confidence": 1}}), &noul));
        assert!(!valid_answers(&json!({}), &noul));
        assert!(!valid_answers(&json!([]), &noul));
        assert!(!valid_answers(&json!({"q1": 0.8}), &noul));

        let score = json!({"s": {"type": "score"}});
        assert!(valid_answers(&json!({"s": {"type": "score", "score": 7, "confidence": 0.5}}), &score));
        assert!(!valid_answers(&json!({"s": {"type": "score", "score": 7}}), &score));
        assert!(!valid_answers(&json!({"s": {"type": "score", "score": "7", "confidence": 1}}), &score));
    }

    #[test]
    fn extra_answers_and_fields_pass_through() {
        let noul = json!({"q1": {"type": "noul"}});
        let answers = json!({"q1": {"type": "noul", "noul": 0.2, "why": "x"}, "q9": {}});
        assert!(valid_answers(&answers, &noul));
    }
}
```

`crates/jevd/src/key.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_shapes() {
        assert_eq!(check_key("tsk-abc123"), Ok("tsk-abc123"));
        assert_eq!(check_key("  tsk-abc123\n"), Ok("tsk-abc123"));
        assert_eq!(check_key(""), Err(ErrorCode::NoKey));
        assert_eq!(check_key("   "), Err(ErrorCode::NoKey));
        assert_eq!(check_key("a b"), Err(ErrorCode::BadKey));
        assert_eq!(check_key("abc\u{7}"), Err(ErrorCode::BadKey));
        assert_eq!(check_key("clé"), Err(ErrorCode::BadKey));
        assert_eq!(check_key(&"k".repeat(512)), Ok("k".repeat(512).as_str()));
        assert_eq!(check_key(&"k".repeat(513)), Err(ErrorCode::BadKey));
    }
}
```

`crates/jevd/src/lib.rs`:

```rust
//! The jev daemon's library: checks, settings, and pairing.

pub mod answer;
pub mod key;
pub mod shape;
pub mod units;
```

- [ ] **Step 2: Run the tests and see them fail**

Run: `cargo test -p jevd`
Expected: compile errors: `cannot find function valid_answers`, `cannot find function check_key`.

- [ ] **Step 3: Write the implementations**

Put this above the tests in `crates/jevd/src/answer.rs`:

```rust
//! The answer check (spec section 4, step 7).

use serde_json::Value;

fn finite(value: Option<&Value>) -> Option<f64> {
    value.and_then(Value::as_f64).filter(|number| number.is_finite())
}

/// Every question has an answer of its own type with usable numbers.
/// Extra answers and extra fields pass through: the check tests only what jev reads.
pub fn valid_answers(answers: &Value, questions: &Value) -> bool {
    let (Some(answers), Some(questions)) = (answers.as_object(), questions.as_object()) else {
        return false;
    };
    questions.iter().all(|(id, question)| {
        let Some(answer) = answers.get(id).filter(|answer| answer.is_object()) else {
            return false;
        };
        let kind = question.get("type");
        if answer.get("type") != kind {
            return false;
        }
        match kind.and_then(Value::as_str) {
            Some("noul") => finite(answer.get("noul")).is_some_and(|n| (0.0..=1.0).contains(&n)),
            Some("score") => {
                finite(answer.get("score")).is_some() && finite(answer.get("confidence")).is_some()
            }
            _ => true,
        }
    })
}
```

Put this above the tests in `crates/jevd/src/key.rs`:

```rust
//! The key shape check (spec section 4, step 3).

use jev_proto::ErrorCode;

const MAX_KEY_LENGTH: usize = 512;

/// The trimmed key when it is usable, else `no-key` or `bad-key`. Spaces at
/// either end do not count. Use the returned value, not `raw`, wherever the key
/// is matched or sent, so the checked key and the used key are the same text.
///
/// Printable ASCII only: a control character in a header makes an HTTP
/// library put the key in its error text.
pub fn check_key(raw: &str) -> Result<&str, ErrorCode> {
    let key = raw.trim();
    if key.is_empty() {
        return Err(ErrorCode::NoKey);
    }
    if key.len() > MAX_KEY_LENGTH || !key.bytes().all(|b| (0x21..=0x7e).contains(&b)) {
        return Err(ErrorCode::BadKey);
    }
    Ok(key)
}
```

- [ ] **Step 4: Run the tests and see them pass**

Run: `cargo test -p jevd`
Expected: `test result: ok. 10 passed` (units 4, shape 3, answer 2, key 1).

- [ ] **Step 5: Commit**

```bash
git add crates/jevd
git commit -m "feat(jevd): the answer check and the key shape check" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 7: Redaction (`jevd::redact`)

Ported from `redact.py`. Three differences from Python, each with the same result:

- **No lookbehind or possessive match in the name rule.** Rust's `regex` crate has neither. The search position is always 0 or right after a character that cannot be part of a name, and a leftmost match starts at the beginning of a run. So the plain pattern finds the same names.
- **No lookahead in the `Bearer` rule.** The pattern captures the token, and a closure redacts it only when the token holds a digit, `+`, `/`, or `=`. That is the condition the lookahead tested.
- **No lookahead in the secret-word rule.** The pattern consumes the `_` or `-` after the word. Only `is_match` uses it, so consuming that character changes nothing. The name gets lowercased first, so the match stays ASCII-only, as Python's `re.A` did.

The patterns use `(?-u:\b)`, the ASCII word boundary, so they match Python's `re.A` behavior. This includes tokens glued to CJK text, which has no spaces. Rust's default `\b` is Unicode and would let such tokens through.

`regex` runs in linear time by design, so the prototype's workarounds for Python's backtracking go away. The 1 MiB tests stay.

**Files:**
- Create: `crates/jevd/src/redact.rs`
- Modify: `crates/jevd/src/lib.rs`

- [ ] **Step 1: Write the failing tests**

`crates/jevd/src/redact.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::time::{Duration, Instant};

    fn r(text: &str) -> String {
        redact(text, None)
    }

    #[test]
    fn known_token_shapes() {
        assert_eq!(r("Authorization: Bearer abcdefghijklmnop1234"), "Authorization: Bearer [redacted]");
        assert_eq!(r("Bearer abcdefghijklmnopqrst"), "Bearer abcdefghijklmnopqrst");
        assert_eq!(r("key sk-ABCDEFGHIJKLMNOPQRST"), "key [redacted]");
        assert_eq!(r("key tsk_ABCDEFGHIJKLMNOPQRST"), "key [redacted]");
        assert_eq!(r("token=hunter2"), "token=[redacted]");
        assert_eq!(r("AKIAABCDEFGHIJKLMNOP"), "[redacted]");
        assert_eq!(r(&format!("ghp_{}", "a".repeat(36))), "[redacted]");
        assert_eq!(r(&format!("github_pat_{}", "a".repeat(20))), "[redacted]");
        assert_eq!(r("xoxb-1234567890"), "[redacted]");
    }

    #[test]
    fn the_api_key_itself() {
        assert_eq!(redact("x my-plain-key y", Some("my-plain-key")), "x [redacted] y");
        assert_eq!(redact("x y", Some("")), "x y");
    }

    #[test]
    fn key_is_trimmed_before_it_is_matched() {
        assert_eq!(redact("x tsk1 y", Some("tsk1\n")), "x [redacted] y");
        assert_eq!(redact("x tsk1 y", Some("  \n")), "x tsk1 y");
    }

    #[test]
    fn redact_value_walks_json() {
        let state = json!({"a": ["password: pw1", {"b": "ok"}], "n": 3});
        assert_eq!(
            redact_value(&state, None, false),
            json!({"a": ["password: [redacted]", {"b": "ok"}], "n": 3})
        );
    }

    #[test]
    fn dict_keys_are_not_redacted() {
        assert_eq!(redact_value(&json!({"token=k": "v"}), None, false), json!({"token=k": "v"}));
    }

    #[test]
    fn private_keys() {
        let pem = "a -----BEGIN PRIVATE KEY-----\nQUJD\n-----END PRIVATE KEY----- b";
        assert_eq!(r(pem), "a [redacted] b");
        let cut = "-----BEGIN RSA PRIVATE KEY-----\nQUJD\nREVG\n- done";
        assert_eq!(r(cut), "[redacted]\n- done");
    }

    #[test]
    fn quoted_values() {
        assert_eq!(r("my_api_key: \"a b\""), "my_api_key: [redacted]");
        assert_eq!(r("password='q r'"), "password=[redacted]");
    }

    #[test]
    fn a_value_does_not_hide_a_later_name() {
        assert_eq!(r("tokenx:api-key:v"), "tokenx:api-key:[redacted]");
    }

    #[test]
    fn names_match_without_case() {
        assert_eq!(r("API_KEY=abc"), "API_KEY=[redacted]");
        assert_eq!(r("Password: abc"), "Password: [redacted]");
        assert_eq!(r("tokens=abc"), "tokens=abc");
    }

    #[test]
    fn strict_redaction_is_off_unless_asked() {
        let state = json!({"api_key": "abc", "note": "plain", "nested": {"password": "pw"}});
        assert_eq!(redact_value(&state, None, false), state);
        assert_eq!(
            redact_value(&state, None, true),
            json!({"api_key": "[redacted]", "note": "plain", "nested": {"password": "[redacted]"}})
        );
        assert_eq!(redact_value(&json!({"token": 5}), None, true), json!({"token": 5}));
    }

    #[test]
    fn cjk_text_does_not_hide_tokens() {
        assert_eq!(r("APIキーはsk-ABCDEFGHIJKLMNOPQRSTです"), "APIキーは[redacted]です");
        assert_eq!(r("キー AKIAABCDEFGHIJKLMNOPです"), "キー [redacted]です");
        assert_eq!(r(&format!("トークンghp_{}", "a".repeat(36))), "トークン[redacted]");
        assert_eq!(r("xsk-ABCDEFGHIJKLMNOPQRST"), "xsk-ABCDEFGHIJKLMNOPQRST");
    }

    #[test]
    fn short_basic_credentials() {
        assert_eq!(r("Authorization: Basic dXNlcjpwYXNz"), "Authorization: Basic [redacted]");
        assert_eq!(r("Basic YTpiYw=="), "Basic [redacted]");
        assert_eq!(r("Basic authentication is used"), "Basic authentication is used");
        assert_eq!(r("Basic QUJD"), "Basic QUJD");
    }

    #[test]
    fn one_mib_runs_in_linear_time() {
        const MIB: usize = 1 << 20;
        let units = [
            "-----BEGIN PRIVATE KEY-----\n",
            "ab-",
            "token_",
            "token-",
            "a1-secret-",
            "a:",
            "token=\"",
            "token=\" ",
            "password:'",
            "Bearer ",
            "Basic ",
            "Basic QUJD ",
        ];
        let limit = Duration::from_secs(if cfg!(debug_assertions) { 20 } else { 5 });
        for unit in units {
            let text = unit.repeat(MIB / unit.len());
            let started = Instant::now();
            redact(&text, None);
            assert!(started.elapsed() < limit, "{unit:?}");
        }
    }
}
```

`crates/jevd/src/lib.rs`:

```rust
//! The jev daemon's library: checks, settings, and pairing.

pub mod answer;
pub mod key;
pub mod redact;
pub mod shape;
pub mod units;
```

- [ ] **Step 2: Run the tests and see them fail**

Run: `cargo test -p jevd redact`
Expected: compile errors: `cannot find function redact`.

- [ ] **Step 3: Write the implementation**

Put this above the tests in `crates/jevd/src/redact.rs`:

```rust
//! Replace the API key and anything shaped like a secret before a send
//! (spec section 4, step 5). Ported from the prototype's `redact.py`.

use regex::{Captures, NoExpand, Regex};
use serde_json::Value;
use std::sync::OnceLock;

pub const REDACTED: &str = "[redacted]";

/// A regex compiled once, on first use.
macro_rules! regex {
    ($pattern:expr) => {{
        static CELL: OnceLock<Regex> = OnceLock::new();
        CELL.get_or_init(|| Regex::new($pattern).expect("redaction pattern compiles"))
    }};
}

/// Replace each PEM private key, from BEGIN to END or to the end of its base64 lines.
pub fn redact_private_keys(text: &str) -> String {
    let begin_re = regex!(r"-----BEGIN [A-Z ]*PRIVATE KEY-----");
    let end_re = regex!(r"-----END [A-Z ]*PRIVATE KEY-----");
    let body_re = regex!(r"\A(?:\r?\n[A-Za-z0-9+/=]+)*");
    let mut out = String::with_capacity(text.len());
    let mut pos = 0;
    let mut end_missing = false;
    while let Some(begin) = begin_re.find_at(text, pos) {
        let end = if end_missing {
            None
        } else {
            end_re.find_at(text, begin.end())
        };
        let stop = match end {
            Some(end) => end.end(),
            None => {
                // No END after this point means none after any later BEGIN either.
                end_missing = true;
                begin.end() + body_re.find(&text[begin.end()..]).map_or(0, |m| m.end())
            }
        };
        out.push_str(&text[pos..begin.start()]);
        out.push_str(REDACTED);
        pos = stop;
    }
    out.push_str(&text[pos..]);
    out
}

/// True when `token` is standard padded base64 whose decoded bytes hold a colon.
fn base64_has_colon(token: &str) -> bool {
    let bytes = token.as_bytes();
    if bytes.is_empty() || bytes.len() % 4 != 0 {
        return false;
    }
    let mut decoded = Vec::with_capacity(bytes.len() / 4 * 3);
    let (mut bits, mut count) = (0u32, 0u32);
    for &b in bytes {
        let value = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => break, // padding: only at the end, as the pattern allows
        };
        bits = (bits << 6) | u32::from(value);
        count += 6;
        if count >= 8 {
            count -= 8;
            decoded.push((bits >> count) as u8);
            bits &= (1 << count) - 1;
        }
    }
    decoded.contains(&b':')
}

/// Specific token shapes. They run before the name rule, so "token=sk-..."
/// ends as "token=[redacted]" either way.
fn redact_token_shapes(text: &str) -> String {
    let bearer = regex!(r"(?-u:\b)(Bearer|bearer|Basic|basic)[ \t]+([A-Za-z0-9._~+/=-]{16,})");
    let text = bearer.replace_all(text, |caps: &Captures| {
        let token = &caps[2];
        if token.bytes().any(|b| b.is_ascii_digit() || b == b'+' || b == b'/' || b == b'=') {
            format!("{} {}", &caps[1], REDACTED)
        } else {
            caps[0].to_string()
        }
    });
    // A short `Basic` value is `user:pass` in base64. It is redacted when it
    // decodes to something with a colon.
    let basic = regex!(r"(?-u:\b)(Basic|basic)[ \t]+([A-Za-z0-9+/]+={0,2})");
    let text = basic.replace_all(&text, |caps: &Captures| {
        if base64_has_colon(&caps[2]) {
            format!("{} {}", &caps[1], REDACTED)
        } else {
            caps[0].to_string()
        }
    });
    let mut text = text.into_owned();
    let shapes = [
        regex!(r"(?-u:\b)(?:sk|tsk)[-_][A-Za-z0-9_-]{16,}"),
        regex!(r"(?-u:\b)gh[pousr]_[A-Za-z0-9]{36,}"),
        regex!(r"(?-u:\b)github_pat_[A-Za-z0-9_]{20,}"),
        regex!(r"(?-u:\b)AKIA[0-9A-Z]{16}(?-u:\b)"),
        regex!(r"(?-u:\b)xox[abprs]-[A-Za-z0-9-]{10,}"),
    ];
    for shape in shapes {
        text = shape.replace_all(&text, NoExpand(REDACTED)).into_owned();
    }
    text
}

/// True when a name holds a secret word, such as `api_key` or `DB-PASSWORD`.
pub fn is_secret_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    regex!(r"(?:\A|[_-])(?:password|passwd|secret|token|api[_-]?key)(?:[_-]|\z)").is_match(&lower)
}

/// Replace the value after each `name = value` or `name: value` whose name holds a secret word.
pub fn redact_names(text: &str) -> String {
    let name_re = regex!(r#"([A-Za-z0-9_-]+)(["']?[ \t]*[:=][ \t]*)"#);
    let value_re = regex!(r#"\A(?:"(?:[^"\\]|\\.)*"|'[^']*'|[^ \t\r\n,;}\]]+)"#);
    let mut out = String::with_capacity(text.len());
    let mut pos = 0;
    while let Some(caps) = name_re.captures_at(text, pos) {
        let whole = caps.get(0).expect("group 0 always exists");
        let name = caps.get(1).expect("the name group always matches");
        out.push_str(&text[pos..whole.end()]);
        pos = whole.end();
        if is_secret_name(name.as_str()) {
            if let Some(value) = value_re.find(&text[pos..]) {
                out.push_str(REDACTED);
                pos += value.end();
            }
        }
    }
    out.push_str(&text[pos..]);
    out
}

/// Redact one string: the key itself, private keys, token shapes, then secret names.
pub fn redact(text: &str, key: Option<&str>) -> String {
    let text = match key.map(str::trim) {
        Some(key) if !key.is_empty() => text.replace(key, REDACTED),
        _ => text.to_string(),
    };
    let text = redact_private_keys(&text);
    let text = redact_token_shapes(&text);
    redact_names(&text)
}

/// `redact` every string inside a JSON value.
///
/// Object keys are not redacted: two keys could become the same text, and one
/// value would then be lost. With `strict`, a string value under a secret-looking
/// key becomes `[redacted]` as a whole.
pub fn redact_value(value: &Value, key: Option<&str>, strict: bool) -> Value {
    match value {
        Value::String(text) => Value::String(redact(text, key)),
        Value::Array(items) => Value::Array(items.iter().map(|item| redact_value(item, key, strict)).collect()),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(name, item)| {
                    let item = if strict && item.is_string() && is_secret_name(name) {
                        Value::String(REDACTED.to_string())
                    } else {
                        redact_value(item, key, strict)
                    };
                    (name.clone(), item)
                })
                .collect(),
        ),
        other => other.clone(),
    }
}
```

- [ ] **Step 4: Run the tests and see them pass**

Run: `cargo test -p jevd redact`
Expected: `test result: ok. 12 passed`.

`one_mib_runs_in_linear_time` allows 20 seconds for each input in a debug build and 5 seconds in a release build. If it fails on time in a debug build, run `cargo test -p jevd --release redact` once to tell a slow build from a slow pattern. A pattern that is slow in release too is a defect. Fix the pattern, never the limits.

- [ ] **Step 5: Commit**

```bash
git add crates/jevd
git commit -m "feat(jevd): redaction, ported from the prototype, plus strict mode" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 8: Rate limit (`jevd::ratelimit`)

Ported from `ratelimit.py`, keyed by client instead of plugin. The caller passes the limit on each call, so a per-client override needs no state here.

**Files:**
- Create: `crates/jevd/src/ratelimit.rs`
- Modify: `crates/jevd/src/lib.rs`

- [ ] **Step 1: Write the failing tests**

`crates/jevd/src/ratelimit.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::DEFAULT_RATE_LIMIT;

    fn limit(text: &str) -> Limit {
        text.parse().unwrap()
    }

    #[test]
    fn the_121st_request_in_a_window_is_refused() {
        let mut limiter = RateLimiter::new();
        let t0 = Instant::now();
        for _ in 0..120 {
            assert_eq!(limiter.check("a", DEFAULT_RATE_LIMIT, t0), Ok(()));
        }
        assert_eq!(limiter.check("a", DEFAULT_RATE_LIMIT, t0), Err(60));
    }

    #[test]
    fn the_window_slides() {
        let mut limiter = RateLimiter::new();
        let t0 = Instant::now();
        for _ in 0..120 {
            limiter.check("a", DEFAULT_RATE_LIMIT, t0).unwrap();
        }
        assert!(limiter.check("a", DEFAULT_RATE_LIMIT, t0 + Duration::from_secs(59)).is_err());
        assert_eq!(limiter.check("a", DEFAULT_RATE_LIMIT, t0 + Duration::from_secs(60)), Ok(()));
    }

    #[test]
    fn retry_after_rounds_up_and_never_exceeds_the_window() {
        let mut limiter = RateLimiter::new();
        let t0 = Instant::now();
        limiter.check("a", limit("1/60"), t0).unwrap();
        assert_eq!(limiter.check("a", limit("1/60"), t0 + Duration::from_millis(59_500)), Err(1));
        assert_eq!(limiter.check("a", limit("1/60"), t0 + Duration::from_millis(500)), Err(60));
    }

    #[test]
    fn clients_count_apart() {
        let mut limiter = RateLimiter::new();
        let t0 = Instant::now();
        limiter.check("a", limit("1/60"), t0).unwrap();
        assert_eq!(limiter.check("b", limit("1/60"), t0), Ok(()));
    }

    #[test]
    fn an_override_changes_the_count() {
        let mut limiter = RateLimiter::new();
        let t0 = Instant::now();
        for _ in 0..5 {
            limiter.check("a", limit("5/60"), t0).unwrap();
        }
        assert!(limiter.check("a", limit("5/60"), t0).is_err());
        assert_eq!(limiter.check("a", limit("6/60"), t0), Ok(()));
    }

    #[test]
    fn forget_clears_a_client() {
        let mut limiter = RateLimiter::new();
        let t0 = Instant::now();
        limiter.check("a", limit("1/60"), t0).unwrap();
        limiter.forget("a");
        assert_eq!(limiter.check("a", limit("1/60"), t0), Ok(()));
    }
}
```

`crates/jevd/src/lib.rs`:

```rust
//! The jev daemon's library: checks, settings, and pairing.

pub mod answer;
pub mod key;
pub mod ratelimit;
pub mod redact;
pub mod shape;
pub mod units;
```

- [ ] **Step 2: Run the tests and see them fail**

Run: `cargo test -p jevd ratelimit`
Expected: compile errors: `cannot find type RateLimiter`.

- [ ] **Step 3: Write the implementation**

Put this above the tests in `crates/jevd/src/ratelimit.rs`:

```rust
//! A sliding window of requests for each client (spec section 4, step 4).
//! Counts live in memory, so an idle exit or a restart resets them.

use crate::units::Limit;
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

#[derive(Debug, Default)]
pub struct RateLimiter {
    seen: HashMap<String, VecDeque<Instant>>,
}

impl RateLimiter {
    pub fn new() -> RateLimiter {
        RateLimiter::default()
    }

    /// Ok, and count the request, when `client` is under `limit`.
    /// Else Err with the whole seconds until a slot frees up, at least 1.
    pub fn check(&mut self, client: &str, limit: Limit, now: Instant) -> Result<(), u64> {
        let times = self.seen.entry(client.to_string()).or_default();
        while let Some(&first) = times.front() {
            if now.saturating_duration_since(first) >= limit.window {
                times.pop_front();
            } else {
                break;
            }
        }
        if times.len() >= limit.count as usize {
            // The entry whose expiry brings the length down to count - 1. After a
            // lowered limit that is not the front.
            let first = times[times.len() - limit.count as usize];
            let wait: Duration = limit.window.saturating_sub(now.saturating_duration_since(first));
            return Err(wait.as_secs() + u64::from(wait.subsec_nanos() > 0));
        }
        times.push_back(now);
        Ok(())
    }

    /// Drop a client's counts, for example after `revoke`.
    pub fn forget(&mut self, client: &str) {
        self.seen.remove(client);
    }
}
```

- [ ] **Step 4: Run the tests and see them pass**

Run: `cargo test -p jevd ratelimit`
Expected: `test result: ok. 6 passed`.

- [ ] **Step 5: Commit**

```bash
git add crates/jevd
git commit -m "feat(jevd): sliding-window rate limit per client" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 9: Atomic writes and settings (`jevd::files`, `jevd::settings`)

The settings file holds the 5 settings and the profile names. `Settings::with_change` returns the changed settings and whether the change loosens protection. The CLI (plan 4) asks for a terminal and a typed `y` when it does. This task builds only the rule.

**Files:**
- Create: `crates/jevd/src/files.rs`, `crates/jevd/src/settings.rs`
- Modify: `crates/jevd/src/lib.rs`

- [ ] **Step 1: Write the failing tests**

`crates/jevd/src/files.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_and_replaces_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub").join("a.json");
        write_atomic(&path, "one").unwrap();
        write_atomic(&path, "two").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "two");
        let leftovers: Vec<_> = std::fs::read_dir(path.parent().unwrap()).unwrap().collect();
        assert_eq!(leftovers.len(), 1, "no temporary file stays behind");
    }

    #[cfg(unix)]
    #[test]
    fn only_the_owner_can_read_it() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.json");
        write_atomic(&path, "x").unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn a_stale_tmp_file_does_not_block_a_write() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.json");
        std::fs::write(dir.path().join("a.json.tmp"), "junk from a crashed writer").unwrap();
        write_atomic(&path, "new").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
    }

    #[test]
    fn no_temp_file_is_left_after_a_failed_rename() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.json");
        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join("inside"), "x").unwrap();
        assert!(write_atomic(&path, "new").is_err());
        let names: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, [std::ffi::OsString::from("a.json")]);
    }

    #[test]
    fn writers_that_run_at_the_same_time_all_succeed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.json");
        std::thread::scope(|scope| {
            for writer in 0..8 {
                let path = &path;
                scope.spawn(move || {
                    for round in 0..50 {
                        write_atomic(path, &format!("{writer}-{round}")).unwrap();
                    }
                });
            }
        });
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains('-'), "{text:?}");
        let (writer, round) = text.split_once('-').unwrap();
        assert!(writer.parse::<u32>().unwrap() < 8 && round.parse::<u32>().unwrap() < 50);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn a_target_held_open_by_a_reader_is_still_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.json");
        write_atomic(&path, "one").unwrap();
        let _reader = std::fs::File::open(&path).unwrap();
        write_atomic(&path, "two").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "two");
    }
}
```

`crates/jevd/src/settings.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn change(settings: &Settings, name: &str, value: &str) -> Change {
        settings.with_change(name, value).unwrap()
    }

    #[test]
    fn defaults_match_the_spec() {
        let settings = Settings::default();
        assert_eq!(settings.get("idle-timeout").unwrap(), "2h");
        assert_eq!(settings.get("key-cache").unwrap(), "2h");
        assert_eq!(settings.get("rate-limit").unwrap(), "120/60");
        assert_eq!(settings.get("redact-strict").unwrap(), "off");
        assert_eq!(settings.get("max-connections").unwrap(), "32");
        assert_eq!(settings.get("nope"), None);
        let names: Vec<_> = settings.list().into_iter().map(|(name, _)| name).collect();
        assert_eq!(names, SETTING_NAMES);
    }

    #[test]
    fn a_missing_file_gives_the_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let loaded = Settings::load(&dir.path().join("settings.toml")).unwrap();
        assert_eq!(loaded, Settings::default());
    }

    #[test]
    fn settings_round_trip_through_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.toml");
        let mut settings = change(&Settings::default(), "idle-timeout", "off").settings;
        settings = change(&settings, "rate-limit", "10/60").settings;
        settings = change(&settings, "redact-strict", "on").settings;
        settings.add_profile("work").unwrap();
        settings.save(&path).unwrap();
        assert_eq!(Settings::load(&path).unwrap(), settings);
        assert!(!std::fs::read_to_string(&path).unwrap().contains("tsk"));
    }

    #[test]
    fn a_bad_file_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.toml");
        std::fs::write(&path, "rate-limit = \"fast\"\n").unwrap();
        assert!(Settings::load(&path).is_err());
    }

    #[test]
    fn loosening_changes_are_marked() {
        let base = Settings::default();
        let loosening = [
            ("rate-limit", "200/60"),
            ("rate-limit", "120/30"),
            ("idle-timeout", "off"),
            ("idle-timeout", "3h"),
            ("key-cache", "3h"),
        ];
        for (name, value) in loosening {
            assert!(change(&base, name, value).loosens, "{name} {value}");
        }
        let tightening = [
            ("rate-limit", "60/60"),
            ("idle-timeout", "1h"),
            ("key-cache", "30m"),
            ("redact-strict", "on"),
            ("max-connections", "200"),
            ("rate-limit", "120/60"),
        ];
        for (name, value) in tightening {
            assert!(!change(&base, name, value).loosens, "{name} {value}");
        }
        let strict = change(&base, "redact-strict", "on").settings;
        assert!(change(&strict, "redact-strict", "off").loosens);
    }

    #[test]
    fn bad_changes_are_errors() {
        let base = Settings::default();
        for (name, value) in [
            ("key-cache", "off"),
            ("rate-limit", "fast"),
            ("redact-strict", "yes"),
            ("max-connections", "0"),
            ("max-connections", "1025"),
            ("idle-timeout", "2d"),
            ("no-such-setting", "1"),
        ] {
            assert!(base.with_change(name, value).is_err(), "{name} {value}");
        }
    }

    #[test]
    fn profiles_stay_sorted_and_unique() {
        let mut settings = Settings::default();
        settings.add_profile("work").unwrap();
        settings.add_profile("default").unwrap();
        settings.add_profile("work").unwrap();
        assert_eq!(settings.profiles, ["default", "work"]);
        assert!(settings.add_profile("Bad Name").is_err());
        assert!(settings.remove_profile("work"));
        assert!(!settings.remove_profile("work"));
        assert_eq!(settings.profiles, ["default"]);
    }

    #[test]
    fn a_hand_edited_file_with_a_bad_key_or_value_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.toml");
        for text in [
            "redact_strict = true\n",
            "key-cache = \"off\"\n",
            "max-connections = 0\n",
        ] {
            std::fs::write(&path, text).unwrap();
            assert!(Settings::load(&path).is_err(), "{text:?}");
        }
    }
}
```

`crates/jevd/src/lib.rs`:

```rust
//! The jev daemon's library: checks, settings, and pairing.

pub mod answer;
pub mod files;
pub mod key;
pub mod ratelimit;
pub mod redact;
pub mod settings;
pub mod shape;
pub mod units;
```

- [ ] **Step 2: Run the tests and see them fail**

Run: `cargo test -p jevd`
Expected: compile errors: `cannot find function write_atomic`, `cannot find type Settings`.

- [ ] **Step 3: Write the implementations**

Put this above the tests in `crates/jevd/src/files.rs`:

```rust
//! File writes that never leave a half-written file behind.

use std::io::Write;
use std::path::Path;

/// Write `contents` to a new temporary file in the same folder as `path`, then
/// rename it over `path`. The temporary file has a random name, so writers that
/// run at the same time do not meet, and a failed write leaves nothing behind.
/// On Unix the file is created with mode 0600 and never has a wider mode. On
/// Windows no mode is set: the file inherits the ACL of the user profile folder.
/// The rename comes from `std::fs`, not from `tempfile`'s `persist`: on Windows,
/// `persist` cannot replace a file that another process has open, and `std` can.
/// The parent folder gets created when missing.
pub fn write_atomic(path: &Path, contents: &str) -> std::io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(contents.as_bytes())?;
    temp.as_file().sync_all()?;
    let temp = temp.into_temp_path().keep().map_err(|error| error.error)?;
    if let Err(error) = std::fs::rename(&temp, path) {
        let _ = std::fs::remove_file(&temp);
        return Err(error);
    }
    // Best effort: the file is already replaced here, so a failed folder sync
    // only weakens durability after a power loss. It must not report an error
    // for a change that was made.
    #[cfg(unix)]
    let _ = std::fs::File::open(parent).and_then(|dir| dir.sync_all());
    Ok(())
}
```

Put this above the tests in `crates/jevd/src/settings.rs`:

```rust
//! The settings file (spec section 5). The CLI writes it; jevd reads it on
//! start and on `reload`. It never holds a key.

use crate::files::write_atomic;
use crate::units::{Limit, Period, DEFAULT_RATE_LIMIT};
use jev_proto::valid_name;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Duration;

pub const SETTING_NAMES: [&str; 5] = [
    "idle-timeout",
    "key-cache",
    "rate-limit",
    "redact-strict",
    "max-connections",
];

const MAX_CONNECTIONS_LIMIT: u32 = 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub idle_timeout: Period,
    pub key_cache: Duration,
    pub rate_limit: Limit,
    pub redact_strict: bool,
    pub max_connections: u32,
    /// Profile names. The keyring cannot list entries on every OS.
    pub profiles: Vec<String>,
}

/// The result of a proposed change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub settings: Settings,
    /// True when the change weakens protection. The CLI then needs a terminal and a typed `y`.
    pub loosens: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", default, deny_unknown_fields)]
struct Raw {
    idle_timeout: Option<String>,
    key_cache: Option<String>,
    rate_limit: Option<String>,
    redact_strict: Option<bool>,
    max_connections: Option<u32>,
    profiles: Option<Vec<String>>,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            idle_timeout: Period::After(Duration::from_secs(2 * 3600)),
            key_cache: Duration::from_secs(2 * 3600),
            rate_limit: DEFAULT_RATE_LIMIT,
            redact_strict: false,
            max_connections: 32,
            profiles: Vec::new(),
        }
    }
}

fn on_off(value: bool) -> String {
    String::from(if value { "on" } else { "off" })
}

impl Settings {
    /// Read the file, or the defaults when it does not exist.
    pub fn load(path: &Path) -> Result<Settings, String> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Settings::default())
            }
            Err(error) => return Err(format!("cannot read {}: {error}", path.display())),
        };
        let raw: Raw = toml::from_str(&text).map_err(|error| format!("{}: {error}", path.display()))?;
        let mut settings = Settings::default();
        let fields = [
            ("idle-timeout", raw.idle_timeout),
            ("key-cache", raw.key_cache),
            ("rate-limit", raw.rate_limit),
            ("redact-strict", raw.redact_strict.map(on_off)),
            ("max-connections", raw.max_connections.map(|n| n.to_string())),
        ];
        for (name, value) in fields {
            if let Some(value) = value {
                settings = settings
                    .with_change(name, &value)
                    .map_err(|error| format!("{}: {error}", path.display()))?
                    .settings;
            }
        }
        for profile in raw.profiles.unwrap_or_default() {
            settings
                .add_profile(&profile)
                .map_err(|error| format!("{}: {error}", path.display()))?;
        }
        Ok(settings)
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let raw = Raw {
            idle_timeout: Some(self.idle_timeout.to_string()),
            key_cache: Some(Period::After(self.key_cache).to_string()),
            rate_limit: Some(self.rate_limit.to_string()),
            redact_strict: Some(self.redact_strict),
            max_connections: Some(self.max_connections),
            profiles: Some(self.profiles.clone()),
        };
        let text = toml::to_string(&raw).map_err(|error| error.to_string())?;
        write_atomic(path, &text).map_err(|error| format!("cannot write {}: {error}", path.display()))
    }

    pub fn get(&self, name: &str) -> Option<String> {
        Some(match name {
            "idle-timeout" => self.idle_timeout.to_string(),
            "key-cache" => Period::After(self.key_cache).to_string(),
            "rate-limit" => self.rate_limit.to_string(),
            "redact-strict" => on_off(self.redact_strict),
            "max-connections" => self.max_connections.to_string(),
            _ => return None,
        })
    }

    /// Every setting and its value, in `SETTING_NAMES` order.
    pub fn list(&self) -> Vec<(&'static str, String)> {
        SETTING_NAMES
            .iter()
            .map(|name| (*name, self.get(name).expect("every listed name has a value")))
            .collect()
    }

    /// The settings after `name = value`, and whether that loosens protection.
    pub fn with_change(&self, name: &str, value: &str) -> Result<Change, String> {
        let mut settings = self.clone();
        let loosens = match name {
            "idle-timeout" => {
                let period: Period = value.parse()?;
                settings.idle_timeout = period;
                period.looser_than(self.idle_timeout)
            }
            "key-cache" => {
                let Period::After(duration) = value.parse::<Period>()? else {
                    return Err("key-cache cannot be off".to_string());
                };
                settings.key_cache = duration;
                duration > self.key_cache
            }
            "rate-limit" => {
                let limit: Limit = value.parse()?;
                settings.rate_limit = limit;
                limit.looser_than(self.rate_limit)
            }
            "redact-strict" => {
                let strict = match value {
                    "on" => true,
                    "off" => false,
                    _ => return Err(format!("{value:?}: use on or off")),
                };
                settings.redact_strict = strict;
                self.redact_strict && !strict
            }
            "max-connections" => {
                let count: u32 = value
                    .parse()
                    .ok()
                    .filter(|count| (1..=MAX_CONNECTIONS_LIMIT).contains(count))
                    .ok_or_else(|| format!("{value:?}: use a number from 1 to {MAX_CONNECTIONS_LIMIT}"))?;
                settings.max_connections = count;
                false
            }
            _ => return Err(format!("no setting named {name:?}")),
        };
        Ok(Change { settings, loosens })
    }

    /// Add a profile name. Adding a name that exists does nothing.
    pub fn add_profile(&mut self, name: &str) -> Result<(), String> {
        if !valid_name(name) {
            return Err(format!("{name:?}: profile names use lowercase letters, digits, and hyphens"));
        }
        if !self.profiles.iter().any(|profile| profile == name) {
            self.profiles.push(name.to_string());
            self.profiles.sort();
        }
        Ok(())
    }

    /// Remove a profile name. False when it was not there.
    pub fn remove_profile(&mut self, name: &str) -> bool {
        let before = self.profiles.len();
        self.profiles.retain(|profile| profile != name);
        self.profiles.len() != before
    }
}
```

- [ ] **Step 4: Run the tests and see them pass**

Run: `cargo test -p jevd`
Expected: every jevd test passes, including 8 new settings tests and 5 or 6 new files tests (the 0600 test runs on Unix only).

- [ ] **Step 5: Commit**

```bash
git add crates/jevd
git commit -m "feat(jevd): settings file with the loosening rule, and atomic writes" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 10: Pairing store (`jevd::pairing`)

Spec section 3. Two choices the spec leaves open, settled here:

- **A full pending list.** The 17th pending request gets `rate-limited`. The client can try again after a request expires or gets approved.
- **An unknown `request_id`.** `pair-status` reports `expired`, the same as a real expiry, so a wrong ID tells the caller nothing.

The store keeps the hashes of revoked and replaced tokens, so `jevd` can tell `revoked` from `not-paired`.

Three rules from the security review:

- **The collection window restarts at approval.** The client gets a full `PENDING_EXPIRY` to collect the token, however late the person approved.
- **Save before change.** Every change builds the new client list and revoked list as a copy, saves the copy, and only then puts it in memory. A failed save changes nothing, and an approved request turns `approved` only after its client is on disk.
- **No token in `Debug`.** `PairState` prints `Approved(<redacted>)`. The pairing file and each of its clients refuse unknown keys, and a name or a token hash that appears twice. `load` also refuses a client whose `limit` does not parse, so a hand-set limit never silently becomes the looser global default.

**Files:**
- Create: `crates/jevd/src/pairing.rs`
- Modify: `crates/jevd/src/lib.rs`

- [ ] **Step 1: Write the failing tests**

`crates/jevd/src/pairing.rs`:

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

    /// Pair `name` all the way and return its token.
    fn pair(pairings: &mut Pairings, name: &str, now: Instant) -> String {
        let (request_id, code) = pairings.start(name, None, now).unwrap();
        pairings.approve(&code, "default", now, T).unwrap();
        match pairings.status(&request_id, now) {
            PairState::Approved(token) => token,
            other => panic!("expected approved, got {other:?}"),
        }
    }

    #[test]
    fn codes_and_tokens_have_the_spec_shape() {
        let (_dir, mut pairings) = store();
        let now = Instant::now();
        let (request_id, code) = pairings.start("app", None, now).unwrap();
        assert_eq!(request_id.len(), 32);
        assert_eq!(code.len(), CODE_LENGTH);
        assert!(code.bytes().all(|b| CODE_ALPHABET.contains(&b)));
        for look_alike in ["0", "O", "1", "I", "L"] {
            assert!(!std::str::from_utf8(CODE_ALPHABET).unwrap().contains(look_alike));
        }
        let token = pair(&mut pairings, "other", now);
        assert!(token.starts_with(TOKEN_PREFIX));
        assert_eq!(token.len(), TOKEN_PREFIX.len() + 64);
    }

    #[test]
    fn the_token_comes_back_once_only() {
        let (_dir, mut pairings) = store();
        let now = Instant::now();
        let (request_id, code) = pairings.start("app", Some("work"), now).unwrap();
        assert_eq!(pairings.status(&request_id, now), PairState::Pending);
        assert_eq!(
            pairings.pending_list(now),
            [PendingView { code: code.clone(), name: "app".into(), profile: Some("work".into()) }]
        );
        let (name, replaced) = pairings.approve(&code.to_lowercase(), "work", now, T).unwrap();
        assert_eq!((name.as_str(), replaced), ("app", false));
        assert!(matches!(pairings.status(&request_id, now), PairState::Approved(_)));
        assert_eq!(pairings.status(&request_id, now), PairState::Expired);
        assert!(pairings.pending_list(now).is_empty());
    }

    #[test]
    fn a_wrong_request_id_reports_expired() {
        let (_dir, mut pairings) = store();
        let now = Instant::now();
        pairings.start("app", None, now).unwrap();
        assert_eq!(pairings.status("0123456789abcdef0123456789abcdef", now), PairState::Expired);
    }

    #[test]
    fn deny_ends_the_request() {
        let (_dir, mut pairings) = store();
        let now = Instant::now();
        let (request_id, code) = pairings.start("app", None, now).unwrap();
        pairings.deny(&code, now).unwrap();
        assert_eq!(pairings.status(&request_id, now), PairState::Denied);
        assert_eq!(pairings.status(&request_id, now), PairState::Expired);
        assert_eq!(pairings.deny(&code, now), Err(ErrorCode::BadRequest));
    }

    #[test]
    fn the_17th_pending_request_is_refused() {
        let (_dir, mut pairings) = store();
        let now = Instant::now();
        for i in 0..MAX_PENDING {
            pairings.start(&format!("app{i}"), None, now).unwrap();
        }
        assert_eq!(pairings.start("one-more", None, now), Err(ErrorCode::RateLimited));
    }

    #[test]
    fn a_pending_request_expires_after_ten_minutes() {
        let (_dir, mut pairings) = store();
        let t0 = Instant::now();
        let (request_id, code) = pairings.start("app", None, t0).unwrap();
        let almost = t0 + PENDING_EXPIRY - Duration::from_secs(1);
        assert_eq!(pairings.status(&request_id, almost), PairState::Pending);
        let late = t0 + PENDING_EXPIRY;
        assert_eq!(pairings.status(&request_id, late), PairState::Expired);
        assert_eq!(pairings.approve(&code, "default", late, T), Err(ErrorCode::BadRequest));
    }

    #[test]
    fn bad_names_are_refused() {
        let (_dir, mut pairings) = store();
        let now = Instant::now();
        assert_eq!(pairings.start("Bad Name", None, now), Err(ErrorCode::BadRequest));
        assert_eq!(pairings.start("app", Some("Bad"), now), Err(ErrorCode::BadRequest));
        let (_, code) = pairings.start("app", None, now).unwrap();
        assert_eq!(pairings.approve(&code, "Bad", now, T), Err(ErrorCode::BadRequest));
    }

    #[test]
    fn a_token_authenticates_its_client() {
        let (_dir, mut pairings) = store();
        let now = Instant::now();
        let token = pair(&mut pairings, "app", now);
        let client = pairings.authenticate(&token, T).unwrap();
        assert_eq!((client.name.as_str(), client.profile.as_str()), ("app", "default"));
        assert_eq!(pairings.authenticate("jevt_wrong", T), Err(ErrorCode::NotPaired));
    }

    #[test]
    fn approving_an_existing_name_replaces_the_client() {
        let (_dir, mut pairings) = store();
        let now = Instant::now();
        let old = pair(&mut pairings, "app", now);
        let (_, code) = pairings.start("app", None, now).unwrap();
        let (_, replaced) = pairings.approve(&code, "default", now, T).unwrap();
        assert!(replaced);
        assert_eq!(pairings.authenticate(&old, T), Err(ErrorCode::Revoked));
        assert_eq!(pairings.clients().len(), 1);
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
        let (_, code) = pairings.start("c", None, now).unwrap();
        pairings.approve(&code, "work", now, T).unwrap();
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
        assert_eq!(pairings.authenticate(&token, T + 59).unwrap().last_used, Some(T));
        assert_eq!(pairings.authenticate(&token, T + 60).unwrap().last_used, Some(T + 60));
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
        assert!(reloaded.pending_list(now).is_empty(), "pending requests live in memory only");
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
        let (_, code) = pairings.start("a", None, t0).unwrap();
        assert_eq!(pairings.approve(&code, "default", late, T), Err(ErrorCode::BadRequest));
        assert!(pairings.clients().is_empty());
        let (_dir, mut pairings) = store();
        let (_, code) = pairings.start("b", None, t0).unwrap();
        assert_eq!(pairings.deny(&code, late), Err(ErrorCode::BadRequest));
    }

    #[test]
    fn expired_requests_leave_the_list_and_free_their_slots() {
        let (_dir, mut pairings) = store();
        let t0 = Instant::now();
        for i in 0..MAX_PENDING {
            pairings.start(&format!("app{i}"), None, t0).unwrap();
        }
        let late = t0 + PENDING_EXPIRY;
        assert!(pairings.pending_list(late).is_empty());
        let (_dir, mut pairings) = store();
        for i in 0..MAX_PENDING {
            pairings.start(&format!("app{i}"), None, t0).unwrap();
        }
        assert!(pairings.start("one-more", None, late).is_ok());
    }

    #[test]
    fn an_approved_request_frees_its_slot_and_leaves_the_list() {
        let (_dir, mut pairings) = store();
        let now = Instant::now();
        let mut codes = Vec::new();
        for i in 0..MAX_PENDING {
            codes.push(pairings.start(&format!("app{i}"), None, now).unwrap().1);
        }
        pairings.approve(&codes[0], "default", now, T).unwrap();
        assert!(pairings.pending_list(now).iter().all(|view| view.code != codes[0]));
        assert!(pairings.start("one-more", None, now).is_ok());
    }

    #[test]
    fn a_code_works_once_only() {
        let (_dir, mut pairings) = store();
        let now = Instant::now();
        let (_, code) = pairings.start("a", None, now).unwrap();
        pairings.approve(&code, "default", now, T).unwrap();
        assert_eq!(pairings.approve(&code, "default", now, T), Err(ErrorCode::BadRequest));
        assert_eq!(pairings.deny(&code, now), Err(ErrorCode::BadRequest));
        let (_, code) = pairings.start("b", None, now).unwrap();
        pairings.deny(&code, now).unwrap();
        assert_eq!(pairings.approve(&code, "default", now, T), Err(ErrorCode::BadRequest));
        assert_eq!(pairings.clients().len(), 1);
    }

    #[test]
    fn a_typed_code_may_have_spaces_around_it() {
        let (_dir, mut pairings) = store();
        let now = Instant::now();
        let (_, code) = pairings.start("app", None, now).unwrap();
        assert!(pairings.approve(&format!("  {code}\n"), "default", now, T).is_ok());
    }

    #[test]
    fn the_pending_list_is_sorted_by_code() {
        let (_dir, mut pairings) = store();
        let now = Instant::now();
        for i in 0..MAX_PENDING {
            pairings.start(&format!("app{i}"), None, now).unwrap();
        }
        let codes: Vec<String> = pairings.pending_list(now).into_iter().map(|view| view.code).collect();
        let mut sorted = codes.clone();
        sorted.sort();
        assert_eq!(codes, sorted);
    }

    #[test]
    fn approve_records_the_created_time() {
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
        let (_, code) = pairings.start("app", None, now).unwrap();
        pairings.approve(&code, "work", now, T).unwrap();
        // Reload after each change: every save writes the whole store, so a
        // later save would hide an earlier change that did not save.
        let limit: Limit = "10/60".parse().unwrap();
        pairings.set_limit("app", Some(limit)).unwrap();
        let mut pairings = Pairings::load(&path).unwrap();
        assert_eq!(pairings.clients().iter().find(|c| c.name == "app").unwrap().limit(), Some(limit));
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
        assert!(Pairings::load(dir.path()).is_err(), "a folder cannot be read as the file");
    }

    // Security review: collection window, save before change, Debug, file rules.

    #[test]
    fn an_approved_token_can_be_collected_for_a_full_window_after_approval() {
        let (_dir, mut pairings) = store();
        let t0 = Instant::now();
        let (late_id, late_code) = pairings.start("late", None, t0).unwrap();
        let (gone_id, gone_code) = pairings.start("gone", None, t0).unwrap();
        let approved_at = t0 + Duration::from_secs(599);
        pairings.approve(&late_code, "default", approved_at, T).unwrap();
        pairings.approve(&gone_code, "default", approved_at, T).unwrap();
        let collect = t0 + Duration::from_secs(601);
        assert!(matches!(pairings.status(&late_id, collect), PairState::Approved(_)));
        assert_eq!(pairings.status(&gone_id, approved_at + PENDING_EXPIRY), PairState::Expired);
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
        pairings.set_limit("app", Some("10/60".parse().unwrap())).unwrap();
        let before = pairings.clients();
        let (request_id, code) = pairings.start("app", None, now).unwrap();
        break_saves(&path);

        assert!(pairings.approve(&code, "default", now, T).is_err());
        assert_eq!(pairings.status(&request_id, now), PairState::Pending);
        assert_eq!(pairings.authenticate(&old, T).unwrap().name, "app");

        assert!(pairings.revoke("app").is_err());
        assert_eq!(pairings.authenticate(&old, T).unwrap().name, "app");

        assert!(pairings.revoke_profile("default").is_err());
        assert_eq!(pairings.authenticate(&old, T).unwrap().name, "app");

        assert!(pairings.set_limit("app", None).is_err());
        assert_eq!(pairings.clients(), before, "no limit or last-used change without a save");
        assert_eq!(pairings.authenticate(&old, T + 600).unwrap().last_used, before[0].last_used);
        assert_eq!(pairings.clients(), before);
    }

    #[test]
    fn debug_output_never_holds_a_token() {
        let (_dir, mut pairings) = store();
        let now = Instant::now();
        let (request_id, code) = pairings.start("app", None, now).unwrap();
        pairings.approve(&code, "default", now, T).unwrap();
        let state = pairings.status(&request_id, now);
        assert!(matches!(state, PairState::Approved(_)));
        assert_eq!(format!("{state:?}"), "Approved(<redacted>)");
        assert!(!format!("{state:#?}").contains(TOKEN_PREFIX));
        assert_eq!(format!("{:?}", PairState::Pending), "Pending");
        assert_eq!(format!("{:?}", PairState::Denied), "Denied");
        assert_eq!(format!("{:?}", PairState::Expired), "Expired");
    }

    #[test]
    fn the_file_refuses_unknown_keys() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pairings.json");
        std::fs::write(&path, r#"{"clients": [], "revoked": [], "extra": 1}"#).unwrap();
        assert!(Pairings::load(&path).is_err());
    }

    #[test]
    fn the_file_refuses_a_name_or_a_hash_twice() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pairings.json");
        let client = |name: &str, hash: &str| {
            format!(r#"{{"name": "{name}", "profile": "default", "token_sha256": "{hash}", "created": 1}}"#)
        };
        let good = format!(r#"{{"clients": [{}, {}]}}"#, client("a", "aa"), client("b", "bb"));
        std::fs::write(&path, good).unwrap();
        assert!(Pairings::load(&path).is_ok());
        let same_name = format!(r#"{{"clients": [{}, {}]}}"#, client("a", "aa"), client("a", "bb"));
        std::fs::write(&path, same_name).unwrap();
        assert!(Pairings::load(&path).is_err());
        let same_hash = format!(r#"{{"clients": [{}, {}]}}"#, client("a", "aa"), client("b", "aa"));
        std::fs::write(&path, same_hash).unwrap();
        assert!(Pairings::load(&path).is_err());
    }

    #[test]
    fn a_client_with_an_unknown_key_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pairings.json");
        let text = r#"{"clients": [{"name": "a", "profile": "default", "token_sha256": "aa", "created": 1, "limt": "1/60"}]}"#;
        std::fs::write(&path, text).unwrap();
        assert!(Pairings::load(&path).is_err(), "a typo such as limt must not drop a limit silently");
    }
}
```

`crates/jevd/src/lib.rs`:

```rust
//! The jev daemon's library: checks, settings, and pairing.

pub mod answer;
pub mod files;
pub mod key;
pub mod pairing;
pub mod ratelimit;
pub mod redact;
pub mod settings;
pub mod shape;
pub mod units;
```

- [ ] **Step 2: Run the tests and see them fail**

Run: `cargo test -p jevd pairing`
Expected: compile errors: `cannot find type Pairings`.

- [ ] **Step 3: Write the implementation**

Put this above the tests in `crates/jevd/src/pairing.rs`:

```rust
//! Pairing (spec section 3): pending requests in memory, paired clients in a
//! JSON file that holds token hashes, never tokens. Only jevd writes the file.

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
pub const PENDING_EXPIRY: Duration = Duration::from_secs(600);
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
    /// Carries the token. Reported once only.
    Approved(String),
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

enum Stage {
    Waiting,
    Approved(String),
    Denied,
}

struct Pending {
    code: String,
    name: String,
    profile: Option<String>,
    created: Instant,
    stage: Stage,
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
            Ok(text) => serde_json::from_str(&text).map_err(|error| format!("{}: {error}", path.display()))?,
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
                return Err(format!("{}: the client {} appears twice", path.display(), client.name));
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
    fn commit(&mut self, clients: BTreeMap<String, Client>, revoked: BTreeSet<String>) -> Result<(), ErrorCode> {
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

    /// Start a pairing. Returns the request ID and the code.
    pub fn start(&mut self, name: &str, profile: Option<&str>, now: Instant) -> Result<(String, String), ErrorCode> {
        if !valid_name(name) || profile.is_some_and(|profile| !valid_name(profile)) {
            return Err(ErrorCode::BadRequest);
        }
        self.purge(now);
        let waiting = self
            .pending
            .values()
            .filter(|pending| matches!(pending.stage, Stage::Waiting))
            .count();
        if waiting >= MAX_PENDING {
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
                created: now,
                stage: Stage::Waiting,
            },
        );
        Ok((request_id, code))
    }

    /// The state of a request. `Approved` and `Denied` end the request.
    pub fn status(&mut self, request_id: &str, now: Instant) -> PairState {
        self.purge(now);
        match self
            .pending
            .get(request_id)
            .map(|pending| matches!(pending.stage, Stage::Waiting))
        {
            None => return PairState::Expired,
            Some(true) => return PairState::Pending,
            Some(false) => {}
        }
        match self.pending.remove(request_id).expect("present above").stage {
            Stage::Approved(token) => PairState::Approved(token),
            Stage::Denied => PairState::Denied,
            Stage::Waiting => PairState::Pending,
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

    /// Approve the request with `code`, bound to `profile`. Returns the client
    /// name, and true when an existing client of that name got replaced.
    pub fn approve(&mut self, code: &str, profile: &str, now: Instant, now_unix: u64) -> Result<(String, bool), ErrorCode> {
        if !valid_name(profile) {
            return Err(ErrorCode::BadRequest);
        }
        self.purge(now);
        let id = self.waiting_id(code).ok_or(ErrorCode::BadRequest)?;
        let token = format!("{TOKEN_PREFIX}{}", random_hex(32));
        let name = self.pending[&id].name.clone();
        let mut clients = self.clients.clone();
        let mut revoked = self.revoked.clone();
        let replaced = match clients.remove(&name) {
            Some(old) => {
                revoked.insert(old.token_sha256);
                true
            }
            None => false,
        };
        clients.insert(
            name.clone(),
            Client {
                name: name.clone(),
                profile: profile.to_string(),
                token_sha256: hash_token(&token),
                created: now_unix,
                last_used: None,
                limit: None,
            },
        );
        self.commit(clients, revoked)?;
        let pending = self.pending.get_mut(&id).expect("found above");
        pending.stage = Stage::Approved(token);
        // The expiry window restarts at approval, so the client gets a full
        // PENDING_EXPIRY to collect the token, however late the approval came.
        pending.created = now;
        Ok((name, replaced))
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
        let fresh = matches!(client.last_used, Some(t) if now_unix.saturating_sub(t) < LAST_USED_STEP);
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

    pub fn revoke(&mut self, name: &str) -> Result<(), ErrorCode> {
        let mut clients = self.clients.clone();
        let mut revoked = self.revoked.clone();
        let client = clients.remove(name).ok_or(ErrorCode::BadRequest)?;
        revoked.insert(client.token_sha256);
        self.commit(clients, revoked)
    }

    /// Revoke every client bound to `profile`, for `jev key remove --force`.
    /// Returns their names, sorted.
    pub fn revoke_profile(&mut self, profile: &str) -> Result<Vec<String>, ErrorCode> {
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

- [ ] **Step 4: Run the tests and see them pass**

Run: `cargo test -p jevd pairing`
Expected: `test result: ok. 32 passed`.

- [ ] **Step 5: Commit**

```bash
git add crates/jevd
git commit -m "feat(jevd): pairing store with codes, one-time tokens, and revocation" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 11: Lints, full test run, and spec updates

**Files:**
- Modify: `docs/superpowers/specs/2026-11-02-jev-daemon-design.md`
- Modify: any source file that `clippy` or `rustfmt` flags

- [ ] **Step 1: Format**

Run: `cargo fmt --all`
Then: `cargo fmt --all -- --check`
Expected: the check prints nothing and exits 0.

- [ ] **Step 2: Lint**

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: `Finished` with no warnings. If clippy flags a line, fix the code. Do not add `#[allow(...)]` unless the lint is wrong for that line, and then add a comment that says why.

- [ ] **Step 3: Full test run**

Run: `cargo test --workspace --locked`
Expected: every test passes. jev-proto has 11 tests. jevd has 93 on Windows and 94 on Unix (the extra one is the file-mode test).

- [ ] **Step 4: Bring the spec in line with this plan**

In `docs/superpowers/specs/2026-11-02-jev-daemon-design.md`, section 2, "Messages, protocol version 1", add this row below the `ask` row:

```markdown
| `pending` | `jev` only | none | The waiting requests: code, name, suggested profile |
```

In section 3, "Values", add these 2 rows below "Pending requests at once":

```markdown
| Full pending list | The next `pair` gets `rate-limited` |
| Unknown request ID | `pair-status` reports `expired`, the same as a real expiry |
```

In section 4, step 1, change "nesting depth limited" to "nesting depth at most 64 levels".

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "chore: format, lint, and record plan 1 choices in the spec" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

## Not tested by this plan

- Nothing here touches the network, a socket, a pipe, or the keyring. Plans 2 and 3 own those.
- Linux and macOS: this plan runs on Windows only, unless the person also runs it in WSL. CI (plan 6) covers all 3.
