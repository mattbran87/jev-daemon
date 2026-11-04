# jev-daemon CLI — implementation plan (plan 4 of 7)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the `jev` command line from spec section 6: daemon control, keys, pairing with the CLI token flow, settings with the loosening rule, and `jev ask`, with the spec's exit codes.

**Architecture:** The spec is `docs/superpowers/specs/2026-11-02-jev-daemon-design.md`. `jev` becomes a library and a thin `main`. Every command runs through `jev::run(args, env)`, where `Env` holds the folders, the keyring, the terminal, the standard streams, and a factory for the Jev client. Tests run `jev::run` in the test process, against a `jevd` that also runs in the test process, with a shared `MemoryKeyring`, a fake Jev, and a scripted terminal. So CI never touches a real keyring, and the release binary has no test-only keyring. `jev` uses the `jevd` library for the keyring, the settings, the rate-limit units, and `jev key add --check`.

**Tech Stack:** Rust stable (edition 2021, MSRV 1.85, `resolver = "3"`), plus plans 1 to 3's crates. New: `clap` 4 (derive; 4.6.7 resolves under the MSRV), `rpassword` 7 (7.5.4).

**Builds on:** plans 1 to 3, merged into `main` at `87a8a9e`. The spec is the current record of plan 3's design; its plan file is history.

**Commit rule:** gate each commit on `cargo test --workspace --locked`'s own exit status. Never pipe the test run into another command before the commit decision; redirecting it to a file is fine. End every commit message with the line `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

**TDD rule:** run the red step for real and read the failure. After green, run the mutation self-check that the task lists: break each rule one at a time, check that a test fails, and restore the file. After you restore a file, touch it (or run `cargo clean -p <crate>`), because cargo can skip a rebuild when a restored file keeps an old timestamp. If a mutant survives, add a test in a separate `test(...)` commit.

**Platform rule:** every test in this plan ran on Windows 11 only. Code under `#[cfg(unix)]` did not run here. Copy it exactly. Plan 6's CI tests it on Linux and macOS.

**Process rule:** a test that starts a `jevd` process must stop it. After each task, `tasklist | findstr jevd` (Windows) or `pgrep jevd` (Unix) must find none.

The code blocks are the files as tested, after `cargo fmt`.

## Choices this plan makes

- **`jev key add --check` is opt-in.** TypeSafe documents no key-check endpoint; the only known endpoint is `POST /v1/systemone`. So `--check` sends one small real request, which spends one request, and stores the key only if TypeSafe accepts it. Without `--check`, `jev key add` sends nothing. The person chose this on 2026-11-03.
- **`jev` runs commands through `Env`.** Tests swap in a `MemoryKeyring`, a `ScriptedTerminal`, a `TextInput`, and `Buffer` streams. The real binary uses `Env::from_os()`.
- **Interactive text goes to the terminal device.** Prompts, the waiting list of `jev allow`, and the replace warning go through `Terminal::say` and `Terminal::ask`, never stdout. Stdout holds only the command's result, so `--json` output stays one JSON object.
- **Exit codes.** A known error code maps through `ErrorCode::exit_code`; an unknown one is 1. A bad command line is 2, also when clap finds it. A command that needs a terminal and has none, or a loosening change that the person does not confirm, is `forbidden` (9).
- **`jev stop` with no daemon succeeds** and prints "jevd is not running", so a script can always stop.
- **Settings changes reach a running daemon only.** `jev config set` and `jev key add` save the file, then send `reload` only when a daemon runs. A daemon that starts later reads the file anyway.
- **`jev ask` reads stdin only when the request does not come from the command line, or to refuse.** With question flags or `--request`, `jev` reads a stdin that is not a terminal to its end; any data there is a bad command line (exit 2). A caller whose stdin is an open pipe that never closes would wait; the Claude Code mod closes the child's stdin at once.
- **A Windows bug from plan 3: `jevd` held the caller's pipes.** Windows starts every child with handle inheritance on, so a `jevd` that `jev` started got `jev`'s own standard handles. A caller that reads `jev`'s output through a pipe, as `$(jev start)` or a Node `spawn` does, then waited until `jevd` exited. Task 4's binary tests found it; `start_daemon` now clears the inherit flag on the standard handles first.
- **The spec is the current record.** Code reviews in Tasks 1 to 4 changed the design after this plan was written. Task 5 records the changes in the spec.

---

## File structure

| File | Responsibility |
| --- | --- |
| `crates/jev/Cargo.toml` | Dependencies |
| `crates/jev/src/lib.rs` | Module list, and `run`: parse, dispatch, report |
| `crates/jev/src/main.rs` | The binary: `Env::from_os`, `run`, the exit code |
| `crates/jev/src/term.rs` | `Terminal`, `DeviceTerminal` (`/dev/tty`, `CONIN$`), `ScriptedTerminal` |
| `crates/jev/src/env.rs` | `Env`, `Input`, `StdInput`, `TextInput`, `Buffer` |
| `crates/jev/src/out.rs` | `Failure`, `Outcome`, `success`, `report`, `failure_json`, `clean` |
| `crates/jev/src/cli.rs` | The clap command line |
| `crates/jev/src/link.rs` | `open`, `call`, `reload_if_running`, `text` |
| `crates/jev/src/ask.rs` | `jev ask`: build the request from one source, print JSON or a value |
| `crates/jev/src/daemon.rs` | `jev start`, `stop`, `status` |
| `crates/jev/src/config.rs` | `jev config`, and `confirm_loosening` |
| `crates/jev/src/keys.rs` | `jev key add`, `remove`, `list` |
| `crates/jev/src/pairing.rs` | `jev pair`, `allow`, `deny`, `revoke`, `clients`, `clients limit`, and the CLI commit flow |
| `crates/jev/tests/cli.rs` | Every command against an in-process `jevd` |
| `crates/jev/tests/binary.rs` | The real `jev` with the real `jevd` |
| `crates/jev-proto/src/client.rs` | Changed: `start_daemon` keeps the caller's handles from `jevd` (Task 4) |
| `crates/jev-proto/Cargo.toml` | Changed: the `Win32_System_Console` feature (Task 4) |
| `docs/superpowers/specs/2026-11-02-jev-daemon-design.md` | Records this plan's choices (Task 5) |

---

### Task 1: The terminal, the environment, and the output rules

Spec section 6, "Shared behavior" and "Exit codes".

- `Terminal::available` is true only when the terminal device opens: `/dev/tty` on Unix, `CONIN$` and `CONOUT$` on Windows. An agent or a script with no terminal gets false, so it cannot answer a prompt.
- `ask_hidden` uses `rpassword::prompt_password`, which also reads the terminal device.
- `Failure` carries a `WireError`, a `usage` flag for exit code 2, and extra JSON fields such as `pair_code`.
- `report` prints `jev: <code>: <message>` on stderr, or with `--json` the object `{"ok": false, "error": {...}}` on stdout. `clean` turns control, line-separator, and bidi characters in a message into spaces, so jevd's text cannot move the cursor or reorder a line.

**Files:**
- Modify: `crates/jev/Cargo.toml`
- Create: `crates/jev/src/lib.rs`, `crates/jev/src/term.rs`, `crates/jev/src/env.rs`, `crates/jev/src/out.rs`

- [ ] **Step 1: Add the dependencies**

Replace `crates/jev/Cargo.toml` with:

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
clap = { version = "4", features = ["derive"] }
jev-proto = { version = "0.1.0", path = "../jev-proto" }
jevd = { version = "0.1.0", path = "../jevd" }
rpassword = "7"
serde_json = "1"
tokio = { version = "1", features = ["rt", "time"] }
zeroize = "1.9"

[dev-dependencies]
tempfile = "3"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
wiremock = "0.6"
```

Run: `cargo build -p jev --locked`. If cargo says the lock file needs an update, run `cargo build -p jev` once without `--locked`, check with `git diff Cargo.lock` that only new crates were added (clap 4.6.x, rpassword 7.5.x, and their dependencies), then go on with `--locked`.

- [ ] **Step 2: Write the tests**

Create `crates/jev/src/lib.rs` with only these lines for now:

```rust
//! The jev command line (spec section 6).

pub mod env;
pub mod out;
pub mod term;
```

Create `crates/jev/src/term.rs` with only its test module for now:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scripted_terminal_answers_in_order_and_records_prompts() {
        let mut terminal = ScriptedTerminal::answering(&["a", "b"]);
        assert!(terminal.available());
        assert_eq!(terminal.ask("first? ").unwrap(), "a");
        terminal.say("note").unwrap();
        assert_eq!(terminal.ask_hidden("second? ").unwrap(), "b");
        assert!(terminal.ask("third? ").is_err());
        assert_eq!(terminal.shown(), ["first? ", "note", "second? ", "third? "]);
    }

    #[test]
    fn no_terminal_answers_nothing() {
        let mut terminal = ScriptedTerminal::none();
        assert!(!terminal.available());
        assert!(terminal.ask("x? ").is_err());
        assert!(terminal.say("x").is_err());
    }
}
```

Create `crates/jev/src/out.rs` with only its test module for now:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use jev_proto::WireCode;

    #[test]
    fn clean_turns_control_and_bidi_characters_into_spaces() {
        assert_eq!(
            clean("a\nb\x1b[2Jc\u{202e}d\u{2066}e\u{2028}f"),
            "a b [2Jc d e f"
        );
        assert_eq!(clean("plain text"), "plain text");
    }

    #[test]
    fn every_error_code_has_its_spec_exit_code() {
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
            assert_eq!(Failure::new(code, "x").exit_code(), exit, "{code:?}");
        }
        assert_eq!(Failure::usage("x").exit_code(), 2);
    }

    #[test]
    fn an_unknown_code_exits_1_and_the_json_keeps_the_raw_code() {
        let failure = Failure::from(WireError {
            code: WireCode::from_raw("quota-exceeded"),
            ..WireError::new(ErrorCode::Internal, "try later")
        });
        assert_eq!(failure.exit_code(), 1);
        assert_eq!(failure_json(&failure)["error"]["code"], "quota-exceeded");
    }

    #[test]
    fn the_json_cleans_the_message_and_carries_extra_fields() {
        let failure = Failure::new(ErrorCode::Pending, "run jev allow\u{202e}")
            .with("pair_code", json!("ABCD2345"));
        let value = failure_json(&failure);
        assert_eq!(value["ok"], false);
        assert_eq!(value["error"]["code"], "pending");
        assert_eq!(value["error"]["message"], "run jev allow ");
        assert_eq!(value["pair_code"], "ABCD2345");
    }
}
```

`env.rs` has no tests of its own; the later tasks use it.

- [ ] **Step 3: Run the tests and read the failure**

Run: `cargo test -p jev --lib`
Expected: FAIL to compile: `ScriptedTerminal`, `Failure`, `clean`, and `failure_json` are not found, and `env` has no file.

- [ ] **Step 4: Write the code**

Put this above the test module in `crates/jev/src/term.rs`:

```rust
//! The person's terminal (spec section 6, "Shared behavior"). Prompts read
//! the terminal device, `/dev/tty` or `CONIN$`, never stdin, so a script or
//! an agent that writes to stdin cannot answer them.

use std::collections::VecDeque;
use std::io::{self, BufRead, BufReader, Write};
use std::sync::{Arc, Mutex};

pub trait Terminal: Send {
    /// True when the terminal device opens. An agent or a script with no
    /// terminal gets false.
    fn available(&self) -> bool;
    /// Shows `prompt` and reads one line, without its line end.
    fn ask(&mut self, prompt: &str) -> io::Result<String>;
    /// As `ask`, with no echo, for a key.
    fn ask_hidden(&mut self, prompt: &str) -> io::Result<String>;
    /// Shows one line on the terminal, not on stdout.
    fn say(&mut self, text: &str) -> io::Result<()>;
}

/// The real terminal device.
pub struct DeviceTerminal;

#[cfg(unix)]
fn open() -> io::Result<(std::fs::File, std::fs::File)> {
    let input = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")?;
    let output = input.try_clone()?;
    Ok((input, output))
}

