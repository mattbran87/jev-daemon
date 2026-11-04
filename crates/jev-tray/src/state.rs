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
