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

/// On Windows, an exclusive `LockFileEx` lock on the file's first byte is
/// the lock. The file is open with read and write sharing, so another
/// program that opens it, such as a virus scanner, does not stop a copy
/// from starting; only a second lock fails. The lock goes when the handle
/// closes.
#[cfg(windows)]
fn take(file: &std::path::Path) -> io::Result<Option<File>> {
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::{ERROR_IO_PENDING, ERROR_LOCK_VIOLATION};
    use windows_sys::Win32::Storage::FileSystem::{
        LockFileEx, FILE_SHARE_READ, FILE_SHARE_WRITE, LOCKFILE_EXCLUSIVE_LOCK,
        LOCKFILE_FAIL_IMMEDIATELY,
    };
    use windows_sys::Win32::System::IO::OVERLAPPED;
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .open(file)?;
    // SAFETY: OVERLAPPED is plain data, and all zeros is a valid value: it
    // locks from offset 0.
    let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
    // SAFETY: the handle is open for the call, and `overlapped` lives past
    // it. The file is not open for overlapped I/O, so the call does not
    // return before the lock is taken or refused.
    let locked = unsafe {
        LockFileEx(
            lock.as_raw_handle(),
            LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
            0,
            1,
            0,
            &mut overlapped,
        )
    };
    if locked == 0 {
        let error = io::Error::last_os_error();
        return match error.raw_os_error() {
            Some(code)
                if code == ERROR_LOCK_VIOLATION as i32 || code == ERROR_IO_PENDING as i32 =>
            {
                Ok(None)
            }
            _ => Err(error),
        };
    }
    Ok(Some(lock))
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

    #[test]
    fn a_plain_open_neither_takes_nor_breaks_the_lock() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::under(dir.path()).unwrap();
        let first = lock(&paths).unwrap().expect("the first lock");
        let reader = File::open(paths.runtime_dir.join(LOCK_FILE)).expect("a plain open");
        assert!(lock(&paths).unwrap().is_none(), "the reader takes no lock");
        drop(reader);
        assert!(
            lock(&paths).unwrap().is_none(),
            "the first copy still holds it"
        );
        drop(first);
        assert!(lock(&paths).unwrap().is_some(), "the lock is free again");
    }

    #[test]
    fn a_reader_open_before_the_lock_does_not_block_it() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::under(dir.path()).unwrap();
        paths.prepare_runtime_dir().unwrap();
        let file = paths.runtime_dir.join(LOCK_FILE);
        std::fs::write(&file, b"").unwrap();
        // A virus scanner that has the file open.
        let reader = File::open(&file).unwrap();
        let first = lock(&paths).unwrap().expect("the lock despite the reader");
        assert!(lock(&paths).unwrap().is_none(), "a second copy must exit");
        drop(reader);
        drop(first);
    }
}