#[cfg(windows)]
fn open() -> io::Result<(std::fs::File, std::fs::File)> {
    let input = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("CONIN$")?;
    let output = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("CONOUT$")?;
    Ok((input, output))
}

impl Terminal for DeviceTerminal {
    fn available(&self) -> bool {
        open().is_ok()
    }

    fn ask(&mut self, prompt: &str) -> io::Result<String> {
        let (input, mut output) = open()?;
        output.write_all(prompt.as_bytes())?;
        output.flush()?;
        let mut line = String::new();
        if BufReader::new(input).read_line(&mut line)? == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        Ok(line.trim_end_matches(['\r', '\n']).to_string())
    }

    fn ask_hidden(&mut self, prompt: &str) -> io::Result<String> {
        rpassword::prompt_password(prompt)
    }

    fn say(&mut self, text: &str) -> io::Result<()> {
        let (_, mut output) = open()?;
        writeln!(output, "{text}")
    }
}

/// A terminal for tests: no terminal, or one with answers given ahead.
#[derive(Clone, Default)]
pub struct ScriptedTerminal {
    present: bool,
    answers: Arc<Mutex<VecDeque<String>>>,
    shown: Arc<Mutex<Vec<String>>>,
}

impl ScriptedTerminal {
    /// No terminal, as for an agent.
    pub fn none() -> ScriptedTerminal {
        ScriptedTerminal::default()
    }

    /// A terminal whose person types `answers`, one per prompt.
    pub fn answering(answers: &[&str]) -> ScriptedTerminal {
        ScriptedTerminal {
            present: true,
            answers: Arc::new(Mutex::new(answers.iter().map(|a| a.to_string()).collect())),
            shown: Arc::default(),
        }
    }

    /// Every prompt and line shown so far.
    pub fn shown(&self) -> Vec<String> {
        self.shown.lock().unwrap().clone()
    }

    fn next(&mut self, prompt: &str) -> io::Result<String> {
        if !self.present {
            return Err(io::ErrorKind::NotFound.into());
        }
        self.shown.lock().unwrap().push(prompt.to_string());
        self.answers
            .lock()
            .unwrap()
            .pop_front()
            .ok_or_else(|| io::ErrorKind::UnexpectedEof.into())
    }
}

impl Terminal for ScriptedTerminal {
    fn available(&self) -> bool {
        self.present
    }

    fn ask(&mut self, prompt: &str) -> io::Result<String> {
        self.next(prompt)
    }

    fn ask_hidden(&mut self, prompt: &str) -> io::Result<String> {
        self.next(prompt)
    }

    fn say(&mut self, text: &str) -> io::Result<()> {
        if !self.present {
            return Err(io::ErrorKind::NotFound.into());
        }
        self.shown.lock().unwrap().push(text.to_string());
        Ok(())
    }
}
```

Create `crates/jev/src/env.rs`:

```rust
//! Everything a command touches outside its own code: the folders, the
//! keyring, the terminal, the standard streams, and the Jev client for
//! `jev key add --check`. Tests put in-memory parts in its place.

use std::io::{self, IsTerminal, Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use jev_proto::Paths;
use jevd::jev_http::JevClient;
use jevd::keystore::{KeyStore, OsKeyring};

use crate::term::{DeviceTerminal, Terminal};

/// Standard input.
pub trait Input: Send {
    fn is_terminal(&self) -> bool;
    fn read_all(&mut self) -> io::Result<String>;
}

pub struct StdInput;

impl Input for StdInput {
    fn is_terminal(&self) -> bool {
        io::stdin().is_terminal()
    }

    fn read_all(&mut self) -> io::Result<String> {
        let mut text = String::new();
        io::stdin().read_to_string(&mut text)?;
        Ok(text)
    }
}

/// Standard input for tests: a terminal (`None`) or this text.
pub struct TextInput(pub Option<String>);

impl Input for TextInput {
    fn is_terminal(&self) -> bool {
        self.0.is_none()
    }

    fn read_all(&mut self) -> io::Result<String> {
        Ok(self.0.clone().unwrap_or_default())
    }
}

/// An output stream that tests can read back.
#[derive(Clone, Default)]
pub struct Buffer(Arc<Mutex<Vec<u8>>>);

impl Buffer {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap()).into_owned()
    }
}

impl Write for Buffer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Looks up one environment variable.
pub type Vars = Box<dyn Fn(&str) -> Option<String> + Send>;
/// Builds the client for `jev key add --check`.
pub type JevFactory = Box<dyn Fn() -> Result<JevClient, String> + Send>;

pub struct Env {
    pub paths: Paths,
    /// The `jevd` binary that `connect_or_start` starts.
    pub jevd: PathBuf,
    pub store: Arc<dyn KeyStore>,
    pub terminal: Box<dyn Terminal>,
    pub stdin: Box<dyn Input>,
    pub out: Box<dyn Write + Send>,
    pub err: Box<dyn Write + Send>,
    /// The environment variables, such as `JEV_CLIENT`.
    pub vars: Vars,
    /// Builds the client for `jev key add --check`.
    pub jev_client: JevFactory,
}

impl Env {
    /// The real environment: the OS keyring, the terminal device, the
    /// standard streams, and the `jevd` next to this `jev`.
    pub fn from_os() -> Result<Env, String> {
        let paths =
            Paths::from_env().map_err(|error| format!("cannot find the jev folders: {error}"))?;
        let me = std::env::current_exe()
            .map_err(|error| format!("cannot find this program: {error}"))?;
        let folder = me.parent().map(PathBuf::from).unwrap_or_default();
        Ok(Env {
            paths,
            jevd: folder.join(format!("jevd{}", std::env::consts::EXE_SUFFIX)),
            store: Arc::new(OsKeyring),
            terminal: Box::new(DeviceTerminal),
            stdin: Box::new(StdInput),
            out: Box::new(io::stdout()),
            err: Box::new(io::stderr()),
            vars: Box::new(|name| std::env::var(name).ok()),
            jev_client: Box::new(JevClient::new),
        })
    }
}
```

Put this above the test module in `crates/jev/src/out.rs`:

```rust
//! What a command prints, and its exit code (spec section 6, "Exit codes").
//! Human text by default; `--json` prints one JSON object on stdout.

use std::io::Write;

use jev_proto::{ErrorCode, WireError};
use serde_json::{json, Map, Value};

use crate::env::Env;

/// Why a command failed.
#[derive(Debug)]
pub struct Failure {
    pub error: WireError,
    /// A bad command line: exit code 2, whatever the error code.
    pub usage: bool,
    /// Extra fields for the JSON output, such as a pairing code.
    pub data: Map<String, Value>,
}

impl Failure {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Failure {
        Failure::from(WireError::new(code, message))
    }

    /// A bad command line. The JSON code is `bad-request`; the exit code is 2.
    pub fn usage(message: impl Into<String>) -> Failure {
        Failure {
            usage: true,
            ..Failure::new(ErrorCode::BadRequest, message)
        }
    }

    pub fn with(mut self, key: &str, value: Value) -> Failure {
        self.data.insert(key.to_string(), value);
        self
    }

    pub fn exit_code(&self) -> i32 {
        if self.usage {
            2
        } else {
            self.error.code.exit_code()
        }
    }
}

impl From<WireError> for Failure {
    fn from(error: WireError) -> Failure {
        Failure {
            error,
            usage: false,
            data: Map::new(),
        }
    }
}

pub type Outcome = Result<(), Failure>;

/// Text from jevd or the keyring, safe to print: control and bidi
/// characters become spaces, so a reply cannot move the cursor or reorder
/// the line on the screen.
pub fn clean(text: &str) -> String {
    text.chars()
        .map(|c| if unsafe_char(c) { ' ' } else { c })
        .collect()
}

fn unsafe_char(c: char) -> bool {
    c.is_control()
        || ('\u{202a}'..='\u{202e}').contains(&c)
        || ('\u{2066}'..='\u{2069}').contains(&c)
        || matches!(
            c,
            '\u{200e}' | '\u{200f}' | '\u{061c}' | '\u{2028}' | '\u{2029}'
        )
}

/// Prints a success: `human` as a line, or `data` with `"ok": true` as JSON.
pub fn success(env: &mut Env, json: bool, human: &str, data: Value) {
    if json {
        let mut object = match data {
            Value::Object(map) => map,
            _ => Map::new(),
        };
        object.insert("ok".into(), Value::Bool(true));
        let _ = writeln!(env.out, "{}", Value::Object(object));
    } else if !human.is_empty() {
        let _ = writeln!(env.out, "{human}");
    }
}

/// The failure as the JSON object that `--json` and `jev ask` print.
pub fn failure_json(failure: &Failure) -> Value {
    let mut error = serde_json::to_value(&failure.error).expect("an error always serializes");
    error["message"] = json!(clean(&failure.error.message));
    let mut object = failure.data.clone();
    object.insert("ok".into(), Value::Bool(false));
    object.insert("error".into(), error);
    Value::Object(object)
}

/// Prints a failure and returns its exit code. Human text goes to stderr as
/// `jev: <code>: <message>`; with `--json`, the object goes to stdout.
pub fn report(env: &mut Env, json: bool, failure: &Failure) -> i32 {
    if json {
        let _ = writeln!(env.out, "{}", failure_json(failure));
    } else {
        let _ = writeln!(
            env.err,
            "jev: {}: {}",
            failure.error.code.as_str(),
            clean(&failure.error.message)
        );
    }
    failure.exit_code()
}
```

- [ ] **Step 5: Run the tests**

Run: `cargo test -p jev --lib`
Expected: PASS, 6 tests.

- [ ] **Step 6: Mutation self-check**

1. In `out.rs`'s `unsafe_char`, drop the `'\u{202a}'..='\u{202e}'` range. `clean_turns_control_and_bidi_characters_into_spaces` must fail.
2. In `Failure::exit_code`, return the error code's exit code even for `usage`. `every_error_code_has_its_spec_exit_code` must fail.
3. In `ScriptedTerminal::next`, skip the `present` check. `no_terminal_answers_nothing` must fail.

- [ ] **Step 7: Commit**

```bash
git add Cargo.lock crates/jev
git commit -m "feat(jev): the terminal, the command environment, and the output rules" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: The command line and `jev ask`'s request

Spec section 6, "Commands" and "`jev ask`". `cli.rs` holds the whole clap command line. `ask.rs` builds the request from exactly one source:

- Flags: `--state <text>` or `--state @<file>`, and repeatable `--noul <id>=<question>` and `--score <id>=<question>`.
- `--request <json>`, for callers whose stdin is closed.
- JSON on stdin, `{"state": ..., "questions": {...}}`.

Flags and stdin JSON for the same question give byte-identical requests. Flags or `--request` plus data on stdin is exit 2. A terminal stdin with no request is exit 2. `--value <id>` must name a question (exit 2 before any connection). On failure with `--value`, stdout stays empty and stderr gets the error code. Without `--value`, every outcome is one JSON object on stdout.

The client comes from `--client`, or the `JEV_CLIENT` variable through `Env::vars`; there is no default. The token comes from the keyring entry `client:<name>`; none is `not-paired` (exit 4).

**Files:**
- Create: `crates/jev/src/cli.rs`, `crates/jev/src/link.rs`, `crates/jev/src/ask.rs`
- Modify: `crates/jev/src/lib.rs`

- [ ] **Step 1: Write the tests**

Create `crates/jev/src/cli.rs`:

