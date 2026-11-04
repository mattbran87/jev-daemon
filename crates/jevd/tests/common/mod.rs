//! Helpers that the test files share.

use jev_proto::Paths;

/// Kills the daemon named in the PID file, when it drops.
pub struct Reaper(pub Paths);

impl Drop for Reaper {
    fn drop(&mut self) {
        if let Ok(text) = std::fs::read_to_string(self.0.pid_file()) {
            if let Ok(pid) = text.trim().parse::<u32>() {
                kill(pid);
            }
        }
    }
}

#[cfg(unix)]
pub fn kill(pid: u32) {
    // SAFETY: a plain signal to a process this test started.
    unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) };
}

#[cfg(windows)]
pub fn kill(pid: u32) {
    let _ = std::process::Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/F"])
        .output();
}
