//! A small blocking client for jevd (spec section 1, "Repo layout"): it
//! connects, checks the channel, sends `hello`, then sends one request at a
//! time. `connect_or_start` starts `jevd` when no daemon runs.
//!
//! Every failure is a `WireError`, so a caller handles a local failure and a
//! daemon's reply the same way. The client never logs a reply, because a
//! `pair-status` or `approve` reply can carry a token.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::Child;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::paths::HOME_VAR;
use crate::{ErrorCode, Paths, Reply, Request, WireError, PROTOCOL_VERSION};

/// How long `connect_or_start` waits for a daemon it started. A cold start
/// on Windows can take a while, because of a virus scan.
pub const START_WAIT: Duration = Duration::from_secs(10);
/// How long `hello` waits for its reply.
pub const HELLO_TIMEOUT: Duration = Duration::from_secs(5);
/// How long a call waits for its reply. One `ask` can take about 31 s in Jev
/// alone, plus a keyring prompt, so this is well above that.
pub const REPLY_TIMEOUT: Duration = Duration::from_secs(120);
/// The longest reply line, newline not counted.
pub const MAX_REPLY: usize = 2 << 20;

#[cfg(unix)]
type Stream = std::os::unix::net::UnixStream;
#[cfg(windows)]
type Stream = std::fs::File;

fn unavailable(message: impl Into<String>) -> WireError {
    WireError::new(ErrorCode::DaemonUnavailable, message)
}

type Outcome = Result<Vec<u8>, WireError>;

/// Why a connect failed. Only `NoChannel` means that no daemon runs, so only
/// it may start one. Any other failure goes back to the caller as it is: a
/// daemon that is busy, stopping, or unsafe to talk to is not a missing one.
enum Failed {
    NoChannel(WireError),
    Other(WireError),
}

impl Failed {
    fn into_error(self) -> WireError {
        match self {
            Failed::NoChannel(error) | Failed::Other(error) => error,
        }
    }
}

/// One open connection, past `hello`. A worker thread owns the stream and
/// does each write and then its read. On Windows, a blocking read on a pipe
/// blocks a write on a copy of the same handle, so one thread does both.
///
/// jevd closes a connection after 60 s with no message, so a long-lived
/// connection gets `daemon-unavailable` on its next call. Then reconnect, and
/// do not send a request that changes state again without checking first.
///
/// On Unix, a client that resets SIGPIPE to its default can die when it
/// writes to a socket that jevd closed. Rust binaries ignore SIGPIPE by
/// default, so this holds only for a program that changes that.
pub struct Connection {
    jobs: Option<mpsc::Sender<Vec<u8>>>,
    replies: mpsc::Receiver<Outcome>,
    timeout: Duration,
    /// Set after a timeout: a late reply would answer the wrong call.
    timed_out: bool,
    /// A second handle to the socket, to cut it from outside the worker.
    #[cfg(unix)]
    cut: Stream,
    /// The worker, so a blocked read can be cancelled.
    #[cfg(windows)]
    worker: std::thread::JoinHandle<()>,
}

impl Connection {
    /// Connects to a running daemon, checks the channel, and sends `hello`.
    /// No daemon gives `daemon-unavailable`.
    pub fn connect(paths: &Paths) -> Result<Connection, WireError> {
        connect_inner(paths).map_err(Failed::into_error)
    }

    /// As `connect`, but no daemon gives `Ok(None)`. Only a missing channel
    /// means no daemon: a daemon that is busy, full, stopping, or unsafe to
    /// talk to is an error.
    pub fn connect_if_running(paths: &Paths) -> Result<Option<Connection>, WireError> {
        match connect_inner(paths) {
            Ok(connection) => Ok(Some(connection)),
            Err(Failed::NoChannel(_)) => Ok(None),
            Err(Failed::Other(error)) => Err(error),
        }
    }

    /// Changes how long each call waits for its reply.
    pub fn set_timeout(&mut self, timeout: Duration) {
        self.timeout = timeout;
    }