```rust
//! The command line (spec section 6, "Commands").

use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "jev", version, about = "Ask TypeSafe's Jev model through jevd")]
pub struct Cli {
    /// Print one JSON object on stdout instead of text.
    #[arg(long, global = true)]
    pub json: bool,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Start jevd when it does not run.
    Start,
    /// Stop jevd. Requests in flight finish first.
    Stop,
    /// Show whether jevd runs, its version, and the profiles.
    Status,
    /// Manage the TypeSafe keys.
    Key {
        #[command(subcommand)]
        command: KeyCommand,
    },
    /// Pair a client whose token the jev command keeps.
    Pair {
        #[arg(long)]
        name: String,
        /// The profile to suggest at the approval.
        #[arg(long)]
        profile: Option<String>,
    },
    /// Approve a waiting pairing request. Needs a terminal.
    Allow { code: Option<String> },
    /// Refuse a waiting pairing request.
    Deny { code: String },
    /// End a client's pairing at once.
    Revoke { name: String },
    /// List the paired clients, or change one client's rate limit.
    Clients {
        #[command(subcommand)]
        command: Option<ClientsCommand>,
    },
    /// Read or change the settings.
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    /// Send one request to Jev. Prints JSON.
    Ask(AskArgs),
}

#[derive(Debug, Subcommand)]
pub enum KeyCommand {
    /// Store a key in the OS keyring.
    Add {
        #[arg(long, default_value = "default")]
        profile: String,
        /// Read the key from stdin instead of a hidden prompt.
        #[arg(long)]
        stdin: bool,
        /// Send one real request to check that TypeSafe accepts the key.
        #[arg(long)]
        check: bool,
    },
    /// Delete a profile and its key.
    Remove {
        #[arg(long)]
        profile: String,
        /// Also revoke the clients that use the profile.
        #[arg(long)]
        force: bool,
    },
    /// List the profiles.
    List,
}

#[derive(Debug, Subcommand)]
pub enum ClientsCommand {
    /// Set a client's rate limit, `<count>/<seconds>`, or `default`.
    Limit { name: String, limit: String },
}

#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    Get { name: String },
    Set { name: String, value: String },
    List,
}

#[derive(Debug, Args)]
pub struct AskArgs {
    /// The client whose token to use. Default: the JEV_CLIENT variable.
    #[arg(long)]
    pub client: Option<String>,
    /// The state text, or @<file> for a file's text.
    #[arg(long)]
    pub state: Option<String>,
    /// A noul question, <id>=<question>. Repeatable.
    #[arg(long)]
    pub noul: Vec<String>,
    /// A score question, <id>=<question>. Repeatable.
    #[arg(long)]
    pub score: Vec<String>,
    /// The whole request as JSON, for callers whose stdin is closed.
    #[arg(long)]
    pub request: Option<String>,
    /// Print only this answer's number.
    #[arg(long)]
    pub value: Option<String>,
}
```

Create `crates/jev/src/link.rs`:

```rust
//! The connection to jevd (spec section 6, "Shared behavior"): every command
//! starts jevd when no daemon runs, except `jev stop` and `jev status`.

use jev_proto::client::{connect_or_start, Connection};
use jev_proto::{ErrorCode, Request, WireError};
use serde_json::{Map, Value};

use crate::env::Env;

/// A connection to jevd. With `start`, starts jevd when no daemon runs.
pub fn open(env: &Env, start: bool) -> Result<Connection, WireError> {
    if start {
        connect_or_start(&env.paths, &env.jevd)
    } else {
        Connection::connect(&env.paths)
    }
}

/// Sends one request. A reply with `ok: false` becomes its error.
pub fn call(
    connection: &mut Connection,
    request: &Request,
) -> Result<Map<String, Value>, WireError> {
    let reply = connection.call(request)?;
    if reply.ok {
        return Ok(reply.data);
    }
    Err(reply
        .error
        .unwrap_or_else(|| WireError::new(ErrorCode::Internal, "jevd failed with no error")))
}

/// Sends `reload` when a daemon runs, so it reads the settings and the keys
/// again. No daemon: nothing to do, because jevd reads both at its start.
pub fn reload_if_running(env: &Env) -> Result<(), WireError> {
    match Connection::connect(&env.paths) {
        Ok(mut connection) => call(&mut connection, &Request::Reload).map(|_| ()),
        Err(error) if error.code.known() == Some(ErrorCode::DaemonUnavailable) => Ok(()),
        Err(error) => Err(error),
    }
}

/// A string field of a reply.
pub fn text(data: &Map<String, Value>, field: &str) -> Result<String, WireError> {
    data.get(field)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| {
            WireError::new(
                ErrorCode::Internal,
                format!("jevd's reply has no {field} field"),
            )
        })
}
```

Create `crates/jev/src/ask.rs` with only its test module for now:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{Cli, Command};
    use crate::env::{Buffer, TextInput};
    use crate::term::ScriptedTerminal;
    use clap::Parser;
    use jevd::keystore::MemoryKeyring;
    use std::sync::Arc;

    fn env(stdin: Option<&str>) -> Env {
        Env {
            paths: jev_proto::Paths::under(std::path::Path::new("/nowhere")).unwrap(),
            jevd: "/nowhere/jevd".into(),
            store: Arc::new(MemoryKeyring::new()),
            terminal: Box::new(ScriptedTerminal::none()),
            stdin: Box::new(TextInput(stdin.map(str::to_string))),
            out: Box::new(Buffer::default()),
            err: Box::new(Buffer::default()),
            vars: Box::new(|_| None),
            jev_client: Box::new(|| Err("no Jev in this test".to_string())),
        }
    }

    fn args(line: &[&str]) -> AskArgs {
        let mut full = vec!["jev", "ask"];
        full.extend_from_slice(line);
        match Cli::parse_from(full).command {
            Command::Ask(args) => args,
            other => panic!("not ask: {other:?}"),
        }
    }

    #[test]
    fn flags_and_stdin_json_give_the_same_request() {
        let flags = build(
            &args(&[
                "--state",
                "the state",
                "--noul",
                "q=Done?",
                "--score",
                "s=How good?",
            ]),
            &mut env(Some("")),
        )
        .unwrap();
        let stdin = r#"{"state":"the state","questions":{"q":{"type":"noul","question":"Done?"},"s":{"type":"score","question":"How good?"}}}"#;
        let piped = build(&args(&[]), &mut env(Some(stdin))).unwrap();
        assert_eq!(flags, piped);
        let line = |built: &Built| {
            serde_json::to_string(&Request::Ask {
                token: "t".into(),
                state: built.state.clone(),
                questions: built.questions.clone(),
            })
            .unwrap()
        };
        assert_eq!(line(&flags), line(&piped), "byte-identical requests");
        let request = build(&args(&["--request", stdin]), &mut env(Some(""))).unwrap();
        assert_eq!(request, piped);
    }

    #[test]
    fn flags_plus_piped_stdin_is_a_usage_error() {
        let failure = build(
            &args(&["--state", "s", "--noul", "q=x"]),
            &mut env(Some("{}")),
        )
        .unwrap_err();
        assert_eq!(failure.exit_code(), 2);
    }

    #[test]
    fn a_terminal_stdin_with_no_flags_is_a_usage_error() {
        let failure = build(&args(&[]), &mut env(None)).unwrap_err();
        assert_eq!(failure.exit_code(), 2);
    }

    #[test]
    fn bad_flags_are_usage_errors() {
        for line in [
            &["--noul", "q=x"][..],
            &["--state", "s"],
            &["--state", "s", "--noul", "no-equals"],
            &["--state", "s", "--noul", "q=a", "--score", "q=b"],
            &["--state", "s", "--request", "{}"],
            &["--request", "not json"],
            &["--request", r#"{"state":"s"}"#],
        ] {
            let failure = build(&args(line), &mut env(Some(""))).unwrap_err();
            assert_eq!(failure.exit_code(), 2, "{line:?}");
        }
    }

    #[test]
    fn state_from_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("state.txt");
        std::fs::write(&file, "from the file").unwrap();
        let state = format!("@{}", file.display());
        let built = build(
            &args(&["--state", &state, "--noul", "q=x"]),
            &mut env(Some("")),
        )
        .unwrap();
        assert_eq!(built.state, json!("from the file"));
    }

    #[test]
    fn no_client_and_an_unknown_value_id_exit_2_before_any_connection() {
        let mut no_client = env(Some(""));
        let code = run(&mut no_client, &args(&["--state", "s", "--noul", "q=x"]));
        assert_eq!(code, 2);
        let mut unknown = env(Some(""));
        let code = run(
            &mut unknown,
            &args(&[
                "--client", "app", "--state", "s", "--noul", "q=x", "--value", "other",
            ]),
        );
        assert_eq!(code, 2);
    }

    #[test]
    fn no_token_is_not_paired_with_json_on_stdout() {
        let out = Buffer::default();
        let mut env = env(Some(""));
        env.out = Box::new(out.clone());
        let code = run(
            &mut env,
            &args(&["--client", "app", "--state", "s", "--noul", "q=x"]),
        );
        assert_eq!(code, 4);
        let value: Value = serde_json::from_str(out.text().trim()).unwrap();
        assert_eq!(value["error"]["code"], "not-paired");
    }

    #[test]
    fn a_failure_with_value_leaves_stdout_empty_and_names_the_code_on_stderr() {
        let (out, err) = (Buffer::default(), Buffer::default());
        let mut env = env(Some(""));
        env.out = Box::new(out.clone());
        env.err = Box::new(err.clone());
        let code = run(
            &mut env,
            &args(&[
                "--client", "app", "--state", "s", "--noul", "q=x", "--value", "q",
            ]),
        );
        assert_eq!(code, 4);
        assert_eq!(out.text(), "");
        assert_eq!(err.text().trim(), "not-paired");
    }
}
```

Add `pub mod ask;`, `pub mod cli;`, and `pub mod link;` to `crates/jev/src/lib.rs`, in name order.

- [ ] **Step 2: Run the tests and read the failure**

Run: `cargo test -p jev --lib ask::`
Expected: FAIL to compile: `build`, `run`, and `Built` are not found.

- [ ] **Step 3: Write the code**

Put this above the test module in `crates/jev/src/ask.rs`:

```rust
//! `jev ask` (spec section 6). The request comes from one place: flags,
//! `--request`, or JSON on stdin. The output is always JSON on stdout,
//! except with `--value`, which prints only that answer's number.

use std::io::Write;

use jev_proto::{valid_name, ErrorCode, Request};
use jevd::keystore::client_account;
use serde_json::{json, Map, Value};

use crate::cli::AskArgs;
use crate::env::Env;
use crate::link::open;
use crate::out::{failure_json, Failure};

/// The state and the questions of one request.
#[derive(Debug, PartialEq)]
pub struct Built {
    pub state: Value,
    pub questions: Value,
}

/// `id=question` into its two parts.
fn split(flag: &str, text: &str) -> Result<(String, String), Failure> {
    text.split_once('=')
        .map(|(id, question)| (id.to_string(), question.to_string()))
        .ok_or_else(|| Failure::usage(format!("--{flag} takes <id>=<question>, not {text:?}")))
}

/// Builds the request from exactly one source.
pub fn build(args: &AskArgs, env: &mut Env) -> Result<Built, Failure> {
    let flags = args.state.is_some() || !args.noul.is_empty() || !args.score.is_empty();
    if flags && args.request.is_some() {
        return Err(Failure::usage(
            "--request cannot go with --state, --noul, or --score",
        ));
    }
    if flags || args.request.is_some() {
        if !env.stdin.is_terminal() {
            let piped = env
                .stdin
                .read_all()
                .map_err(|error| Failure::usage(format!("cannot read stdin: {error}")))?;
            if !piped.trim().is_empty() {
                return Err(Failure::usage(
                    "the request comes from the command line, and stdin has data too; use one",
                ));
            }
        }
        if let Some(request) = &args.request {
            return from_json(request);
        }
        return from_flags(args);
    }
    if env.stdin.is_terminal() {
        return Err(Failure::usage(
            "give the request as JSON on stdin, with --request, or with --state and --noul or --score",
        ));
    }
    let text = env
        .stdin
        .read_all()
        .map_err(|error| Failure::usage(format!("cannot read stdin: {error}")))?;
    from_json(&text)
}

fn from_json(text: &str) -> Result<Built, Failure> {
    let value: Value = serde_json::from_str(text)
        .map_err(|error| Failure::usage(format!("the request is not JSON: {error}")))?;
    let (Some(state), Some(questions)) = (value.get("state"), value.get("questions")) else {
        return Err(Failure::usage("the request needs a state and questions"));
    };
    Ok(Built {
        state: state.clone(),
        questions: questions.clone(),
    })
}

