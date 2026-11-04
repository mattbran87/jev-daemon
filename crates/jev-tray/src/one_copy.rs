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