    /// Sends one request and waits for its reply. A reply with `ok: false`
    /// comes back as `Ok`, so the caller sees the daemon's error as sent.
    ///
    /// A `too-large` reply from jevd is best effort: jevd closes the
    /// connection after it, and a client still writing the rest of an
    /// over-long line may never read it.
    pub fn call(&mut self, request: &Request) -> Result<Reply, WireError> {
        if self.timed_out {
            return Err(unavailable("this connection timed out; open a new one"));
        }
        let mut line = serde_json::to_vec(request).expect("a request always serializes");
        line.push(b'\n');
        match &self.jobs {
            Some(jobs) if jobs.send(line).is_ok() => {}
            _ => return Err(unavailable("jevd closed the connection")),
        }
        let bytes = match self.replies.recv_timeout(self.timeout) {
            Ok(outcome) => outcome?,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                self.timed_out = true;
                self.cut();
                return Err(unavailable("jevd did not answer in time"));
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(unavailable("jevd closed the connection"))
            }
        };
        serde_json::from_slice(&bytes).map_err(|_| {
            WireError::new(ErrorCode::Internal, "jevd sent a reply that cannot be read")
        })
    }

    /// Ends the worker, so jevd can free the connection: the worker's loop
    /// ends when its job channel closes, and the read it may be blocked in is
    /// cut.
    fn cut(&mut self) {
        self.jobs = None;
        #[cfg(unix)]
        {
            let _ = self.cut.shutdown(std::net::Shutdown::Both);
        }
        #[cfg(windows)]
        if self.timed_out {
            use std::os::windows::io::AsRawHandle;
            use windows_sys::Win32::System::IO::CancelSynchronousIo;
            // The cancel fails when the worker is not inside an I/O call
            // yet, so try again until it has ended.
            for _ in 0..20 {
                if self.worker.is_finished() {
                    break;
                }
                // SAFETY: the handle belongs to a thread that this struct
                // still owns, and the call has no other precondition.
                unsafe { CancelSynchronousIo(self.worker.as_raw_handle()) };
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.cut();
    }
}

fn connect_inner(paths: &Paths) -> Result<Connection, Failed> {
    let stream = open(paths)?;
    #[cfg(unix)]
    let cut = stream
        .try_clone()
        .map_err(|error| Failed::Other(unavailable(format!("cannot use the socket: {error}"))))?;
    let (jobs, inbox) = mpsc::channel();
    let (outbox, replies) = mpsc::channel();
    let worker = std::thread::Builder::new()
        .name("jev-client".into())
        .spawn(move || work(stream, inbox, outbox))
        .map_err(|error| Failed::Other(unavailable(format!("cannot start a thread: {error}"))))?;
    #[cfg(unix)]
    drop(worker);
    let mut connection = Connection {
        jobs: Some(jobs),
        replies,
        timeout: HELLO_TIMEOUT,
        timed_out: false,
        #[cfg(unix)]
        cut,
        #[cfg(windows)]
        worker,
    };
    let reply = connection
        .call(&Request::Hello {
            protocol: PROTOCOL_VERSION,
        })
        .map_err(Failed::Other)?;
    if !reply.ok {
        return Err(Failed::Other(reply.error.unwrap_or_else(|| {
            WireError::new(ErrorCode::Internal, "jevd refused hello without an error")
        })));
    }
    if reply.data.get("protocol").and_then(|value| value.as_u64())
        != Some(u64::from(PROTOCOL_VERSION))
    {
        return Err(Failed::Other(WireError::new(
            ErrorCode::ProtocolUnsupported,
            "jevd answered hello with another protocol",
        )));
    }
    connection.timeout = REPLY_TIMEOUT;
    Ok(connection)
}

/// For each job: write the line, then read one reply line. Ends when the
/// connection or the channel closes.
fn work(stream: Stream, jobs: mpsc::Receiver<Vec<u8>>, replies: mpsc::Sender<Outcome>) {
    let mut reader = BufReader::new(stream);
    for line in jobs {
        let sent = reader
            .get_mut()
            .write_all(&line)
            .and_then(|()| reader.get_mut().flush());
        let outcome = match sent {
            Ok(()) => read_reply(&mut reader),
            Err(error) => Err(unavailable(format!("cannot send to jevd: {error}"))),
        };
        let stop = outcome.is_err();
        if replies.send(outcome).is_err() || stop {
            return;
        }
    }
}

/// Reads one reply line, without its newline.
fn read_reply<R: BufRead>(reader: &mut R) -> Outcome {
    let mut line = Vec::new();
    let limit = MAX_REPLY as u64 + 1;
    match reader.by_ref().take(limit).read_until(b'\n', &mut line) {
        Ok(0) => return Err(unavailable("jevd closed the connection")),
        Ok(_) => {}
        Err(error) => return Err(unavailable(format!("cannot read from jevd: {error}"))),
    }
    if line.last() == Some(&b'\n') {
        line.pop();
        return Ok(line);
    }
    if line.len() > MAX_REPLY {
        // Not `too-large`: that code blames the caller's request.
        return Err(WireError::new(
            ErrorCode::Internal,
            "jevd sent a reply over 2 MiB",
        ));
    }
    Err(unavailable(
        "jevd closed the connection in the middle of a reply",
    ))
}

#[cfg(unix)]
fn open(paths: &Paths) -> Result<Stream, Failed> {
    use std::io::ErrorKind;
    if let Err(error) = paths.check_runtime_dir() {
        return Err(if error.kind() == ErrorKind::NotFound {
            Failed::NoChannel(unavailable(format!("jevd is not running: {error}")))
        } else {
            Failed::Other(unavailable(format!(
                "the jevd runtime folder is not safe to use: {error}"
            )))
        });
    }
    // On macOS, a full listen backlog also gives `ConnectionRefused`. That
    // starts a jevd that exits 0 (another daemon runs), and the wait loop
    // connects anyway.
    Stream::connect(&paths.socket).map_err(|error| {
        if matches!(
            error.kind(),
            ErrorKind::NotFound | ErrorKind::ConnectionRefused
        ) {
            Failed::NoChannel(unavailable(format!("jevd is not running: {error}")))
        } else {
            Failed::Other(unavailable(format!("cannot connect to jevd: {error}")))
        }
    })
}

#[cfg(windows)]
fn open(paths: &Paths) -> Result<Stream, Failed> {
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PIPE_BUSY};
    use windows_sys::Win32::Storage::FileSystem::SECURITY_IDENTIFICATION;
    // Every pipe instance can be busy for a moment, between two accepts.
    let deadline = Instant::now() + Duration::from_secs(2);
    let file = loop {
        match std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            // A server that squatted the pipe can then at most identify this
            // client, never impersonate it. std adds SECURITY_SQOS_PRESENT.
            .security_qos_flags(SECURITY_IDENTIFICATION)
            .open(&paths.socket)
        {
            Ok(file) => break file,
            Err(error) => match error.raw_os_error() {
                Some(code) if code == ERROR_PIPE_BUSY as i32 => {
                    if Instant::now() >= deadline {
                        return Err(Failed::Other(unavailable(
                            "jevd is busy: every pipe instance is in use",
                        )));
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
                Some(code) if code == ERROR_FILE_NOT_FOUND as i32 => {
                    return Err(Failed::NoChannel(unavailable(format!(
                        "jevd is not running: {error}"
                    ))));
                }
                _ => {
                    return Err(Failed::Other(unavailable(format!(
                        "cannot open the jevd pipe: {error}"
                    ))));
                }
            },
        }
    };
    // A pipe that another user created first is a squatter.
    match crate::os::owned_by_current_user(file.as_raw_handle()) {
        Ok(true) => Ok(file),
        Ok(false) => Err(Failed::Other(unavailable(
            "the jevd pipe belongs to another user",
        ))),
        Err(error) => Err(Failed::Other(unavailable(format!(
            "cannot check the jevd pipe: {error}"
        )))),
    }
}

/// Clears the inherit flag on some handles while it lives, and sets it again
/// on drop, also when the code in between fails. It changes only the handles
/// that had the flag, and skips a null or invalid handle.
///
/// Windows starts every child with handle inheritance on, so a started jevd
/// would get the caller's own standard handles. A caller that reads this
/// process's output through a pipe (a script's `$(jev start)`, a Node
/// `spawn`) would then wait until jevd exits.
#[cfg(windows)]
struct NoInherit {
    cleared: Vec<windows_sys::Win32::Foundation::HANDLE>,
}

#[cfg(windows)]
impl NoInherit {
    /// The guard on the three standard handles of this process.
    fn std_handles() -> NoInherit {
        use windows_sys::Win32::System::Console::{
            GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
        };
        // SAFETY: GetStdHandle has no preconditions.
        let handles = [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE]
            .map(|which| unsafe { GetStdHandle(which) });
        NoInherit::on(&handles)
    }

    fn on(handles: &[windows_sys::Win32::Foundation::HANDLE]) -> NoInherit {
        use windows_sys::Win32::Foundation::{
            GetHandleInformation, SetHandleInformation, HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE,
        };
        let mut cleared = Vec::new();
        for &handle in handles {
            // Standard output and error are often the same handle.
            if handle.is_null() || handle == INVALID_HANDLE_VALUE || cleared.contains(&handle) {
                continue;
            }
            let mut flags = 0;
            // SAFETY: both calls only read or change the flags of a handle,
            // and fail on a handle that is not valid.
            let was_inheritable = unsafe {
                GetHandleInformation(handle, &mut flags) != 0
                    && flags & HANDLE_FLAG_INHERIT != 0
                    && SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0) != 0
            };
            if was_inheritable {
                cleared.push(handle);
            }
        }
        NoInherit { cleared }
    }
}

#[cfg(windows)]
impl Drop for NoInherit {
    fn drop(&mut self) {
        use windows_sys::Win32::Foundation::{SetHandleInformation, HANDLE_FLAG_INHERIT};
        // This sets the flag again even if the caller changed it during the
        // spawn; that is accepted, because the window is only the spawn (one
        // CreateProcess call, or two after the breakaway fallback).
        for &handle in &self.cleared {
            // SAFETY: as in `on`; the handle had the flag when the guard
            // was made.
            unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT) };
        }
    }
}