fn from_flags(args: &AskArgs) -> Result<Built, Failure> {
    let state = match args.state.as_deref() {
        None => return Err(Failure::usage("--state is needed with --noul or --score")),
        Some(text) => match text.strip_prefix('@') {
            Some(path) => std::fs::read_to_string(path)
                .map_err(|error| Failure::usage(format!("cannot read {path}: {error}")))?,
            None => text.to_string(),
        },
    };
    let mut questions = Map::new();
    let flags = [("noul", &args.noul), ("score", &args.score)];
    for (kind, list) in flags {
        for entry in list {
            let (id, question) = split(kind, entry)?;
            if questions.contains_key(&id) {
                return Err(Failure::usage(format!(
                    "the question id {id:?} appears twice"
                )));
            }
            questions.insert(id, json!({ "type": kind, "question": question }));
        }
    }
    if questions.is_empty() {
        return Err(Failure::usage(
            "give at least one --noul or --score question",
        ));
    }
    Ok(Built {
        state: Value::String(state),
        questions: Value::Object(questions),
    })
}

/// Runs `jev ask` and returns its exit code. It prints its own output.
pub fn run(env: &mut Env, args: &AskArgs) -> i32 {
    match ask(env, args) {
        Ok(reply) => print_reply(env, args, reply),
        Err(failure) => {
            if args.value.is_some() {
                let _ = writeln!(env.err, "{}", failure.error.code.as_str());
            } else {
                let _ = writeln!(env.out, "{}", failure_json(&failure));
            }
            failure.exit_code()
        }
    }
}

fn ask(env: &mut Env, args: &AskArgs) -> Result<jev_proto::Reply, Failure> {
    let client = args
        .client
        .clone()
        .or_else(|| (env.vars)("JEV_CLIENT"))
        .filter(|name| !name.is_empty())
        .ok_or_else(|| Failure::usage("name the client with --client or JEV_CLIENT"))?;
    if !valid_name(&client) {
        return Err(Failure::usage(format!(
            "{client:?} is not a valid client name"
        )));
    }
    let built = build(args, env)?;
    if let Some(id) = &args.value {
        if built.questions.get(id).is_none() {
            return Err(Failure::usage(format!("--value {id:?} names no question")));
        }
    }
    let token = env
        .store
        .get(&client_account(&client))
        .map_err(|error| Failure::new(ErrorCode::Internal, format!("the keyring failed: {error}")))?
        .ok_or_else(|| {
            Failure::new(
                ErrorCode::NotPaired,
                format!("client {client} has no token; run jev pair --name {client}"),
            )
        })?;
    let mut connection = open(env, true)?;
    Ok(connection.call(&Request::Ask {
        token: token.to_string(),
        state: built.state,
        questions: built.questions,
    })?)
}

