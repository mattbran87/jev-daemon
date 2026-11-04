# jev-tray Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `jev-tray`, a small program with one tray icon that shows whether `jevd` runs. It has a **Start at login** item, its own release archives, and CI on the 3 systems.

**Architecture:** A new crate, `crates/jev-tray`, with a library and one binary. The library holds the parts that need no tray: the state check (`state.rs`), the icon choice (`icon.rs`), the login entry (`login.rs`), and the one-copy lock (`one_copy.rs`). The binary (`main.rs`) runs a `tao` event loop with a `tray-icon` icon and a `muda` menu. A worker thread checks `jevd` every 5 seconds and sends each result to the event loop. The icons are PNGs that a developer renders once from 2 SVG files. `jev-tray` is its own `dist` app, so the `jev` archive does not change.

**Tech Stack:** Rust 1.99.1 (MSRV 1.85), `tray-icon` 0.24.2, `tao` 0.37.1, `muda` 0.19.3, `png` 0.18.1, `windows-sys` 0.59, `libc` 0.2, `resvg` 0.48.1 (a developer tool, not a dependency), `dist` 0.33.1, `cargo-deny` 0.20.2.

**Spec:** `docs/superpowers/specs/2026-11-04-jev-tray-design.md`. It adds to `docs/superpowers/specs/2026-11-02-jev-daemon-design.md`. **Plans 1 to 6** and the jev-tray spec sit on the branch you start from.

---

## Before you start

- Work in the worktree that the executing skill makes. Run every command from its root, in Git Bash on Windows.
- Stage explicit paths. Never run `git add -A`.
- Commit as `mattbran87@users.noreply.github.com`: `git config user.email` must print that address. If it does not, stop and report it.
- End every commit message with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`, as a separate `-m` argument. Each task gives the exact `git commit` command.
- Run each gate on its own, never in a pipe that hides its exit status. Run no two cargo commands at the same time.
- Write every file with LF line endings. On Windows, Python's text mode writes CRLF; use `newline='\n'` or binary mode. Check with `git ls-files --eol <file>` after `git add`: the index column must say `i/lf`.
- Each code block below is the exact file content. Copy it byte for byte. Do not retype it.
- Each patch below applies with `git apply <file>`. Save the patch text to a file in `<scratchpad>` first, byte for byte, with LF line endings. `<scratchpad>` is any temporary folder outside the repo. If `git apply` refuses a patch, stop and report it: do not edit by hand.
- This machine has Rust 1.99.1 as the default toolchain, the `1.85` toolchain, `dist` 0.33.1, and `cargo-deny` 0.20.2. If one is missing, stop and report it. Task 2 installs `resvg` 0.48.1; install nothing else.
- To count tests, save the output of `cargo test` to a file, and then add up the `test result` lines:

```bash
cargo test --workspace --locked > <scratchpad>/test.log 2>&1
echo "exit $?"
grep "^test result" <scratchpad>/test.log | awk '{p += $4; i += $8} END {print p " passed, " i " ignored"}'
```

- No test sends a request to `api.typesafe.ai`. No test touches the real `Run` key, the real `~/Library/LaunchAgents`, or the real `~/.config/autostart`.

## What the runs showed

These facts come from runs of this plan's code on 2026-11-04.

| Fact | What it changes |
| --- | --- |
| On Windows, `cargo test --workspace --locked` passes 391, 394, 399, 407, 412, 412, and 412 tests after Tasks 1 to 7, and ignores 1 test each time. | Each task's gate step gives its total. |
| In WSL Ubuntu 24.04 with Rust 1.99.1 and `CARGO_BUILD_JOBS=2`, `cargo test --workspace --locked` at the end of Task 7 exited 0 with 414 passed, 0 failed, and 1 ignored. That is 2 more than on Windows, from the unix-only tests. No `jevd` was left running. | The Linux test suite passes. The tray UI on Linux stays a check by hand. |
| `tray-icon` 0.24 shows no tooltip on Linux. | On Linux, only the icon shows the state. Task 7 records this in the jev-tray spec and the README. |
| With the new crates, cargo picks `proc-macro-crate` 2.0.2 for the Linux GTK crates. Its exact pins on `toml_edit` 0.20.2 and `toml_datetime` 0.6.3 move `toml` back from 0.8.23 to 0.8.2, and `winnow` from 0.7.15 to 0.5.40. | Task 5 pins `proc-macro-crate` to 2.0.0, then moves `toml` and `toml_edit` forward again. |
| `tao` 0.37.2 (2026-10-23) and `crc32fast` 1.5.4 (2026-11-01) came out after the cutoff of 2026-10-21. | Task 5 pins `tao` 0.37.1 and `crc32fast` 1.5.3. |
| `cargo deny check` on the new tree finds 2 new items, both on Linux only: `option-ext` (through `dirs` 6) has the `MPL-2.0` license, and `proc-macro-error` (through `glib-macros` 0.18) has the "unmaintained" advisory RUSTSEC-2024-0370. | Task 6 adds `MPL-2.0` to the license list and ignores the advisory, with its reason. |
| `sh crates/jev-tray/icons/render.sh` with `resvg` 0.48.1 gives the 20 committed PNGs byte for byte. | Task 2 renders the PNGs and checks each sha256. The plan holds no binary file. |

## Choices this plan makes

- Every crate and tool version is at least two weeks old on 2026-11-04, so the cutoff is 2026-10-21:

| Crate or tool | Version | Published | Why |
| --- | --- | --- | --- |
| `tray-icon` | 0.24.2 | 2026-07-27 | 0.25 and later need Rust 1.90. The MSRV is 1.85. |
| `tao` | 0.37.1 | 2026-09-26 | 0.37.2 came out on 2026-10-23, after the cutoff. |
| `muda` | 0.19.3 | 2026-06-17 | The menu crate that `tray-icon` 0.24.2 uses. |
| `png` | 0.18.1 | 2026-02-14 | Decodes the committed PNGs to RGBA. |
| `resvg` | 0.48.1 | 2026-08-02 | The command line tool that renders the PNGs. The build does not use it. |
| `crc32fast` | 1.5.3 | 2026-10-20 | 1.5.4 came out on 2026-11-01, after the cutoff. |
| `proc-macro-crate` | 2.0.0 | 2023-10-03 | 2.0.2 moves `toml` and `winnow` back (see "What the runs showed"). |

- **The check:** every 5 seconds, `connect_if_running`, then `status`. A check that takes more than 5 seconds counts as off. The check runs on its own thread, so a daemon that never answers cannot hold the event loop.
- **The icon size:** the smallest committed size that covers the bar size times the screen scale. The bar size is 16 points on Windows, 18 on macOS, and 24 on Linux. The committed sizes are 16, 20, 24, 32, and 48 pixels. A larger need gets 48.
- **The Windows theme:** `jev-tray` reads `SystemUsesLightTheme` at each check. A missing value counts as a dark taskbar, so the icon is white.
- **One copy:** on Windows, `jev-tray` opens `jev-tray.lock` with share mode 0, so a second open fails with a sharing violation. On Unix, it takes `flock` with `LOCK_EX | LOCK_NB`. Both lock the file in the runtime folder from `jev_proto::paths`. The lock goes when the process exits.
- **The release:** `[package.metadata.dist.dependencies.apt]` in `crates/jev-tray/Cargo.toml` names the 3 Linux packages, so the `dist` release build installs them. `dist` runs its own `apt-get install`; the metadata holds no `-y` flag.
- **CI:** the Linux `rust` job and the `msrv` job install `libgtk-3-dev`, `libxdo-dev`, and `libayatana-appindicator3-dev` with `apt-get install -y` first.

## Files

```
Cargo.toml                         the workspace member (Task 1)
Cargo.lock                         Tasks 1, 3, and 5
crates/jev-tray/Cargo.toml         Task 1; edits in Tasks 3, 5, and 6
crates/jev-tray/src/lib.rs         Task 1; edits in Tasks 3, 4, and 5
crates/jev-tray/src/state.rs       the state check (Task 1); the tooltip (Task 5)
crates/jev-tray/tests/state.rs     the state rule against a real daemon (Task 1)
crates/jev-tray/icons/             2 SVG files, render.sh, and 20 PNGs (Task 2)
crates/jev-tray/tests/icons.rs     the icon files (Task 2)
crates/jev-tray/src/icon.rs        the icon choice (Task 3); RGBA decoding (Task 5)
crates/jev-tray/src/login.rs       the Start at login entry (Task 4)
crates/jev-tray/src/one_copy.rs    the one-copy lock (Task 5)
crates/jev-tray/src/main.rs        the tray app (Task 5)
.github/workflows/ci.yml, deny.toml    Task 6
README.md, docs/superpowers/specs/2026-11-02-jev-daemon-design.md    Task 7
docs/superpowers/specs/2026-11-04-jev-tray-design.md                 Task 7
```

---

### Task 1: the state check

This task adds the crate, and the state rule of spec section 2: on when `jevd` answers `status` with a success reply, and off for anything else. The tests run a real `jevd` in the test process, from the `jevd` library, which is a dev-dependency only.

**Files:**
- Modify: `Cargo.toml`, `Cargo.lock`
- Create: `crates/jev-tray/Cargo.toml`, `crates/jev-tray/src/lib.rs`, `crates/jev-tray/src/state.rs`, `crates/jev-tray/tests/state.rs`

- [ ] **Step 1: Add the crate to the workspace**

Save this patch as `<scratchpad>/t1-workspace.patch` and run `git apply <scratchpad>/t1-workspace.patch`. It adds the workspace member, and the `jev-tray` entry to the lock file. Every dependency of `jev-tray` is in the lock file already, so the lock file gets no other change.

```diff
diff --git a/Cargo.lock b/Cargo.lock
index 9d24226..9befbb3 100644
--- a/Cargo.lock
+++ b/Cargo.lock
@@ -1159,6 +1159,16 @@ dependencies = [
  "windows-sys 0.59.0",
 ]
 
+[[package]]
+name = "jev-tray"
+version = "0.1.0"
+dependencies = [
+ "jev-proto",
+ "jevd",
+ "tempfile",
+ "tokio",
+]
+
 [[package]]
 name = "jevd"
 version = "0.1.0"
diff --git a/Cargo.toml b/Cargo.toml
index 173eb4a..f023a23 100644
--- a/Cargo.toml
+++ b/Cargo.toml
@@ -1,6 +1,6 @@
 [workspace]
 resolver = "3"
-members = ["crates/jev-proto", "crates/jevd", "crates/jev"]
+members = ["crates/jev-proto", "crates/jevd", "crates/jev", "crates/jev-tray"]
 
 [workspace.package]
 version = "0.1.0"
```

Write `crates/jev-tray/Cargo.toml`:

```toml
[package]
name = "jev-tray"
description = "A tray icon that shows whether jevd runs"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true
repository.workspace = true

[dependencies]
jev-proto = { version = "0.1.0", path = "../jev-proto" }

[dev-dependencies]
jevd = { version = "0.1.0", path = "../jevd" }
tempfile = "3"
tokio = { version = "1", features = ["io-util", "macros", "rt-multi-thread", "time"] }
```

Write `crates/jev-tray/src/lib.rs`:

```rust
//! The parts of `jev-tray` that do not need a tray: the state check, the
//! icon choice, and the login entry (the jev-tray spec).

pub mod state;
```

- [ ] **Step 2: Write the failing test**

Write `crates/jev-tray/tests/state.rs`:

```rust
//! The state rule (jev-tray spec section 6) against a daemon in this
//! process, and against fake servers built on jevd's own listener.

use std::sync::Arc;
use std::time::{Duration, Instant};

use jev_proto::{Paths, PROTOCOL_VERSION};
use jev_tray::state::{check, check_within, State, CHECK_LIMIT};
use jevd::daemon::{Config, Daemon};
use jevd::jev_http::JevClient;
use jevd::keystore::MemoryKeyring;
use jevd::listen::bind;
use jevd::peer::AdminCheck;
use jevd::settings::Settings;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

fn paths_in(dir: &tempfile::TempDir) -> Paths {
    Paths::under(dir.path()).unwrap()
}

/// A real daemon in this process. Returns the daemon and its task.
fn start_daemon(paths: &Paths) -> (Arc<Daemon>, tokio::task::JoinHandle<()>) {
    let mut settings = Settings::default();
    settings.add_profile("default").unwrap();
    settings.save(&paths.settings_file()).unwrap();
    let jev = JevClient::with_endpoint(
        "http://127.0.0.1:9/",
        Duration::from_secs(5),
        Duration::from_millis(10),
    )
    .unwrap();
    let daemon = Daemon::new(Config {
        settings_file: paths.settings_file(),
        pairing_file: paths.pairing_file(),
        store: Arc::new(MemoryKeyring::new()),
        jev,
    })
    .unwrap();
    let listener = bind(paths).unwrap();
    let me = std::env::current_exe().unwrap();
    let task = tokio::spawn(jevd::run::run(
        Arc::clone(&daemon),
        listener,
        AdminCheck::new(&me),
    ));
    (daemon, task)
}

/// A fake server: it answers `hello` as jevd does, then answers the next
/// line with `then` (when `Some`), or never (when `None`).
fn fake_server(paths: &Paths, then: Option<&'static str>) -> tokio::task::JoinHandle<()> {
    paths.prepare_runtime_dir().unwrap();
    let mut listener = bind(paths).unwrap();
    tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((stream, _)) = listener.accept().await {
            let (read, mut write) = tokio::io::split(stream);
            held.push(tokio::spawn(async move {
                let mut lines = BufReader::new(read).lines();
                let hello = format!("{{\"ok\":true,\"protocol\":{PROTOCOL_VERSION}}}\n");
                if let Ok(Some(_)) = lines.next_line().await {
                    let _ = write.write_all(hello.as_bytes()).await;
                }
                while let Ok(Some(_)) = lines.next_line().await {
                    if let Some(reply) = then {
                        let _ = write.write_all(format!("{reply}\n").as_bytes()).await;
                    }
                }
            }));
        }
    })
}

