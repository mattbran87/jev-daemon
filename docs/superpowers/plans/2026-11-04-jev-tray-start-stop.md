# jev-tray Start and Stop Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a **Start jevd** / **Stop jevd** item to the `jev-tray` menu. A click runs `jev start` or `jev stop` from the folder that holds `jev-tray`.

**Architecture:** A new module, `crates/jev-tray/src/control.rs`, finds `jev` next to `jev-tray` and runs one command with a 30-second limit. `app.rs` adds the menu item, runs the command on a worker thread, and wakes the watch thread for a check at once after the command. `state::tooltip` takes one failure line with its prefix, so a login failure and a Start or Stop failure share it.

**Tech Stack:** Rust 1.85 (MSRV), `tray-icon` 0.24.2 (`muda` menus), `tao` 0.37.1, `windows-sys` 0.59, `std::process::Command`.

**Spec:** `docs/superpowers/specs/2026-11-04-jev-tray-start-stop-design.md`. It adds to the tray spec, `docs/superpowers/specs/2026-11-04-jev-tray-design.md`.

---

## Before you start

- Work on the branch `jev-tray-start-stop`. It holds the spec commit, 2fd12c8.
- The repo has `core.autocrlf=true`. Write files with LF line ends; Git stores them with LF.
- Commit as the user, and end each commit message with the trailer:

```bash
git -c user.name="Matthew Brandenburg" -c user.email="mattbran87@users.noreply.github.com" commit -m "<subject>" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

- The gates on each task are, from the repo root:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test -p jev-tray --locked
```

- `jev-tray` is a library crate. The binary is `crates/jev/src/bin/jev-tray.rs` in the `jev` package.

## File map

| File | Change |
| --- | --- |
| `crates/jev-tray/src/state.rs` | `tooltip` takes one failure line with its prefix. |
| `crates/jev-tray/src/control.rs` | New: `Action`, `jev_next_to`, `run_action`, `run`, `RUN_LIMIT`, `NO_JEV`. |
| `crates/jev-tray/src/lib.rs` | `pub mod control;` and the crate doc. |
| `crates/jev-tray/Cargo.toml` | The `windows-sys` feature `Win32_System_Threading`, for `CREATE_NO_WINDOW`. |
| `crates/jev-tray/src/app.rs` | The menu item, the `Ran` message, the wake channel, the failure rule. |
| `docs/superpowers/specs/2026-11-04-jev-tray-design.md` | The 5 changes of section 6 of the Start and Stop spec. |
| `README.md` | The menu has three items; Stop does not last. |

---

### Task 1: The tooltip takes a failure line

**Files:**
- Modify: `crates/jev-tray/src/state.rs` (`tooltip` and its tests)
- Modify: `crates/jev-tray/src/app.rs` (the login caller, and the `Tray::show` parameter name)

- [ ] **Step 1: Change the tests**

In `crates/jev-tray/src/state.rs`, in `mod tests`, replace the test `a_login_error_shows_under_the_state` with:

```rust
    #[test]
    fn a_failure_shows_under_the_state() {
        assert_eq!(
            tooltip(State::On, Some("Start at login failed: access denied")),
            "jevd runs\nStart at login failed: access denied"
        );
        assert_eq!(
            tooltip(State::Off, Some("Start jevd failed: no key")),
            "jevd is not running\nStart jevd failed: no key"
        );
        assert_eq!(
            tooltip(State::On, Some("Stop jevd failed: busy")),
            "jevd runs\nStop jevd failed: busy"
        );
    }
```

In `a_long_tooltip_is_cut_to_the_windows_limit`, replace the line `let long = "x".repeat(500);` with:

```rust
        let long = format!("Stop jevd failed: {}", "x".repeat(500));
```

In `a_cut_never_splits_a_character_of_two_utf16_units`, replace the line `let error = format!("x{}", "\u{1F600}".repeat(100));` with:

```rust
        let error = format!("Start at login failed: x{}", "\u{1F600}".repeat(100));
```

The expected text of that test stays the same: the caller now gives the prefix.

- [ ] **Step 2: Run the tests and see them fail**

