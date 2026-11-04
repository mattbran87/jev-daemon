//! File writes that never leave a half-written file behind.

use std::io::Write;
use std::path::Path;

/// Write `contents` to a new temporary file in the same folder as `path`, then
/// rename it over `path`. The temporary file has a random name, so writers that
/// run at the same time do not meet, and a failed write leaves nothing behind.
/// On Unix the file is created with mode 0600 and never has a wider mode. On
/// Windows no mode is set: the file inherits the ACL of the user profile folder.
/// The rename comes from `std::fs`, not from `tempfile`'s `persist`: on Windows,
/// `persist` cannot replace a file that another process has open, and `std` can.
/// The parent folder gets created when missing.
pub fn write_atomic(path: &Path, contents: &str) -> std::io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(contents.as_bytes())?;
    temp.as_file().sync_all()?;
    let temp = temp.into_temp_path().keep().map_err(|error| error.error)?;
    if let Err(error) = std::fs::rename(&temp, path) {
        let _ = std::fs::remove_file(&temp);
        return Err(error);
    }
    // Best effort: the file is already replaced here, so a failed folder sync
    // only weakens durability after a power loss. It must not report an error
    // for a change that was made.
    #[cfg(unix)]
    let _ = std::fs::File::open(parent).and_then(|dir| dir.sync_all());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_and_replaces_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub").join("a.json");
        write_atomic(&path, "one").unwrap();
        write_atomic(&path, "two").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "two");
        let leftovers: Vec<_> = std::fs::read_dir(path.parent().unwrap()).unwrap().collect();
        assert_eq!(leftovers.len(), 1, "no temporary file stays behind");
    }

    #[cfg(unix)]
    #[test]
    fn only_the_owner_can_read_it() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.json");
        write_atomic(&path, "x").unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn a_stale_tmp_file_does_not_block_a_write() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.json");
        std::fs::write(dir.path().join("a.json.tmp"), "junk from a crashed writer").unwrap();
        write_atomic(&path, "new").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
    }

    #[test]
    fn no_temp_file_is_left_after_a_failed_rename() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.json");
        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join("inside"), "x").unwrap();
        assert!(write_atomic(&path, "new").is_err());
        let names: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, [std::ffi::OsString::from("a.json")]);
    }

    #[test]
    fn writers_that_run_at_the_same_time_all_succeed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.json");
        std::thread::scope(|scope| {
            for writer in 0..8 {
                let path = &path;
                scope.spawn(move || {
                    for round in 0..50 {
                        write_atomic(path, &format!("{writer}-{round}")).unwrap();
                    }
                });
            }
        });
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains('-'), "{text:?}");
        let (writer, round) = text.split_once('-').unwrap();
        assert!(writer.parse::<u32>().unwrap() < 8 && round.parse::<u32>().unwrap() < 50);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn a_target_held_open_by_a_reader_is_still_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.json");
        write_atomic(&path, "one").unwrap();
        let _reader = std::fs::File::open(&path).unwrap();
        write_atomic(&path, "two").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "two");
    }
}