fn print_reply(env: &mut Env, args: &AskArgs, reply: jev_proto::Reply) -> i32 {
    let failure = (!reply.ok).then(|| {
        Failure::from(reply.error.clone().unwrap_or_else(|| {
            jev_proto::WireError::new(ErrorCode::Internal, "jevd failed with no error")
        }))
    });
    let Some(id) = &args.value else {
        return match failure {
            Some(failure) => {
                let _ = writeln!(env.out, "{}", failure_json(&failure));
                failure.exit_code()
            }
            None => {
                let _ = write!(env.out, "{}", reply.to_line());
                0
            }
        };
    };
    if let Some(failure) = failure {
        let _ = writeln!(env.err, "{}", failure.error.code.as_str());
        return failure.exit_code();
    }
    let answer = &reply.data["answers"][id];
    match answer.get("noul").or_else(|| answer.get("score")) {
        Some(number) => {
            let _ = writeln!(env.out, "{number}");
            0
        }
        None => {
            let _ = writeln!(env.err, "bad-answer");
            ErrorCode::BadAnswer.exit_code()
        }
    }
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p jev --lib`
Expected: PASS, 14 tests.

- [ ] **Step 5: Mutation self-check**

1. In `build`, skip the stdin check when the request comes from flags. `flags_plus_piped_stdin_is_a_usage_error` must fail.
2. In `from_flags`, allow a question id twice. `bad_flags_are_usage_errors` must fail.
3. In `ask`, drop the `--value` id check. `no_client_and_an_unknown_value_id_exit_2_before_any_connection` must fail.
4. In `run`, print the failure JSON to stdout also with `--value`. `a_failure_with_value_leaves_stdout_empty_and_names_the_code_on_stderr` must fail.

- [ ] **Step 6: Commit**

```bash
git add crates/jev/src
git commit -m "feat(jev): the command line and jev ask's request" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: Every command

Spec section 6 and the CLI flow of section 3. This task adds the four command modules, `run`, and `main`, and tests every command end to end against a `jevd` that runs in the test process.

- **Daemon.** `jev start` connects and starts `jevd` when no daemon runs. `jev status` and `jev stop` never start one. `jev stop` with no daemon succeeds.
- **Keys.** `jev key add` reads the key from a hidden terminal prompt, or from stdin with `--stdin` (no terminal: exit 9). It checks the key's shape (`no-key`, `bad-key`: exit 6). With `--check`, it sends one real request and stores the key only if TypeSafe accepts it. It writes `profile:<name>`, adds the profile to the settings, and sends `reload` when a daemon runs. `jev key remove` refuses while clients use the profile; with `--force` it revokes them, deletes the keyring entry, removes the profile, and sends `reload`.
- **Pairing.** `jev pair` without a terminal prints the code and exits 3, with `pair_code` in its JSON. With a terminal, the person approves at once. `jev allow` needs a terminal (exit 9 without one): it lists the waiting requests, takes the code, offers the profiles, warns about a replace, and asks for a typed `y`. For a CLI request, `commit_cli` writes the token to the keyring, then sends `pair-status`. If the commit fails, it puts the old entry back, or deletes the new one, but only while the keyring still holds this run's token.
- **Limits and settings.** `jev clients limit` and `jev config set` call `confirm_loosening` for a change that loosens protection: no terminal, or any answer but `y`, is exit 9. A tightening change runs anywhere.
- **Never printed:** a key, a token, or an `approve` or `pair-status` reply.

**Files:**
- Create: `crates/jev/src/daemon.rs`, `crates/jev/src/config.rs`, `crates/jev/src/keys.rs`, `crates/jev/src/pairing.rs`, `crates/jev/tests/cli.rs`
- Modify: `crates/jev/src/lib.rs` (the whole file), `crates/jev/src/main.rs` (the whole file)

- [ ] **Step 1: Write the tests**

Create `crates/jev/tests/cli.rs`:

```rust
//! `jev` commands against a jevd that runs in this process, on its real
//! channel, with a shared in-memory keyring and a fake Jev. The test binary
//! counts as `jev`, so admin messages pass the peer check.

use std::ffi::OsString;
use std::sync::Arc;
use std::time::Duration;

use jev::env::{Buffer, Env, TextInput};
use jev::term::ScriptedTerminal;
use jev_proto::Paths;
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

struct Setup {
    _dir: tempfile::TempDir,
    paths: Paths,
    store: Arc<MemoryKeyring>,
    jev_url: String,
    daemon: Option<tokio::task::JoinHandle<()>>,
    _server: MockServer,
}

struct Ran {
    code: i32,
    out: String,
    err: String,
    shown: Vec<String>,
}

impl Ran {
    fn json(&self) -> Value {
        serde_json::from_str(self.out.trim()).unwrap_or_else(|_| panic!("not JSON: {:?}", self.out))
    }
}

async fn fake_jev(status: u16) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(status)
                .set_body_json(json!({"answers": {"q": {"type": "noul", "noul": 0.75}, "check": {"type": "noul", "noul": 0.5}}})),
        )
        .mount(&server)
        .await;
    server
}

/// A home with no daemon yet.
async fn setup() -> Setup {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::under(dir.path()).unwrap();
    let server = fake_jev(200).await;
    Setup {
        _dir: dir,
        paths,
        store: Arc::new(MemoryKeyring::new()),
        jev_url: server.uri(),
        daemon: None,
        _server: server,
    }
}

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
    /// Runs one `jev` command line off the runtime, as the blocking CLI does.
    async fn jev(&self, line: &[&str], terminal: ScriptedTerminal, stdin: Option<&str>) -> Ran {
        let (out, err) = (Buffer::default(), Buffer::default());
        let url = self.jev_url.clone();
        let mut env = Env {
            paths: self.paths.clone(),
            // No jevd binary: these tests always run a daemon in process, or none.
            jevd: self.paths.config_dir.join("no-such-jevd"),
            store: self.store.clone(),
            terminal: Box::new(terminal.clone()),
            stdin: Box::new(TextInput(stdin.map(str::to_string))),
            out: Box::new(out.clone()),
            err: Box::new(err.clone()),
            vars: Box::new(|_| None),
            jev_client: Box::new(move || {
                JevClient::with_endpoint(&url, Duration::from_secs(5), Duration::from_millis(10))
            }),
        };
        let mut args: Vec<OsString> = vec!["jev".into()];
        args.extend(line.iter().map(OsString::from));
        let code = tokio::time::timeout(
            Duration::from_secs(30),
            tokio::task::spawn_blocking(move || jev::run(args, &mut env)),
        )
        .await
        .expect("jev finished in 30 s")
        .unwrap();
        Ran {
            code,
            out: out.text(),
            err: err.text(),
            shown: terminal.shown(),
        }
    }

    async fn quiet(&self, line: &[&str]) -> Ran {
        self.jev(line, ScriptedTerminal::none(), Some("")).await
    }

    async fn add_key(&self, profile: &str) {
        let ran = self
            .jev(
                &["key", "add", "--stdin", "--profile", profile],
                ScriptedTerminal::none(),
                Some(KEY),
            )
            .await;
        assert_eq!(ran.code, 0, "{}", ran.err);
    }

    /// Pairs a CLI client in one step with a terminal, and returns its token.
    async fn pair(&self, name: &str) -> String {
        let ran = self
            .jev(
                &["pair", "--name", name],
                ScriptedTerminal::answering(&["", "y"]),
                Some(""),
            )
            .await;
        assert_eq!(ran.code, 0, "{} {:?}", ran.err, ran.shown);
        self.token(name).expect("the token is in the keyring")
    }

    fn token(&self, name: &str) -> Option<String> {
        self.store
            .get(&client_account(name))
            .unwrap()
            .map(|token| token.to_string())
    }

    fn ask(&self, client: &str) -> Vec<String> {
        [
            "ask",
            "--client",
            client,
            "--state",
            "the state",
            "--noul",
            "q=Done?",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    }
}

fn strs(owned: &[String]) -> Vec<&str> {
    owned.iter().map(String::as_str).collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn status_and_stop_without_a_daemon() {
    let setup = setup().await;
    let status = setup.quiet(&["status"]).await;
    assert_eq!(status.code, 8, "{}", status.err);
    let stop = setup.quiet(&["stop"]).await;
    assert_eq!(stop.code, 0);
    assert!(stop.out.contains("not running"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn start_status_and_stop_with_a_daemon() {
    let mut setup = running().await;
    assert_eq!(setup.quiet(&["start"]).await.code, 0);
    let status = setup.quiet(&["status", "--json"]).await;
    assert_eq!(status.code, 0);
    assert_eq!(status.json()["daemon_version"], env!("CARGO_PKG_VERSION"));
    let stop = setup.quiet(&["stop"]).await;
    assert_eq!(stop.code, 0, "{}", stop.err);
    let task = setup.daemon.take().unwrap();
    tokio::time::timeout(Duration::from_secs(10), task)
        .await
        .expect("jevd stopped")
        .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn key_add_from_stdin_stores_the_key_and_reloads_the_daemon() {
    let setup = running().await;
    let ran = setup
        .jev(
            &["key", "add", "--stdin", "--profile", "work"],
            ScriptedTerminal::none(),
            Some(&format!("  {KEY}\n")),
        )
        .await;
    assert_eq!(ran.code, 0, "{}", ran.err);
    assert!(
        !ran.out.contains(KEY) && !ran.err.contains(KEY),
        "the key is never printed"
    );
    assert_eq!(
        setup
            .store
            .get(&profile_account("work"))
            .unwrap()
            .unwrap()
            .as_str(),
        KEY
    );
    assert_eq!(
        Settings::load(&setup.paths.settings_file())
            .unwrap()
            .profiles,
        ["work"]
    );
    let status = setup.quiet(&["status", "--json"]).await;
    assert_eq!(
        status.json()["profiles"],
        json!(["work"]),
        "the daemon reloaded"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn key_add_needs_a_terminal_or_stdin_and_checks_the_shape() {
    let setup = running().await;
    let no_terminal = setup.quiet(&["key", "add"]).await;
    assert_eq!(no_terminal.code, 9);
    let typed = setup
        .jev(
            &["key", "add"],
            ScriptedTerminal::answering(&[KEY]),
            Some(""),
        )
        .await;
    assert_eq!(typed.code, 0, "{}", typed.err);
    assert_eq!(
        setup
            .store
            .get(&profile_account("default"))
            .unwrap()
            .unwrap()
            .as_str(),
        KEY
    );
    let bad = setup
        .jev(
            &["key", "add", "--stdin", "--profile", "bad"],
            ScriptedTerminal::none(),
            Some("has a space"),
        )
        .await;
    assert_eq!(bad.code, 6);
    assert!(setup.store.get(&profile_account("bad")).unwrap().is_none());
    let name = setup
        .jev(
            &["key", "add", "--stdin", "--profile", "Bad"],
            ScriptedTerminal::none(),
            Some(KEY),
        )
        .await;
    assert_eq!(name.code, 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn key_add_check_stores_only_a_key_that_jev_accepts() {
    let mut setup = running().await;
    let ok = setup
        .jev(
            &["key", "add", "--stdin", "--check"],
            ScriptedTerminal::none(),
            Some(KEY),
        )
        .await;
    assert_eq!(ok.code, 0, "{}", ok.err);
    assert!(ok.out.contains("accepted"));
    let refusing = fake_jev(401).await;
    setup.jev_url = refusing.uri();
    let refused = setup
        .jev(
            &["key", "add", "--stdin", "--check", "--profile", "other"],
            ScriptedTerminal::none(),
            Some(KEY),
        )
        .await;
    assert_eq!(refused.code, 6, "{}", refused.err);
    assert!(refused.err.contains("jev-unauthorized"));
    assert!(setup
        .store
        .get(&profile_account("other"))
        .unwrap()
        .is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pair_without_a_terminal_prints_the_code_and_exits_3() {
    let setup = running().await;
    setup.add_key("default").await;
    let first = setup.quiet(&["pair", "--name", "script", "--json"]).await;
    assert_eq!(first.code, 3, "{}", first.out);
    let code = first.json()["pair_code"].as_str().unwrap().to_string();
    assert_eq!(first.json()["error"]["code"], "pending");
    let repeat = setup.quiet(&["pair", "--name", "script", "--json"]).await;
    assert_eq!(
        repeat.json()["pair_code"],
        code,
        "a repeat gets the same code"
    );
    let allow = setup.quiet(&["allow", &code]).await;
    assert_eq!(allow.code, 9, "jev allow needs a terminal");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_cli_flow_stores_the_token_and_ask_uses_it() {
    let setup = running().await;
    setup.add_key("default").await;
    let pending = setup.quiet(&["pair", "--name", "script", "--json"]).await;
    let code = pending.json()["pair_code"].as_str().unwrap().to_string();
    let allow = setup
        .jev(
            &["allow"],
            ScriptedTerminal::answering(&[&code.to_lowercase(), "", "y"]),
            Some(""),
        )
        .await;
    assert_eq!(allow.code, 0, "{} {:?}", allow.err, allow.shown);
    let token = setup.token("script").expect("jev wrote the token");
    assert!(
        !allow.out.contains(&token) && !allow.err.contains(&token),
        "the token is never printed"
    );
    assert!(allow.shown.iter().all(|line| !line.contains(&token)));
    let ask = setup.ask("script");
    let answer = setup.quiet(&strs(&ask)).await;
    assert_eq!(answer.code, 0, "{}", answer.out);
    assert_eq!(answer.json()["answers"]["q"]["noul"], 0.75);
    let mut value = ask.clone();
    value.extend(["--value".to_string(), "q".to_string()]);
    let number = setup.quiet(&strs(&value)).await;
    assert_eq!(number.out.trim(), "0.75");
    let piped = setup
        .jev(
            &["ask", "--client", "script"],
            ScriptedTerminal::none(),
            Some(r#"{"state":"s","questions":{"q":{"type":"noul","question":"Done?"}}}"#),
        )
        .await;
    assert_eq!(piped.code, 0, "{}", piped.out);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_new_pairing_replaces_the_old_token_at_the_commit() {
    let setup = running().await;
    setup.add_key("default").await;
    let old = setup.pair("app").await;
    let new = setup.pair("app").await;
    assert_ne!(old, new);
    let ran = setup.quiet(&strs(&setup.ask("app"))).await;
    assert_eq!(ran.code, 0, "the keyring holds the new token: {}", ran.out);
    let clients = setup.quiet(&["clients", "--json"]).await;
    assert_eq!(clients.json()["clients"].as_array().unwrap().len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_failed_commit_puts_the_old_token_back() {
    let setup = running().await;
    setup.add_key("default").await;
    let old = setup.pair("app").await;
    let file = setup.paths.pairing_file();
    std::fs::remove_file(&file).unwrap();
    std::fs::create_dir(&file).unwrap();
    std::fs::write(file.join("inside"), "x").unwrap();
    let failed = setup
        .jev(
            &["pair", "--name", "app"],
            ScriptedTerminal::answering(&["", "y"]),
            Some(""),
        )
        .await;
    assert_eq!(failed.code, 1, "internal: {}", failed.err);
    assert_eq!(setup.token("app").as_deref(), Some(old.as_str()));
    std::fs::remove_dir_all(&file).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_failed_first_commit_leaves_no_token() {
    let setup = running().await;
    setup.add_key("default").await;
    let file = setup.paths.pairing_file();
    std::fs::create_dir_all(file.join("inside")).unwrap();
    let failed = setup
        .jev(
            &["pair", "--name", "fresh"],
            ScriptedTerminal::answering(&["", "y"]),
            Some(""),
        )
        .await;
    assert_eq!(failed.code, 1);
    assert_eq!(setup.token("fresh"), None);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_unconfirmed_approval_approves_nothing() {
    let setup = running().await;
    setup.add_key("default").await;
    let ran = setup
        .jev(
            &["pair", "--name", "app"],
            ScriptedTerminal::answering(&["", "n"]),
            Some(""),
        )
        .await;
    assert_eq!(ran.code, 9);
    assert_eq!(setup.token("app"), None);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn revoke_stops_a_client_and_deny_ends_a_request() {
    let setup = running().await;
    setup.add_key("default").await;
    setup.pair("app").await;
    assert_eq!(setup.quiet(&["revoke", "app"]).await.code, 0);
    let ran = setup.quiet(&strs(&setup.ask("app"))).await;
    assert_eq!(ran.code, 4);
    assert_eq!(ran.json()["error"]["code"], "revoked");
    let pending = setup.quiet(&["pair", "--name", "other", "--json"]).await;
    let code = pending.json()["pair_code"].as_str().unwrap().to_string();
    assert_eq!(setup.quiet(&["deny", &code]).await.code, 0);
    assert_eq!(setup.quiet(&["deny", &code]).await.code, 9);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_looser_client_limit_needs_a_terminal() {
    let setup = running().await;
    setup.add_key("default").await;
    setup.pair("app").await;
    assert_eq!(
        setup
            .quiet(&["clients", "limit", "app", "10/60"])
            .await
            .code,
        0,
        "tighter"
    );
    let looser = setup.quiet(&["clients", "limit", "app", "100/60"]).await;
    assert_eq!(looser.code, 9);
    let confirmed = setup
        .jev(
            &["clients", "limit", "app", "100/60"],
            ScriptedTerminal::answering(&["y"]),
            Some(""),
        )
        .await;
    assert_eq!(confirmed.code, 0, "{}", confirmed.err);
    let default = setup.quiet(&["clients", "limit", "app", "default"]).await;
    assert_eq!(default.code, 9, "120/60 is looser than 100/60");
    assert_eq!(
        setup.quiet(&["clients", "limit", "app", "fast"]).await.code,
        2
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn config_changes_follow_the_loosening_rule() {
    let setup = running().await;
    assert_eq!(
        setup
            .quiet(&["config", "set", "idle-timeout", "30m"])
            .await
            .code,
        0,
        "tighter"
    );
    assert_eq!(
        setup
            .quiet(&["config", "set", "idle-timeout", "4h"])
            .await
            .code,
        9
    );
    assert_eq!(
        setup
            .quiet(&["config", "set", "redact-strict", "on"])
            .await
            .code,
        0
    );
    let confirmed = setup
        .jev(
            &["config", "set", "idle-timeout", "4h"],
            ScriptedTerminal::answering(&["y"]),
            Some(""),
        )
        .await;
    assert_eq!(confirmed.code, 0);
    assert_eq!(
        setup
            .quiet(&["config", "get", "idle-timeout"])
            .await
            .out
            .trim(),
        "4h"
    );
    let list = setup.quiet(&["config", "list", "--json"]).await;
    assert_eq!(list.json()["settings"]["redact-strict"], "on");
    assert_eq!(setup.quiet(&["config", "get", "nope"]).await.code, 2);
    assert_eq!(
        setup
            .quiet(&["config", "set", "key-cache", "off"])
            .await
            .code,
        2
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn key_remove_refuses_while_clients_use_the_profile() {
    let setup = running().await;
    setup.add_key("default").await;
    setup.pair("app").await;
    let refused = setup
        .quiet(&["key", "remove", "--profile", "default"])
        .await;
    assert_eq!(refused.code, 9);
    assert!(refused.err.contains("app"));
    let forced = setup
        .quiet(&["key", "remove", "--profile", "default", "--force"])
        .await;
    assert_eq!(forced.code, 0, "{}", forced.err);
    assert!(setup
        .store
        .get(&profile_account("default"))
        .unwrap()
        .is_none());
    assert!(Settings::load(&setup.paths.settings_file())
        .unwrap()
        .profiles
        .is_empty());
    let ran = setup.quiet(&strs(&setup.ask("app"))).await;
    assert_eq!(ran.json()["error"]["code"], "revoked");
    let list = setup.quiet(&["key", "list", "--json"]).await;
    assert_eq!(list.json()["profiles"], json!([]));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_bad_command_line_exits_2() {
    let setup = setup().await;
    assert_eq!(setup.quiet(&["nope"]).await.code, 2);
    assert_eq!(setup.quiet(&["pair"]).await.code, 2);
    let version = setup.quiet(&["--version"]).await;
    assert_eq!(version.code, 0);
    assert!(version.out.contains(env!("CARGO_PKG_VERSION")));
}
```

The test module of `crates/jev/src/pairing.rs` (the file comes in Step 3; put this at its end):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use jevd::keystore::MemoryKeyring;

    const ACCOUNT: &str = "client:app";

    #[test]
    fn restore_puts_the_old_token_back() {
        let store = MemoryKeyring::new();
        store.set(ACCOUNT, "mine").unwrap();
        restore(&store, ACCOUNT, Some("old"), "mine").unwrap();
        assert_eq!(store.get(ACCOUNT).unwrap().unwrap().as_str(), "old");
    }

    #[test]
    fn restore_deletes_a_new_entry_when_there_was_none() {
        let store = MemoryKeyring::new();
        store.set(ACCOUNT, "mine").unwrap();
        restore(&store, ACCOUNT, None, "mine").unwrap();
        assert!(store.get(ACCOUNT).unwrap().is_none());
    }

    #[test]
    fn restore_keeps_a_token_that_another_run_wrote() {
        let store = MemoryKeyring::new();
        store.set(ACCOUNT, "theirs").unwrap();
        restore(&store, ACCOUNT, Some("old"), "mine").unwrap();
        assert_eq!(store.get(ACCOUNT).unwrap().unwrap().as_str(), "theirs");
        restore(&store, ACCOUNT, None, "mine").unwrap();
        assert_eq!(store.get(ACCOUNT).unwrap().unwrap().as_str(), "theirs");
    }
}
```

- [ ] **Step 2: Run the tests and read the failure**

Run: `cargo test -p jev --test cli`
Expected: FAIL to compile: `jev::run` is not found.

- [ ] **Step 3: Write the commands**

Create `crates/jev/src/daemon.rs`:

```rust
//! `jev start`, `jev stop`, `jev status`.