Run: `cargo test -p jev-tray --locked --lib state::`
Expected: FAIL in `a_failure_shows_under_the_state`, `a_long_tooltip_is_cut_to_the_windows_limit`, and `a_cut_never_splits_a_character_of_two_utf16_units`. The old `tooltip` adds `Start at login failed: ` a second time.

- [ ] **Step 3: Change `tooltip`**

In `crates/jev-tray/src/state.rs`, replace:

```rust
/// The tooltip: the state, and under it a failed **Start at login** change
/// when there is one, cut to `TOOLTIP_MAX`.
pub fn tooltip(state: State, login_error: Option<&str>) -> String {
    let mut text = match login_error {
        None => state.tooltip().to_string(),
        Some(error) => format!("{}\nStart at login failed: {error}", state.tooltip()),
    };
```

with:

```rust
/// The tooltip: the state, and under it one failure line when there is one,
/// cut to `TOOLTIP_MAX`. The line names what failed, for example
/// `Start at login failed: <error>` or `Stop jevd failed: <error>`.
pub fn tooltip(state: State, failure: Option<&str>) -> String {
    let mut text = match failure {
        None => state.tooltip().to_string(),
        Some(line) => format!("{}\n{line}", state.tooltip()),
    };
```

- [ ] **Step 4: Give the login caller its prefix**

In `crates/jev-tray/src/app.rs`, in `impl Tray`, replace:

```rust
    fn show(&mut self, state: State, shade: Shade, size: u32, login_error: Option<&str>) {
```

with:

```rust
    fn show(&mut self, state: State, shade: Shade, size: u32, failure: Option<&str>) {
```

and replace:

```rust
        let text = state::tooltip(state, login_error);
```

with:

```rust
        let text = state::tooltip(state, failure);
```

In `run`, in the login branch of the menu handler, replace:

```rust
                            tray.show(state, shade, size, Some(&error));
```

with:

```rust
                            let line = format!("Start at login failed: {error}");
                            tray.show(state, shade, size, Some(&line));
```

- [ ] **Step 5: Run the gates**

Run the three gates from "Before you start".
Expected: all pass. `cargo test` gives `test result: ok` for each test binary.

- [ ] **Step 6: Commit**

```bash
git add crates/jev-tray/src/state.rs crates/jev-tray/src/app.rs
git -c user.name="Matthew Brandenburg" -c user.email="mattbran87@users.noreply.github.com" commit -m "refactor(jev-tray): the tooltip takes a whole failure line" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: The control module

**Files:**
- Create: `crates/jev-tray/src/control.rs`
- Modify: `crates/jev-tray/src/lib.rs`
- Modify: `crates/jev-tray/Cargo.toml`

- [ ] **Step 1: Add the Windows feature**

In `crates/jev-tray/Cargo.toml`, in the `windows-sys` features list, add `"Win32_System_Threading"` after `"Win32_System_Registry"`:

```toml
[target.'cfg(windows)'.dependencies]
windows-sys = { version = "0.59", features = [
    "Win32_Foundation",
    "Win32_Storage_FileSystem",
    "Win32_System_IO",
    "Win32_System_Registry",
    "Win32_System_Threading",
    "Win32_UI_WindowsAndMessaging",
] }
```

A new feature of a crate already in the lock file does not change `Cargo.lock`, so `--locked` still passes.

- [ ] **Step 2: Add the module to the crate**

In `crates/jev-tray/src/lib.rs`, replace the crate doc and the module list start:

```rust
//! `jev-tray`: the tray program (in [`app`]), and the parts of it that do
//! not need a tray: the state check, the icon choice, the login entry, the
//! one-copy lock, and the start-up log (the jev-tray spec). The `jev-tray`
//! binary is in the `jev` package, so it ships with `jev` and `jevd`.

pub mod app;
```

with:

```rust
//! `jev-tray`: the tray program (in [`app`]), and the parts of it that do
//! not need a tray: the state check, the icon choice, the login entry, the
//! one-copy lock, the start-up log, and the Start and Stop commands (the
//! jev-tray spec). The `jev-tray` binary is in the `jev` package, so it ships
//! with `jev` and `jevd`.

