//! The channel (spec section 2, "Transport"): a Unix socket in a private
//! folder, or a Windows named pipe that only this user can open. Binding
//! fails with `Running` when another daemon holds the channel.

use std::io;
use std::path::PathBuf;

use jev_proto::Paths;

/// Why `bind` failed.
#[derive(Debug)]
pub enum BindError {
    /// Another process holds the channel: another jevd, or on Windows a
    /// squatter that created the pipe first.
    Running,
    Io(io::Error),
}

impl std::fmt::Display for BindError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BindError::Running => {
                f.write_str("another process holds the channel (another jevd, or a squatter)")
            }
            BindError::Io(error) => write!(f, "cannot open the channel: {error}"),
        }
    }
}

impl From<io::Error> for BindError {
    fn from(error: io::Error) -> BindError {
        BindError::Io(error)
    }
}

#[cfg(unix)]
pub use unix::{bind, Listener, Stream};
#[cfg(windows)]
pub use windows::{bind, Listener, Stream};

#[cfg(unix)]
mod unix {
    use super::*;
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    use std::os::unix::io::AsRawFd;

    pub type Stream = tokio::net::UnixStream;

    /// The PID file is a hint: a reader trusts it only while the lock on it
    /// is held. It is never deleted, so the lock always sits on one file.
    pub struct Listener {
        inner: tokio::net::UnixListener,
        socket: PathBuf,
        /// Holds the lock for the life of the daemon.
        lock: std::fs::File,
    }

    /// Takes the lock on the PID file, then binds the socket. The lock makes
    /// the check and the bind one step: of two daemons, only one gets it.
    /// Call it inside a tokio runtime.
    pub fn bind(paths: &Paths) -> Result<Listener, BindError> {
        paths.prepare_runtime_dir()?;
        let pid_file = paths.pid_file();
        let mut lock = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(&pid_file)?;
        // SAFETY: the descriptor is open for the call.
        if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            let error = io::Error::last_os_error();
            return Err(if error.kind() == io::ErrorKind::WouldBlock {
                BindError::Running
            } else {
                BindError::Io(error)
            });
        }
        // `.mode(0o600)` applies only when the file is created.
        lock.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        lock.set_len(0)?;
        writeln!(lock, "{}", std::process::id())?;
        // A socket file left by a daemon that crashed: the lock shows that no
        // daemon uses it.
        match std::fs::remove_file(&paths.socket) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error.into()),
            _ => {}
        }
        let inner = tokio::net::UnixListener::bind(&paths.socket)?;
        Ok(Listener {
            inner,
            socket: paths.socket.clone(),
            lock,
        })
    }

    impl Listener {
        /// The next connection, and the peer's process ID when the OS gives it.
        /// An `Err` is transient: the caller retries after a short pause.
        pub async fn accept(&mut self) -> io::Result<(Stream, Option<u32>)> {
            let (stream, _) = self.inner.accept().await?;
            let pid = stream
                .peer_cred()
                .ok()
                .and_then(|cred| cred.pid())
                .and_then(|pid| u32::try_from(pid).ok());
            Ok((stream, pid))
        }

        /// Deletes the socket file and empties the PID file, then lets the
        /// lock go. The PID file stays: deleting it while locked would let
        /// two daemons lock two different files.
        pub fn close(self) {
            let _ = std::fs::remove_file(&self.socket);
            let _ = self.lock.set_len(0);
        }
    }
}

#[cfg(windows)]
mod windows {
    use super::*;
    use std::ffi::c_void;
    use std::os::windows::io::AsRawHandle;
    use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
    use windows_sys::Win32::Foundation::{LocalFree, ERROR_ACCESS_DENIED, ERROR_PIPE_BUSY, HANDLE};
    use windows_sys::Win32::Security::Authorization::{
        ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
    };
    use windows_sys::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
    use windows_sys::Win32::System::Pipes::GetNamedPipeClientProcessId;

    pub type Stream = NamedPipeServer;