use jev_proto::{ErrorCode, Request};
use serde_json::{json, Value};

use crate::env::Env;
use crate::link::{call, open};
use crate::out::{clean, success, Outcome};

pub fn start(env: &mut Env, json: bool) -> Outcome {
    let mut connection = open(env, true)?;
    let data = call(&mut connection, &Request::Status)?;
    let version = clean(
        data.get("daemon_version")
            .and_then(Value::as_str)
            .unwrap_or("?"),
    );
    success(
        env,
        json,
        &format!("jevd {version} runs"),
        Value::Object(data),
    );
    Ok(())
}

pub fn status(env: &mut Env, json: bool) -> Outcome {
    let mut connection = open(env, false)?;
    let data = call(&mut connection, &Request::Status)?;
    let version = clean(
        data.get("daemon_version")
            .and_then(Value::as_str)
            .unwrap_or("?"),
    );
    let uptime = data.get("uptime").and_then(Value::as_u64).unwrap_or(0);
    let profiles: Vec<String> = data
        .get("profiles")
        .and_then(Value::as_array)
        .map(|list| list.iter().filter_map(Value::as_str).map(clean).collect())
        .unwrap_or_default();
    let human = format!(
        "jevd {version} runs, up {uptime} s; profiles: {}",
        if profiles.is_empty() {
            "none".to_string()
        } else {
            profiles.join(", ")
        }
    );
    success(env, json, &human, Value::Object(data));
    Ok(())
}

/// Stopping a daemon that does not run succeeds, so a script can always stop.
pub fn stop(env: &mut Env, json: bool) -> Outcome {
    let mut connection = match open(env, false) {
        Ok(connection) => connection,
        Err(error) if error.code.known() == Some(ErrorCode::DaemonUnavailable) => {
            success(
                env,
                json,
                "jevd is not running",
                json!({ "running": false }),
            );
            return Ok(());
        }
        Err(error) => return Err(error.into()),
    };
    call(&mut connection, &Request::Stop)?;
    success(env, json, "jevd stops", json!({ "running": true }));
    Ok(())
}
```

Create `crates/jev/src/config.rs`:

```rust
//! `jev config get|set|list` (spec section 5). A change that loosens
//! protection needs a terminal and a typed `y`; without one it exits 9.

use jev_proto::ErrorCode;
use jevd::settings::{Settings, SETTING_NAMES};
use serde_json::{json, Map, Value};

use crate::env::Env;
use crate::link::reload_if_running;
use crate::out::{success, Failure, Outcome};

/// Asks the person to confirm a change that loosens protection.
/// No terminal, or any answer but `y`: `forbidden` (exit 9).
pub fn confirm_loosening(env: &mut Env, what: &str) -> Outcome {
    if !env.terminal.available() {
        return Err(Failure::new(
            ErrorCode::Forbidden,
            format!("this change loosens protection ({what}); run it in a terminal"),
        ));
    }
    let answer = env
        .terminal
        .ask(&format!(
            "This loosens protection: {what}. Type y to go on: "
        ))
        .map_err(|error| Failure::new(ErrorCode::Forbidden, format!("no answer: {error}")))?;
    if answer.trim() != "y" {
        return Err(Failure::new(
            ErrorCode::Forbidden,
            "the change was not confirmed",
        ));
    }
    Ok(())
}

pub fn load(env: &Env) -> Result<Settings, Failure> {
    Settings::load(&env.paths.settings_file())
        .map_err(|problem| Failure::new(ErrorCode::Internal, problem))
}

pub fn save(env: &Env, settings: &Settings) -> Outcome {
    settings
        .save(&env.paths.settings_file())
        .map_err(|problem| Failure::new(ErrorCode::Internal, problem))
}

pub fn get(env: &mut Env, json: bool, name: &str) -> Outcome {
    let settings = load(env)?;
    let value = settings.get(name).ok_or_else(|| {
        Failure::usage(format!(
            "no setting named {name:?}; names: {}",
            SETTING_NAMES.join(", ")
        ))
    })?;
    success(env, json, &value, json!({ "name": name, "value": value }));
    Ok(())
}

pub fn list(env: &mut Env, json: bool) -> Outcome {
    let settings = load(env)?;
    let pairs = settings.list();
    let human: Vec<String> = pairs
        .iter()
        .map(|(name, value)| format!("{name} = {value}"))
        .collect();
    let data: Map<String, Value> = pairs
        .into_iter()
        .map(|(name, value)| (name.to_string(), Value::String(value)))
        .collect();
    success(env, json, &human.join("\n"), json!({ "settings": data }));
    Ok(())
}

pub fn set(env: &mut Env, json: bool, name: &str, value: &str) -> Outcome {
    let settings = load(env)?;
    let change = settings.with_change(name, value).map_err(Failure::usage)?;
    if change.loosens {
        let old = settings.get(name).unwrap_or_default();
        confirm_loosening(env, &format!("{name} from {old} to {value}"))?;
    }
    save(env, &change.settings)?;
    reload_if_running(env)?;
    success(
        env,
        json,
        &format!("{name} = {value}"),
        json!({ "name": name, "value": value }),
    );
    Ok(())
}
```

Create `crates/jev/src/keys.rs`:

```rust
//! `jev key add`, `jev key remove`, `jev key list` (spec section 6). A key
//! goes from the person straight into the OS keyring; it never passes
//! through the socket, and jev never prints it.

use jev_proto::{valid_name, ErrorCode, Request};
use jevd::key::check_key;
use jevd::keystore::profile_account;
use serde_json::{json, Value};
use zeroize::Zeroizing;

use crate::config::{load, save};
use crate::env::Env;
use crate::link::{call, open, reload_if_running};
use crate::out::{success, Failure, Outcome};

fn check_name(profile: &str) -> Outcome {
    if valid_name(profile) {
        return Ok(());
    }
    Err(Failure::usage(format!(
        "{profile:?}: profile names use lowercase letters, digits, and hyphens, 1 to 64 characters"
    )))
}

fn keyring_failure(error: impl std::fmt::Display) -> Failure {
    Failure::new(ErrorCode::Internal, format!("the keyring failed: {error}"))
}

pub fn add(env: &mut Env, json: bool, profile: &str, stdin: bool, check: bool) -> Outcome {
    check_name(profile)?;
    let raw = Zeroizing::new(if stdin {
        env.stdin.read_all().map_err(|error| {
            Failure::new(ErrorCode::Internal, format!("cannot read stdin: {error}"))
        })?
    } else {
        if !env.terminal.available() {
            return Err(Failure::new(
                ErrorCode::Forbidden,
                "jev key add needs a terminal for its hidden prompt; or pass the key with --stdin",
            ));
        }
        env.terminal
            .ask_hidden(&format!("TypeSafe key for profile {profile}: "))
            .map_err(|error| {
                Failure::new(ErrorCode::Internal, format!("cannot read the key: {error}"))
            })?
    });
    let key = check_key(&raw).map_err(|code| {
        Failure::new(
            code,
            match code {
                ErrorCode::NoKey => "the key is empty".to_string(),
                _ => "the key must be printable ASCII with no spaces, at most 512 characters"
                    .to_string(),
            },
        )
    })?;
    if check {
        check_with_jev(env, key)?;
    }
    env.store
        .set(&profile_account(profile), key)
        .map_err(keyring_failure)?;
    let mut settings = load(env)?;
    settings
        .add_profile(profile)
        .map_err(|problem| Failure::new(ErrorCode::Internal, problem))?;
    save(env, &settings)?;
    reload_if_running(env)?;
    let checked = if check { "; TypeSafe accepted it" } else { "" };
    success(
        env,
        json,
        &format!("Stored the key for profile {profile}{checked}"),
        json!({ "profile": profile, "checked": check }),
    );
    Ok(())
}

/// Sends one small real request with `key`. It spends one request.
fn check_with_jev(env: &Env, key: &str) -> Outcome {
    let client =
        (env.jev_client)().map_err(|problem| Failure::new(ErrorCode::Internal, problem))?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| {
            Failure::new(
                ErrorCode::Internal,
                format!("cannot start the runtime: {error}"),
            )
        })?;
    let state = json!("jev key add --check");
    let questions = json!({ "check": { "type": "noul", "question": "Is this a test request?" } });
    runtime
        .block_on(client.ask(&state, &questions, key, false))
        .map(|_| ())
        .map_err(|failure| Failure::from(failure.to_wire()))
}

pub fn remove(env: &mut Env, json: bool, profile: &str, force: bool) -> Outcome {
    check_name(profile)?;
    let mut connection = open(env, true)?;
    let data = call(&mut connection, &Request::Clients)?;
    let users: Vec<String> = data
        .get("clients")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|client| client["profile"] == profile)
        .filter_map(|client| client["name"].as_str().map(str::to_string))
        .collect();
    if !users.is_empty() && !force {
        return Err(Failure::new(
            ErrorCode::BadRequest,
            format!(
                "profile {profile} is in use by: {}; jev key remove --force also revokes them",
                users.join(", ")
            ),
        ));
    }
    for name in &users {
        match call(&mut connection, &Request::Revoke { name: name.clone() }) {
            Ok(_) => {}
            // Revoked by someone else in the meantime.
            Err(error) if error.code.known() == Some(ErrorCode::BadRequest) => {}
            Err(error) => return Err(error.into()),
        }
    }
    env.store
        .delete(&profile_account(profile))
        .map_err(keyring_failure)?;
    let mut settings = load(env)?;
    settings.remove_profile(profile);
    save(env, &settings)?;
    call(&mut connection, &Request::Reload)?;
    success(
        env,
        json,
        &format!("Removed profile {profile}; revoked {} clients", users.len()),
        json!({ "profile": profile, "revoked": users }),
    );
    Ok(())
}

pub fn list(env: &mut Env, json: bool) -> Outcome {
    let settings = load(env)?;
    let human = if settings.profiles.is_empty() {
        "No profiles; run jev key add".to_string()
    } else {
        settings.profiles.join("\n")
    };
    success(env, json, &human, json!({ "profiles": settings.profiles }));
    Ok(())
}
```

Create `crates/jev/src/pairing.rs` with this code, then its test module from Step 1:

```rust
//! Pairing from the CLI (spec section 3): `jev pair`, `jev allow`, `jev deny`,
//! `jev revoke`, `jev clients`. For a CLI client, `approve` gives jev the
//! token; jev writes it to the keyring, then commits it with `pair-status`.
//! Replies that carry a token are never printed.

use jev_proto::client::Connection;
use jev_proto::{ErrorCode, Request, WireError};
use jevd::keystore::{client_account, KeyStore, StoreError};
use jevd::pairing::normalize_code;
use jevd::units::Limit;
use serde_json::{json, Map, Value};
use zeroize::Zeroizing;

use crate::config::{confirm_loosening, load};
use crate::env::Env;
use crate::link::{call, open, text};
use crate::out::{clean, success, Failure, Outcome};

/// One waiting request, as `pending` lists it.
struct Waiting {
    code: String,
    name: String,
    profile: Option<String>,
}

fn waiting(connection: &mut Connection) -> Result<Vec<Waiting>, Failure> {
    let data = call(connection, &Request::Pending)?;
    Ok(data
        .get("requests")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|request| {
            Some(Waiting {
                code: request["code"].as_str()?.to_string(),
                name: request["name"].as_str()?.to_string(),
                profile: request["profile"].as_str().map(str::to_string),
            })
        })
        .collect())
}

fn say(env: &mut Env, text: &str) -> Outcome {
    env.terminal.say(text).map_err(|error| {
        Failure::new(
            ErrorCode::Internal,
            format!("cannot write to the terminal: {error}"),
        )
    })
}