pub mod app;
pub mod control;
```

- [ ] **Step 3: Write the module with its tests, and stub bodies**

Create `crates/jev-tray/src/control.rs` with this text. The three functions `jev_next_to`, `run_action`, and `run` are stubs for now; Step 5 gives their bodies.

```rust
//! **Start jevd** and **Stop jevd** (the Start and Stop spec): run
//! `jev start` or `jev stop` from the folder of `jev-tray`, with a time limit.
//! `jevd` accepts `stop` only from the `jev` next to it, so the tray cannot
//! send it itself.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::state::State;

/// The longest time one `jev start` or `jev stop` may run.
pub const RUN_LIMIT: Duration = Duration::from_secs(30);

/// How long to wait for the error output after `jev` exits. A program that
/// `jev` started and that still holds the pipe must not hold the tray.
const OUTPUT_WAIT: Duration = Duration::from_secs(1);

/// The time between two checks of a running command.
const POLL: Duration = Duration::from_millis(50);

/// The error when there is no `jev` next to `jev-tray`.
pub const NO_JEV: &str = "cannot find jev next to jev-tray";

/// What the menu item does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Start,
    Stop,
}

impl Action {
    /// The action for the state on show: stop what runs, start what does not.
    pub fn for_state(state: State) -> Self {
        match state {
            State::On => Action::Stop,
            State::Off => Action::Start,
        }
    }

    /// The menu label.
    pub fn label(self) -> &'static str {
        match self {
            Action::Start => "Start jevd",
            Action::Stop => "Stop jevd",
        }
    }

    /// The `jev` command.
    pub fn command(self) -> &'static str {
        match self {
            Action::Start => "start",
            Action::Stop => "stop",
        }
    }
}

/// The path of `jev` in the folder of `program`.
pub fn jev_next_to(program: &Path) -> Option<PathBuf> {
    let _ = program;
    todo!()
}

/// Runs `action` with the `jev` next to `program`, within `limit`.
pub fn run_action(action: Action, program: &Path, limit: Duration) -> Result<(), String> {
    let _ = (action, program, limit);
    todo!()
}

/// Runs `command` with no input, no console window on Windows, and its normal
/// output dropped. Returns `Ok` on exit code 0. Otherwise returns the last
/// line of the error output that is not empty, or the exit code. A command
/// that runs past `limit` is killed.
pub fn run(command: Command, limit: Duration) -> Result<(), String> {
    let _ = (command, limit);
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A command that runs `script` in the system shell.
    fn shell(script: &str) -> Command {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            let mut command = Command::new("cmd");
            command.arg("/C").raw_arg(script);
            command
        }
        #[cfg(not(windows))]
        {
            let mut command = Command::new("sh");
            command.args(["-c", script]);
            command
        }
    }

    /// A command that runs for 30 s, as one process, so a kill ends it.
    fn long() -> Command {
        #[cfg(windows)]
        {
            let mut command = Command::new("powershell");
            command.args(["-NoProfile", "-Command", "Start-Sleep 30"]);
            command
        }
        #[cfg(not(windows))]
        {
            let mut command = Command::new("sleep");
            command.arg("30");
            command
        }
    }

    #[test]
    fn the_action_follows_the_state() {
        assert_eq!(Action::for_state(State::On), Action::Stop);
        assert_eq!(Action::for_state(State::Off), Action::Start);
        assert_eq!(Action::Start.label(), "Start jevd");
        assert_eq!(Action::Stop.label(), "Stop jevd");
        assert_eq!(Action::Start.command(), "start");
        assert_eq!(Action::Stop.command(), "stop");
    }

    #[test]
    fn jev_is_in_the_folder_of_the_program() {
        let dir = Path::new("bin");
        let jev = jev_next_to(&dir.join("jev-tray")).unwrap();
        #[cfg(windows)]
        assert_eq!(jev, dir.join("jev.exe"));
        #[cfg(not(windows))]
        assert_eq!(jev, dir.join("jev"));
    }

    #[test]
    fn no_jev_next_to_the_program_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let program = dir.path().join("jev-tray");
        assert_eq!(
            run_action(Action::Start, &program, RUN_LIMIT),
            Err(NO_JEV.to_string())
        );
    }

    #[test]
    fn exit_code_0_is_success() {
        assert_eq!(run(shell("exit 0"), RUN_LIMIT), Ok(()));
    }

    #[test]
    fn a_failure_gives_the_last_error_line() {
        let result = run(shell("echo first>&2 && echo last>&2 && exit 3"), RUN_LIMIT);
        assert_eq!(result, Err("last".to_string()));
    }

    #[test]
    fn a_failure_with_no_error_output_gives_the_code() {
        let result = run(shell("exit 3"), RUN_LIMIT);
        assert_eq!(result, Err("jev exited with code 3".to_string()));
    }

    #[test]
    fn a_program_that_does_not_exist_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let result = run(Command::new(dir.path().join("missing")), RUN_LIMIT);
        assert!(result.is_err(), "{result:?}");
    }

    #[test]
    fn a_command_past_the_limit_is_killed() {
        let started = Instant::now();
        let result = run(long(), Duration::from_secs(1));
        assert_eq!(result, Err("jev gave no answer in 1 s".to_string()));
        // Far below the command's 30 s, with room for a slow machine.
        assert!(
            started.elapsed() < Duration::from_secs(15),
            "{:?}",
            started.elapsed()
        );
    }
}
```

The stubs leave `Read`, `ExitStatus`, `Stdio`, `mpsc`, `OUTPUT_WAIT`, and `POLL` unused, so the build warns. Step 5 uses them all.

- [ ] **Step 4: Run the tests and see them fail**

Run: `cargo test -p jev-tray --locked --lib control::`
Expected: `the_action_follows_the_state` passes. The other 7 tests FAIL with `not yet implemented`.

- [ ] **Step 5: Write the bodies**

In `crates/jev-tray/src/control.rs`, replace the three stubs, from `/// The path of `jev` in the folder of `program`.` to the end of `run`, with:

```rust
/// The path of `jev` in the folder of `program`.
pub fn jev_next_to(program: &Path) -> Option<PathBuf> {
    let name = format!("jev{}", std::env::consts::EXE_SUFFIX);
    program.parent().map(|dir| dir.join(name))
}

/// Runs `action` with the `jev` next to `program`, within `limit`.
pub fn run_action(action: Action, program: &Path, limit: Duration) -> Result<(), String> {
    let jev = jev_next_to(program)
        .filter(|jev| jev.is_file())
        .ok_or_else(|| NO_JEV.to_string())?;
    let mut command = Command::new(jev);
    command.arg(action.command());
    run(command, limit)
}

/// Runs `command` with no input, no console window on Windows, and its normal
/// output dropped. Returns `Ok` on exit code 0. Otherwise returns the last
/// line of the error output that is not empty, or the exit code. A command
/// that runs past `limit` is killed.
pub fn run(mut command: Command, limit: Duration) -> Result<(), String> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let deadline = Instant::now() + limit;
    let mut child = command.spawn().map_err(|error| error.to_string())?;
    let output = read_all(child.stderr.take());
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("jev gave no answer in {} s", limit.as_secs()));
            }
            Ok(None) => std::thread::sleep(POLL),
            Err(error) => return Err(error.to_string()),
        }
    };
    if status.success() {
        return Ok(());
    }
    let text = output.recv_timeout(OUTPUT_WAIT).unwrap_or_default();
    Err(failure(&text, status))
}

/// Reads `pipe` to its end on its own thread, and sends the text.
fn read_all(pipe: Option<impl Read + Send + 'static>) -> mpsc::Receiver<String> {
    let (done, text) = mpsc::channel();
    if let Some(mut pipe) = pipe {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = pipe.read_to_end(&mut bytes);
            let _ = done.send(String::from_utf8_lossy(&bytes).into_owned());
        });
    }
    text
}

/// The error text for a failed command: its last error line that is not
/// empty, or else its exit code.
fn failure(output: &str, status: ExitStatus) -> String {
    let last = output.lines().map(str::trim).rfind(|line| !line.is_empty());
    match (last, status.code()) {
        (Some(line), _) => line.to_string(),
        (None, Some(code)) => format!("jev exited with code {code}"),
        (None, None) => format!("jev exited: {status}"),
    }
}
```

Why the runner waits at most 1 second for the error output: `jev start` can start `jevd`. `jev_proto::client::start_daemon` gives `jevd` no standard streams, so `jevd` never holds the pipe. The 1-second wait is a second guard: a program that holds the pipe cannot hold the tray.

