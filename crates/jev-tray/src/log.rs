//! The start-up log: one line for each failure that has no other place to
//! show, because `jev-tray` has no console on Windows.

use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// The log file's name in the state folder.
pub const LOG_FILE: &str = "jev-tray.log";

/// One log line: the Unix time in seconds, a space, and the message, with
/// any line break in the message made a space.
pub fn line(unix_seconds: u64, message: &str) -> String {
    let message: String = message
        .chars()
        .map(|ch| if ch == '\n' || ch == '\r' { ' ' } else { ch })
        .collect();
    format!("{unix_seconds} {message}\n")
}

/// Appends one line for `message` to `LOG_FILE` in `dir`, and makes `dir`
/// when it is missing.
pub fn append(dir: &Path, message: &str) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(LOG_FILE))?;
    file.write_all(line(now, message).as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_is_the_time_and_the_message() {
        assert_eq!(line(42, "cannot start"), "42 cannot start\n");
        assert_eq!(line(7, "two\r\nlines"), "7 two  lines\n");
    }

    #[test]
    fn append_makes_the_folder_and_adds_lines() {
        let dir = tempfile::tempdir().unwrap();
        let state = dir.path().join("state").join("deeper");
        append(&state, "first").unwrap();
        append(&state, "second").unwrap();
        let text = std::fs::read_to_string(state.join(LOG_FILE)).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2, "{text:?}");
        for (each, message) in lines.iter().zip(["first", "second"]) {
            let (seconds, rest) = each.split_once(' ').unwrap();
            assert!(seconds.parse::<u64>().unwrap() > 1_700_000_000, "{each}");
            assert_eq!(rest, message);
        }
    }
}