/// Starts `jevd` detached: no console window on Windows, a new session on
/// Unix, and no standard streams. It does not wait for the daemon. The
/// daemon gets `paths.home` as `JEV_HOME`, and runs in its own folder, so a
/// long-lived `jevd` does not hold the caller's folder; a bare program name
/// found on the search path is the exception, and the working folder is not
/// changed.
///
/// The started `jevd` stays a child of the caller (`setsid` does not change
/// that). A long-lived caller holds a zombie after `jevd` exits, until it
/// reaps the returned `Child` (`try_wait`) or exits.
///
/// On Windows, the caller's standard handles are not inheritable only while
/// `jevd` is spawned; then their flags come back. Other inheritable handles
/// of the caller (for example extra pipes from a Node parent) still reach
/// `jevd`. TODO: use `CommandExt::inherit_handles(false)` once it is stable.
pub fn start_daemon(jevd: &Path, paths: &Paths) -> std::io::Result<Child> {
    use std::process::{Command, Stdio};
    // A relative path with a folder in it would not survive the change of
    // the working folder. A bare name is left to the search path.
    let has_folder = jevd.parent().is_some_and(|dir| !dir.as_os_str().is_empty());
    let jevd = if has_folder {
        std::path::absolute(jevd).unwrap_or_else(|_| jevd.to_path_buf())
    } else {
        jevd.to_path_buf()
    };
    let mut command = Command::new(&jevd);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    match &paths.home {
        Some(home) => command.env(HOME_VAR, home),
        // The child must match `paths`, not the caller's environment.
        None => command.env_remove(HOME_VAR),
    };
    if let Some(dir) = jevd.parent().filter(|dir| !dir.as_os_str().is_empty()) {
        command.current_dir(dir);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Caller descriptors 3 and up without close-on-exec still reach
        // jevd. Rust's own descriptors have the flag; closing the others is
        // out of scope for now.
        // SAFETY: setsid is async-signal-safe, so it may run between fork and exec.
        unsafe {
            command.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
        command.spawn()
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use windows_sys::Win32::Foundation::ERROR_ACCESS_DENIED;
        use windows_sys::Win32::System::Threading::{
            CREATE_BREAKAWAY_FROM_JOB, CREATE_NEW_PROCESS_GROUP, DETACHED_PROCESS,
        };
        let _no_inherit = NoInherit::std_handles();
        let detached = DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP;
        // Leave the caller's job, so the daemon outlives a terminal that
        // closes its job. A job that forbids this refuses the flag with
        // access denied; only then try again without it. In a job that
        // kills its processes when it closes (some terminals, CI, ssh), the
        // fallback daemon ends with the job, and the next command starts it
        // again.
        command.creation_flags(detached | CREATE_BREAKAWAY_FROM_JOB);
        match command.spawn() {
            Err(error) if error.raw_os_error() == Some(ERROR_ACCESS_DENIED as i32) => {
                command.creation_flags(detached);
                command.spawn()
            }
            other => other,
        }
    }
}

fn exited_at_start(status: std::process::ExitStatus, paths: &Paths) -> WireError {
    unavailable(format!(
        "jevd exited with status {} at start; see its log in {}",
        status
            .code()
            .map_or_else(|| "unknown".to_string(), |code| code.to_string()),
        paths.state_dir.display()
    ))
}

/// Connects, and starts `jevd` first when there is no channel. Waits up to
/// `START_WAIT` for the new daemon. Only a missing channel starts one: a
/// daemon that is busy, stopping, or unsafe to talk to is not started again.
/// After it started a `jevd`, a failure other than "no channel" can wait up
/// to 1 s more, to report a `jevd` that exited at start.
pub fn connect_or_start(paths: &Paths, jevd: &Path) -> Result<Connection, WireError> {
    match connect_inner(paths) {
        Ok(connection) => return Ok(connection),
        Err(Failed::NoChannel(_)) => {}
        Err(Failed::Other(error)) => return Err(error),
    }
    let mut child = start_daemon(jevd, paths)
        .map_err(|error| unavailable(format!("cannot start jevd: {error}")))?;
    let deadline = Instant::now() + START_WAIT;
    loop {
        std::thread::sleep(Duration::from_millis(50));
        // A zero exit means that another daemon won the race, so keep waiting.
        if let Ok(Some(status)) = child.try_wait() {
            if !status.success() {
                return Err(exited_at_start(status, paths));
            }
        }
        match connect_inner(paths) {
            Ok(connection) => {
                // Reap the child when it has already exited.
                let _ = child.try_wait();
                return Ok(connection);
            }
            Err(Failed::NoChannel(error)) => {
                if Instant::now() >= deadline {
                    let _ = child.try_wait();
                    return Err(error);
                }
            }
            Err(Failed::Other(error)) => {
                // jevd binds its channel before it loads its settings, so
                // this may be a daemon that is about to exit. Give it a
                // moment, and report its exit in place of the closed
                // connection.
                let wait_until = Instant::now() + Duration::from_secs(1);
                loop {
                    match child.try_wait() {
                        Ok(Some(status)) if !status.success() => {
                            return Err(exited_at_start(status, paths));
                        }
                        Ok(Some(_)) | Err(_) => return Err(error),
                        Ok(None) if Instant::now() >= wait_until => return Err(error),
                        Ok(None) => std::thread::sleep(Duration::from_millis(20)),
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn no_daemon_is_daemon_unavailable() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::under(dir.path()).unwrap();
        paths.prepare_runtime_dir().unwrap();
        let error = Connection::connect(&paths).err().unwrap();
        assert_eq!(error.code.known(), Some(ErrorCode::DaemonUnavailable));
    }

    #[test]
    fn connect_if_running_with_no_daemon_is_none() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::under(dir.path()).unwrap();
        assert!(Connection::connect_if_running(&paths).unwrap().is_none());
        paths.prepare_runtime_dir().unwrap();
        assert!(Connection::connect_if_running(&paths).unwrap().is_none());
    }

    #[test]
    fn a_missing_jevd_cannot_start() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::under(dir.path()).unwrap();
        paths.prepare_runtime_dir().unwrap();
        let error = connect_or_start(&paths, &dir.path().join("no-such-jevd"))
            .err()
            .unwrap();
        assert_eq!(error.code.known(), Some(ErrorCode::DaemonUnavailable));
        assert!(
            error.message.contains("cannot start jevd"),
            "{}",
            error.message
        );
    }

    #[test]
    fn start_daemon_reports_a_missing_binary() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::under(dir.path()).unwrap();
        assert!(start_daemon(&dir.path().join("no-such-jevd"), &paths).is_err());
    }

    /// The guard runs on pipe ends that this test makes, not on the standard
    /// handles, so other tests in this process see no change.
    #[cfg(windows)]
    #[test]
    fn no_inherit_clears_the_flag_while_alive_and_restores_it_on_drop() {
        use windows_sys::Win32::Foundation::{
            CloseHandle, GetHandleInformation, SetHandleInformation, HANDLE, HANDLE_FLAG_INHERIT,
            INVALID_HANDLE_VALUE,
        };
        use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
        use windows_sys::Win32::System::Pipes::CreatePipe;
        fn inheritable(handle: HANDLE) -> bool {
            let mut flags = 0;
            // SAFETY: the handle is a pipe end that the test owns.
            assert_ne!(unsafe { GetHandleInformation(handle, &mut flags) }, 0);
            flags & HANDLE_FLAG_INHERIT != 0
        }
        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: std::ptr::null_mut(),
            bInheritHandle: 1,
        };
        let (mut read, mut write): (HANDLE, HANDLE) = (std::ptr::null_mut(), std::ptr::null_mut());
        // SAFETY: the out pointers and the attributes are valid for the call.
        assert_ne!(
            unsafe { CreatePipe(&mut read, &mut write, &attributes, 0) },
            0
        );
        // The write end starts without the flag, so the guard must leave it.
        // SAFETY: the handle is a pipe end that the test owns.
        assert_ne!(
            unsafe { SetHandleInformation(write, HANDLE_FLAG_INHERIT, 0) },
            0
        );
        assert!(inheritable(read));
        {
            let guard = NoInherit::on(&[
                std::ptr::null_mut(),
                INVALID_HANDLE_VALUE,
                read,
                read,
                write,
            ]);
            assert_eq!(guard.cleared, vec![read]);
            assert!(!inheritable(read));
            assert!(!inheritable(write));
        }
        assert!(inheritable(read), "the guard restores the flag on drop");
        assert!(!inheritable(write), "the guard sets no flag that was clear");
        // SAFETY: the test owns both ends and uses them no more.
        unsafe {
            CloseHandle(read);
            CloseHandle(write);
        }
    }

    #[test]
    fn a_partial_line_at_eof_is_a_closed_connection() {
        let mut reader = Cursor::new(b"{\"ok\":tr".to_vec());
        let error = read_reply(&mut reader).unwrap_err();
        assert_eq!(error.code.known(), Some(ErrorCode::DaemonUnavailable));
        assert!(
            error.message.contains("middle of a reply"),
            "{}",
            error.message
        );
    }

    #[test]
    fn a_line_of_exactly_the_cap_is_accepted() {
        let mut bytes = vec![b'x'; MAX_REPLY];
        bytes.push(b'\n');
        let line = read_reply(&mut Cursor::new(bytes)).unwrap();
        assert_eq!(line.len(), MAX_REPLY);
    }

    #[test]
    fn a_line_over_the_cap_is_internal() {
        let mut reader = Cursor::new(vec![b'x'; MAX_REPLY + 1]);
        let error = read_reply(&mut reader).unwrap_err();
        assert_eq!(error.code.known(), Some(ErrorCode::Internal));
        assert!(error.message.contains("over 2 MiB"), "{}", error.message);
    }

    #[test]
    fn an_empty_stream_is_a_closed_connection() {
        let error = read_reply(&mut Cursor::new(Vec::new())).unwrap_err();
        assert_eq!(error.code.known(), Some(ErrorCode::DaemonUnavailable));
    }

    #[test]
    fn two_replies_in_one_buffer_are_read_one_at_a_time() {
        let mut reader = Cursor::new(b"first\nsecond\n".to_vec());
        assert_eq!(read_reply(&mut reader).unwrap(), b"first");
        assert_eq!(read_reply(&mut reader).unwrap(), b"second");
        assert!(read_reply(&mut reader).is_err());
    }
}