async fn check_off_thread(paths: &Paths) -> State {
    let paths = paths.clone();
    tokio::task::spawn_blocking(move || check(&paths))
        .await
        .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_running_daemon_is_on() {
    let dir = tempfile::tempdir().unwrap();
    let paths = paths_in(&dir);
    let (_daemon, task) = start_daemon(&paths);
    let state = check_off_thread(&paths).await;
    task.abort();
    assert_eq!(state, State::On);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_stopped_daemon_is_off() {
    let dir = tempfile::tempdir().unwrap();
    let paths = paths_in(&dir);
    let (daemon, task) = start_daemon(&paths);
    assert_eq!(check_off_thread(&paths).await, State::On);
    daemon.request_stop();
    tokio::time::timeout(Duration::from_secs(10), task)
        .await
        .expect("the daemon stops")
        .unwrap();
    assert_eq!(check_off_thread(&paths).await, State::Off);
}

#[test]
fn no_channel_is_off() {
    let dir = tempfile::tempdir().unwrap();
    let paths = paths_in(&dir);
    assert_eq!(check(&paths), State::Off);
    paths.prepare_runtime_dir().unwrap();
    assert_eq!(check(&paths), State::Off);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_error_reply_is_off() {
    let dir = tempfile::tempdir().unwrap();
    let paths = paths_in(&dir);
    let server = fake_server(
        &paths,
        Some(r#"{"ok":false,"error":{"code":"internal","message":"no"}}"#),
    );
    let state = check_off_thread(&paths).await;
    server.abort();
    assert_eq!(state, State::Off);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_reply_that_never_comes_is_off_after_the_limit() {
    let dir = tempfile::tempdir().unwrap();
    let paths = paths_in(&dir);
    let server = fake_server(&paths, None);
    let started = Instant::now();
    let state = check_off_thread(&paths).await;
    let took = started.elapsed();
    server.abort();
    assert_eq!(state, State::Off);
    assert!(took >= CHECK_LIMIT - Duration::from_millis(100), "{took:?}");
    assert!(took < CHECK_LIMIT + Duration::from_secs(1), "{took:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_shorter_limit_holds_too() {
    let dir = tempfile::tempdir().unwrap();
    let paths = paths_in(&dir);
    let server = fake_server(&paths, None);
    let check_paths = paths.clone();
    let started = Instant::now();
    let state =
        tokio::task::spawn_blocking(move || check_within(&check_paths, Duration::from_millis(300)))
            .await
            .unwrap();
    let took = started.elapsed();
    server.abort();
    assert_eq!(state, State::Off);
    assert!(took < Duration::from_secs(2), "{took:?}");
}
```

- [ ] **Step 3: Run the test and see it fail**

Run: `cargo test -p jev-tray --locked --test state`
Expected: FAIL to compile, with `` error[E0583]: file not found for module `state` `` and `` could not compile `jev-tray` (lib) due to 1 previous error ``.

- [ ] **Step 4: Write the state check**

Write `crates/jev-tray/src/state.rs`:

```rust
//! One check of `jevd` (jev-tray spec section 2): connect if a daemon runs,
//! send `status`, and close the connection.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use jev_proto::client::Connection;
use jev_proto::{Paths, Request};

/// The time between two checks, and the longest time one check may take.
pub const CHECK_LIMIT: Duration = Duration::from_secs(5);

/// What the icon shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// `jevd` answered `status` with a success reply.
    On,
    /// Anything else: no channel, no reply in time, or an error reply.
    Off,
}

/// One check, with the 5 s limit.
pub fn check(paths: &Paths) -> State {
    check_within(paths, CHECK_LIMIT)
}

/// One check that counts as off when it takes longer than `limit`.
///
/// The check runs on its own thread, so a daemon that never answers cannot
/// hold the caller past `limit`. That thread ends soon after: its `status`
/// call waits only for the time that is left, and its connection closes when
/// it ends.
pub fn check_within(paths: &Paths, limit: Duration) -> State {
    let deadline = Instant::now() + limit;
    let paths = paths.clone();
    let (done, result) = mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("jev-tray-check".into())
        .spawn(move || {
            let _ = done.send(check_now(&paths, deadline));
        });
    if spawned.is_err() {
        return State::Off;
    }
    result
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .unwrap_or(State::Off)
}

fn check_now(paths: &Paths, deadline: Instant) -> State {
    let mut connection = match Connection::connect_if_running(paths) {
        Ok(Some(connection)) => connection,
        Ok(None) | Err(_) => return State::Off,
    };
    let left = deadline.saturating_duration_since(Instant::now());
    if left.is_zero() {
        return State::Off;
    }
    connection.set_timeout(left);
    match connection.call(&Request::Status) {
        Ok(reply) if reply.ok => State::On,
        _ => State::Off,
    }
}
```

- [ ] **Step 5: Run the test and see it pass**

Run: `cargo test -p jev-tray --locked --test state`
Expected: `test result: ok. 6 passed; 0 failed`. It takes about 5 seconds, because `a_reply_that_never_comes_is_off_after_the_limit` waits for the full limit.

Run: `git diff --stat Cargo.lock`
Expected: `1 file changed, 10 insertions(+)`.

- [ ] **Step 6: Mutation self-checks**

Make each change alone. Copy `crates/jev-tray/src/state.rs` to `<scratchpad>` first, and copy it back after the run. Run `cargo test -p jev-tray --locked --test state` each time.

| Change in `check_now` | The test that must fail |
| --- | --- |
| `Ok(reply) if reply.ok => State::On,` becomes `Ok(_) => State::On,` (an error reply counts as on) | `an_error_reply_is_off` |
| `Ok(None) \| Err(_) => return State::Off,` becomes `Ok(None) \| Err(_) => return State::On,` (no channel counts as on) | `no_channel_is_off` and `a_stopped_daemon_is_off` |

After you copy the file back, run the test again. Expected: 6 passed.

- [ ] **Step 7: Run the gates**

Run each command alone, and read its exit code:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo +1.85 check --workspace --locked
```

Expected: each exits 0. On Windows, `cargo test` passes 391 tests in all and ignores 1 (count them as "Before you start" shows). Check that no `jevd` still runs: `tasklist | findstr jevd` prints nothing.

- [ ] **Step 8: Commit**

```bash
git add Cargo.toml Cargo.lock crates/jev-tray/Cargo.toml crates/jev-tray/src/lib.rs crates/jev-tray/src/state.rs crates/jev-tray/tests/state.rs
git commit -m "jev-tray: the state check" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: the icon files

This task adds the 2 SVG sources of spec section 3, the script that renders them, and the 20 PNGs. The plan holds no PNG: you render them, and then check each sha256 against the committed file.

**Files:**
- Create: `crates/jev-tray/icons/on.svg`, `crates/jev-tray/icons/off.svg`, `crates/jev-tray/icons/render.sh`, 20 PNGs `crates/jev-tray/icons/{on,off}-{black,white}-{16,20,24,32,48}.png`, `crates/jev-tray/tests/icons.rs`

- [ ] **Step 1: Write the failing test**

Write `crates/jev-tray/tests/icons.rs`:

```rust
//! The committed icon files (jev-tray spec section 3): every PNG that the
//! name scheme asks for exists, and each has the size its name gives.

use std::path::PathBuf;

const STATES: [&str; 2] = ["on", "off"];
const COLORS: [&str; 2] = ["black", "white"];
const SIZES: [u32; 5] = [16, 20, 24, 32, 48];
const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];

fn icons() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("icons")
}

/// The width and height from the IHDR chunk, which a PNG must put first.
fn size_of_png(bytes: &[u8]) -> (u32, u32) {
    assert!(bytes.len() >= 24, "too short for a PNG");
    assert_eq!(bytes[..8], SIGNATURE, "not a PNG");
    assert_eq!(&bytes[12..16], b"IHDR", "the first chunk is not IHDR");
    let width = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
    let height = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
    (width, height)
}

#[test]
fn every_png_has_the_size_its_name_gives() {
    for state in STATES {
        for color in COLORS {
            for size in SIZES {
                let name = format!("{state}-{color}-{size}.png");
                let bytes = std::fs::read(icons().join(&name))
                    .unwrap_or_else(|error| panic!("{name}: {error}"));
                assert_eq!(size_of_png(&bytes), (size, size), "{name}");
            }
        }
    }
}

#[test]
fn no_other_png_is_in_the_folder() {
    let mut found = 0;
    for entry in std::fs::read_dir(icons()).unwrap() {
        let name = entry.unwrap().file_name().into_string().unwrap();
        let Some(stem) = name.strip_suffix(".png") else {
            continue;
        };
        let parts: Vec<&str> = stem.split('-').collect();
        assert_eq!(parts.len(), 3, "{name}");
        assert!(STATES.contains(&parts[0]), "{name}");
        assert!(COLORS.contains(&parts[1]), "{name}");
        assert!(SIZES.contains(&parts[2].parse().unwrap()), "{name}");
        found += 1;
    }
    assert_eq!(found, STATES.len() * COLORS.len() * SIZES.len());
}

#[test]
fn both_svgs_use_the_16_grid_and_one_color() {
    for state in STATES {
        let text = std::fs::read_to_string(icons().join(format!("{state}.svg"))).unwrap();
        assert!(text.contains(r#"viewBox="0 0 16 16""#), "{state}.svg");
        assert!(text.contains("currentColor"), "{state}.svg");
        assert!(!text.contains('#'), "{state}.svg has a fixed color");
    }
}
```

- [ ] **Step 2: Run the test and see it fail**

Run: `cargo test -p jev-tray --locked --test icons`
Expected: `test result: FAILED. 0 passed; 3 failed`. Each test panics, because `crates/jev-tray/icons/` does not exist yet.

- [ ] **Step 3: Write the SVG files and the render script**

Write `crates/jev-tray/icons/on.svg`:

```svg
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" width="16" height="16">
  <rect x="1.75" y="1.75" width="12.5" height="12.5" rx="2" fill="none" stroke="currentColor" stroke-width="1.5"/>
  <path fill="currentColor" d="M10.6 3.8h1.8v1.8h-1.8z M10.6 6.6h1.8v4.7a2.1 2.1 0 0 1-2.1 2.1H8.6v-1.7h1.6a.4.4 0 0 0 .4-.4z"/>
  <path d="M3.8 8.4l1.6 1.7 3-3.6" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/>
</svg>
```

Write `crates/jev-tray/icons/off.svg`:

```svg
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" width="16" height="16">
  <rect x="1.75" y="1.75" width="12.5" height="12.5" rx="2" fill="none" stroke="currentColor" stroke-width="1.5"/>
  <path fill="currentColor" d="M10.6 3.8h1.8v1.8h-1.8z M10.6 6.6h1.8v4.7a2.1 2.1 0 0 1-2.1 2.1H8.6v-1.7h1.6a.4.4 0 0 0 .4-.4z"/>
  <path d="M4.2 6.6l3.2 3.2 M7.4 6.6l-3.2 3.2" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
</svg>
```

Write `crates/jev-tray/icons/render.sh`:

```sh
#!/bin/sh
# Renders the tray PNGs from on.svg and off.svg. Run it after a change to an
# SVG, then commit the PNGs. The build does not run it. It needs the resvg
# command line tool:
#   cargo install resvg --version 0.48.1 --locked
# Pass the tool's path as the first argument when it is not on PATH.
set -eu
resvg=${1:-resvg}
cd "$(dirname "$0")"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
for state in on off; do
  for color in black white; do
    case $color in
      black) hex='#000000' ;;
      white) hex='#ffffff' ;;
    esac
    sed "s/currentColor/$hex/g" "$state.svg" > "$tmp/$state-$color.svg"
    for size in 16 20 24 32 48; do
      "$resvg" -w "$size" -h "$size" "$tmp/$state-$color.svg" "$state-$color-$size.png"
    done
  done
done
```

- [ ] **Step 4: Render the PNGs**

Install the renderer, then run the script:

```bash
cargo install resvg --version 0.48.1 --locked
sh crates/jev-tray/icons/render.sh
```

Expected: both exit 0, and `ls crates/jev-tray/icons/*.png | wc -l` prints `20`.

If `resvg` is not on `PATH`, pass its path: `sh crates/jev-tray/icons/render.sh ~/.cargo/bin/resvg`.

- [ ] **Step 5: Check each PNG against the committed file**

Save these lines as `<scratchpad>/icons.sha256`, with LF line endings:

```
eca8bf8ee488e823bbb602984234736a02bc52f91fefe5a56c73db53c718943f  off-black-16.png
b7917b719bdfa1a46c9df89ca66e11dd9f507a5a56f9fd146ced3c660d2b34fa  off-black-20.png
36a57614816c065801e951e12b8c9eb5b995b715f67ba5bec6d2fc92de7419fb  off-black-24.png
ccc58081177c5c2c39dd1d0af305146c3a2e73071101c48928872acc699b3b44  off-black-32.png
b6659645f217380800843d5227485d1be41a544fe0637dd3f3a70291b6441adf  off-black-48.png
16b10c9c0ffa4e3ef5f605494fb7fdac4546994fffcec19eb30972c1b3b91414  off-white-16.png
00bd590c4c81724f09b1a7709b9ae58a9c9783cb072d7f61be943de78f5a94f0  off-white-20.png
73b68b48c04540e64b77c6963a23d4723923ce8a497bd11beee150340ee40eee  off-white-24.png
aef6eb343cafa60fd4bcf2c2b915a5465059408cc2be5b2228219f89b3942f10  off-white-32.png
7c567e9ef66d7fca7d2573dd705bc31fc85396c329246b34df7b51c32455f76e  off-white-48.png
aa6119bb4eae161becd28552cf7f09fd4403ed36baa0692e2c6486c55bad92e9  on-black-16.png
535aba00d7181676fe24dad31eaa03af33d4dae64ababce3eec19bbcad98501d  on-black-20.png
e7096b5d9485021049d512b8940415de87402a159e16a91d00fcd9a0ca792028  on-black-24.png
dda568dc26d77a7d2525fff681d84809ba27b5ab629f7e263b1c559241637bae  on-black-32.png
c03efb53203e44fafa17b203c799b9e758f792a443713791b2634d2f4ed3c372  on-black-48.png
2dfb6fac4161c2bb624f6b9c8c1048072bb79cf1c63954b46fb904068ed8370f  on-white-16.png
8ccd98953f0c8180ac2228d350bd344c8c311ab5312869b5b365d88e38634c53  on-white-20.png
b9b4c5f111bbd511012d31029537488cce93a68a6a96a52fcd1d2c88a5af69af  on-white-24.png
6c1a5658d86e2ced53586977e960efa2f458f367ef137111e8ba894b080e457f  on-white-32.png
98d99b3daa53689045c658157fc5a0309d4195e69ba40a3400650c094d5d7b7f  on-white-48.png
```

Run: `(cd crates/jev-tray/icons && sha256sum -c <scratchpad>/icons.sha256)`, with the full path of the file.
Expected: 20 lines that end in `OK`, and exit 0. If one hash differs, stop and report `resvg --version` and the names that differ. Do not commit a PNG with another hash.

- [ ] **Step 6: Run the test and see it pass**

Run: `cargo test -p jev-tray --locked --test icons`
Expected: `test result: ok. 3 passed; 0 failed`.

- [ ] **Step 7: Mutation self-checks**

Make each change alone, run `cargo test -p jev-tray --locked --test icons`, and then undo it:

| Change | The test that must fail |
| --- | --- |
| Copy `on-black-16.png` to `on-black-64.png` | `no_other_png_is_in_the_folder` |
| Move `off-white-48.png` to `<scratchpad>` | `every_png_has_the_size_its_name_gives` and `no_other_png_is_in_the_folder` |
| In `on.svg`, change `stroke="currentColor"` on the `rect` to `stroke="#000"` | `both_svgs_use_the_16_grid_and_one_color` |

Undo each change, and run Step 5 and Step 6 again. Expected: 20 `OK` lines, and 3 passed.

- [ ] **Step 8: Run the gates**

Run each command alone, and read its exit code:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo +1.85 check --workspace --locked
```

Expected: each exits 0. On Windows, `cargo test` passes 394 tests in all and ignores 1 (count them as "Before you start" shows). Check that no `jevd` still runs: `tasklist | findstr jevd` prints nothing.

- [ ] **Step 9: Commit**

`render.sh` is executable in the repo. On Windows, `git add` does not see the mode, so the command sets it.

```bash
git add crates/jev-tray/icons/on.svg crates/jev-tray/icons/off.svg crates/jev-tray/tests/icons.rs crates/jev-tray/icons/*.png
git add --chmod=+x crates/jev-tray/icons/render.sh
git commit -m "jev-tray: the icon files" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

Run: `git ls-files -s crates/jev-tray/icons/render.sh`
Expected: the line starts with `100755`.

---

### Task 3: the icon choice for the bar

This task picks the icon file (spec section 3). The state gives the mark, the system gives the color, and the screen scale gives the size. The PNGs go into the binary with `include_bytes!`. On Windows, `icon.rs` reads `SystemUsesLightTheme` from the registry, so the crate gets `windows-sys`.

**Files:**
- Modify: `crates/jev-tray/Cargo.toml`, `Cargo.lock`, `crates/jev-tray/src/lib.rs`
- Create: `crates/jev-tray/src/icon.rs`

- [ ] **Step 1: Add `windows-sys` and the module**

Save this patch as `<scratchpad>/t3-icon.patch` and run `git apply <scratchpad>/t3-icon.patch`. `windows-sys` 0.59 is in the lock file already, so the lock file gets 1 line.

```diff
diff --git a/Cargo.lock b/Cargo.lock
index 9befbb3..516f694 100644
--- a/Cargo.lock
+++ b/Cargo.lock
@@ -1167,6 +1167,7 @@ dependencies = [
  "jevd",
  "tempfile",
  "tokio",
+ "windows-sys 0.59.0",
 ]
 
 [[package]]
diff --git a/crates/jev-tray/Cargo.toml b/crates/jev-tray/Cargo.toml
index a21d372..df1b77a 100644
--- a/crates/jev-tray/Cargo.toml
+++ b/crates/jev-tray/Cargo.toml
@@ -10,6 +10,9 @@ repository.workspace = true
 [dependencies]
 jev-proto = { version = "0.1.0", path = "../jev-proto" }
 
+[target.'cfg(windows)'.dependencies]
+windows-sys = { version = "0.59", features = ["Win32_Foundation", "Win32_System_Registry"] }
+
 [dev-dependencies]
 jevd = { version = "0.1.0", path = "../jevd" }
 tempfile = "3"
diff --git a/crates/jev-tray/src/lib.rs b/crates/jev-tray/src/lib.rs
index 359089c..cde13a6 100644
--- a/crates/jev-tray/src/lib.rs
+++ b/crates/jev-tray/src/lib.rs
@@ -1,4 +1,5 @@
 //! The parts of `jev-tray` that do not need a tray: the state check, the
 //! icon choice, and the login entry (the jev-tray spec).
 
+pub mod icon;
 pub mod state;
```

- [ ] **Step 2: Write the failing tests**

Write `crates/jev-tray/src/icon.rs` with the tests only. This text is the end of the full file in Step 4:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_light_taskbar_gets_black_and_a_dark_one_white() {
        assert_eq!(shade_for_windows(Some(1)), Shade::Black);
        assert_eq!(shade_for_windows(Some(0)), Shade::White);
    }

    #[test]
    fn a_missing_theme_value_counts_as_dark() {
        assert_eq!(shade_for_windows(None), Shade::White);
    }

    #[test]
    fn macos_and_linux_use_a_fixed_shade() {
        #[cfg(target_os = "macos")]
        assert_eq!(shade_now(), Shade::Black);
        #[cfg(not(any(target_os = "macos", windows)))]
        assert_eq!(shade_now(), Shade::White);
        assert_eq!(TEMPLATE, cfg!(target_os = "macos"));
    }

    #[test]
    fn the_size_covers_the_scaled_points() {
        assert_eq!(size_for(16, 1.0), 16);
        assert_eq!(size_for(16, 1.25), 20);
        assert_eq!(size_for(16, 1.5), 24);
        assert_eq!(size_for(16, 2.0), 32);
        assert_eq!(size_for(16, 2.5), 48);
        assert_eq!(size_for(16, 4.0), 48);
        assert_eq!(size_for(18, 2.0), 48);
        assert_eq!(size_for(24, 1.0), 24);
    }

    #[test]
    fn each_choice_gives_its_own_file() {
        let mut seen = std::collections::HashSet::new();
        for state in [State::On, State::Off] {
            for shade in [Shade::Black, Shade::White] {
                for size in SIZES {
                    let bytes = png(state, shade, size);
                    assert_eq!(&bytes[1..4], b"PNG");
                    assert_eq!(u32::from_be_bytes(bytes[16..20].try_into().unwrap()), size);
                    assert!(seen.insert(bytes.as_ptr()));
                }
            }
        }
    }
}
```

- [ ] **Step 3: Run the tests and see them fail**

Run: `cargo test -p jev-tray --locked --lib`
Expected: FAIL to compile, with `` could not compile `jev-tray` (lib test) due to 21 previous errors ``. The errors include `` cannot find function `shade_for_windows` in this scope ``, `` cannot find function `size_for` in this scope ``, `` cannot find function `png` in this scope ``, and `` cannot find type `Shade` in this scope ``.

- [ ] **Step 4: Write the icon choice**

Replace `crates/jev-tray/src/icon.rs` with:

```rust
//! Which icon file to show (jev-tray spec section 3): the state gives the
//! mark, the system gives the color, and the screen scale gives the size.

use crate::state::State;

/// The color of the icon's lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shade {
    Black,
    White,
}

/// The sizes in `icons/`, in pixels, smallest first.
pub const SIZES: [u32; 5] = [16, 20, 24, 32, 48];

/// True where the bar recolors the icon itself: on macOS the black icon is
/// a template image.
pub const TEMPLATE: bool = cfg!(target_os = "macos");

/// The icon size in points on this system's bar.
pub const POINTS: u32 = if cfg!(target_os = "macos") {
    18
} else if cfg!(windows) {
    16
} else {
    24
};

/// The shade for a Windows taskbar, from the value `SystemUsesLightTheme`.
/// A light taskbar (1) gets black; a dark one (0) gets white. With no value,
/// the taskbar is dark, as on Windows 10 by default.
pub fn shade_for_windows(system_uses_light_theme: Option<u32>) -> Shade {
    match system_uses_light_theme {
        Some(0) | None => Shade::White,
        Some(_) => Shade::Black,
    }
}

/// The shade for this system's bar now: black on macOS, where it is a
/// template image.
#[cfg(target_os = "macos")]
pub fn shade_now() -> Shade {
    Shade::Black
}

/// The shade for this system's bar now. It reads the registry each time,
/// so a theme change shows at the next check.
#[cfg(windows)]
pub fn shade_now() -> Shade {
    shade_for_windows(windows_theme::system_uses_light_theme())
}

/// The shade for this system's bar now: white, because Linux panels do not
/// report their theme reliably, and most panels are dark.
#[cfg(not(any(target_os = "macos", windows)))]
pub fn shade_now() -> Shade {
    Shade::White
}

/// The smallest committed size that covers `points` at `scale`, or the
/// largest one.
pub fn size_for(points: u32, scale: f64) -> u32 {
    let wanted = f64::from(points) * scale;
    SIZES
        .into_iter()
        .find(|&size| f64::from(size) >= wanted - 0.01)
        .unwrap_or(SIZES[SIZES.len() - 1])
}

/// The files for one state and shade, in the order of `SIZES`.
macro_rules! files {
    ($name:literal) => {
        [
            include_bytes!(concat!("../icons/", $name, "-16.png")) as &[u8],
            include_bytes!(concat!("../icons/", $name, "-20.png")),
            include_bytes!(concat!("../icons/", $name, "-24.png")),
            include_bytes!(concat!("../icons/", $name, "-32.png")),
            include_bytes!(concat!("../icons/", $name, "-48.png")),
        ]
    };
}

const ON_BLACK: [&[u8]; 5] = files!("on-black");
const ON_WHITE: [&[u8]; 5] = files!("on-white");
const OFF_BLACK: [&[u8]; 5] = files!("off-black");
const OFF_WHITE: [&[u8]; 5] = files!("off-white");

/// The PNG bytes of one committed icon. A size not in `SIZES` gives the
/// next larger file, or the largest one.
pub fn png(state: State, shade: Shade, size: u32) -> &'static [u8] {
    let files = match (state, shade) {
        (State::On, Shade::Black) => ON_BLACK,
        (State::On, Shade::White) => ON_WHITE,
        (State::Off, Shade::Black) => OFF_BLACK,
        (State::Off, Shade::White) => OFF_WHITE,
    };
    let size = size_for(size, 1.0);
    let index = SIZES.iter().position(|&each| each == size).unwrap_or(0);
    files[index]
}

/// Reads the taskbar theme. Not unit-tested: it reads the real registry.
#[cfg(windows)]
mod windows_theme {
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};

    const KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";
    const VALUE: &str = "SystemUsesLightTheme";

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(Some(0)).collect()
    }

    /// The DWORD `SystemUsesLightTheme`, or `None` when it cannot be read.
    pub fn system_uses_light_theme() -> Option<u32> {
        let (key, value) = (wide(KEY), wide(VALUE));
        let mut data: u32 = 0;
        let mut size = std::mem::size_of::<u32>() as u32;
        // SAFETY: both names end in a zero, and `data` and `size` are valid
        // for writes of the sizes given.
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                value.as_ptr(),
                RRF_RT_REG_DWORD,
                std::ptr::null_mut(),
                (&mut data as *mut u32).cast(),
                &mut size,
            )
        };
        (status == ERROR_SUCCESS).then_some(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_light_taskbar_gets_black_and_a_dark_one_white() {
        assert_eq!(shade_for_windows(Some(1)), Shade::Black);
        assert_eq!(shade_for_windows(Some(0)), Shade::White);
    }

    #[test]
    fn a_missing_theme_value_counts_as_dark() {
        assert_eq!(shade_for_windows(None), Shade::White);
    }

    #[test]
    fn macos_and_linux_use_a_fixed_shade() {
        #[cfg(target_os = "macos")]
        assert_eq!(shade_now(), Shade::Black);
        #[cfg(not(any(target_os = "macos", windows)))]
        assert_eq!(shade_now(), Shade::White);
        assert_eq!(TEMPLATE, cfg!(target_os = "macos"));
    }

    #[test]
    fn the_size_covers_the_scaled_points() {
        assert_eq!(size_for(16, 1.0), 16);
        assert_eq!(size_for(16, 1.25), 20);
        assert_eq!(size_for(16, 1.5), 24);
        assert_eq!(size_for(16, 2.0), 32);
        assert_eq!(size_for(16, 2.5), 48);
        assert_eq!(size_for(16, 4.0), 48);
        assert_eq!(size_for(18, 2.0), 48);
        assert_eq!(size_for(24, 1.0), 24);
    }

    #[test]
    fn each_choice_gives_its_own_file() {
        let mut seen = std::collections::HashSet::new();
        for state in [State::On, State::Off] {
            for shade in [Shade::Black, Shade::White] {
                for size in SIZES {
                    let bytes = png(state, shade, size);
                    assert_eq!(&bytes[1..4], b"PNG");
                    assert_eq!(u32::from_be_bytes(bytes[16..20].try_into().unwrap()), size);
                    assert!(seen.insert(bytes.as_ptr()));
                }
            }
        }
    }
}
```

- [ ] **Step 5: Run the tests and see them pass**

Run: `cargo test -p jev-tray --locked --lib`
Expected: `test result: ok. 5 passed; 0 failed`.

Run: `git diff --stat Cargo.lock`
Expected: `1 file changed, 1 insertion(+)`.

- [ ] **Step 6: Mutation self-checks**

Make each change alone in `crates/jev-tray/src/icon.rs`. Copy the file to `<scratchpad>` first, and copy it back after the run. Run `cargo test -p jev-tray --locked --lib` each time.

| Change | The test that must fail |
| --- | --- |
| In `shade_for_windows`, `Some(0) \| None => Shade::White,` becomes `Some(0) => Shade::White,` and `Some(_) => Shade::Black,` becomes `_ => Shade::Black,` (a missing value counts as light) | `a_missing_theme_value_counts_as_dark` |
| In `size_for`, `>= wanted - 0.01` becomes `> wanted` | `the_size_covers_the_scaled_points` and `each_choice_gives_its_own_file` |

After you copy the file back, run the tests again. Expected: 5 passed.

- [ ] **Step 7: Run the gates**

Run each command alone, and read its exit code:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo +1.85 check --workspace --locked
```

Expected: each exits 0. On Windows, `cargo test` passes 399 tests in all and ignores 1 (count them as "Before you start" shows). Check that no `jevd` still runs: `tasklist | findstr jevd` prints nothing.

- [ ] **Step 8: Commit**

```bash
git add crates/jev-tray/Cargo.toml Cargo.lock crates/jev-tray/src/lib.rs crates/jev-tray/src/icon.rs
git commit -m "jev-tray: the icon choice for the bar" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: start at login

This task adds the **Start at login** entry of spec section 4. It is a registry value on Windows, a launch agent plist on macOS, and an autostart desktop file on Linux. Each place is injectable. The tests use a temporary home and the test key `HKCU\Software\jev-tray-test`, never the real places.

**Files:**
- Modify: `crates/jev-tray/src/lib.rs`
- Create: `crates/jev-tray/src/login.rs`

- [ ] **Step 1: Add the module**

Save this patch as `<scratchpad>/t4-lib.patch` and run `git apply <scratchpad>/t4-lib.patch`:

```diff
diff --git a/crates/jev-tray/src/lib.rs b/crates/jev-tray/src/lib.rs
index cde13a6..fdbc28e 100644
--- a/crates/jev-tray/src/lib.rs
+++ b/crates/jev-tray/src/lib.rs
@@ -2,4 +2,5 @@
 //! icon choice, and the login entry (the jev-tray spec).
 
 pub mod icon;
+pub mod login;
 pub mod state;
```

- [ ] **Step 2: Write the failing tests**

Write `crates/jev-tray/src/login.rs` with the tests only. This text is the end of the full file in Step 4:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn exe() -> PathBuf {
        if cfg!(windows) {
            PathBuf::from(r"C:\Program Files\jev\jev-tray.exe")
        } else {
            PathBuf::from("/opt/my tools/jev-tray")
        }
    }

    #[test]
    fn the_run_value_is_the_quoted_full_path() {
        assert_eq!(
            run_value(Path::new(r"C:\Program Files\jev\jev-tray.exe")),
            r#""C:\Program Files\jev\jev-tray.exe""#
        );
    }

    #[test]
    fn the_plist_runs_the_path_at_load() {
        let text = plist_text(Path::new("/Applications/jev & co/jev-tray"));
        assert!(text.contains("<string>com.github.mattbran87.jev-tray</string>"));
        assert!(text.contains("<string>/Applications/jev &amp; co/jev-tray</string>"));
        assert!(text.contains("<key>RunAtLoad</key>\n\t<true/>"));
    }

    #[test]
    fn the_desktop_file_execs_the_quoted_path() {
        let text = desktop_text(Path::new("/opt/my tools/jev-tray"));
        assert!(text.starts_with("[Desktop Entry]\n"));
        assert!(text.contains("\nType=Application\n"));
        assert!(text.contains("\nExec=\"/opt/my tools/jev-tray\"\n"));
    }

    #[test]
    fn exec_quoting_escapes_the_reserved_characters() {
        assert_eq!(exec_quote("/a/b"), r#""/a/b""#);
        assert_eq!(exec_quote("/a/$x"), r#""/a/\\$x""#);
        assert_eq!(exec_quote("/a/100%"), r#""/a/100%%""#);
        assert_eq!(exec_quote(r"/a\b"), r#""/a\\\\b""#);
    }

    /// Unix only: `/x/conf` is not an absolute path on Windows.
    #[cfg(unix)]
    #[test]
    fn the_autostart_folder_follows_xdg() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |name: &str| {
                pairs
                    .iter()
                    .find(|(key, _)| *key == name)
                    .map(|(_, value)| OsString::from(value))
            }
        };
        assert_eq!(
            autostart_dir(env(&[("XDG_CONFIG_HOME", "/x/conf"), ("HOME", "/home/u")])),
            Some(PathBuf::from("/x/conf").join("autostart"))
        );
        assert_eq!(
            autostart_dir(env(&[("HOME", "/home/u")])),
            Some(PathBuf::from("/home/u").join(".config").join("autostart"))
        );
        assert_eq!(
            autostart_dir(env(&[("XDG_CONFIG_HOME", "conf"), ("HOME", "/home/u")])),
            Some(PathBuf::from("/home/u").join(".config").join("autostart"))
        );
        assert_eq!(autostart_dir(env(&[])), None);
    }

    /// The file entries are plain files, so this runs on every system.
    #[test]
    fn a_file_entry_is_written_read_and_deleted() {
        let home = tempfile::tempdir().unwrap();
        let entries = [
            (
                Login::launch_agent(home.path()),
                plist_text(&exe()),
                home.path()
                    .join("Library/LaunchAgents/com.github.mattbran87.jev-tray.plist"),
            ),
            (
                Login::Autostart(home.path().join(".config/autostart/jev-tray.desktop")),
                desktop_text(&exe()),
                home.path().join(".config/autostart/jev-tray.desktop"),
            ),
        ];
        for (login, text, file) in entries {
            assert!(!login.is_on().unwrap());
            login.delete().unwrap();
            login.write(&exe()).unwrap();
            assert!(login.is_on().unwrap());
            assert_eq!(std::fs::read_to_string(&file).unwrap(), text);
            assert_eq!(login.read().unwrap(), Some(text));
            login.delete().unwrap();
            assert!(!login.is_on().unwrap());
            assert!(!file.exists());
        }
    }

    #[test]
    fn writing_again_replaces_another_path() {
        let home = tempfile::tempdir().unwrap();
        let login = Login::launch_agent(home.path());
        login.write(Path::new("/old/jev-tray")).unwrap();
        assert!(login.is_on().unwrap(), "another path still counts as on");
        login.write(&exe()).unwrap();
        assert_eq!(login.read().unwrap(), Some(plist_text(&exe())));
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    #[test]
    fn this_user_gets_an_autostart_file() {
        match Login::for_this_user() {
            Ok(Login::Autostart(file)) => assert!(file.ends_with("autostart/jev-tray.desktop")),
            other => panic!("{other:?}"),
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn this_user_gets_a_launch_agent() {
        match Login::for_this_user() {
            Ok(Login::LaunchAgent(file)) => {
                assert!(file.ends_with("Library/LaunchAgents/com.github.mattbran87.jev-tray.plist"))
            }
            other => panic!("{other:?}"),
        }
    }

    #[cfg(windows)]
    #[test]
    fn this_user_gets_the_run_value() {
        assert_eq!(
            Login::for_this_user().unwrap(),
            Login::Run {
                key: RUN_KEY.to_string(),
                name: "jev-tray".to_string()
            }
        );
    }

    /// Uses a test key, never the real `Run` key.
    #[cfg(windows)]
    #[test]
    fn a_registry_entry_is_written_read_and_deleted() {
        const TEST_KEY: &str = r"Software\jev-tray-test";
        struct Cleanup;
        impl Drop for Cleanup {
            fn drop(&mut self) {
                registry::delete_tree(TEST_KEY);
            }
        }
        let _cleanup = Cleanup;
        let login = Login::Run {
            key: format!(r"{TEST_KEY}\Run"),
            name: NAME.to_string(),
        };
        assert!(!login.is_on().unwrap());
        login.delete().unwrap();
        login.write(Path::new("/old/jev-tray.exe")).unwrap();
        assert!(login.is_on().unwrap());
        login.write(&exe()).unwrap();
        assert_eq!(
            login.read().unwrap(),
            Some(r#""C:\Program Files\jev\jev-tray.exe""#.to_string())
        );
        login.delete().unwrap();
        assert!(!login.is_on().unwrap());
    }
}
```

- [ ] **Step 3: Run the tests and see them fail**

Run: `cargo test -p jev-tray --locked --lib`
Expected on Windows: FAIL to compile, with `` could not compile `jev-tray` (lib test) due to 27 previous errors ``. The errors include `` cannot find function `run_value` in this scope ``, `` cannot find function `exec_quote` in this scope ``, `` cannot find type `Login` in this scope ``, and `` cannot find module or crate `registry` in this scope ``.

- [ ] **Step 4: Write the login entry**

Replace `crates/jev-tray/src/login.rs` with:

```rust
//! The **Start at login** entry (jev-tray spec section 4): one entry per
//! system that starts `jev-tray` when the user signs in. Every place is
//! injectable, so tests never touch the real one.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

/// The launchd label, and the name of the macOS plist file without `.plist`.
pub const LABEL: &str = "com.github.mattbran87.jev-tray";
/// The registry key that Windows reads at sign-in, under `HKEY_CURRENT_USER`.
pub const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
/// The value name under `RUN_KEY`, and the name of the Linux desktop file
/// without `.desktop`.
pub const NAME: &str = "jev-tray";

/// Where one login entry lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Login {
    /// A macOS launch agent plist file.
    LaunchAgent(PathBuf),
    /// A Linux autostart desktop file.
    Autostart(PathBuf),
    /// A string value under a key of `HKEY_CURRENT_USER`.
    #[cfg(windows)]
    Run { key: String, name: String },
}

impl Login {
    /// The entry for this user on this system.
    #[cfg(windows)]
    pub fn for_this_user() -> io::Result<Login> {
        Ok(Login::Run {
            key: RUN_KEY.to_string(),
            name: NAME.to_string(),
        })
    }

    /// The entry for this user on this system.
    #[cfg(target_os = "macos")]
    pub fn for_this_user() -> io::Result<Login> {
        let home = std::env::var_os("HOME")
            .filter(|home| !home.is_empty())
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME is not set"))?;
        Ok(Login::launch_agent(Path::new(&home)))
    }

    /// The entry for this user on this system.
    #[cfg(not(any(windows, target_os = "macos")))]
    pub fn for_this_user() -> io::Result<Login> {
        let dir = autostart_dir(|name| std::env::var_os(name)).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "neither XDG_CONFIG_HOME nor HOME is set",
            )
        })?;
        Ok(Login::Autostart(dir.join(format!("{NAME}.desktop"))))
    }

    /// The launch agent under the home folder `home`.
    pub fn launch_agent(home: &Path) -> Login {
        Login::LaunchAgent(
            home.join("Library")
                .join("LaunchAgents")
                .join(format!("{LABEL}.plist")),
        )
    }

    /// True when the entry exists, whatever path it names.
    pub fn is_on(&self) -> io::Result<bool> {
        Ok(self.read()?.is_some())
    }

    /// The entry's text (a file's content, or the registry string), or
    /// `None` when there is no entry.
    pub fn read(&self) -> io::Result<Option<String>> {
        match self {
            Login::LaunchAgent(file) | Login::Autostart(file) => {
                match std::fs::read_to_string(file) {
                    Ok(text) => Ok(Some(text)),
                    Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
                    Err(error) => Err(error),
                }
            }
            #[cfg(windows)]
            Login::Run { key, name } => registry::read(key, name),
        }
    }

    /// Writes the entry, so it starts `exe` at sign-in. An entry that is
    /// there already gets the new path.
    pub fn write(&self, exe: &Path) -> io::Result<()> {
        match self {
            Login::LaunchAgent(file) => write_file(file, &plist_text(exe)),
            Login::Autostart(file) => write_file(file, &desktop_text(exe)),
            #[cfg(windows)]
            Login::Run { key, name } => registry::write(key, name, &run_value(exe)),
        }
    }

    /// Deletes the entry. No entry is not an error.
    pub fn delete(&self) -> io::Result<()> {
        match self {
            Login::LaunchAgent(file) | Login::Autostart(file) => match std::fs::remove_file(file) {
                Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
                _ => Ok(()),
            },
            #[cfg(windows)]
            Login::Run { key, name } => registry::delete(key, name),
        }
    }
}

fn write_file(file: &Path, text: &str) -> io::Result<()> {
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(file, text)
}

/// The Linux autostart folder: `$XDG_CONFIG_HOME/autostart`, or
/// `$HOME/.config/autostart`. The XDG rules ignore a relative
/// `XDG_CONFIG_HOME`. `var` stands in for the environment.
pub fn autostart_dir(var: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let set = |name: &str| var(name).filter(|value| !value.is_empty());
    if let Some(config) = set("XDG_CONFIG_HOME").map(PathBuf::from) {
        if config.is_absolute() {
            return Some(config.join("autostart"));
        }
    }
    set("HOME").map(|home| PathBuf::from(home).join(".config").join("autostart"))
}

/// The registry string: the full path of `jev-tray.exe`, quoted.
pub fn run_value(exe: &Path) -> String {
    format!("\"{}\"", exe.display())
}

/// The macOS launch agent that runs `exe` at load.
pub fn plist_text(exe: &Path) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>Label</key>
	<string>{LABEL}</string>
	<key>ProgramArguments</key>
	<array>
		<string>{}</string>
	</array>
	<key>RunAtLoad</key>
	<true/>
</dict>
</plist>
"#,
        xml_escape(&exe.to_string_lossy())
    )
}

/// The Linux autostart desktop file that runs `exe`.
pub fn desktop_text(exe: &Path) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=jev-tray\n\
         Comment=Shows whether jevd runs\n\
         Exec={}\n\
         Terminal=false\n\
         X-GNOME-Autostart-enabled=true\n",
        exec_quote(&exe.to_string_lossy())
    )
}

fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// One `Exec=` argument, by the desktop entry rules: in double quotes, with
/// `"`, `` ` ``, `$`, and `\` escaped by a backslash, `%` doubled, and then
/// every backslash doubled again, because `Exec` is also a string value.
fn exec_quote(arg: &str) -> String {
    let mut quoted = String::from("\"");
    for c in arg.chars() {
        match c {
            '"' | '`' | '$' | '\\' => {
                quoted.push('\\');
                quoted.push(c);
            }
            '%' => quoted.push_str("%%"),
            _ => quoted.push(c),
        }
    }
    quoted.push('"');
    quoted.replace('\\', "\\\\")
}

#[cfg(windows)]
mod registry {
    use std::io;
    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegDeleteKeyValueW, RegGetValueW, RegSetValueExW, HKEY,
        HKEY_CURRENT_USER, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ, RRF_RT_REG_SZ,
    };

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(Some(0)).collect()
    }

    fn failed(status: u32) -> io::Error {
        io::Error::from_raw_os_error(status as i32)
    }

    pub fn read(key: &str, name: &str) -> io::Result<Option<String>> {
        let (key, name) = (wide(key), wide(name));
        let mut size: u32 = 0;
        // SAFETY: both names end in a zero; a null buffer asks for the size.
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut size,
            )
        };
        if status == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        if status != ERROR_SUCCESS {
            return Err(failed(status));
        }
        let mut buffer = vec![0u16; (size as usize).div_ceil(2)];
        let mut size = (buffer.len() * 2) as u32;
        // SAFETY: as above, and the buffer holds `size` bytes.
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                buffer.as_mut_ptr().cast(),
                &mut size,
            )
        };
        if status == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        if status != ERROR_SUCCESS {
            return Err(failed(status));
        }
        let chars = (size as usize / 2).min(buffer.len());
        let text = &buffer[..chars];
        let end = text.iter().position(|&c| c == 0).unwrap_or(text.len());
        Ok(Some(String::from_utf16_lossy(&text[..end])))
    }

    pub fn write(key: &str, name: &str, value: &str) -> io::Result<()> {
        let (key, name, value) = (wide(key), wide(name), wide(value));
        let mut handle: HKEY = std::ptr::null_mut();
        // SAFETY: the key name ends in a zero, and `handle` is valid for a write.
        let status = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                0,
                std::ptr::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE,
                std::ptr::null(),
                &mut handle,
                std::ptr::null_mut(),
            )
        };
        if status != ERROR_SUCCESS {
            return Err(failed(status));
        }
        // SAFETY: the handle is open, the name ends in a zero, and the data
        // is the string with its zero, in bytes.
        let status = unsafe {
            RegSetValueExW(
                handle,
                name.as_ptr(),
                0,
                REG_SZ,
                value.as_ptr().cast(),
                (value.len() * 2) as u32,
            )
        };
        // SAFETY: the handle is open, and is not used again.
        unsafe { RegCloseKey(handle) };
        if status != ERROR_SUCCESS {
            return Err(failed(status));
        }
        Ok(())
    }

    pub fn delete(key: &str, name: &str) -> io::Result<()> {
        let (key, name) = (wide(key), wide(name));
        // SAFETY: both names end in a zero.
        let status = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr()) };
        if status != ERROR_SUCCESS && status != ERROR_FILE_NOT_FOUND {
            return Err(failed(status));
        }
        Ok(())
    }

    /// Deletes a key and everything under it. Tests use it to clean up.
    #[cfg(test)]
    pub fn delete_tree(key: &str) {
        use windows_sys::Win32::System::Registry::RegDeleteTreeW;
        let key = wide(key);
        // SAFETY: the name ends in a zero.
        unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, key.as_ptr()) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exe() -> PathBuf {
        if cfg!(windows) {
            PathBuf::from(r"C:\Program Files\jev\jev-tray.exe")
        } else {
            PathBuf::from("/opt/my tools/jev-tray")
        }
    }

    #[test]
    fn the_run_value_is_the_quoted_full_path() {
        assert_eq!(
            run_value(Path::new(r"C:\Program Files\jev\jev-tray.exe")),
            r#""C:\Program Files\jev\jev-tray.exe""#
        );
    }

    #[test]
    fn the_plist_runs_the_path_at_load() {
        let text = plist_text(Path::new("/Applications/jev & co/jev-tray"));
        assert!(text.contains("<string>com.github.mattbran87.jev-tray</string>"));
        assert!(text.contains("<string>/Applications/jev &amp; co/jev-tray</string>"));
        assert!(text.contains("<key>RunAtLoad</key>\n\t<true/>"));
    }

    #[test]
    fn the_desktop_file_execs_the_quoted_path() {
        let text = desktop_text(Path::new("/opt/my tools/jev-tray"));
        assert!(text.starts_with("[Desktop Entry]\n"));
        assert!(text.contains("\nType=Application\n"));
        assert!(text.contains("\nExec=\"/opt/my tools/jev-tray\"\n"));
    }

    #[test]
    fn exec_quoting_escapes_the_reserved_characters() {
        assert_eq!(exec_quote("/a/b"), r#""/a/b""#);
        assert_eq!(exec_quote("/a/$x"), r#""/a/\\$x""#);
        assert_eq!(exec_quote("/a/100%"), r#""/a/100%%""#);
        assert_eq!(exec_quote(r"/a\b"), r#""/a\\\\b""#);
    }

    /// Unix only: `/x/conf` is not an absolute path on Windows.
    #[cfg(unix)]
    #[test]
    fn the_autostart_folder_follows_xdg() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |name: &str| {
                pairs
                    .iter()
                    .find(|(key, _)| *key == name)
                    .map(|(_, value)| OsString::from(value))
            }
        };
        assert_eq!(
            autostart_dir(env(&[("XDG_CONFIG_HOME", "/x/conf"), ("HOME", "/home/u")])),
            Some(PathBuf::from("/x/conf").join("autostart"))
        );
        assert_eq!(
            autostart_dir(env(&[("HOME", "/home/u")])),
            Some(PathBuf::from("/home/u").join(".config").join("autostart"))
        );
        assert_eq!(
            autostart_dir(env(&[("XDG_CONFIG_HOME", "conf"), ("HOME", "/home/u")])),
            Some(PathBuf::from("/home/u").join(".config").join("autostart"))
        );
        assert_eq!(autostart_dir(env(&[])), None);
    }

    /// The file entries are plain files, so this runs on every system.
    #[test]
    fn a_file_entry_is_written_read_and_deleted() {
        let home = tempfile::tempdir().unwrap();
        let entries = [
            (
                Login::launch_agent(home.path()),
                plist_text(&exe()),
                home.path()
                    .join("Library/LaunchAgents/com.github.mattbran87.jev-tray.plist"),
            ),
            (
                Login::Autostart(home.path().join(".config/autostart/jev-tray.desktop")),
                desktop_text(&exe()),
                home.path().join(".config/autostart/jev-tray.desktop"),
            ),
        ];
        for (login, text, file) in entries {
            assert!(!login.is_on().unwrap());
            login.delete().unwrap();
            login.write(&exe()).unwrap();
            assert!(login.is_on().unwrap());
            assert_eq!(std::fs::read_to_string(&file).unwrap(), text);
            assert_eq!(login.read().unwrap(), Some(text));
            login.delete().unwrap();
            assert!(!login.is_on().unwrap());
            assert!(!file.exists());
        }
    }

    #[test]
    fn writing_again_replaces_another_path() {
        let home = tempfile::tempdir().unwrap();
        let login = Login::launch_agent(home.path());
        login.write(Path::new("/old/jev-tray")).unwrap();
        assert!(login.is_on().unwrap(), "another path still counts as on");
        login.write(&exe()).unwrap();
        assert_eq!(login.read().unwrap(), Some(plist_text(&exe())));
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    #[test]
    fn this_user_gets_an_autostart_file() {
        match Login::for_this_user() {
            Ok(Login::Autostart(file)) => assert!(file.ends_with("autostart/jev-tray.desktop")),
            other => panic!("{other:?}"),
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn this_user_gets_a_launch_agent() {
        match Login::for_this_user() {
            Ok(Login::LaunchAgent(file)) => {
                assert!(file.ends_with("Library/LaunchAgents/com.github.mattbran87.jev-tray.plist"))
            }
            other => panic!("{other:?}"),
        }
    }

    #[cfg(windows)]
    #[test]
    fn this_user_gets_the_run_value() {
        assert_eq!(
            Login::for_this_user().unwrap(),
            Login::Run {
                key: RUN_KEY.to_string(),
                name: "jev-tray".to_string()
            }
        );
    }

    /// Uses a test key, never the real `Run` key.
    #[cfg(windows)]
    #[test]
    fn a_registry_entry_is_written_read_and_deleted() {
        const TEST_KEY: &str = r"Software\jev-tray-test";
        struct Cleanup;
        impl Drop for Cleanup {
            fn drop(&mut self) {
                registry::delete_tree(TEST_KEY);
            }
        }
        let _cleanup = Cleanup;
        let login = Login::Run {
            key: format!(r"{TEST_KEY}\Run"),
            name: NAME.to_string(),
        };
        assert!(!login.is_on().unwrap());
        login.delete().unwrap();
        login.write(Path::new("/old/jev-tray.exe")).unwrap();
        assert!(login.is_on().unwrap());
        login.write(&exe()).unwrap();
        assert_eq!(
            login.read().unwrap(),
            Some(r#""C:\Program Files\jev\jev-tray.exe""#.to_string())
        );
        login.delete().unwrap();
        assert!(!login.is_on().unwrap());
    }
}
```

- [ ] **Step 5: Run the tests and see them pass**

Run: `cargo test -p jev-tray --locked --lib`
Expected: `test result: ok. 13 passed; 0 failed`: 5 icon tests and 8 login tests. Each system runs 8 login tests: 6 shared, and 2 for that system.

On Windows, check that the tests left no registry entry. `MSYS_NO_PATHCONV=1` stops Git Bash from changing `/v` to a path:

```bash
MSYS_NO_PATHCONV=1 reg query 'HKCU\Software\Microsoft\Windows\CurrentVersion\Run' /v jev-tray
MSYS_NO_PATHCONV=1 reg query 'HKCU\Software\jev-tray-test'
```

Expected: each exits 1 with `ERROR: The system was unable to find the specified registry key or value.`

- [ ] **Step 6: Mutation self-checks**

Make each change alone in `crates/jev-tray/src/login.rs`. Copy the file to `<scratchpad>` first, and copy it back after the run. Run `cargo test -p jev-tray --locked --lib` each time.

| Change | The test that must fail |
| --- | --- |
| In `exec_quote`, delete `'$' \|` from the first match arm, so `$` is not escaped | `exec_quoting_escapes_the_reserved_characters` |
| In `xml_escape`, `text.replace('&', "&amp;")` becomes `text.replace('&', "&")`, so `&` is not escaped | `the_plist_runs_the_path_at_load` |
| In `registry::delete`, `if status != ERROR_SUCCESS && status != ERROR_FILE_NOT_FOUND {` becomes `if status != ERROR_SUCCESS {` (no entry is an error) | `a_registry_entry_is_written_read_and_deleted` (Windows only) |

After you copy the file back, run the tests again. Expected: 13 passed.

- [ ] **Step 7: Run the gates**

Run each command alone, and read its exit code:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo +1.85 check --workspace --locked
```

Expected: each exits 0. On Windows, `cargo test` passes 407 tests in all and ignores 1 (count them as "Before you start" shows). Check that no `jevd` still runs: `tasklist | findstr jevd` prints nothing.

- [ ] **Step 8: Commit**

```bash
git add crates/jev-tray/src/lib.rs crates/jev-tray/src/login.rs
git commit -m "jev-tray: start at login" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: the tray app

This task adds the binary: the event loop, the icon, the menu, the 5 s worker thread, and the one-copy lock. It adds the tray crates, so the lock file grows by 125 packages. It also adds the tooltip text and the RGBA decoding to the library, with their tests.

**Files:**
- Modify: `crates/jev-tray/Cargo.toml`, `Cargo.lock`, `crates/jev-tray/src/lib.rs`, `crates/jev-tray/src/state.rs`, `crates/jev-tray/src/icon.rs`
- Create: `crates/jev-tray/src/one_copy.rs`, `crates/jev-tray/src/main.rs`

- [ ] **Step 1: Add the tray crates and the module**

Save this patch as `<scratchpad>/t5-deps.patch` and run `git apply <scratchpad>/t5-deps.patch`:

```diff
diff --git a/crates/jev-tray/Cargo.toml b/crates/jev-tray/Cargo.toml
index df1b77a..ff115cb 100644
--- a/crates/jev-tray/Cargo.toml
+++ b/crates/jev-tray/Cargo.toml
@@ -9,6 +9,13 @@ repository.workspace = true
 
 [dependencies]
 jev-proto = { version = "0.1.0", path = "../jev-proto" }
+png = "0.18"
+# tray-icon 0.25 and later need Rust 1.90; the workspace MSRV is 1.85.
+tao = "0.37.1"
+tray-icon = "0.24.2"
+
+[target.'cfg(unix)'.dependencies]
+libc = "0.2"
 
 [target.'cfg(windows)'.dependencies]
 windows-sys = { version = "0.59", features = ["Win32_Foundation", "Win32_System_Registry"] }
diff --git a/crates/jev-tray/src/lib.rs b/crates/jev-tray/src/lib.rs
index fdbc28e..81e687e 100644
--- a/crates/jev-tray/src/lib.rs
+++ b/crates/jev-tray/src/lib.rs
@@ -1,6 +1,7 @@
 //! The parts of `jev-tray` that do not need a tray: the state check, the
-//! icon choice, and the login entry (the jev-tray spec).
+//! icon choice, the login entry, and the one-copy lock (the jev-tray spec).
 
 pub mod icon;
 pub mod login;
+pub mod one_copy;
 pub mod state;
```

- [ ] **Step 2: Update and pin the lock file**

Run: `cargo metadata --format-version 1 > /dev/null`
Expected: exit 0. Among the `Adding` lines, cargo prints `Locking 127 packages to highest Rust 1.85 compatible versions` and these 3 lines:

```
 Downgrading toml v0.8.23 -> v0.8.2 (available: v0.8.23)
 Downgrading toml_datetime v0.6.11 -> v0.6.3 (available: v0.6.11)
 Downgrading winnow v0.7.15 -> v0.5.40
```

The cause is `proc-macro-crate` 2.0.2, which the Linux GTK crates use. It pins `toml_edit` to exactly 0.20.2 and `toml_datetime` to exactly 0.6.3. `proc-macro-crate` 2.0.0 has no exact pins. `tao` 0.37.2 and `crc32fast` 1.5.4 are newer than the cutoff. Run these 5 commands, each alone, in this order:

```bash
cargo update -p proc-macro-crate@2.0.2 --precise 2.0.0
cargo update -p toml@0.8.2 --precise 0.8.23
cargo update -p toml_edit@0.20.2 --precise 0.20.7
cargo update -p crc32fast --precise 1.5.3
cargo update -p tao --precise 0.37.1
```

Expected, in order: `Downgrading proc-macro-crate v2.0.2 -> v2.0.0`; `Updating toml v0.8.2 -> v0.8.23` and `Updating toml_datetime v0.6.3 -> v0.6.11`; `Updating toml_edit v0.20.2 -> v0.20.7`; `Downgrading crc32fast v1.5.4 -> v1.5.3`; `Downgrading tao v0.37.2 -> v0.37.1`.

Check the result:
1. `sha256sum Cargo.lock` prints `5f250b1ca243b1247cc35f64275eb8c6d33142078dd17839eed53de41191fe30`. If the hash differs, stop and report it, with the output of the 6 commands above. A crate release after 2026-11-04 can change the resolution. Do not edit `Cargo.lock` by hand.
2. `git diff --stat Cargo.lock` prints `1 file changed, 1424 insertions(+), 53 deletions(-)`.
3. `grep -c '^\[\[package\]\]' Cargo.lock` prints `437`. It printed `312` before this step.
4. `grep -A1 -E '^name = "(tray-icon|tao|muda|png|crc32fast|toml)"$' Cargo.lock` shows `tray-icon` 0.24.2, `tao` 0.37.1, `muda` 0.19.3, `png` 0.18.1, `crc32fast` 1.5.3, and `toml` 0.8.23, each once.
5. `grep -A1 -E '^name = "(proc-macro-crate|toml_edit)"$' Cargo.lock` shows `proc-macro-crate` 1.3.1, 2.0.0, and 3.5.0, and `toml_edit` 0.19.15, 0.20.7, 0.22.27, and 0.25.17+spec-1.1.0.

- [ ] **Step 3: Write the failing tests**

Save this patch as `<scratchpad>/t5-tests.patch` and run `git apply <scratchpad>/t5-tests.patch`. It adds the new tests to `icon.rs` and `state.rs`, and no code:

```diff
diff --git a/crates/jev-tray/src/icon.rs b/crates/jev-tray/src/icon.rs
--- a/crates/jev-tray/src/icon.rs
+++ b/crates/jev-tray/src/icon.rs
@@ -184,4 +184,26 @@
             }
         }
     }
-}
+
+    #[test]
+    fn every_icon_decodes_in_its_own_color() {
+        for state in [State::On, State::Off] {
+            for (shade, value) in [(Shade::Black, 0), (Shade::White, 255)] {
+                for size in SIZES {
+                    let image = rgba(png(state, shade, size)).unwrap();
+                    assert_eq!((image.width, image.height), (size, size));
+                    assert_eq!(image.pixels.len(), (size * size * 4) as usize);
+                    let solid: Vec<&[u8]> = image
+                        .pixels
+                        .chunks_exact(4)
+                        .filter(|pixel| pixel[3] == 255)
+                        .collect();
+                    assert!(!solid.is_empty(), "{state:?} {shade:?} {size}");
+                    for pixel in solid {
+                        assert_eq!(&pixel[..3], &[value; 3], "{state:?} {shade:?} {size}");
+                    }
+                }
+            }
+        }
+    }
+}
diff --git a/crates/jev-tray/src/state.rs b/crates/jev-tray/src/state.rs
--- a/crates/jev-tray/src/state.rs
+++ b/crates/jev-tray/src/state.rs
@@ -62,3 +62,30 @@
         _ => State::Off,
     }
 }
+
+#[cfg(test)]
+mod tests {
+    use super::*;
+
+    #[test]
+    fn the_tooltip_names_the_state() {
+        assert_eq!(tooltip(State::On, None), "jevd runs");
+        assert_eq!(tooltip(State::Off, None), "jevd is not running");
+    }
+
+    #[test]
+    fn a_login_error_shows_under_the_state() {
+        assert_eq!(
+            tooltip(State::On, Some("access denied")),
+            "jevd runs\nStart at login failed: access denied"
+        );
+    }
+
+    #[test]
+    fn a_long_tooltip_is_cut_to_the_windows_limit() {
+        let long = "x".repeat(500);
+        let text = tooltip(State::Off, Some(&long));
+        assert_eq!(text.encode_utf16().count(), TOOLTIP_MAX);
+        assert!(text.starts_with("jevd is not running\n"));
+    }
+}
```

Write `crates/jev-tray/src/one_copy.rs` with the tests only. This text is the end of the full file in Step 5:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_lock_fails_until_the_first_one_closes() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::under(dir.path()).unwrap();
        let first = lock(&paths).unwrap().expect("the first lock");
        assert!(lock(&paths).unwrap().is_none(), "a second copy must exit");
        drop(first);
        assert!(lock(&paths).unwrap().is_some(), "the lock is free again");
        assert!(paths.runtime_dir.join(LOCK_FILE).exists());
    }
}
```

- [ ] **Step 4: Run the tests and see them fail**

Run: `cargo test -p jev-tray --locked --lib`
Expected: FAIL to compile, with `` could not compile `jev-tray` (lib test) due to 11 previous errors ``. The errors include `` cannot find function `tooltip` in this scope ``, `` cannot find value `TOOLTIP_MAX` in this scope ``, `` cannot find function `rgba` in this scope ``, `` cannot find function `lock` in this scope ``, and `` cannot find value `LOCK_FILE` in this scope ``.

- [ ] **Step 5: Write the library code**

Save this patch as `<scratchpad>/t5-code.patch` and run `git apply <scratchpad>/t5-code.patch`. It adds the tooltip text to `state.rs` and the RGBA decoding to `icon.rs`:

```diff
diff --git a/crates/jev-tray/src/icon.rs b/crates/jev-tray/src/icon.rs
--- a/crates/jev-tray/src/icon.rs
+++ b/crates/jev-tray/src/icon.rs
@@ -97,6 +97,48 @@
     let size = size_for(size, 1.0);
     let index = SIZES.iter().position(|&each| each == size).unwrap_or(0);
     files[index]
+}
+
+/// The pixels of one decoded icon, 4 bytes per pixel (red, green, blue,
+/// alpha), row by row.
+#[derive(Debug, Clone, PartialEq, Eq)]
+pub struct Rgba {
+    pub pixels: Vec<u8>,
+    pub width: u32,
+    pub height: u32,
+}
+
+/// Decodes PNG bytes to 8-bit RGBA, the form a tray icon takes.
+pub fn rgba(png_bytes: &[u8]) -> Result<Rgba, String> {
+    let mut decoder = png::Decoder::new(std::io::Cursor::new(png_bytes));
+    decoder.set_transformations(png::Transformations::normalize_to_color8());
+    let mut reader = decoder.read_info().map_err(|error| error.to_string())?;
+    let size = reader
+        .output_buffer_size()
+        .ok_or_else(|| "the image is too large".to_string())?;
+    let mut buffer = vec![0; size];
+    let info = reader
+        .next_frame(&mut buffer)
+        .map_err(|error| error.to_string())?;
+    let data = &buffer[..info.buffer_size()];
+    let pixels = match info.color_type {
+        png::ColorType::Rgba => data.to_vec(),
+        png::ColorType::Rgb => data
+            .chunks_exact(3)
+            .flat_map(|p| [p[0], p[1], p[2], 255])
+            .collect(),
+        png::ColorType::GrayscaleAlpha => data
+            .chunks_exact(2)
+            .flat_map(|p| [p[0], p[0], p[0], p[1]])
+            .collect(),
+        png::ColorType::Grayscale => data.iter().flat_map(|&g| [g, g, g, 255]).collect(),
+        png::ColorType::Indexed => return Err("an indexed image was not expanded".to_string()),
+    };
+    Ok(Rgba {
+        pixels,
+        width: info.width,
+        height: info.height,
+    })
 }
 
 /// Reads the taskbar theme. Not unit-tested: it reads the real registry.
diff --git a/crates/jev-tray/src/state.rs b/crates/jev-tray/src/state.rs
--- a/crates/jev-tray/src/state.rs
+++ b/crates/jev-tray/src/state.rs
@@ -17,6 +17,32 @@
     On,
     /// Anything else: no channel, no reply in time, or an error reply.
     Off,
+}
+
+impl State {
+    /// The tooltip text for this state.
+    pub fn tooltip(self) -> &'static str {
+        match self {
+            State::On => "jevd runs",
+            State::Off => "jevd is not running",
+        }
+    }
+}
+
+/// The longest tooltip, in UTF-16 units: Windows keeps 128 with the zero.
+pub const TOOLTIP_MAX: usize = 127;
+
+/// The tooltip: the state, and under it a failed **Start at login** change
+/// when there is one, cut to `TOOLTIP_MAX`.
+pub fn tooltip(state: State, login_error: Option<&str>) -> String {
+    let mut text = match login_error {
+        None => state.tooltip().to_string(),
+        Some(error) => format!("{}\nStart at login failed: {error}", state.tooltip()),
+    };
+    while text.encode_utf16().count() > TOOLTIP_MAX {
+        text.pop();
+    }
+    text
 }
 
 /// One check, with the 5 s limit.
```

Replace `crates/jev-tray/src/one_copy.rs` with:

```rust
//! One copy of `jev-tray` per user (jev-tray spec section 5): the copy that
//! locks `jev-tray.lock` in the runtime folder runs; any other exits.

use std::fs::File;
use std::io;

use jev_proto::Paths;

/// The lock file's name in the runtime folder.
pub const LOCK_FILE: &str = "jev-tray.lock";

/// Takes the lock. `Ok(None)` means that another copy holds it. Keep the
/// file open while the program runs: closing it lets the lock go.
pub fn lock(paths: &Paths) -> io::Result<Option<File>> {
    paths.prepare_runtime_dir()?;
    take(&paths.runtime_dir.join(LOCK_FILE))
}

#[cfg(unix)]
fn take(file: &std::path::Path) -> io::Result<Option<File>> {
    use std::os::unix::fs::OpenOptionsExt;
    use std::os::unix::io::AsRawFd;
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(file)?;
    // SAFETY: the descriptor is open for the call.
    if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        let error = io::Error::last_os_error();
        return if error.kind() == io::ErrorKind::WouldBlock {
            Ok(None)
        } else {
            Err(error)
        };
    }
    Ok(Some(lock))
}

/// On Windows, a file opened with no sharing is the lock: a second open
/// fails with a sharing violation until the first handle closes.
#[cfg(windows)]
fn take(file: &std::path::Path) -> io::Result<Option<File>> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Foundation::ERROR_SHARING_VIOLATION;
    match std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .share_mode(0)
        .open(file)
    {
        Ok(lock) => Ok(Some(lock)),
        Err(error) if error.raw_os_error() == Some(ERROR_SHARING_VIOLATION as i32) => Ok(None),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_lock_fails_until_the_first_one_closes() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::under(dir.path()).unwrap();
        let first = lock(&paths).unwrap().expect("the first lock");
        assert!(lock(&paths).unwrap().is_none(), "a second copy must exit");
        drop(first);
        assert!(lock(&paths).unwrap().is_some(), "the lock is free again");
        assert!(paths.runtime_dir.join(LOCK_FILE).exists());
    }
}
```

- [ ] **Step 6: Run the tests and see them pass**

Run: `cargo test -p jev-tray --locked --lib`
Expected: `test result: ok. 18 passed; 0 failed`: 6 icon tests, 8 login tests, 3 state tests, and 1 one-copy test.

- [ ] **Step 7: Write the tray app**

Write `crates/jev-tray/src/main.rs`:

```rust
//! `jev-tray`: one tray icon that shows whether `jevd` runs (the jev-tray
//! spec). A worker thread checks every 5 seconds and sends each result to
//! the event loop, which changes the icon and tooltip only on a change.

// No console window on Windows.
#![cfg_attr(windows, windows_subsystem = "windows")]

use std::time::Instant;

use jev_proto::Paths;
use jev_tray::icon::{self, Shade};
use jev_tray::login::Login;
use jev_tray::one_copy;
use jev_tray::state::{self, State, CHECK_LIMIT};
use tao::event::{Event, StartCause};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy, EventLoopWindowTarget};
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

/// What reaches the event loop from other threads.
enum Message {
    /// The result of one check, with the bar's shade at that time.
    Checked { state: State, shade: Shade },
    /// A click on a menu item.
    Menu(MenuEvent),
}

/// The icon in the tray, and what it shows now.
struct Tray {
    icon: TrayIcon,
    /// The last state that a check gave.
    state: State,
    /// The state, shade, and size of the image on show.
    shown: (State, Shade, u32),
    /// The tooltip on show.
    tooltip: String,
}

impl Tray {
    /// Shows `state`, and changes the image or the tooltip only when it
    /// differs from what is on show.
    fn show(&mut self, state: State, shade: Shade, size: u32, login_error: Option<&str>) {
        self.state = state;
        if (state, shade, size) != self.shown {
            if let Some(image) = image(state, shade, size) {
                if self
                    .icon
                    .set_icon_with_as_template(Some(image), icon::TEMPLATE)
                    .is_ok()
                {
                    self.shown = (state, shade, size);
                }
            }
        }
        let text = state::tooltip(state, login_error);
        if text != self.tooltip && self.icon.set_tooltip(Some(&text)).is_ok() {
            self.tooltip = text;
        }
    }
}

fn image(state: State, shade: Shade, size: u32) -> Option<Icon> {
    let image = icon::rgba(icon::png(state, shade, size)).ok()?;
    Icon::from_rgba(image.pixels, image.width, image.height).ok()
}

/// The icon size in pixels for the primary screen.
fn size_now(target: &EventLoopWindowTarget<Message>) -> u32 {
    let scale = target
        .primary_monitor()
        .map_or(1.0, |monitor| monitor.scale_factor());
    icon::size_for(icon::POINTS, scale)
}

fn fail(message: &str) -> ! {
    eprintln!("jev-tray: {message}");
    std::process::exit(1);
}

/// Checks every 5 seconds, until the event loop is gone.
fn watch(paths: Paths, proxy: EventLoopProxy<Message>) {
    loop {
        let started = Instant::now();
        let message = Message::Checked {
            state: state::check(&paths),
            // Read at each check, so a Windows theme change shows.
            shade: icon::shade_now(),
        };
        if proxy.send_event(message).is_err() {
            return;
        }
        std::thread::sleep(CHECK_LIMIT.saturating_sub(started.elapsed()));
    }
}

/// Writes or deletes the login entry, so it matches the check item. The
/// item has already changed when the click arrives.
fn apply_login(item: &CheckMenuItem, login: &Result<Login, String>) -> Result<(), String> {
    let login = login.as_ref().map_err(Clone::clone)?;
    if item.is_checked() {
        let exe = std::env::current_exe().map_err(|error| error.to_string())?;
        login.write(&exe).map_err(|error| error.to_string())
    } else {
        login.delete().map_err(|error| error.to_string())
    }
}

fn main() {
    let paths = Paths::from_env()
        .unwrap_or_else(|error| fail(&format!("cannot find the jev folders: {error}")));
    // Held until the process exits: `run` below never returns.
    let _lock = match one_copy::lock(&paths) {
        Ok(Some(lock)) => lock,
        // Another jev-tray runs.
        Ok(None) => return,
        Err(error) => fail(&format!("cannot take the one-copy lock: {error}")),
    };

    #[allow(unused_mut)]
    let mut event_loop = EventLoopBuilder::<Message>::with_user_event().build();
    #[cfg(target_os = "macos")]
    {
        use tao::platform::macos::{ActivationPolicy, EventLoopExtMacOS};
        // No Dock icon.
        event_loop.set_activation_policy(ActivationPolicy::Accessory);
    }

    let login = Login::for_this_user().map_err(|error| error.to_string());
    let login_on = login
        .as_ref()
        .ok()
        .and_then(|login| login.is_on().ok())
        .unwrap_or(false);
    let login_item = CheckMenuItem::new("Start at login", true, login_on, None);
    let quit_item = MenuItem::new("Quit", true, None);
    let menu = Menu::new();
    menu.append_items(&[&login_item, &quit_item])
        .unwrap_or_else(|error| fail(&format!("cannot build the menu: {error}")));
    let mut menu = Some(menu);

    let proxy = event_loop.create_proxy();
    MenuEvent::set_event_handler(Some(move |event| {
        let _ = proxy.send_event(Message::Menu(event));
    }));
    let proxy = event_loop.create_proxy();
    std::thread::Builder::new()
        .name("jev-tray-watch".into())
        .spawn(move || watch(paths, proxy))
        .unwrap_or_else(|error| fail(&format!("cannot start the check thread: {error}")));

    let mut tray: Option<Tray> = None;
    event_loop.run(move |event, target, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            // The tray must be made inside the running loop (GTK on Linux).
            Event::NewEvents(StartCause::Init) => {
                let (state, shade, size) = (State::Off, icon::shade_now(), size_now(target));
                let text = state::tooltip(state, None);
                let mut builder = TrayIconBuilder::new()
                    .with_tooltip(&text)
                    .with_icon_as_template(icon::TEMPLATE);
                if let Some(menu) = menu.take() {
                    builder = builder.with_menu(Box::new(menu));
                }
                if let Some(image) = image(state, shade, size) {
                    builder = builder.with_icon(image);
                }
                match builder.build() {
                    Ok(icon) => {
                        tray = Some(Tray {
                            icon,
                            state,
                            shown: (state, shade, size),
                            tooltip: text,
                        })
                    }
                    Err(error) => {
                        eprintln!("jev-tray: cannot show the tray icon: {error}");
                        *control_flow = ControlFlow::ExitWithCode(1);
                    }
                }
            }
            Event::UserEvent(Message::Checked { state, shade }) => {
                if let Some(tray) = tray.as_mut() {
                    // A new check ends the display of a login error.
                    tray.show(state, shade, size_now(target), None);
                }
            }
            Event::UserEvent(Message::Menu(event)) => {
                if event.id == *quit_item.id() {
                    // Drop the icon first, so Windows does not keep a ghost.
                    tray = None;
                    *control_flow = ControlFlow::Exit;
                } else if event.id == *login_item.id() {
                    if let Err(error) = apply_login(&login_item, &login) {
                        login_item.set_checked(!login_item.is_checked());
                        if let Some(tray) = tray.as_mut() {
                            let (_, shade, size) = tray.shown;
                            let state = tray.state;
                            tray.show(state, shade, size, Some(&error));
                        }
                    }
                }
            }
            _ => {}
        }
    });
}
```

Run: `cargo build -p jev-tray --locked`
Expected: exit 0, with no warning. `target/debug/jev-tray.exe` exists.

Run: `cargo test -p jev-tray --locked`
Expected: lib 18 passed, `src/main.rs` 0, `tests/icons.rs` 3 passed, `tests/state.rs` 6 passed.

- [ ] **Step 8: Mutation self-checks**

Make each change alone. Copy the file to `<scratchpad>` first, and copy it back after the run. Run `cargo test -p jev-tray --locked --lib` each time.

| Change | The test that must fail |
| --- | --- |
| In `state.rs`, delete the `while` loop in `tooltip` (the tooltip is not cut) | `a_long_tooltip_is_cut_to_the_windows_limit` |
| In `icon.rs`, in `png`, `(State::On, Shade::Black) => ON_BLACK,` becomes `(State::On, Shade::Black) => ON_WHITE,` | `every_icon_decodes_in_its_own_color` and `each_choice_gives_its_own_file` |
| In `one_copy.rs`, on Windows, `.share_mode(0)` becomes `.share_mode(7)`, so the file allows a second open | `a_second_lock_fails_until_the_first_one_closes` |
| In `one_copy.rs`, on Unix, `libc::LOCK_EX` becomes `libc::LOCK_SH`, so a second shared lock succeeds | `a_second_lock_fails_until_the_first_one_closes` |

After you copy each file back, run the tests again. Expected: 18 passed.

- [ ] **Step 9: Run the gates**

Run each command alone, and read its exit code:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo +1.85 check --workspace --locked
```

Expected: each exits 0. On Windows, `cargo test` passes 412 tests in all and ignores 1 (count them as "Before you start" shows). Check that no `jevd` still runs: `tasklist | findstr jevd` prints nothing.

`cargo +1.85 check` builds the tray crates with Rust 1.85, so it shows that `tray-icon` 0.24.2 and `tao` 0.37.1 meet the MSRV.

- [ ] **Step 10: Commit**

```bash
git add crates/jev-tray/Cargo.toml Cargo.lock crates/jev-tray/src/lib.rs crates/jev-tray/src/state.rs crates/jev-tray/src/icon.rs crates/jev-tray/src/one_copy.rs crates/jev-tray/src/main.rs
git commit -m "jev-tray: the tray app" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: ci and release

This task makes the Linux CI jobs and the Linux release builds install the 3 GTK packages, and adds the 2 Linux-only `cargo deny` entries.

**Files:**
- Modify: `.github/workflows/ci.yml`, `crates/jev-tray/Cargo.toml`, `deny.toml`

- [ ] **Step 1: Apply the edits**

Save this patch as `<scratchpad>/t6-ci.patch` and run `git apply <scratchpad>/t6-ci.patch`:

```diff
diff --git a/.github/workflows/ci.yml b/.github/workflows/ci.yml
index de1675c..b751fa8 100644
--- a/.github/workflows/ci.yml
+++ b/.github/workflows/ci.yml
@@ -40,6 +40,12 @@ jobs:
       - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1  # v7.0.1
         with:
           persist-credentials: false
+      - name: Install the jev-tray build packages
+        # jev-tray's tray library needs GTK 3, libxdo, and an AppIndicator library on Linux.
+        if: runner.os == 'Linux'
+        run: |
+          sudo apt-get update
+          sudo apt-get install -y libgtk-3-dev libxdo-dev libayatana-appindicator3-dev
       - name: Install Rust
         run: |
           rustup toolchain install "$RUST_VERSION" --profile minimal --component rustfmt,clippy
@@ -61,6 +67,11 @@ jobs:
       - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1  # v7.0.1
         with:
           persist-credentials: false
+      - name: Install the jev-tray build packages
+        # jev-tray's tray library needs GTK 3, libxdo, and an AppIndicator library on Linux.
+        run: |
+          sudo apt-get update
+          sudo apt-get install -y libgtk-3-dev libxdo-dev libayatana-appindicator3-dev
       - name: Install Rust
         run: |
           rustup toolchain install "$MSRV" --profile minimal
diff --git a/crates/jev-tray/Cargo.toml b/crates/jev-tray/Cargo.toml
index ff115cb..7f6903e 100644
--- a/crates/jev-tray/Cargo.toml
+++ b/crates/jev-tray/Cargo.toml
@@ -7,6 +7,13 @@ license.workspace = true
 rust-version.workspace = true
 repository.workspace = true
 
+# jev-tray is its own dist app, with its own archives. Its Linux build needs
+# GTK 3, libxdo, and an AppIndicator library.
+[package.metadata.dist.dependencies.apt]
+libgtk-3-dev = "*"
+libxdo-dev = "*"
+libayatana-appindicator3-dev = "*"
+
 [dependencies]
 jev-proto = { version = "0.1.0", path = "../jev-proto" }
 png = "0.18"
diff --git a/deny.toml b/deny.toml
index eec3573..252e642 100644
--- a/deny.toml
+++ b/deny.toml
@@ -23,6 +23,12 @@ ignore = [
     { id = "RUSTSEC-2026-0370", reason = "zvariant unmaintained; via keyring 3.6, Linux only" },
     { id = "RUSTSEC-2026-0371", reason = "zbus_names unmaintained; via keyring 3.6, Linux only" },
     { id = "RUSTSEC-2026-0372", reason = "zvariant_derive unmaintained; via keyring 3.6, Linux only" },
+    # proc-macro-error: an "unmaintained" notice, not a vulnerability. It is a
+    # build-time macro crate that reaches us on Linux only: jev-tray ->
+    # tray-icon 0.24 and tao -> gtk 0.18 -> glib-macros 0.18. tray-icon 0.25
+    # and later need Rust 1.90, above the 1.85 MSRV. Look again when the MSRV
+    # moves.
+    { id = "RUSTSEC-2024-0370", reason = "proc-macro-error unmaintained; via gtk 0.18 for jev-tray, Linux only, build time" },
 ]
 
 [licenses]
@@ -37,6 +43,8 @@ allow = [
     "ISC",
     "MIT",
     "MIT-0",
+    # option-ext, via dirs 6, which tray-icon uses for jev-tray on Linux.
+    "MPL-2.0",
     "Unicode-3.0",
     "Unlicense",
 ]
```

- [ ] **Step 2: Check the release workflow**

Run: `dist generate --check`
Expected: exit 0. `release.yml` needs no change: `dist` reads the apt packages from the crate's metadata when the workflow runs.

Run: `dist plan`
Expected: exit 0, and 2 apps:
- `jev 0.1.0`, as before: `jev-installer.sh`, `jev-installer.ps1`, and 4 archives. Each archive holds `[bin] jev` and `[bin] jevd` (`jev.exe` and `jevd.exe` in the Windows `.zip`), and `[misc] LICENSE, README.md`.
- `jev-tray 0.1.0`: `jev-tray-installer.sh`, `jev-tray-installer.ps1`, and 4 archives, `jev-tray-aarch64-apple-darwin.tar.xz`, `jev-tray-aarch64-unknown-linux-gnu.tar.xz`, `jev-tray-x86_64-pc-windows-msvc.zip`, and `jev-tray-x86_64-unknown-linux-gnu.tar.xz`. Each holds `[bin] jev-tray` (`jev-tray.exe` on Windows) and `[misc] LICENSE, README.md`.

- [ ] **Step 3: Check the dependencies**

Run: `cargo deny check`
Expected: exit 0, and `advisories ok, bans ok, licenses ok, sources ok`. It also prints 15 `warning[duplicate]` lines on Windows; they are not defects.

- [ ] **Step 4: Mutation self-checks**

Make each change alone in `deny.toml`. Copy the file to `<scratchpad>` first, and copy it back after the run.

| Change | The command that must fail |
| --- | --- |
| Delete the line `"MPL-2.0",` | `cargo deny check licenses`: exit 4, `error[rejected]: failed to satisfy license requirements`, for `option-ext` |
| Delete the line `{ id = "RUSTSEC-2024-0370", ... },` | `cargo deny check advisories`: exit 1, `error[unmaintained]: proc-macro-error is unmaintained` |

After you copy the file back, run `cargo deny check` again. Expected: exit 0.

- [ ] **Step 5: Run the gates**

Run each command alone, and read its exit code:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo +1.85 check --workspace --locked
```

Expected: each exits 0. On Windows, `cargo test` passes 412 tests in all and ignores 1 (count them as "Before you start" shows). Check that no `jevd` still runs: `tasklist | findstr jevd` prints nothing.

- [ ] **Step 6: Commit**

```bash
git add .github/workflows/ci.yml crates/jev-tray/Cargo.toml deny.toml
git commit -m "ci and release: jev-tray" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 7: the README and the specs

This task adds a `jev-tray` section to the README, records `jev-tray` in the main spec, and records in the jev-tray spec what the build found.

**Files:**
- Modify: `README.md`, `docs/superpowers/specs/2026-11-02-jev-daemon-design.md`, `docs/superpowers/specs/2026-11-04-jev-tray-design.md`

- [ ] **Step 1: Update the README and the main spec**

Save this patch as `<scratchpad>/t7-docs.patch` and run `git apply <scratchpad>/t7-docs.patch`:

~~~diff
diff --git a/README.md b/README.md
index 4b6daec..8c41539 100644
--- a/README.md
+++ b/README.md
@@ -4,6 +4,7 @@ jev lets local programs ask TypeSafe's Jev model while one background process, `
 
 - `jevd`: the per-user daemon. Programs and the `jev` CLI start it when they need it, and it exits after 2 hours with no use.
 - `jev`: the CLI. It manages keys, pairing, and settings, and `jev ask` sends a request.
+- `jev-tray`: a tray icon that shows whether `jevd` runs.
 - `plugins/claude-code`: a Claude Code plugin that gives other plugins `$.jev`.
 
 **Status:** before the first release. No release archive exists yet.
@@ -18,6 +19,33 @@ cargo install --locked --git https://github.com/mattbran87/jev-daemon jev
 
 That installs `jev` and `jevd` into the same folder, `~/.cargo/bin`. `jevd` accepts admin commands only from the `jev` next to it, so keep the two together.
 
+## jev-tray
+
+`jev-tray` shows one icon in the system tray. The icon has a check mark when `jevd` runs, and an x when it does not. The tooltip says the same. `jev-tray` checks every 5 seconds. It never starts, stops, or configures `jevd`.
+
+Install it from source:
+
+```bash
+cargo install --locked --git https://github.com/mattbran87/jev-daemon jev-tray
+```
+
+On Linux, the build needs GTK 3, libxdo, and an AppIndicator library. On Debian and Ubuntu, install them first:
+
+```bash
+sudo apt-get install libgtk-3-dev libxdo-dev libayatana-appindicator3-dev
+```
+
+Then run `jev-tray`. Only one copy runs: a second copy exits at once.
+
+Click the icon to open its menu. The menu has two items:
+
+- **Start at login**: tick it, and `jev-tray` starts each time you sign in. Untick it to stop this. The entry holds the path of the `jev-tray` that you ticked it from, so tick it again after you move `jev-tray`.
+- **Quit**: closes `jev-tray`. `jevd` keeps running.
+
+**Start at login** writes one entry: a value under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` on Windows, a file in `~/Library/LaunchAgents` on macOS, and a file in `~/.config/autostart` on Linux.
+
+On Linux, the desktop needs a tray host. GNOME has none until you install its AppIndicator extension. With no tray host, `jev-tray` shows nothing. On Linux, the icon is always white, and the panel shows no tooltip.
+
 ## Claude Code plugin
 
 Add the marketplace, then install the plugin. Run these in Claude Code:
@@ -31,7 +59,7 @@ The plugin needs `jev` on your `PATH`. Install it first (see above).
 
 ## Design
 
-The design is in [docs/superpowers/specs/2026-11-02-jev-daemon-design.md](docs/superpowers/specs/2026-11-02-jev-daemon-design.md).
+The design is in [docs/superpowers/specs/2026-11-02-jev-daemon-design.md](docs/superpowers/specs/2026-11-02-jev-daemon-design.md). The design of `jev-tray` is in [docs/superpowers/specs/2026-11-04-jev-tray-design.md](docs/superpowers/specs/2026-11-04-jev-tray-design.md).
 
 ## License
 
diff --git a/docs/superpowers/specs/2026-11-02-jev-daemon-design.md b/docs/superpowers/specs/2026-11-02-jev-daemon-design.md
index b511516..656c0b0 100644
--- a/docs/superpowers/specs/2026-11-02-jev-daemon-design.md
+++ b/docs/superpowers/specs/2026-11-02-jev-daemon-design.md
@@ -52,6 +52,7 @@ The Python prototype, jev-service, stays private. It serves as the reference for
 | `crates/jev-proto` | library | Wire types, the protocol version, and a small client that connects and starts `jevd` when needed. No tokio or reqwest dependency. |
 | `crates/jevd` | library | Socket server, pairing store, checks, keyring read, Jev HTTPS client. Its binary is built from the `jev` package. |
 | `crates/jev` | library and the binaries jev and jevd | The CLI, and the `jevd` binary (`src/bin/jevd.rs`), so one release archive holds both. |
+| `crates/jev-tray` | library and the binary jev-tray | A tray icon that shows whether `jevd` runs. It depends on `jev-proto`, not on `jevd`. Its design is in `2026-11-04-jev-tray-design.md`. |
 | `plugins/claude-code` | TypeScript mod | `$.jev` for other mods, and toasts |
 
 ### Storage
@@ -356,7 +357,8 @@ The mod finds `jev` on the PATH. The plugin option `jevPath` overrides that, and
 | Targets | `x86_64-pc-windows-msvc`, `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, `aarch64-apple-darwin` |
 | macOS | Apple Silicon (M1 and later) only. There is no Intel Mac archive. |
 | Archive | One app, the `jev` package, owns both binaries, so each archive holds `jev` and `jevd` in one folder. The peer check depends on that. |
-| Installers | Shell and PowerShell, from `dist`. They install both binaries into `$CARGO_HOME/bin`. `cargo install --locked --git https://github.com/mattbran87/jev-daemon jev` also works and installs both. |
+| jev-tray | A second app, the `jev-tray` package, with its own archive for each of the 4 targets. The `jev` archive does not hold it, so `jev` and `jevd` still run on a Linux server with no GTK. Its Linux builds install `libgtk-3-dev`, `libxdo-dev`, and `libayatana-appindicator3-dev` first; `[package.metadata.dist.dependencies.apt]` in its `Cargo.toml` names them. |
+| Installers | Shell and PowerShell, from `dist`. They install both binaries into `$CARGO_HOME/bin`. `cargo install --locked --git https://github.com/mattbran87/jev-daemon jev` also works and installs both. `jev-tray` gets its own pair of installers. |
 | Linux | glibc (`gnu`) builds on `ubuntu-22.04` and `ubuntu-22.04-arm`, so they need glibc 2.35 or newer. No musl build in v1. Every target builds on a native runner. |
 | crates.io | Nothing is published in v1. |
 | Pull requests | The release workflow builds every archive on a pull request and uploads them to the run, with no release (`pr-run-mode = "upload"`). |
@@ -369,10 +371,11 @@ The mod finds `jev` on the PATH. The plugin option `jevPath` overrides that, and
 ### CI on each push and pull request
 
 - Windows, Linux, and macOS: `cargo fmt --check`, `cargo clippy` with warnings as errors, `cargo test --locked`, on Rust 1.99.1. CI pins its toolchain, so a new Rust release cannot fail a commit that passed.
+- The Linux `rust` job and the `msrv` job install `libgtk-3-dev`, `libxdo-dev`, and `libayatana-appindicator3-dev` first, because `jev-tray` needs them to build.
 - The macOS jobs run on `macos-14`, an Apple Silicon runner.
 - The MSRV, Rust 1.85: `cargo check --workspace --locked`, with no test targets, because `wiremock`, a test-only dependency, needs Rust 1.88.
 - The mod, on the 3 systems, with Claude Code 2.1.300: `claude plugin validate` on the marketplace and the plugin, and `claude plugin test`. `tsc` runs locally only: `/plugin-types` works only in an interactive session, and its file is Claude Code's own text, so the repo does not hold it.
-- `cargo deny` for licenses, security advisories, bans, and sources, with `deny.toml`. It ignores 3 "unmaintained" notices for the `zbus` 4 crates (RUSTSEC-2026-0370, -0371, -0372), each with its reason, until the MSRV allows `keyring` 4.
+- `cargo deny` for licenses, security advisories, bans, and sources, with `deny.toml`. It ignores 3 "unmaintained" notices for the `zbus` 4 crates (RUSTSEC-2026-0370, -0371, -0372), each with its reason, until the MSRV allows `keyring` 4. It also ignores the "unmaintained" notice for `proc-macro-error` (RUSTSEC-2024-0370), a build-time crate that `jev-tray` gets on Linux through `gtk` 0.18, until the MSRV allows `tray-icon` 0.25. The license list holds `MPL-2.0` for `option-ext`, which `jev-tray` also gets on Linux.
 - The `deny` job runs 2 checks: `advisories`, and `bans licenses sources`. A new advisory can appear with no change in the repo. So on a pull request, a failed `advisories` step does not fail the job. On `main`, it fails the job.
 - `.github/workflows/advisories.yml` runs `cargo deny check advisories` each day, and fails when a new advisory appears. GitHub turns off a scheduled workflow after 60 days with no repo activity.
 - `deny.toml` bans `openssl` and `openssl-sys`, because the binaries use rustls.
~~~

- [ ] **Step 2: Update the jev-tray spec**

Save this patch as `<scratchpad>/t7-tray-spec.patch` and run `git apply <scratchpad>/t7-tray-spec.patch`. It records 3 facts from the build: Linux shows no tooltip, a missing theme value counts as dark, and the 2 new `deny.toml` entries.

```diff
diff --git a/docs/superpowers/specs/2026-11-04-jev-tray-design.md b/docs/superpowers/specs/2026-11-04-jev-tray-design.md
--- a/docs/superpowers/specs/2026-11-04-jev-tray-design.md
+++ b/docs/superpowers/specs/2026-11-04-jev-tray-design.md
@@ -10,7 +10,7 @@
 | --- | --- |
 | On | `jevd` answers `status` with a success reply. The icon shows a check mark. |
 | Off | Anything else: no channel, no reply in time, or an error reply. The icon shows an x. |
-| Tooltip | `jevd runs` when on, `jevd is not running` when off. |
+| Tooltip | `jevd runs` when on, `jevd is not running` when off. Windows and macOS show it. Linux does not: `tray-icon` has no tooltips on Linux, so there only the icon shows the state. |
 | Menu | Two items: **Start at login**, a check item, and **Quit**. |
 | Start state | Off, until the first check finishes. |
 | One copy | A second `jev-tray` exits at once with success, and shows nothing. |
@@ -48,7 +48,7 @@
 | System | How the icon matches the bar |
 | --- | --- |
 | macOS | The black PNG is a template image, so the menu bar recolors it for light and dark mode. |
-| Windows | `jev-tray` reads `SystemUsesLightTheme` under `HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize`. A light taskbar gets the black icon; a dark one, the white icon. It reads the value again at each check, so a theme change shows within 5 seconds. |
+| Windows | `jev-tray` reads `SystemUsesLightTheme` under `HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize`. A light taskbar gets the black icon; a dark one, the white icon. When the value is missing, the taskbar counts as dark. It reads the value again at each check, so a theme change shows within 5 seconds. |
 | Linux | The white icon, always. Linux panels do not report their theme reliably, and most panels are dark. |
 
 ## 4. Start at login
@@ -85,6 +85,10 @@
 - Linux builds of `jev-tray` need `libgtk-3-dev`, `libxdo-dev`, and `libayatana-appindicator3-dev`. The Linux `rust` job and the Linux `jev-tray` release builds install them first.
 - The workspace tests run `jev-tray`'s unit tests on all three systems.
 - `cargo deny` checks the new dependencies' licenses. A license outside the allow list fails CI, and the list grows only by a reviewed change.
+- `jev-tray` adds 2 entries to `deny.toml`. Both are for Linux only:
+  - The license list holds `MPL-2.0`, for `option-ext`. `jev-tray` gets it through `dirs` 6.
+  - `deny.toml` ignores the advisory RUSTSEC-2024-0370. It says that `proc-macro-error` has no maintainer. `jev-tray` uses that crate at build time only, through `glib-macros` 0.18.
+  - When the MSRV allows `tray-icon` 0.25, look at both entries again.
 
 ## 6. Testing
 
@@ -98,6 +102,7 @@
 
 - A Linux desktop with no tray host shows no icon.
 - On Linux, the icon is white on every panel, so it is hard to see on a light panel.
+- On Linux, the panel shows no tooltip. A failed **Start at login** change then shows no error.
 - `jev-tray` shows only whether `jevd` answers. It does not show keys, clients, or pending pairings.
 - Moving `jev-tray` after you tick **Start at login** leaves the entry pointing at the old path until you tick it again.
 
```

- [ ] **Step 3: Check the prose**

Run the STE lint on the jev-tray spec, as the `simple-english-docs` skill says, and fix every ERROR in the lines that Step 2 added. Expected: no ERROR in those lines.

- [ ] **Step 4: Run the gates**

Run each command alone, and read its exit code:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo +1.85 check --workspace --locked
```

Expected: each exits 0. On Windows, `cargo test` passes 412 tests in all and ignores 1 (count them as "Before you start" shows). Check that no `jevd` still runs: `tasklist | findstr jevd` prints nothing.

- [ ] **Step 5: Commit**

```bash
git add README.md docs/superpowers/specs/2026-11-02-jev-daemon-design.md docs/superpowers/specs/2026-11-04-jev-tray-design.md
git commit -m "docs: jev-tray in the README and the spec" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

## Not tested by this plan

- **The Windows theme reader.** `icon::windows_theme` reads the real registry, so no test runs it. The tests cover `shade_for_windows` with both values and with no value.
- **The tray UI, by hand, on each system.** Check these once on Windows, macOS, and a Linux desktop with a tray host. WSL has no tray.
  - The icon appears in the tray, with the x.
  - It switches to the check mark within 5 seconds of `jev start`, and back to the x within 5 seconds of `jev stop`.
  - On Windows, it follows a taskbar theme change within 5 seconds. On macOS, the menu bar recolors it.
  - **Start at login** and **Quit** work from the menu.
  - **Start at login** survives a sign-out and a sign-in.
  - A second `jev-tray` exits at once, and shows nothing.
- **macOS.** No run of this plan used a Mac. The macOS CI job runs the tests on `macos-14`.
- **The `ci.yml` apt steps on a real runner.** The plan checks the YAML with `dist generate --check` and by review only. The first push runs them.
- **The `jev-tray` release archives on GitHub.** `dist plan` shows them. A pull request builds them.

## Notes for the next plans

- When the MSRV reaches 1.90: `tray-icon` 0.25 or later. Then check whether `deny.toml` still needs `MPL-2.0` and the RUSTSEC-2024-0370 ignore, and drop each entry that no crate needs.
- `tray-icon` shows no tooltip on Linux, so a failed **Start at login** change shows no error there. A later release can show the error as a disabled menu item.
- Check the tray UI by hand on each system (see "Not tested by this plan") before the first tag.
- The docs plan writes the `jev-tray` install steps for the release archives and installers.
- `cargo deny check` prints 15 duplicate-version warnings on Windows after Task 6.