fn ask(env: &mut Env, prompt: &str) -> Result<String, Failure> {
    env.terminal
        .ask(prompt)
        .map(|answer| answer.trim().to_string())
        .map_err(|error| {
            Failure::new(
                ErrorCode::Forbidden,
                format!("no answer from the terminal: {error}"),
            )
        })
}

fn need_terminal(env: &Env, command: &str) -> Outcome {
    if env.terminal.available() {
        return Ok(());
    }
    Err(Failure::new(
        ErrorCode::Forbidden,
        format!("{command} needs a terminal, so that a person approves the client"),
    ))
}

/// `jev pair`. With a terminal, the person approves at once. Without one,
/// it prints the code and exits 3 (`pending`); `jev allow` finishes it.
pub fn pair(env: &mut Env, json: bool, name: &str, profile: Option<&str>) -> Outcome {
    let mut connection = open(env, true)?;
    let data = call(
        &mut connection,
        &Request::Pair {
            name: name.to_string(),
            profile: profile.map(str::to_string),
        },
    )?;
    let code = text(&data, "code")?;
    if !env.terminal.available() {
        return Err(Failure::new(
            ErrorCode::Pending,
            format!("approve this client in a terminal: jev allow {code}"),
        )
        .with("pair_code", json!(code))
        .with("name", json!(name)));
    }
    say(env, &format!("Pairing code for {name}: {code}"))?;
    let request = Waiting {
        code,
        name: name.to_string(),
        profile: profile.map(str::to_string),
    };
    approve(env, json, &mut connection, &request)
}

/// `jev allow [<code>]`: show the waiting requests, and approve one.
pub fn allow(env: &mut Env, json: bool, code: Option<&str>) -> Outcome {
    need_terminal(env, "jev allow")?;
    let mut connection = open(env, true)?;
    let list = waiting(&mut connection)?;
    let code = match code {
        Some(code) => code.to_string(),
        None => {
            if list.is_empty() {
                return Err(Failure::new(
                    ErrorCode::BadRequest,
                    "no pairing request waits",
                ));
            }
            say(env, "Waiting pairing requests:")?;
            for request in &list {
                let line = format!("  {}  {}", request.code, clean(&request.name));
                say(env, &line)?;
            }
            ask(env, "Code to approve: ")?
        }
    };
    let code = normalize_code(&code);
    let request = list
        .into_iter()
        .find(|request| request.code == code)
        .ok_or_else(|| Failure::new(ErrorCode::BadRequest, "no waiting request has that code"))?;
    approve(env, json, &mut connection, &request)
}

/// Picks the profile, warns about a replace, asks for a `y`, and approves.
fn approve(env: &mut Env, json: bool, connection: &mut Connection, request: &Waiting) -> Outcome {
    let profiles = load(env)?.profiles;
    if profiles.is_empty() {
        return Err(Failure::new(
            ErrorCode::BadRequest,
            "no profile has a key yet; run jev key add first",
        ));
    }
    let suggested = request
        .profile
        .clone()
        .filter(|profile| profiles.contains(profile))
        .unwrap_or_else(|| profiles[0].clone());
    let name = clean(&request.name);
    say(env, &format!("Profiles: {}", profiles.join(", ")))?;
    let answer = ask(env, &format!("Profile for {name} [{suggested}]: "))?;
    let profile = if answer.is_empty() { suggested } else { answer };
    if !profiles.contains(&profile) {
        return Err(Failure::new(
            ErrorCode::BadRequest,
            format!("no profile named {:?}", clean(&profile)),
        ));
    }
    let clients = call(connection, &Request::Clients)?;
    let exists = clients
        .get("clients")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .any(|client| client["name"] == request.name);
    if exists {
        say(
            env,
            &format!(
                "A client named {name} exists. The new one replaces it when its token is stored. \
                 jev revoke {name} stops the old one at once."
            ),
        )?;
    }
    if ask(
        env,
        &format!("Approve {name} on profile {profile}? Type y: "),
    )? != "y"
    {
        return Err(Failure::new(
            ErrorCode::Forbidden,
            "the approval was not confirmed",
        ));
    }
    let data = call(
        connection,
        &Request::Approve {
            code: request.code.clone(),
            profile: profile.clone(),
        },
    )?;
    if data.contains_key("token") {
        commit_cli(env, connection, &request.name, &data)?;
    }
    let human = if data.contains_key("token") {
        format!("Paired {name} on profile {profile}; its token is in the keyring")
    } else {
        format!("Approved {name} on profile {profile}; the client collects its token")
    };
    success(
        env,
        json,
        &human,
        json!({ "name": request.name, "profile": profile }),
    );
    Ok(())
}

/// The CLI flow (spec section 3): write the token to the keyring, then
/// commit with `pair-status`. If the commit fails, put the old entry back,
/// but only if the keyring still holds this run's token, so a second
/// `jev allow` that ran meanwhile keeps its token.
fn commit_cli(
    env: &mut Env,
    connection: &mut Connection,
    name: &str,
    data: &Map<String, Value>,
) -> Outcome {
    let request_id = text(data, "request_id")?;
    let token = Zeroizing::new(text(data, "token")?);
    let account = client_account(name);
    let old = env.store.get(&account).map_err(keyring)?;
    env.store.set(&account, &token).map_err(keyring)?;
    let committed = call(connection, &Request::PairStatus { request_id }).and_then(|reply| {
        match reply.get("state").and_then(Value::as_str) {
            Some("approved") => Ok(()),
            other => Err(WireError::new(
                ErrorCode::BadRequest,
                format!("the pairing ended as {:?}", other.unwrap_or("nothing")),
            )),
        }
    });
    let Err(error) = committed else {
        return Ok(());
    };
    restore(
        env.store.as_ref(),
        &account,
        old.as_deref().map(String::as_str),
        &token,
    )
    .map_err(keyring)?;
    Err(error.into())
}

/// After a failed commit: puts `old` back in `account`, or deletes the
/// entry when there was none, but only while the keyring still holds
/// `token`, the token that this run wrote. Another run's newer token stays.
fn restore(
    store: &dyn KeyStore,
    account: &str,
    old: Option<&str>,
    token: &str,
) -> Result<(), StoreError> {
    let current = store.get(account)?;
    if current.as_deref().map(String::as_str) != Some(token) {
        return Ok(());
    }
    match old {
        Some(old) => store.set(account, old),
        None => store.delete(account).map(|_| ()),
    }
}

fn keyring(error: StoreError) -> Failure {
    Failure::new(ErrorCode::Internal, format!("the keyring failed: {error}"))
}

pub fn deny(env: &mut Env, json: bool, code: &str) -> Outcome {
    let mut connection = open(env, true)?;
    call(
        &mut connection,
        &Request::Deny {
            code: code.to_string(),
        },
    )?;
    success(env, json, "Denied", json!({}));
    Ok(())
}

pub fn revoke(env: &mut Env, json: bool, name: &str) -> Outcome {
    let mut connection = open(env, true)?;
    call(
        &mut connection,
        &Request::Revoke {
            name: name.to_string(),
        },
    )?;
    success(
        env,
        json,
        &format!("Revoked {}", clean(name)),
        json!({ "name": name }),
    );
    Ok(())
}

