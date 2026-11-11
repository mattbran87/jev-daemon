//! Where jev keeps its files and its channel (spec section 1, "Storage", and
//! section 2, "Transport"). `jevd`, `jev`, and other clients all find the
//! channel here.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

/// When set, every jev file and the channel go under this folder. This is a
/// test hook: tests use it, so they never meet a real daemon. The `JEV_HOME`
/// folder must be private to the user (on Unix, `check_runtime_dir` checks
/// only its `run` folder), and `jevd` and every client must see the same folder.
pub const HOME_VAR: &str = "JEV_HOME";

/// The OS keyring service for keys and client tokens when `JEV_HOME` is not
/// set. `Paths::keyring_service` gives the service for a `JEV_HOME` folder.
pub const KEYRING_SERVICE: &str = "jev";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    /// The settings file and the pairing file.
    pub config_dir: PathBuf,
    /// The log file, and the waiting pairing requests.
    pub state_dir: PathBuf,
    /// The PID file, and on Unix the socket.
    pub runtime_dir: PathBuf,
    /// The Unix socket file, or the Windows pipe name.
    pub socket: PathBuf,
    /// The `JEV_HOME` folder these paths were built under, or `None` for the
    /// platform defaults. `client::start_daemon` passes it on to `jevd`, so
    /// the daemon it starts uses the same files and channel.
    pub home: Option<PathBuf>,
}

impl Paths {
    /// The paths for this user, from the environment.
    pub fn from_env() -> io::Result<Paths> {
        Paths::from_vars(|name| std::env::var_os(name))
    }

    /// The paths, with `var` in place of the environment.
    pub fn from_vars(var: impl Fn(&str) -> Option<OsString>) -> io::Result<Paths> {
        let set = |name: &str| var(name).filter(|value| !value.is_empty());
        if let Some(home) = set(HOME_VAR) {
            return Paths::under(Path::new(&home));
        }
        platform(&set)
    }

    /// Every path under `home`.
    pub fn under(home: &Path) -> io::Result<Paths> {
        // A daemon runs in its own folder, so a relative home would mean
        // another folder to it.
        let home = std::path::absolute(home)?;
        let home = home.as_path();
        let runtime_dir = home.join("run");
        #[cfg(unix)]
        let socket = runtime_dir.join("jevd.sock");
        #[cfg(windows)]
        let socket = PathBuf::from(format!(
            r"\\.\pipe\jev-{}-{:016x}",
            crate::os::user_sid()?,
            home_hash(home)
        ));
        Ok(Paths {
            config_dir: home.join("config"),
            state_dir: home.join("state"),
            runtime_dir,
            socket,
            home: Some(home.to_path_buf()),
        })
    }

    pub fn settings_file(&self) -> PathBuf {
        self.config_dir.join("config.toml")
    }

    pub fn pairing_file(&self) -> PathBuf {
        self.config_dir.join("pairings.json")
    }

    pub fn log_file(&self) -> PathBuf {
        self.state_dir.join("jevd.log")
    }

    /// The pairing requests that wait for a person. Only `jevd` writes it.
    pub fn pending_file(&self) -> PathBuf {
        self.state_dir.join("pending.json")
    }

    pub fn pid_file(&self) -> PathBuf {
        self.runtime_dir.join("jevd.pid")
    }

    /// The OS keyring service for keys and client tokens: `jev`, or under
    /// `JEV_HOME`, `jev-` and the 16-digit hash of the folder. A test folder
    /// then never reads or writes the real entries.
    pub fn keyring_service(&self) -> String {
        match &self.home {
            None => KEYRING_SERVICE.to_string(),
            Some(home) => format!("jev-{:016x}", home_hash(home)),
        }
    }

    /// Creates the runtime folder when it is missing, then checks it as
    /// `check_runtime_dir` does. `jevd` calls this before it binds.
    pub fn prepare_runtime_dir(&self) -> io::Result<()> {
        let mut builder = std::fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
        builder.create(&self.runtime_dir)?;
        self.check_runtime_dir()
    }