    /// A security descriptor that names this user as owner and gives only
    /// this user access. No inherited entries.
    struct PipeSecurity(PSECURITY_DESCRIPTOR);

    // SAFETY: the descriptor is never changed after it is built, and it is
    // freed once, in `drop`.
    unsafe impl Send for PipeSecurity {}
    unsafe impl Sync for PipeSecurity {}

    impl PipeSecurity {
        fn for_current_user() -> io::Result<PipeSecurity> {
            let sid = jev_proto::os::user_sid()?;
            let sddl: Vec<u16> = format!("O:{sid}D:P(A;;GA;;;{sid})")
                .encode_utf16()
                .chain(Some(0))
                .collect();
            let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
            // SAFETY: `sddl` ends in a 0, and `descriptor` gets a LocalAlloc
            // pointer that `drop` frees.
            let ok = unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    sddl.as_ptr(),
                    SDDL_REVISION_1,
                    &mut descriptor,
                    std::ptr::null_mut(),
                )
            };
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(PipeSecurity(descriptor))
        }

        fn create(&self, name: &std::path::Path, first: bool) -> io::Result<NamedPipeServer> {
            let mut attributes = SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: self.0,
                bInheritHandle: 0,
            };
            // SAFETY: `attributes` and the descriptor outlive the call.
            unsafe {
                ServerOptions::new()
                    .first_pipe_instance(first)
                    .reject_remote_clients(true)
                    .create_with_security_attributes_raw(
                        name,
                        (&mut attributes as *mut SECURITY_ATTRIBUTES).cast::<c_void>(),
                    )
            }
        }
    }

    impl Drop for PipeSecurity {
        fn drop(&mut self) {
            // SAFETY: the pointer came from LocalAlloc, and this is its only free.
            unsafe { LocalFree(self.0.cast()) };
        }
    }

    pub struct Listener {
        name: PathBuf,
        security: PipeSecurity,
        /// The instance that waits for the next client.
        next: NamedPipeServer,
        pid_file: PathBuf,
    }

    /// Creates the first pipe instance. `FILE_FLAG_FIRST_PIPE_INSTANCE`
    /// makes this fail when any process holds the name, so jevd never joins
    /// a pipe that someone else created (the client's owner check catches a
    /// squatter), and of two daemons only one binds. Call it inside a tokio
    /// runtime.
    pub fn bind(paths: &Paths) -> Result<Listener, BindError> {
        paths.prepare_runtime_dir()?;
        let security = PipeSecurity::for_current_user()?;
        let next = match security.create(&paths.socket, true) {
            Ok(next) => next,
            Err(error)
                if [ERROR_ACCESS_DENIED, ERROR_PIPE_BUSY]
                    .map(|code| code as i32)
                    .contains(&error.raw_os_error().unwrap_or(0)) =>
            {
                return Err(BindError::Running)
            }
            Err(error) => return Err(error.into()),
        };
        let pid_file = paths.pid_file();
        write_pid(&pid_file)?;
        Ok(Listener {
            name: paths.socket.clone(),
            security,
            next,
            pid_file,
        })
    }

    fn write_pid(pid_file: &std::path::Path) -> io::Result<()> {
        crate::files::write_atomic(pid_file, &format!("{}\n", std::process::id()))
    }

    impl Listener {
        /// The next connection, and the peer's process ID.
        /// An `Err` is transient: the caller retries after a short pause.
        pub async fn accept(&mut self) -> io::Result<(Stream, Option<u32>)> {
            if let Err(error) = self.next.connect().await {
                // That instance may be broken now; wait on a new one.
                self.next = self.security.create(&self.name, false)?;
                return Err(error);
            }
            // If this fails, the connected client stays in `self.next`, and
            // the next `accept` returns it: tokio treats an already
            // connected instance as connected.
            let fresh = self.security.create(&self.name, false)?;
            let stream = std::mem::replace(&mut self.next, fresh);
            let mut pid = 0u32;
            // SAFETY: the handle is a connected pipe server for the call.
            let ok =
                unsafe { GetNamedPipeClientProcessId(stream.as_raw_handle() as HANDLE, &mut pid) };
            Ok((stream, (ok != 0).then_some(pid)))
        }

        /// Deletes the PID file. The pipe goes away with its last handle.
        pub fn close(self) {
            let _ = std::fs::remove_file(&self.pid_file);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[cfg(unix)]
    async fn dial(paths: &Paths) -> io::Result<impl tokio::io::AsyncRead + tokio::io::AsyncWrite> {
        tokio::net::UnixStream::connect(&paths.socket).await
    }

    #[cfg(windows)]
    async fn dial(paths: &Paths) -> io::Result<impl tokio::io::AsyncRead + tokio::io::AsyncWrite> {
        tokio::net::windows::named_pipe::ClientOptions::new().open(&paths.socket)
    }

    #[tokio::test]
    async fn a_second_bind_finds_the_first_daemon() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::under(dir.path()).unwrap();
        let first = bind(&paths).unwrap();
        assert!(matches!(bind(&paths), Err(BindError::Running)));
        first.close();
        let again = bind(&paths).expect("the channel is free after close");
        again.close();
    }

    #[tokio::test]
    async fn accept_gives_the_stream_and_this_process_id() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::under(dir.path()).unwrap();
        let mut listener = bind(&paths).unwrap();
        let mut client = dial(&paths).await.unwrap();
        let (mut stream, pid) = listener.accept().await.unwrap();
        assert_eq!(pid, Some(std::process::id()));
        client.write_all(b"ping\n").await.unwrap();
        let mut buffer = [0u8; 5];
        stream.read_exact(&mut buffer).await.unwrap();
        assert_eq!(&buffer, b"ping\n");
        let mut second = dial(&paths).await.expect("a new instance waits");
        let (mut second_stream, second_pid) = listener.accept().await.unwrap();
        assert_eq!(second_pid, Some(std::process::id()));
        second.write_all(b"x").await.unwrap();
        let mut one = [0u8; 1];
        second_stream.read_exact(&mut one).await.unwrap();
        assert_eq!(&one, b"x");
        listener.close();
    }

    #[tokio::test]
    async fn the_pid_file_names_this_process() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::under(dir.path()).unwrap();
        let listener = bind(&paths).unwrap();
        let text = std::fs::read_to_string(paths.pid_file()).unwrap();
        assert_eq!(text.trim(), std::process::id().to_string());
        listener.close();
        #[cfg(unix)]
        assert_eq!(std::fs::read_to_string(paths.pid_file()).unwrap(), "");
        #[cfg(windows)]
        assert!(!paths.pid_file().exists());
    }

    #[cfg(windows)]
    #[tokio::test]
    // A non-elevated creator is the default owner anyway, so this test guards
    // the elevated case, where the default owner would be BUILTIN\Administrators.
    async fn the_pipe_is_owned_by_this_user() {
        use std::os::windows::io::AsRawHandle;
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::under(dir.path()).unwrap();
        let listener = bind(&paths).unwrap();
        let client = tokio::net::windows::named_pipe::ClientOptions::new()
            .open(&paths.socket)
            .unwrap();
        assert!(jev_proto::os::owned_by_current_user(client.as_raw_handle()).unwrap());
        listener.close();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn junk_in_an_unlocked_pid_file_is_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::under(dir.path()).unwrap();
        paths.prepare_runtime_dir().unwrap();
        std::fs::write(paths.pid_file(), "junk that is long\n").unwrap();
        let listener = bind(&paths).unwrap();
        let text = std::fs::read_to_string(paths.pid_file()).unwrap();
        assert_eq!(text.trim(), std::process::id().to_string());
        listener.close();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_stale_socket_file_is_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::under(dir.path()).unwrap();
        paths.prepare_runtime_dir().unwrap();
        std::fs::write(&paths.socket, "stale").unwrap();
        let listener = bind(&paths).unwrap();
        dial(&paths).await.unwrap();
        listener.close();
        assert!(!paths.socket.exists());
    }
}