- [ ] **Step 6: Run the gates**

Run the three gates from "Before you start".
Expected: all pass. The `control::` tests give 8 passed. `a_command_past_the_limit_is_killed` takes about 1 second.

- [ ] **Step 7: Commit**

```bash
git add crates/jev-tray/Cargo.toml crates/jev-tray/src/lib.rs crates/jev-tray/src/control.rs
git -c user.name="Matthew Brandenburg" -c user.email="mattbran87@users.noreply.github.com" commit -m "feat(jev-tray): run jev start and jev stop with a time limit" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: The menu item

**Files:**
- Modify: `crates/jev-tray/src/app.rs`

`app.rs` needs a tray and an event loop, so it has no unit tests. The gates check that it builds and lints, and Task 5 checks it by hand.

- [ ] **Step 1: The module doc and the imports**

Replace:

```rust
//! change. The `jev-tray` binary of the `jev` package calls [`run`].

use std::path::Path;
use std::time::Instant;

use crate::icon::{self, Shade};
```

with:

```rust
//! change. The menu's first item runs `jev start` or `jev stop` (the Start
//! and Stop spec). The `jev-tray` binary of the `jev` package calls [`run`].

use std::path::Path;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::Instant;

use crate::control::{self, Action, RUN_LIMIT};
use crate::icon::{self, Shade};
```

- [ ] **Step 2: The `Ran` message**

Replace:

```rust
    /// A click on a menu item.
    Menu(MenuEvent),
}
```

with:

```rust
    /// A click on a menu item.
    Menu(MenuEvent),
    /// The end of `jev start` or `jev stop`.
    Ran {
        action: Action,
        result: Result<(), String>,
    },
}
```

- [ ] **Step 3: The wake channel, and the command thread**

Replace:

```rust
/// Checks every 5 seconds, until the event loop is gone.
fn watch(paths: Paths, proxy: EventLoopProxy<Message>) {
```

with:

```rust
/// Checks every 5 seconds, and at once on a message from `wake`, until the
/// event loop is gone.
fn watch(paths: Paths, proxy: EventLoopProxy<Message>, wake: Receiver<()>) {
```

Then replace the end of `watch`:

```rust
        std::thread::sleep(CHECK_LIMIT.saturating_sub(started.elapsed()));
    }
}
```

with:

```rust
        match wake.recv_timeout(CHECK_LIMIT.saturating_sub(started.elapsed())) {
            Ok(()) | Err(RecvTimeoutError::Timeout) => {}
            // The event loop is gone.
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

/// Runs `action` on its own thread, and sends the result to the event loop.
fn start_action(action: Action, proxy: &EventLoopProxy<Message>) {
    let sender = proxy.clone();
    let spawned = std::thread::Builder::new()
        .name("jev-tray-control".into())
        .spawn(move || {
            let result = std::env::current_exe()
                .map_err(|error| error.to_string())
                .and_then(|exe| control::run_action(action, &exe, RUN_LIMIT));
            let _ = sender.send_event(Message::Ran { action, result });
        });
    if let Err(error) = spawned {
        let result = Err(format!("cannot start a thread: {error}"));
        let _ = proxy.send_event(Message::Ran { action, result });
    }
}
```

- [ ] **Step 4: The menu item**

In `run`, replace:

```rust
    let login_item = CheckMenuItem::new("Start at login", true, login_on, None);
    let quit_item = MenuItem::new("Quit", true, None);
    let menu = Menu::new();
    menu.append_items(&[&login_item, &quit_item])
```

with:

```rust
    // The state is off until the first check, so the label starts as Start.
    let control_item = MenuItem::new(Action::Start.label(), true, None);
    let login_item = CheckMenuItem::new("Start at login", true, login_on, None);
    let quit_item = MenuItem::new("Quit", true, None);
    let menu = Menu::new();
    menu.append_items(&[&control_item, &login_item, &quit_item])
```

- [ ] **Step 5: Start the watch thread with the wake channel, and keep the new state**

Replace:

```rust
    let proxy = event_loop.create_proxy();
    std::thread::Builder::new()
        .name("jev-tray-watch".into())
        .spawn(move || watch(paths, proxy))
        .unwrap_or_else(|error| fail(log, &format!("cannot start the check thread: {error}")));

    let mut tray: Option<Tray> = None;
```

with:

```rust
    let proxy = event_loop.create_proxy();
    let (wake, woken) = mpsc::channel();
    std::thread::Builder::new()
        .name("jev-tray-watch".into())
        .spawn(move || watch(paths, proxy, woken))
        .unwrap_or_else(|error| fail(log, &format!("cannot start the check thread: {error}")));
    let proxy = event_loop.create_proxy();

    let mut tray: Option<Tray> = None;
    // True while `jev start` or `jev stop` runs.
    let mut running = false;
    // A failed Start or Stop, with the state at that time. It shows until
    // the state changes or the next click on the item.
    let mut failure: Option<(String, State)> = None;
```

- [ ] **Step 6: Any click opens the menu**

In the `Event::NewEvents(StartCause::Init)` branch, replace:

```rust
                let mut builder = TrayIconBuilder::new()
                    .with_tooltip(&text)
                    .with_icon_as_template(icon::TEMPLATE);
```

with:

```rust
                let mut builder = TrayIconBuilder::new()
                    .with_tooltip(&text)
                    .with_icon_as_template(icon::TEMPLATE)
                    // Any click opens the menu, whatever the default.
                    .with_menu_on_left_click(true);
```

- [ ] **Step 7: The check result, and the command result**

Replace the `Checked` branch:

```rust
            Event::UserEvent(Message::Checked { state, shade }) => {
                if let Some(tray) = tray.as_mut() {
                    // A new check ends the display of a login error.
                    tray.show(state, shade, size_now(target), None);
                }
            }
```

with:

```rust
            Event::UserEvent(Message::Checked { state, shade }) => {
                if let Some(tray) = tray.as_mut() {
                    if failure.as_ref().is_some_and(|(_, at)| *at != state) {
                        failure = None;
                    }
                    // A new check ends the display of a login error.
                    let line = failure.as_ref().map(|(line, _)| line.as_str());
                    tray.show(state, shade, size_now(target), line);
                    let label = Action::for_state(state).label();
                    if control_item.text() != label {
                        control_item.set_text(label);
                    }
                }
            }
            Event::UserEvent(Message::Ran { action, result }) => {
                running = false;
                control_item.set_enabled(true);
                if let Err(error) = result {
                    let line = format!("{} failed: {error}", action.label());
                    note(Some(&log_dir), &line);
                    if let Some(tray) = tray.as_mut() {
                        let (_, shade, size) = tray.shown;
                        let state = tray.state;
                        tray.show(state, shade, size, Some(&line));
                        failure = Some((line, state));
                    }
                }
                // Check now, so the icon and the label follow at once.
                let _ = wake.send(());
            }
```

- [ ] **Step 8: The click on the item, and the login failure**

In the `Message::Menu` branch, replace:

```rust
                } else if event.id == *login_item.id() {
                    if let Err(error) = apply_login(&login_item, &login) {
                        login_item.set_checked(!login_item.is_checked());
                        if let Some(tray) = tray.as_mut() {
```

with:

```rust
                } else if event.id == *control_item.id() {
                    if let (false, Some(tray)) = (running, tray.as_ref()) {
                        running = true;
                        control_item.set_enabled(false);
                        failure = None;
                        start_action(Action::for_state(tray.state), &proxy);
                    }
                } else if event.id == *login_item.id() {
                    if let Err(error) = apply_login(&login_item, &login) {
                        login_item.set_checked(!login_item.is_checked());
                        // A login error replaces a failed Start or Stop.
                        failure = None;
                        if let Some(tray) = tray.as_mut() {
```

The rest of the login branch stays as Task 1 left it.

- [ ] **Step 9: Run the gates**

Run the three gates from "Before you start". Also build the binary:

```bash
cargo build -p jev --bins --locked
```

Expected: all pass.

- [ ] **Step 10: Commit**

```bash
git add crates/jev-tray/src/app.rs
git -c user.name="Matthew Brandenburg" -c user.email="mattbran87@users.noreply.github.com" commit -m "feat(jev-tray): Start jevd and Stop jevd in the menu" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: The tray spec and the README

**Files:**
- Modify: `docs/superpowers/specs/2026-11-04-jev-tray-design.md`
- Modify: `README.md`

- [ ] **Step 1: The tray spec**

Make the 5 changes of section 6 of the Start and Stop spec:

1. In the first paragraph, replace `It does nothing else: it never starts, stops, or configures `jevd`.` with:

```markdown
It also starts and stops `jevd` through `jev`, from its menu. It never configures `jevd`.
```

2. In the section 1 table, replace the Menu row with:

```markdown
| Menu | Three items: **Start jevd** or **Stop jevd**, **Start at login**, and **Quit**. See the Start and Stop spec, `2026-11-04-jev-tray-start-stop-design.md`. |
```

3. After the section 1 table, replace `` `jev-tray` never sends an admin message. It needs no pairing and no key.`` with:

```markdown
`jev-tray` never sends an admin message itself. `jev stop` sends `stop`. `jev-tray` needs no pairing and no key.
```

4. In section 5, under Release, replace `` - `jev-tray` needs no particular folder: it does not have to sit next to `jev`.`` with:

```markdown
- The icon works in any folder. **Start jevd** and **Stop jevd** need `jev` in the same folder as `jev-tray`.
```

5. In section 8, delete the line `- Starting or stopping `jevd` from the menu.`

- [ ] **Step 2: The README**

In `README.md`, in the `## jev-tray` section:

1. Replace `` `jev-tray` checks every 5 seconds. It never starts, stops, or configures `jevd`.`` with:

```markdown
`jev-tray` checks every 5 seconds. Its menu can also start and stop `jevd`. It never configures `jevd`.
```

2. Replace:

```markdown
Click the icon to open its menu. The menu has two items:

- **Start at login**:
```

with:

```markdown
Click the icon, with either button, to open its menu. The menu has three items:

- **Start jevd** or **Stop jevd**: the label follows the icon. A click runs `jev start` or `jev stop` from the folder that holds `jev-tray`, so keep `jev` in that folder. If the command fails, the error goes to `jev-tray.log` in the jev state folder. On Windows and macOS, the tooltip also shows it.
- **Start at login**:
```

3. After the `- **Quit**: ...` line, add a blank line and:

```markdown
**Stop jevd** does not last. The next request from a plugin or from `jev` starts `jevd` again.
```

- [ ] **Step 3: Commit**

```bash
git add docs/superpowers/specs/2026-11-04-jev-tray-design.md README.md
git -c user.name="Matthew Brandenburg" -c user.email="mattbran87@users.noreply.github.com" commit -m "docs: Start jevd and Stop jevd in the tray spec and the README" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: Final checks

- [ ] **Step 1: All gates on Windows**

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

Expected: all pass.

- [ ] **Step 2: MSRV**

```bash
cargo +1.85 check --workspace --locked
cargo +1.85 check -p jev --no-default-features --locked
```

Expected: both pass.

- [ ] **Step 3: Linux, in WSL**

From a clone of the branch in WSL, with 2 build jobs, because WSL has little memory:

```bash
CARGO_BUILD_JOBS=2 cargo test -p jev-tray --locked
```

Expected: `test result: ok` for each test binary. The `control::` tests use `sh` and `sleep`.

- [ ] **Step 4: By hand, on Windows, with the user**

1. Quit the running `jev-tray` from its menu.
2. Install the branch: `jev stop`, then at once `cargo install --locked --path crates/jev --force`. A plugin can start `jevd` again between the two commands; if the copy of `jevd.exe` fails, run both again.
3. Start `jev-tray`.
4. With `jevd` on, left-click the icon. The menu opens and shows **Stop jevd**. Click it. Within 1 second, the icon shows the x, and the item reads **Start jevd**. No console window opens.
5. Click **Start jevd**. Within 1 second, the icon shows the check mark, and the item reads **Stop jevd**.
6. Right-click the icon. The menu opens.

- [ ] **Step 5: Push and open the pull request, after the user approves**

Do not push before the user approves. Then push the branch and open a pull request into `main`. End the description with:

```markdown
🤖 Generated with [Claude Code](https://claude.com/claude-code)
```

Hand checks still open after this plan: the same Start and Stop check on macOS and on a Linux desktop, with the hand checks of the tray spec.