    /// On Unix: the runtime folder must belong to this user, with no access
    /// for the group or others, so no other user can put a socket there.
    /// On Unix it also checks that the socket path is short enough to bind.
    /// On Windows the pipe has its own access list, so this checks nothing.
    pub fn check_runtime_dir(&self) -> io::Result<()> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let meta = std::fs::symlink_metadata(&self.runtime_dir)?;
            // SAFETY: geteuid has no preconditions.
            let me = unsafe { libc::geteuid() };
            let denied = |what: String| {
                io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!("{}: {what}", self.runtime_dir.display()),
                )
            };
            if !meta.is_dir() {
                return Err(denied("not a folder (or it is a link)".to_string()));
            }
            if meta.uid() != me {
                return Err(denied(format!(
                    "owned by uid {}, but this user is uid {me}",
                    meta.uid()
                )));
            }
            if meta.mode() & 0o077 != 0 {
                return Err(denied(format!(
                    "mode is {:o}, but only the owner may have access; run chmod 700 on it",
                    meta.mode() & 0o777
                )));
            }
            #[cfg(target_os = "macos")]
            const SOCKET_LIMIT: usize = 104;
            #[cfg(not(target_os = "macos"))]
            const SOCKET_LIMIT: usize = 108;
            let length = self.socket.as_os_str().len();
            if length >= SOCKET_LIMIT {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "the socket path {} is too long ({length} bytes; the limit is {})",
                        self.socket.display(),
                        SOCKET_LIMIT - 1
                    ),
                ));
            }
        }
        Ok(())
    }
}

#[cfg(windows)]
fn platform(set: &dyn Fn(&str) -> Option<OsString>) -> io::Result<Paths> {
    let missing =
        |name: &str| io::Error::new(io::ErrorKind::NotFound, format!("{name} is not set"));
    let roaming = PathBuf::from(set("APPDATA").ok_or_else(|| missing("APPDATA"))?);
    let local = PathBuf::from(set("LOCALAPPDATA").ok_or_else(|| missing("LOCALAPPDATA"))?);
    Ok(Paths {
        config_dir: roaming.join("jev"),
        state_dir: local.join("jev"),
        runtime_dir: local.join("jev").join("run"),
        socket: PathBuf::from(format!(r"\\.\pipe\jev-{}", crate::os::user_sid()?)),
        home: None,
    })
}

