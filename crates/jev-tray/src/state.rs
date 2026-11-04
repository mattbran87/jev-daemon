//! One check of `jevd` (jev-tray spec section 2): connect if a daemon runs,
//! send `status`, and close the connection.

use std::sync::mpsc;
use std::thread::JoinHandle;
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

impl State {
    /// The tooltip text for this state.
    pub fn tooltip(self) -> &'static str {
        match self {
            State::On => "jevd runs",
            State::Off => "jevd is not running",
        }
    }
}

/// The longest tooltip, in UTF-16 units: Windows keeps 128 with the zero.
pub const TOOLTIP_MAX: usize = 127;

/// The tooltip: the state, and under it a failed **Start at login** change
/// when there is one, cut to `TOOLTIP_MAX`.
pub fn tooltip(state: State, login_error: Option<&str>) -> String {
    let mut text = match login_error {
        None => state.tooltip().to_string(),
        Some(error) => format!("{}\nStart at login failed: {error}", state.tooltip()),
    };
    while text.encode_utf16().count() > TOOLTIP_MAX {
        text.pop();
    }
    text
}

/// One check, with the 5 s limit. For checks that repeat, use [`Checker`].
pub fn check(paths: &Paths) -> State {
    check_within(paths, CHECK_LIMIT)
}

/// One check that counts as off when it takes longer than `limit`.
///
/// The check runs on its own thread, so a daemon that never answers cannot
/// hold the caller past `limit`: the caller gets its answer at the deadline.
/// The thread itself can live longer. `hello` waits up to 5 s whatever the
/// limit, and on Unix a connect to a daemon that does not accept can block
/// with no limit. For checks that repeat, use [`Checker`].
pub fn check_within(paths: &Paths, limit: Duration) -> State {
    run_check(paths, limit).0
}

/// Checks that repeat, one at a time.
///
/// A check that hangs (see [`check_within`]) must not make each later check
/// add one more thread. While the last check's thread still runs, `check`
/// returns [`State::Off`] at once and starts nothing, so there is at most one
/// check thread besides the caller's.
#[derive(Debug, Default)]
pub struct Checker {
    last: Option<JoinHandle<()>>,
}

impl Checker {
    /// A checker with no check in flight.
    pub fn new() -> Self {
        Self::default()
    }

    /// One check, as [`check_within`] does, unless the last one still runs.
    pub fn check(&mut self, paths: &Paths, limit: Duration) -> State {
        if self.last.as_ref().is_some_and(|last| !last.is_finished()) {
            return State::Off;
        }
        let (state, thread) = run_check(paths, limit);
        self.last = thread;
        state
    }
}

/// Runs one check on its own thread and waits for it until the deadline.
/// Returns the state, and the thread when it still runs after the deadline.
fn run_check(paths: &Paths, limit: Duration) -> (State, Option<JoinHandle<()>>) {
    let deadline = Instant::now() + limit;
    let paths = paths.clone();
    let (done, result) = mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("jev-tray-check".into())
        .spawn(move || {
            let _ = done.send(check_now(&paths, deadline));
        });
    let Ok(thread) = spawned else {
        return (State::Off, None);
    };
    match result.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
        // The thread sends its result before it drops its connection, so
        // wait for it to end: a later check must not find it still running.
        Ok(state) => {
            let _ = thread.join();
            (state, None)
        }
        Err(_) => (State::Off, Some(thread)),
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tooltip_names_the_state() {
        assert_eq!(tooltip(State::On, None), "jevd runs");
        assert_eq!(tooltip(State::Off, None), "jevd is not running");
    }

    #[test]
    fn a_login_error_shows_under_the_state() {
        assert_eq!(
            tooltip(State::On, Some("access denied")),
            "jevd runs\nStart at login failed: access denied"
        );
    }

    #[test]
    fn a_long_tooltip_is_cut_to_the_windows_limit() {
        let long = "x".repeat(500);
        let text = tooltip(State::Off, Some(&long));
        assert_eq!(text.encode_utf16().count(), TOOLTIP_MAX);
        assert!(text.starts_with("jevd is not running\n"));
    }
}