pub fn clients(env: &mut Env, json: bool) -> Outcome {
    let mut connection = open(env, true)?;
    let data = call(&mut connection, &Request::Clients)?;
    let list = data
        .get("clients")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let human = if list.is_empty() {
        "No paired clients".to_string()
    } else {
        list.iter()
            .map(|client| {
                format!(
                    "{}  profile {}  limit {}",
                    clean(client["name"].as_str().unwrap_or("?")),
                    clean(client["profile"].as_str().unwrap_or("?")),
                    clean(client["limit"].as_str().unwrap_or("default")),
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    success(env, json, &human, Value::Object(data));
    Ok(())
}

/// `jev clients limit <name> <count>/<seconds>|default`. A limit looser than
/// the client's current one needs a terminal and a typed `y`.
pub fn limit(env: &mut Env, json: bool, name: &str, value: &str) -> Outcome {
    let new: Option<Limit> = match value {
        "default" => None,
        text => Some(text.parse().map_err(Failure::usage)?),
    };
    let mut connection = open(env, true)?;
    let data = call(&mut connection, &Request::Clients)?;
    let client = data
        .get("clients")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|client| client["name"] == name)
        .ok_or_else(|| {
            Failure::new(
                ErrorCode::BadRequest,
                format!("no client named {:?}", clean(name)),
            )
        })?;
    let default = load(env)?.rate_limit;
    let current = client["limit"]
        .as_str()
        .and_then(|text| text.parse::<Limit>().ok())
        .unwrap_or(default);
    let effective = new.unwrap_or(default);
    if effective.looser_than(current) {
        confirm_loosening(
            env,
            &format!("the limit of {} from {current} to {effective}", clean(name)),
        )?;
    }
    call(
        &mut connection,
        &Request::SetLimit {
            name: name.to_string(),
            limit: new.map(|limit| limit.to_string()),
        },
    )?;
    success(
        env,
        json,
        &format!("The limit of {} is {effective}", clean(name)),
        json!({ "name": name, "limit": new.map(|limit| limit.to_string()) }),
    );
    Ok(())
}
```

Replace `crates/jev/src/lib.rs` with:

```rust
//! The jev command line (spec section 6). `run` parses the arguments, runs
//! one command against an `Env`, prints its output, and returns the exit
//! code. Tests call `run` with an in-memory `Env`.

pub mod ask;
pub mod cli;
pub mod config;
pub mod daemon;
pub mod env;
pub mod keys;
pub mod link;
pub mod out;
pub mod pairing;
pub mod term;

use std::ffi::OsString;
use std::io::Write;

use clap::Parser;

use crate::cli::{Cli, ClientsCommand, Command, ConfigCommand, KeyCommand};
use crate::env::Env;
use crate::out::report;

/// Runs one command line. Returns the exit code (spec section 6).
pub fn run(args: Vec<OsString>, env: &mut Env) -> i32 {
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(error) => {
            let code = error.exit_code();
            let text = error.render().to_string();
            let stream = if code == 0 {
                &mut env.out
            } else {
                &mut env.err
            };
            let _ = write!(stream, "{text}");
            return code;
        }
    };
    let json = cli.json;
    let outcome = match &cli.command {
        Command::Ask(args) => return ask::run(env, args),
        Command::Start => daemon::start(env, json),
        Command::Stop => daemon::stop(env, json),
        Command::Status => daemon::status(env, json),
        Command::Key { command } => match command {
            KeyCommand::Add {
                profile,
                stdin,
                check,
            } => keys::add(env, json, profile, *stdin, *check),
            KeyCommand::Remove { profile, force } => keys::remove(env, json, profile, *force),
            KeyCommand::List => keys::list(env, json),
        },
        Command::Pair { name, profile } => pairing::pair(env, json, name, profile.as_deref()),
        Command::Allow { code } => pairing::allow(env, json, code.as_deref()),
        Command::Deny { code } => pairing::deny(env, json, code),
        Command::Revoke { name } => pairing::revoke(env, json, name),
        Command::Clients { command } => match command {
            None => pairing::clients(env, json),
            Some(ClientsCommand::Limit { name, limit }) => pairing::limit(env, json, name, limit),
        },
        Command::Config { command } => match command {
            ConfigCommand::Get { name } => config::get(env, json, name),
            ConfigCommand::Set { name, value } => config::set(env, json, name, value),
            ConfigCommand::List => config::list(env, json),
        },
    };
    match outcome {
        Ok(()) => 0,
        Err(failure) => report(env, json, &failure),
    }
}
```

Replace `crates/jev/src/main.rs` with:

```rust
//! The `jev` binary. All the work is in the library's `run`.

use std::process::ExitCode;

fn main() -> ExitCode {
    let mut env = match jev::env::Env::from_os() {
        Ok(env) => env,
        Err(problem) => {
            eprintln!("jev: internal: {problem}");
            return ExitCode::from(1);
        }
    };
    let code = jev::run(std::env::args_os().collect(), &mut env);
    ExitCode::from(u8::try_from(code).unwrap_or(1))
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p jev --test cli`
Expected: PASS, 16 tests, in about 1 s.

Run: `cargo test -p jev --lib pairing::`
Expected: PASS, 3 tests.

Run: `cargo test --workspace --locked`
Expected: PASS.

- [ ] **Step 5: Mutation self-check**

1. In `commit_cli`, skip the restore when the commit fails. `a_failed_commit_puts_the_old_token_back` must fail.
2. In `restore`, drop the check that the keyring still holds this run's token. `restore_keeps_a_token_that_another_run_wrote` must fail.
3. In `keys::add`, store the key before the `--check` request. `key_add_check_stores_only_a_key_that_jev_accepts` must fail.
4. In `pairing::limit`, skip `confirm_loosening`. `a_looser_client_limit_needs_a_terminal` must fail.
5. In `config::set`, skip `confirm_loosening`. `config_changes_follow_the_loosening_rule` must fail.
6. In `pairing::allow`, skip `need_terminal`. `pair_without_a_terminal_prints_the_code_and_exits_3` must fail.
7. In `keys::remove`, skip the in-use check. `key_remove_refuses_while_clients_use_the_profile` must fail.

- [ ] **Step 6: Commit**

```bash
git add crates/jev
git commit -m "feat(jev): every command, with the CLI pairing flow and the loosening rule" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: The real binaries, and `jevd` that held the caller's pipes

Spec section 11: "A copy of `jev` in another folder sends `approve`: `forbidden`." These tests run the real `jev` with the real `jevd` next to it in the target folder, under a new `JEV_HOME`. No command here touches the keyring.

The tests find a plan 3 bug on Windows: `Command::output()` waits until every process that holds its pipes closes them, and the `jevd` that `jev start` starts inherits `jev`'s standard handles. So the test hangs. The fix clears the inherit flag on the three standard handles before `start_daemon` spawns `jevd`.

**Files:**
- Create: `crates/jev/tests/binary.rs`
- Modify: `crates/jev-proto/src/client.rs` (`start_daemon`), `crates/jev-proto/Cargo.toml`

- [ ] **Step 1: Write the tests**

Create `crates/jev/tests/binary.rs`:

```rust
//! The real `jev` binary, with the real `jevd` next to it in the target
//! folder, under a new `JEV_HOME`. No command here touches the keyring.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, Instant};

use jev_proto::client::Connection;
use jev_proto::paths::HOME_VAR;
use jev_proto::Paths;

const JEV: &str = env!("CARGO_BIN_EXE_jev");

fn jevd_next_to_jev() -> PathBuf {
    let jevd = Path::new(JEV).with_file_name(format!("jevd{}", std::env::consts::EXE_SUFFIX));
    assert!(
        jevd.exists(),
        "{} is missing; run cargo build -p jevd (cargo test --workspace builds it)",
        jevd.display()
    );
    jevd
}

fn run(program: &Path, home: &Path, args: &[&str]) -> Output {
    Command::new(program)
        .args(args)
        .env(HOME_VAR, home)
        .env_remove("JEV_CLIENT")
        .output()
        .unwrap()
}

/// Stops the daemon under `home` when dropped, also after a failed assert.
struct Stopper(PathBuf);

impl Drop for Stopper {
    fn drop(&mut self) {
        let _ = run(Path::new(JEV), &self.0, &["stop"]);
    }
}

fn wait_until_stopped(paths: &Paths) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Connection::connect(paths).is_ok() {
        assert!(Instant::now() < deadline, "jevd did not stop in 10 s");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn version_and_bad_command_lines() {
    let dir = tempfile::tempdir().unwrap();
    let version = run(Path::new(JEV), dir.path(), &["--version"]);
    assert!(version.status.success());
    assert_eq!(
        String::from_utf8_lossy(&version.stdout).trim(),
        format!("jev {}", env!("CARGO_PKG_VERSION"))
    );
    assert_eq!(
        run(Path::new(JEV), dir.path(), &["nope"]).status.code(),
        Some(2)
    );
    assert_eq!(run(Path::new(JEV), dir.path(), &[]).status.code(), Some(2));
}

#[test]
fn the_real_jev_starts_and_stops_jevd() {
    jevd_next_to_jev();
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::under(dir.path()).unwrap();
    let _stopper = Stopper(dir.path().to_path_buf());
    let start = run(Path::new(JEV), dir.path(), &["start"]);
    assert!(
        start.status.success(),
        "{}",
        String::from_utf8_lossy(&start.stderr)
    );
    let status = run(Path::new(JEV), dir.path(), &["status", "--json"]);
    assert!(status.status.success());
    let stop = run(Path::new(JEV), dir.path(), &["stop"]);
    assert!(stop.status.success(), "the real jev passes the peer check");
    wait_until_stopped(&paths);
}

#[test]
fn a_copy_of_jev_in_another_folder_cannot_stop_jevd() {
    jevd_next_to_jev();
    let dir = tempfile::tempdir().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    let copy = elsewhere.path().join(Path::new(JEV).file_name().unwrap());
    std::fs::copy(JEV, &copy).unwrap();
    let _stopper = Stopper(dir.path().to_path_buf());
    assert!(run(Path::new(JEV), dir.path(), &["start"]).status.success());
    let stop = run(&copy, dir.path(), &["stop"]);
    assert_eq!(
        stop.status.code(),
        Some(9),
        "{}",
        String::from_utf8_lossy(&stop.stderr)
    );
    assert!(String::from_utf8_lossy(&stop.stderr).contains("forbidden"));
}
```

- [ ] **Step 2: Run the tests and read the failure**

Run, from Git Bash: `cargo build -p jevd -p jev`, then `timeout 120 cargo test -p jev --test binary`.
Expected on Windows: `version_and_bad_command_lines` passes, and the run hangs in `the_real_jev_starts_and_stops_jevd` until `timeout` stops it (exit 124). On Linux and macOS the tests pass already, because std sets close-on-exec on its own descriptors and `start_daemon` gives `jevd` null standard streams.

Then stop what the hung run left: `tasklist | findstr "jevd binary-"`, then `taskkill /F /PID <id>` for each process that this test started (its path is under this repo's `target` folder).

- [ ] **Step 3: Write the fix**

In `crates/jev-proto/Cargo.toml`, add `"Win32_System_Console"` to the `windows-sys` feature list, in name order (after `"Win32_Storage_FileSystem"`).

In `crates/jev-proto/src/client.rs`, add this function right above `pub fn start_daemon`:

```rust
/// Windows starts every child with handle inheritance on, so a started jevd
/// would get the caller's own standard handles. A caller that reads this
/// process's output through a pipe (a script's `$(jev start)`, a Node
/// `spawn`) would then wait until jevd exits, up to its idle timeout. Clear
/// the inherit flag on the three standard handles; this process keeps them.
#[cfg(windows)]
fn keep_std_handles_from_children() {
    use windows_sys::Win32::Foundation::{SetHandleInformation, HANDLE_FLAG_INHERIT};
    use windows_sys::Win32::System::Console::{
        GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
    };
    for which in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
        // SAFETY: GetStdHandle has no preconditions; a null or invalid
        // handle makes SetHandleInformation fail, which is ignored.
        unsafe {
            let handle = GetStdHandle(which);
            if !handle.is_null() {
                SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0);
            }
        }
    }
}
```

In the `#[cfg(windows)]` block of `start_daemon`, call it before the first spawn, as the first line after the `use` lines:

```rust
        keep_std_handles_from_children();
        let detached = DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP;
```

- [ ] **Step 4: Run the tests**

Run: `cargo build -p jevd -p jev`, then `timeout 120 cargo test -p jev --test binary`
Expected: PASS, 3 tests, in about 5 s. No `jevd` left running.

Run: `cargo test --workspace --locked`
Expected: PASS.

Run the jev-proto cross-checks, with `CARGO_TARGET_DIR` set to a scratch folder:
`cargo clippy -p jev-proto --all-targets --target x86_64-unknown-linux-gnu -- -D warnings` and the same with `--target aarch64-apple-darwin`.
Expected: no warnings.

- [ ] **Step 5: Commit**

```bash
git add Cargo.lock crates/jev/tests/binary.rs crates/jev-proto
git commit -m "fix(jev-proto): a started jevd never holds the caller's standard handles" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: Lints, full test run, and spec updates

**Files:**
- Modify: `docs/superpowers/specs/2026-11-02-jev-daemon-design.md`

- [ ] **Step 1: Format and lint**

Run: `cargo fmt --all --check`
Expected: no output.

Run: `cargo clippy --workspace --all-targets --locked -- -D warnings`
Expected: no warnings.

- [ ] **Step 2: Run the full suite 3 times**

Run: `cargo test --workspace --locked` three times.
Expected: PASS each time. On Windows: `jev` 17 unit, 16 `cli`, 3 `binary`; `jev-proto` 36; `jevd` 247 and 1 ignored, plus 3, 6, and 6. No `jevd` left running.

- [ ] **Step 3: Update the spec**

Check each value against the code before you write it.

In section 6, replace the paragraph that starts "`jev key add` can send one test request" with:

```markdown
`jev key add` sends nothing by default. With `--check`, it sends one small real request and stores the key only if TypeSafe accepts it; that spends one request. TypeSafe documents no key-check endpoint.
```

Add these items at the end of section 6, "Shared behavior":

```markdown
- Prompts, lists, and warnings go to the terminal device. Stdout holds only the result, so `--json` output stays one JSON object: the reply data with `"ok": true`, or `{"ok": false, "error": {...}}`.
- A command that needs a terminal and has none exits 9 (`forbidden`). So does a loosening change that the person does not confirm with `y`.
- `jev pair` without a terminal exits 3 and prints `pair_code` in its JSON.
- `jev stop` with no daemon succeeds.
- `jev config set` and `jev key add` send `reload` only when a daemon runs.
- `jev` cleans control and bidi characters from every message it prints from jevd.
```

In section 2, "Transport", in the Client row, add at the end: `On Windows, the client clears the inherit flag on its own standard handles before it starts jevd, so a caller that reads its output through a pipe does not wait for jevd.`

In section 12, replace the "TypeSafe key check" item with:

```markdown
- **TypeSafe key check.** Checked in plan 4: no key-check endpoint is documented. `jev key add --check` spends one request, and only when the person asks for it.
```

- [ ] **Step 4: Check the spec style**

Run: `python "<the simple-english-docs skill>/scripts/ste_lint.py" --quiet docs/superpowers/specs/2026-11-02-jev-daemon-design.md`. On another machine, use the `simple-english-docs` skill's `scripts/ste_lint.py`. Fix each ERROR in the lines this task added.

- [ ] **Step 5: Commit**

```bash
git add docs/superpowers/specs/2026-11-02-jev-daemon-design.md
git commit -m "docs: record plan 4 choices in the spec" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

## Not tested by this plan

- Linux and macOS: no test ran there. Plan 6's CI runs the suite on both. The `jev-proto` Unix code type-checks with the cross targets; `jev` and `jevd` cannot, because `aws-lc-sys` needs a C cross compiler.
- The real terminal device (`DeviceTerminal`): every test uses `ScriptedTerminal`. Check `jev allow` and `jev key add` by hand once on each OS.
- The real OS keyring through `jev`: every test uses `MemoryKeyring`.
- `jev key add --check` against the real TypeSafe endpoint.
- A second `jev allow` for the same name that runs between this run's keyring write and its commit. The unit tests of `restore` cover the rule, not the timing.
- The transport failure at the commit's `pair-status`: no test cuts the connection at that call.
- The re-read in `keys::remove` after a race: a client that someone else revokes between `clients` and `revoke`.
- The real terminal and the real keyring through `jev`: check by hand on each OS.

## Notes for plan 5

- The mod spawns `jev ask --client cc-<plugin> --request <json>` and reads one JSON object from stdout. The exit code follows the spec table; an unknown error code is 1, and the JSON keeps the raw code.
- On `not-paired`, the mod spawns `jev pair --name cc-<plugin> --json`. With no terminal, it exits 3 and prints `{"ok": false, "error": {"code": "pending", ...}, "pair_code": "<code>", "name": ...}`. A repeat gets the same code while the request waits.
- `jev` may start `jevd`. On Windows that no longer holds the mod's pipes (Task 4).