#[cfg(unix)]
fn platform(set: &dyn Fn(&str) -> Option<OsString>) -> io::Result<Paths> {
    let home = set("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME is not set"))?;
    // SAFETY: geteuid has no preconditions.
    let uid = unsafe { libc::geteuid() };
    let fallback = || PathBuf::from(format!("/tmp/jev-{uid}"));
    #[cfg(target_os = "macos")]
    let (config_dir, state_dir, runtime_dir) = (
        home.join("Library/Application Support/jev"),
        home.join("Library/Logs/jev"),
        set("TMPDIR")
            .map(|dir| PathBuf::from(dir).join("jev"))
            .unwrap_or_else(fallback),
    );
    #[cfg(not(target_os = "macos"))]
    let (config_dir, state_dir, runtime_dir) = (
        set("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .unwrap_or_else(|| home.join(".config"))
            .join("jev"),
        set("XDG_STATE_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .unwrap_or_else(|| home.join(".local/state"))
            .join("jev"),
        set("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .map(|dir| dir.join("jev"))
            .unwrap_or_else(fallback),
    );
    let socket = runtime_dir.join("jevd.sock");
    Ok(Paths {
        config_dir,
        state_dir,
        runtime_dir,
        socket,
        home: None,
    })
}

/// The UTF-16 units of `home`, without trailing `\` or `/`, with ASCII
/// letters in lower case and `/` as `\`. The same folder then gets the same key whatever
/// its spelling or the Rust version.
#[cfg(windows)]
fn home_key(home: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    let mut units: Vec<u16> = home.as_os_str().encode_wide().collect();
    while matches!(units.last(), Some(&unit) if unit == u16::from(b'\\') || unit == u16::from(b'/'))
    {
        units.pop();
    }
    for unit in &mut units {
        if (u16::from(b'A')..=u16::from(b'Z')).contains(unit) {
            *unit += 32;
        } else if *unit == u16::from(b'/') {
            *unit = u16::from(b'\\');
        }
    }
    units
}

/// A short, stable name for a `JEV_HOME` folder: FNV-1a, 64 bits, over the
/// folder key. Windows hashes the two bytes of each UTF-16 unit of
/// `home_key`, low byte first. Not a security measure; the pipe's access
/// list and the keyring's own access rules are.
#[cfg(windows)]
fn home_hash(home: &Path) -> u64 {
    fnv1a(home_key(home).iter().flat_map(|unit| unit.to_le_bytes()))
}

/// Unix hashes the bytes of the folder path, without trailing `/`. Unix
/// paths keep letter case, so the hash keeps it too.
#[cfg(unix)]
fn home_hash(home: &Path) -> u64 {
    use std::os::unix::ffi::OsStrExt;
    let mut bytes = home.as_os_str().as_bytes();
    while bytes.len() > 1 && bytes.ends_with(b"/") {
        bytes = &bytes[..bytes.len() - 1];
    }
    fnv1a(bytes.iter().copied())
}

fn fnv1a(bytes: impl IntoIterator<Item = u8>) -> u64 {
    bytes.into_iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn vars(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let map: HashMap<String, OsString> = pairs
            .iter()
            .map(|(name, value)| (name.to_string(), OsString::from(value)))
            .collect();
        move |name| map.get(name).cloned()
    }

    #[test]
    fn jev_home_moves_everything_under_it() {
        let home = std::env::temp_dir().join("jev-home-test");
        let paths =
            Paths::from_vars(vars(&[(HOME_VAR, home.to_str().unwrap()), ("HOME", "/x")])).unwrap();
        assert_eq!(
            paths.settings_file(),
            home.join("config").join("config.toml")
        );
        assert_eq!(
            paths.pairing_file(),
            home.join("config").join("pairings.json")
        );
        assert_eq!(paths.log_file(), home.join("state").join("jevd.log"));
        assert_eq!(
            paths.pending_file(),
            home.join("state").join("pending.json")
        );
        assert_eq!(paths.pid_file(), home.join("run").join("jevd.pid"));
        #[cfg(unix)]
        assert_eq!(paths.socket, home.join("run").join("jevd.sock"));
        #[cfg(windows)]
        {
            let name = paths.socket.to_str().unwrap();
            let prefix = format!(r"\\.\pipe\jev-{}-", crate::os::user_sid().unwrap());
            assert!(name.starts_with(&prefix), "{name}");
            assert_eq!(name.len(), prefix.len() + 16);
        }
    }

    #[test]
    fn an_empty_jev_home_counts_as_unset() {
        let with_empty = Paths::from_vars(vars(&[
            (HOME_VAR, ""),
            ("HOME", "/home/u"),
            ("APPDATA", r"C:\r"),
            ("LOCALAPPDATA", r"C:\l"),
        ]))
        .unwrap();
        let without = Paths::from_vars(vars(&[
            ("HOME", "/home/u"),
            ("APPDATA", r"C:\r"),
            ("LOCALAPPDATA", r"C:\l"),
        ]))
        .unwrap();
        assert_eq!(with_empty, without);
        assert_eq!(with_empty.home, None);
    }

    #[test]
    fn a_relative_home_becomes_absolute() {
        let paths = Paths::under(Path::new("rel")).unwrap();
        let home = paths.home.clone().unwrap();
        assert!(home.is_absolute(), "{}", home.display());
        assert!(paths.config_dir.starts_with(&home));
        assert!(paths.runtime_dir.starts_with(&home));
    }

    #[test]
    fn under_remembers_its_home() {
        let home = std::env::temp_dir().join("jev-home-test");
        assert_eq!(Paths::under(&home).unwrap().home, Some(home));
    }

    #[test]
    fn two_homes_get_two_channels() {
        let a = Paths::under(Path::new("/a")).unwrap();
        let b = Paths::under(Path::new("/b")).unwrap();
        assert_ne!(a.socket, b.socket);
    }

    #[test]
    fn no_home_uses_the_default_keyring_service() {
        let paths = Paths::from_vars(vars(&[
            ("HOME", "/home/u"),
            ("APPDATA", r"C:\r"),
            ("LOCALAPPDATA", r"C:\l"),
        ]))
        .unwrap();
        assert_eq!(paths.keyring_service(), "jev");
        assert_eq!(KEYRING_SERVICE, "jev");
    }

    #[test]
    fn a_home_gets_its_own_keyring_service() {
        let service = Paths::under(Path::new("/a")).unwrap().keyring_service();
        let digits = service.strip_prefix("jev-").expect("starts with jev-");
        assert_eq!(digits.len(), 16, "{service}");
        assert!(
            digits
                .chars()
                .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)),
            "{service}"
        );
    }

    #[test]
    fn two_homes_get_two_keyring_services() {
        let a = Paths::under(Path::new("/a")).unwrap();
        let b = Paths::under(Path::new("/b")).unwrap();
        assert_ne!(a.keyring_service(), b.keyring_service());
    }

    #[cfg(windows)]
    #[test]
    fn the_keyring_service_ignores_case_and_trailing_slashes() {
        let service = |home: &str| Paths::under(Path::new(home)).unwrap().keyring_service();
        assert_eq!(service(r"C:\x"), service(r"c:\X"));
        assert_eq!(service(r"C:\x"), service(r"C:\x\"));
        assert_eq!(service(r"C:\x"), service("C:/x/"));
        assert_eq!(service(r"C:\jev-pin"), "jev-2b614bce24bfacb3");
    }

    #[cfg(unix)]
    #[test]
    fn the_keyring_service_ignores_a_trailing_slash() {
        let service = |home: &str| Paths::under(Path::new(home)).unwrap().keyring_service();
        assert_eq!(service("/x"), service("/x/"));
        assert_ne!(service("/x"), service("/X"));
        // FNV-1a over the bytes of "/jev-pin".
        assert_eq!(service("/jev-pin"), "jev-9978e02a32614065");
    }

    #[cfg(windows)]
    #[test]
    fn windows_uses_appdata_and_the_user_pipe() {
        let paths =
            Paths::from_vars(vars(&[("APPDATA", r"C:\r"), ("LOCALAPPDATA", r"C:\l")])).unwrap();
        assert_eq!(paths.config_dir, PathBuf::from(r"C:\r\jev"));
        assert_eq!(paths.state_dir, PathBuf::from(r"C:\l\jev"));
        assert_eq!(paths.runtime_dir, PathBuf::from(r"C:\l\jev\run"));
        assert_eq!(
            paths.socket,
            PathBuf::from(format!(r"\\.\pipe\jev-{}", crate::os::user_sid().unwrap()))
        );
        assert!(Paths::from_vars(vars(&[("APPDATA", r"C:\r")])).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn the_pipe_name_ignores_case_and_trailing_slashes() {
        let pipe = |home: &str| Paths::under(Path::new(home)).unwrap().socket;
        assert_eq!(pipe(r"C:\x"), pipe(r"c:\X"));
        assert_eq!(pipe(r"C:\x"), pipe(r"C:\x\"));
        assert_eq!(pipe(r"C:\x"), pipe("C:/x/"));
        assert_ne!(pipe(r"C:\x"), pipe(r"C:\y"));
    }

    #[cfg(windows)]
    #[test]
    fn the_pipe_name_of_a_known_home_does_not_change() {
        let paths = Paths::under(Path::new(r"C:\jev-pin")).unwrap();
        let expected = format!(
            r"\\.\pipe\jev-{}-2b614bce24bfacb3",
            crate::os::user_sid().unwrap()
        );
        assert_eq!(paths.socket, PathBuf::from(expected));
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn linux_ignores_relative_xdg_folders() {
        let paths = Paths::from_vars(vars(&[
            ("HOME", "/home/u"),
            ("XDG_CONFIG_HOME", "rel/config"),
            ("XDG_STATE_HOME", "rel/state"),
            ("XDG_RUNTIME_DIR", "rel/run"),
        ]))
        .unwrap();
        assert_eq!(paths.config_dir, PathBuf::from("/home/u/.config/jev"));
        assert_eq!(paths.state_dir, PathBuf::from("/home/u/.local/state/jev"));
        assert!(paths.runtime_dir.starts_with("/tmp"));
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_as_the_runtime_folder_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real");
        std::fs::create_dir(&real).unwrap();
        // The target is private, so only the link check can refuse it.
        std::fs::set_permissions(&real, std::os::unix::fs::PermissionsExt::from_mode(0o700))
            .unwrap();
        let paths = Paths::under(&dir.path().join("home")).unwrap();
        std::fs::create_dir_all(paths.runtime_dir.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&real, &paths.runtime_dir).unwrap();
        let error = paths.check_runtime_dir().unwrap_err();
        assert!(error.to_string().contains("link"), "{error}");
    }

    #[cfg(unix)]
    #[test]
    fn a_socket_path_over_the_limit_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let mut paths = Paths::under(dir.path()).unwrap();
        paths.prepare_runtime_dir().unwrap();
        paths.check_runtime_dir().unwrap();
        paths.socket = paths.runtime_dir.join("x".repeat(200));
        let error = paths.check_runtime_dir().unwrap_err();
        assert!(error.to_string().contains("too long"), "{error}");
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn linux_uses_the_xdg_folders() {
        let paths = Paths::from_vars(vars(&[
            ("HOME", "/home/u"),
            ("XDG_RUNTIME_DIR", "/run/user/1000"),
        ]))
        .unwrap();
        assert_eq!(paths.config_dir, PathBuf::from("/home/u/.config/jev"));
        assert_eq!(paths.state_dir, PathBuf::from("/home/u/.local/state/jev"));
        assert_eq!(paths.socket, PathBuf::from("/run/user/1000/jev/jevd.sock"));
        let bare = Paths::from_vars(vars(&[("HOME", "/home/u")])).unwrap();
        assert!(bare.runtime_dir.starts_with("/tmp"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_uses_library_and_tmpdir() {
        let paths = Paths::from_vars(vars(&[
            ("HOME", "/Users/u"),
            ("TMPDIR", "/var/folders/x/T/"),
        ]))
        .unwrap();
        assert_eq!(
            paths.config_dir,
            PathBuf::from("/Users/u/Library/Application Support/jev")
        );
        assert_eq!(
            paths.socket,
            PathBuf::from("/var/folders/x/T/jev/jevd.sock")
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_runtime_folder_must_be_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::under(dir.path()).unwrap();
        paths.prepare_runtime_dir().unwrap();
        let mode = std::fs::metadata(&paths.runtime_dir)
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o700);
        std::fs::set_permissions(&paths.runtime_dir, std::fs::Permissions::from_mode(0o755))
            .unwrap();
        assert!(paths.check_runtime_dir().is_err());
        assert!(paths.prepare_runtime_dir().is_err());
    }

    #[test]
    fn prepare_creates_the_runtime_folder() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::under(dir.path()).unwrap();
        #[cfg(unix)]
        assert!(paths.check_runtime_dir().is_err());
        #[cfg(windows)]
        assert!(paths.check_runtime_dir().is_ok());
        paths.prepare_runtime_dir().unwrap();
        assert!(paths.runtime_dir.is_dir());
        paths.check_runtime_dir().unwrap();
    }
}
