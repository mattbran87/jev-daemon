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
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error.to_string());
            }
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
