//! The peer check (spec section 2, "Transport"): admin messages count only
//! from a peer process whose full path is exactly the `jev` next to `jevd`.

use std::io;
use std::path::{Path, PathBuf};

use crate::daemon::Peer;

/// The `jev` binary in the same folder as the running `jevd`.
pub fn jev_next_to_me() -> io::Result<PathBuf> {
    let me = std::env::current_exe()?;
    let folder = me
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "jevd has no folder"))?;
    Ok(folder.join(format!("jev{}", std::env::consts::EXE_SUFFIX)))
}

/// Decides which peers count as `jev`.
///
/// `jev` is resolved once, at `new`. A `jev` installed later counts only after
/// a restart. On Linux, a `jev` that still runs after its file was replaced
/// shows as `(deleted)` and counts as `Other`. Both fail closed.
#[derive(Debug, Clone)]
pub struct AdminCheck {
    /// The canonical path of `jev`, or `None` when it does not exist, so no
    /// peer counts as `jev`.
    jev: Option<PathBuf>,
}

impl AdminCheck {
    pub fn new(jev: &Path) -> AdminCheck {
        AdminCheck {
            jev: std::fs::canonicalize(jev).ok(),
        }
    }

    /// `Peer::Jev` only when the process `pid` runs the exact `jev` binary.
    /// Any failure, such as a process that ended, gives `Peer::Other`.
    pub fn peer(&self, pid: Option<u32>) -> Peer {
        let (Some(jev), Some(pid)) = (&self.jev, pid) else {
            return Peer::Other;
        };
        match peer_path(pid) {
            Ok(path) if &path == jev => Peer::Jev,
            _ => Peer::Other,
        }
    }
}

/// The peer's path, in the form `AdminCheck::new` compares against.
///
/// Windows: canonicalize, which normalizes the `\\?\` form.
#[cfg(windows)]
fn peer_path(pid: u32) -> io::Result<PathBuf> {
    exe_of(pid).and_then(std::fs::canonicalize)
}

/// Linux and macOS: `/proc/<pid>/exe` and `proc_pidpath` already give the
/// kernel's resolved path. A second `canonicalize` would resolve it again
/// against the filesystem as it is now, which opens a rename-and-symlink race.
#[cfg(not(windows))]
fn peer_path(pid: u32) -> io::Result<PathBuf> {
    exe_of(pid)
}

/// The full path of the executable that process `pid` runs.
#[cfg(target_os = "linux")]
pub fn exe_of(pid: u32) -> io::Result<PathBuf> {
    std::fs::read_link(format!("/proc/{pid}/exe"))
}

#[cfg(target_os = "macos")]
pub fn exe_of(pid: u32) -> io::Result<PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    let mut buffer = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    let pid = libc::c_int::try_from(pid)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "pid out of range"))?;
    // SAFETY: the buffer is writable and its length is passed.
    let len = unsafe { libc::proc_pidpath(pid, buffer.as_mut_ptr().cast(), buffer.len() as u32) };
    if len <= 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(PathBuf::from(std::ffi::OsStr::from_bytes(
        &buffer[..len as usize],
    )))
}

#[cfg(windows)]
pub fn exe_of(pid: u32) -> io::Result<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use std::os::windows::io::{FromRawHandle, OwnedHandle};
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    // SAFETY: the handle is checked, then closed when `process` drops; the
    // buffer is writable and its length is passed.
    unsafe {
        let raw = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if raw.is_null() {
            return Err(io::Error::last_os_error());
        }
        let process = OwnedHandle::from_raw_handle(raw);
        let mut buffer = vec![0u16; 32_768];
        let mut len = buffer.len() as u32;
        let ok = QueryFullProcessImageNameW(
            std::os::windows::io::AsRawHandle::as_raw_handle(&process),
            PROCESS_NAME_WIN32,
            buffer.as_mut_ptr(),
            &mut len,
        );
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(PathBuf::from(std::ffi::OsString::from_wide(
            &buffer[..len as usize],
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn me() -> PathBuf {
        std::fs::canonicalize(std::env::current_exe().unwrap()).unwrap()
    }

    #[test]
    fn exe_of_finds_this_process() {
        let found = std::fs::canonicalize(exe_of(std::process::id()).unwrap()).unwrap();
        assert_eq!(found, me());
    }

    #[test]
    fn only_the_exact_binary_counts_as_jev() {
        let pid = Some(std::process::id());
        assert_eq!(AdminCheck::new(&me()).peer(pid), Peer::Jev);
        let elsewhere = tempfile::tempdir().unwrap();
        let copy = elsewhere.path().join(me().file_name().unwrap());
        std::fs::write(&copy, b"").unwrap();
        assert_eq!(
            AdminCheck::new(&copy).peer(pid),
            Peer::Other,
            "a copy in another folder is not this process"
        );
        assert_eq!(AdminCheck::new(&me()).peer(None), Peer::Other);
    }

    #[test]
    fn a_missing_jev_makes_every_peer_other() {
        let folder = tempfile::tempdir().unwrap();
        let check = AdminCheck::new(&folder.path().join("no-such-jev"));
        assert_eq!(check.peer(Some(std::process::id())), Peer::Other);
    }

    #[test]
    fn a_process_that_does_not_exist_gives_other() {
        assert!(exe_of(u32::MAX).is_err());
        assert_eq!(AdminCheck::new(&me()).peer(Some(u32::MAX)), Peer::Other);
    }

    #[test]
    fn another_real_process_is_not_jev() {
        use std::process::{Command, Stdio};
        #[cfg(windows)]
        let mut command = Command::new("ping");
        #[cfg(windows)]
        command.args(["-n", "30", "127.0.0.1"]);
        #[cfg(not(windows))]
        let mut command = Command::new("sleep");
        #[cfg(not(windows))]
        command.arg("30");
        let mut child = command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let found = exe_of(child.id());
        let peer = AdminCheck::new(&me()).peer(Some(child.id()));
        child.kill().unwrap();
        child.wait().unwrap();
        assert!(found.is_ok(), "{found:?}");
        assert_eq!(peer, Peer::Other);
    }

    #[test]
    fn jev_sits_next_to_the_running_binary() {
        let jev = jev_next_to_me().unwrap();
        assert_eq!(jev.parent(), std::env::current_exe().unwrap().parent());
        assert_eq!(
            jev.file_name().unwrap().to_str().unwrap(),
            format!("jev{}", std::env::consts::EXE_SUFFIX)
        );
    }
}
