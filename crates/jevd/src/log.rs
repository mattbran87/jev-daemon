//! The log file (spec section 1, "Storage"). One line per daemon event:
//! start, stop, and failures. It never holds a key, a token, or request text,
//! so callers pass fixed text, names, and paths only.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

/// A log over this size moves to `<name>.old`, and a new one starts.
pub const MAX_LOG_BYTES: u64 = 1 << 20;

static FILE: OnceLock<PathBuf> = OnceLock::new();

/// Sets the log file. Before this runs, and in tests, `event` does nothing.
pub fn init(path: &Path) {
    let _ = FILE.set(path.to_path_buf());
}

/// Adds one line. A failed write is dropped: the log must never stop jevd.
pub fn event(text: &str) {
    if let Some(path) = FILE.get() {
        let _ = append(path, text);
    }
}

fn append(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if too_big(path) {
        let mut old = path.as_os_str().to_owned();
        old.push(".old");
        // Another writer may have moved the log already. Check again just
        // before the move, and append anyway if the move fails.
        if too_big(path) {
            let _ = std::fs::rename(path, old);
        }
    }
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let line: String = text
        .chars()
        .map(|c| if is_unsafe(c) { ' ' } else { c })
        .collect();
    let mut options = std::fs::OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    writeln!(file, "{seconds} {line}")
}

fn too_big(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|meta| meta.len() > MAX_LOG_BYTES)
}

/// A control character, a Unicode line or paragraph separator, or a
/// direction mark: each could forge or hide a log line.
fn is_unsafe(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{2028}' | '\u{2029}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_writes_one_clean_line() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state").join("jevd.log");
        append(&path, "start\nforged line").unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text.lines().count(), 1);
        assert!(text.trim_end().ends_with("start forged line"));
    }

    #[test]
    fn line_and_direction_marks_become_spaces() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jevd.log");
        let marks = "\u{2028}\u{2029}\u{200E}\u{200F}\u{202A}\u{202B}\u{202C}\u{202D}\u{202E}\u{2066}\u{2067}\u{2068}\u{2069}";
        append(&path, &format!("a{marks}b")).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        let line = text.trim_end().split_once(' ').unwrap().1;
        assert_eq!(line, format!("a{}b", " ".repeat(marks.chars().count())));
    }

    #[cfg(unix)]
    #[test]
    fn the_log_is_private_to_the_user() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jevd.log");
        append(&path, "start").unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn a_full_log_moves_aside() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jevd.log");
        std::fs::write(&path, vec![b'x'; MAX_LOG_BYTES as usize + 1]).unwrap();
        append(&path, "next").unwrap();
        assert!(dir.path().join("jevd.log.old").exists());
        assert!(std::fs::metadata(&path).unwrap().len() < 100);
    }
}
