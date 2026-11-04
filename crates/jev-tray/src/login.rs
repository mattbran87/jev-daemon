//! The **Start at login** entry (jev-tray spec section 4): one entry per
//! system that starts `jev-tray` when the user signs in. Every place is
//! injectable, so tests never touch the real one.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

/// The launchd label, and the name of the macOS plist file without `.plist`.
pub const LABEL: &str = "com.github.mattbran87.jev-tray";
/// The registry key that Windows reads at sign-in, under `HKEY_CURRENT_USER`.
pub const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
/// The value name under `RUN_KEY`, and the name of the Linux desktop file
/// without `.desktop`.
pub const NAME: &str = "jev-tray";

/// Where one login entry lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Login {
    /// A macOS launch agent plist file.
    LaunchAgent(PathBuf),
    /// A Linux autostart desktop file.
    Autostart(PathBuf),
    /// A string value under a key of `HKEY_CURRENT_USER`.
    #[cfg(windows)]
    Run { key: String, name: String },
}

impl Login {
    /// The entry for this user on this system.
    #[cfg(windows)]
    pub fn for_this_user() -> io::Result<Login> {
        Ok(Login::Run {
            key: RUN_KEY.to_string(),
            name: NAME.to_string(),
        })
    }

    /// The entry for this user on this system.
    #[cfg(target_os = "macos")]
    pub fn for_this_user() -> io::Result<Login> {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .filter(|home| home.is_absolute())
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::NotFound, "HOME is not an absolute path")
            })?;
        Ok(Login::launch_agent(&home))
    }

    /// The entry for this user on this system.
    #[cfg(not(any(windows, target_os = "macos")))]
    pub fn for_this_user() -> io::Result<Login> {
        let dir = autostart_dir(|name| std::env::var_os(name)).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "neither XDG_CONFIG_HOME nor HOME is set",
            )
        })?;
        Ok(Login::Autostart(dir.join(format!("{NAME}.desktop"))))
    }

    /// The launch agent under the home folder `home`.
    pub fn launch_agent(home: &Path) -> Login {
        Login::LaunchAgent(
            home.join("Library")
                .join("LaunchAgents")
                .join(format!("{LABEL}.plist")),
        )
    }

    /// True when the entry exists, whatever it holds (spec section 4).
    /// It does not read the entry, so an entry that is not text is on too.
    pub fn is_on(&self) -> io::Result<bool> {
        match self {
            Login::LaunchAgent(file) | Login::Autostart(file) => {
                match std::fs::symlink_metadata(file) {
                    Ok(_) => Ok(true),
                    Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
                    Err(error) => Err(error),
                }
            }
            #[cfg(windows)]
            Login::Run { key, name } => registry::exists(key, name),
        }
    }

    /// The entry's text (a file's content, or the registry string), or
    /// `None` when there is no entry.
    pub fn read(&self) -> io::Result<Option<String>> {
        match self {
            Login::LaunchAgent(file) | Login::Autostart(file) => {
                match std::fs::read_to_string(file) {
                    Ok(text) => Ok(Some(text)),
                    Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
                    Err(error) => Err(error),
                }
            }
            #[cfg(windows)]
            Login::Run { key, name } => registry::read(key, name),
        }
    }

    /// Writes the entry, so it starts `exe` at sign-in. An entry that is
    /// there already gets the new path. A path that is not UTF-8, or that
    /// holds a control character, fails with `InvalidInput` and writes
    /// nothing: no entry could hold it whole.
    pub fn write(&self, exe: &Path) -> io::Result<()> {
        let exe = storable(exe)?;
        match self {
            Login::LaunchAgent(file) => write_file(file, &plist_text(Path::new(exe))),
            Login::Autostart(file) => write_file(file, &desktop_text(Path::new(exe))),
            #[cfg(windows)]
            Login::Run { key, name } => registry::write(key, name, &quoted(exe)),
        }
    }

    /// Deletes the entry. No entry is not an error.
    pub fn delete(&self) -> io::Result<()> {
        match self {
            Login::LaunchAgent(file) | Login::Autostart(file) => match std::fs::remove_file(file) {
                Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
                _ => Ok(()),
            },
            #[cfg(windows)]
            Login::Run { key, name } => registry::delete(key, name),
        }
    }
}

/// `exe` as text that an entry can hold whole, or `InvalidInput`.
fn storable(exe: &Path) -> io::Result<&str> {
    match exe.to_str() {
        Some(text) if !text.chars().any(char::is_control) => Ok(text),
        Some(_) => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the path holds a control character",
        )),
        None => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the path is not UTF-8",
        )),
    }
}

/// Writes `text` to a temporary file next to `file`, syncs it to disk, and
/// then renames it over `file`, so a failed write or a crash never leaves
/// half an entry.
fn write_file(file: &Path, text: &str) -> io::Result<()> {
    use std::io::Write;
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut temp = file.as_os_str().to_owned();
    temp.push(".tmp");
    let temp = PathBuf::from(temp);
    let result = (|| {
        let mut out = std::fs::File::create(&temp)?;
        out.write_all(text.as_bytes())?;
        out.sync_all()?;
        drop(out);
        std::fs::rename(&temp, file)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

/// The Linux autostart folder: `$XDG_CONFIG_HOME/autostart`, or
/// `$HOME/.config/autostart`. The XDG rules ignore a relative
/// `XDG_CONFIG_HOME`, and a relative `HOME` is ignored too. `var` stands in
/// for the environment.
pub fn autostart_dir(var: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let absolute = |name: &str| var(name).map(PathBuf::from).filter(|dir| dir.is_absolute());
    if let Some(config) = absolute("XDG_CONFIG_HOME") {
        return Some(config.join("autostart"));
    }
    absolute("HOME").map(|home| home.join(".config").join("autostart"))
}

/// The registry string: the full path of `jev-tray.exe`, quoted. It is
/// what `write` stores, and it refuses the same paths with `InvalidInput`.
pub fn run_value(exe: &Path) -> io::Result<String> {
    storable(exe).map(quoted)
}

fn quoted(text: &str) -> String {
    format!("\"{text}\"")
}

/// The macOS launch agent that runs `exe` at load.
pub fn plist_text(exe: &Path) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>Label</key>
	<string>{LABEL}</string>
	<key>ProgramArguments</key>
	<array>
		<string>{}</string>
	</array>
	<key>RunAtLoad</key>
	<true/>
</dict>
</plist>
"#,
        xml_escape(&exe.to_string_lossy())
    )
}

/// The Linux autostart desktop file that runs `exe`.
pub fn desktop_text(exe: &Path) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=jev-tray\n\
         Comment=Shows whether jevd runs\n\
         Exec={}\n\
         Terminal=false\n\
         X-GNOME-Autostart-enabled=true\n",
        exec_quote(&exe.to_string_lossy())
    )
}

fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// One `Exec=` argument, by the desktop entry rules: in double quotes, with
/// `"`, `` ` ``, `$`, and `\` escaped by a backslash, `%` doubled, and then
/// every backslash doubled again, because `Exec` is also a string value.
fn exec_quote(arg: &str) -> String {
    let mut quoted = String::from("\"");
    for c in arg.chars() {
        match c {
            '"' | '`' | '$' | '\\' => {
                quoted.push('\\');
                quoted.push(c);
            }
            '%' => quoted.push_str("%%"),
            _ => quoted.push(c),
        }
    }
    quoted.push('"');
    quoted.replace('\\', "\\\\")
}

#[cfg(windows)]
mod registry {
    use std::io;
    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegDeleteKeyValueW, RegGetValueW, RegSetValueExW, HKEY,
        HKEY_CURRENT_USER, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ, RRF_RT_ANY,
        RRF_RT_REG_SZ,
    };

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(Some(0)).collect()
    }

    fn failed(status: u32) -> io::Error {
        io::Error::from_raw_os_error(status as i32)
    }

    /// True when the value exists, whatever its type and data.
    pub fn exists(key: &str, name: &str) -> io::Result<bool> {
        let (key, name) = (wide(key), wide(name));
        // SAFETY: both names end in a zero; with no buffer and no size, the
        // call only looks the value up.
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                RRF_RT_ANY,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        match status {
            ERROR_SUCCESS => Ok(true),
            ERROR_FILE_NOT_FOUND => Ok(false),
            _ => Err(failed(status)),
        }
    }

    pub fn read(key: &str, name: &str) -> io::Result<Option<String>> {
        let (key, name) = (wide(key), wide(name));
        let mut buffer = vec![0u16; 260];
        loop {
            let mut size = (buffer.len() * 2) as u32;
            // SAFETY: both names end in a zero, and the buffer holds `size` bytes.
            let status = unsafe {
                RegGetValueW(
                    HKEY_CURRENT_USER,
                    key.as_ptr(),
                    name.as_ptr(),
                    RRF_RT_REG_SZ,
                    std::ptr::null_mut(),
                    buffer.as_mut_ptr().cast(),
                    &mut size,
                )
            };
            match status {
                ERROR_SUCCESS => {
                    let chars = (size as usize / 2).min(buffer.len());
                    let text = &buffer[..chars];
                    let end = text.iter().position(|&c| c == 0).unwrap_or(text.len());
                    return Ok(Some(String::from_utf16_lossy(&text[..end])));
                }
                ERROR_FILE_NOT_FOUND => return Ok(None),
                // The value grew, or is longer than the buffer: `size` now
                // holds the bytes it needs, so grow the buffer and ask again.
                ERROR_MORE_DATA => {
                    let needed = (size as usize).div_ceil(2);
                    let len = needed.max(buffer.len() * 2);
                    buffer.resize(len, 0);
                }
                _ => return Err(failed(status)),
            }
        }
    }

    pub fn write(key: &str, name: &str, value: &str) -> io::Result<()> {
        let (key, name, value) = (wide(key), wide(name), wide(value));
        let mut handle: HKEY = std::ptr::null_mut();
        // SAFETY: the key name ends in a zero, and `handle` is valid for a write.
        let status = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                0,
                std::ptr::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE,
                std::ptr::null(),
                &mut handle,
                std::ptr::null_mut(),
            )
        };
        if status != ERROR_SUCCESS {
            return Err(failed(status));
        }
        // SAFETY: the handle is open, the name ends in a zero, and the data
        // is the string with its zero, in bytes.
        let status = unsafe {
            RegSetValueExW(
                handle,
                name.as_ptr(),
                0,
                REG_SZ,
                value.as_ptr().cast(),
                (value.len() * 2) as u32,
            )
        };
        // SAFETY: the handle is open, and is not used again.
        unsafe { RegCloseKey(handle) };
        if status != ERROR_SUCCESS {
            return Err(failed(status));
        }
        Ok(())
    }

    pub fn delete(key: &str, name: &str) -> io::Result<()> {
        let (key, name) = (wide(key), wide(name));
        // SAFETY: both names end in a zero.
        let status = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr()) };
        if status != ERROR_SUCCESS && status != ERROR_FILE_NOT_FOUND {
            return Err(failed(status));
        }
        Ok(())
    }

    /// Deletes a key and everything under it. Tests use it to clean up.
    #[cfg(test)]
    pub fn delete_tree(key: &str) {
        use windows_sys::Win32::System::Registry::RegDeleteTreeW;
        let key = wide(key);
        // SAFETY: the name ends in a zero.
        unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, key.as_ptr()) };
    }

    /// Writes a value of any type, from raw bytes. Tests use it to make
    /// entries that `write` never makes.
    #[cfg(test)]
    pub fn write_raw(key: &str, name: &str, kind: u32, data: &[u8]) -> io::Result<()> {
        let (key, name) = (wide(key), wide(name));
        // SAFETY: both names end in a zero, and `data` holds `data.len()` bytes.
        let status = unsafe {
            windows_sys::Win32::System::Registry::RegSetKeyValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                kind,
                data.as_ptr().cast(),
                data.len() as u32,
            )
        };
        if status != ERROR_SUCCESS {
            return Err(failed(status));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exe() -> PathBuf {
        if cfg!(windows) {
            PathBuf::from(r"C:\Program Files\jev\jev-tray.exe")
        } else {
            PathBuf::from("/opt/my tools/jev-tray")
        }
    }

    #[test]
    fn the_run_value_is_the_quoted_full_path() {
        assert_eq!(
            run_value(Path::new(r"C:\Program Files\jev\jev-tray.exe")).unwrap(),
            r#""C:\Program Files\jev\jev-tray.exe""#
        );
        assert_invalid_input(run_value(Path::new("C:\\a\nb\\jev-tray.exe")).map(drop));
    }

    #[test]
    fn the_plist_runs_the_path_at_load() {
        let text = plist_text(Path::new("/Applications/jev & co/jev-tray"));
        assert!(text.contains("<string>com.github.mattbran87.jev-tray</string>"));
        assert!(text.contains("<string>/Applications/jev &amp; co/jev-tray</string>"));
        assert!(text.contains("<key>RunAtLoad</key>\n\t<true/>"));
        assert_well_formed(&text);
    }

    #[test]
    fn the_plist_escapes_markup_and_keeps_other_text() {
        let text = plist_text(Path::new("/Apps/a<b>&\"c\" é ü 日本/jev-tray"));
        assert!(text.contains("<string>/Apps/a&lt;b&gt;&amp;\"c\" é ü 日本/jev-tray</string>"));
        assert_well_formed(&text);
    }

    /// Enough of an XML check for the plist: every `&` starts an entity,
    /// every `<` starts a tag, and the tags nest with one root.
    fn assert_well_formed(text: &str) {
        let mut open: Vec<&str> = Vec::new();
        let mut roots = 0;
        let mut rest = text;
        while let Some(at) = rest.find(['<', '&']) {
            let content = &rest[..at];
            assert!(!content.contains('>'), "a bare > in {content:?}");
            rest = &rest[at..];
            if rest.starts_with('&') {
                let end = rest.find(';').expect("an entity ends in ;");
                let entity = &rest[1..end];
                assert!(
                    ["amp", "lt", "gt", "quot", "apos"].contains(&entity),
                    "unknown entity {entity:?}"
                );
                rest = &rest[end + 1..];
                continue;
            }
            let end = rest.find('>').expect("a tag ends in >");
            let tag = &rest[1..end];
            rest = &rest[end + 1..];
            assert!(!tag.contains('<'), "a < inside the tag {tag:?}");
            if tag.starts_with('?') || tag.starts_with('!') {
                continue;
            }
            if let Some(name) = tag.strip_prefix('/') {
                assert_eq!(open.pop(), Some(name), "a close tag out of order");
            } else {
                if open.is_empty() {
                    roots += 1;
                }
                if !tag.ends_with('/') {
                    open.push(tag.split(' ').next().unwrap());
                }
            }
        }
        assert!(!rest.contains('>'), "a bare > at the end");
        assert!(open.is_empty(), "unclosed tags {open:?}");
        assert_eq!(roots, 1, "one root element");
    }

    #[test]
    fn the_desktop_file_execs_the_quoted_path() {
        let text = desktop_text(Path::new("/opt/my tools/jev-tray"));
        assert!(text.starts_with("[Desktop Entry]\n"));
        assert!(text.contains("\nType=Application\n"));
        assert!(text.contains("\nExec=\"/opt/my tools/jev-tray\"\n"));
    }

    #[test]
    fn exec_quoting_escapes_the_reserved_characters() {
        assert_eq!(exec_quote("/a/b"), r#""/a/b""#);
        assert_eq!(exec_quote("/a/$x"), r#""/a/\\$x""#);
        assert_eq!(exec_quote("/a/100%"), r#""/a/100%%""#);
        assert_eq!(exec_quote(r"/a\b"), r#""/a\\\\b""#);
        assert_eq!(exec_quote("/a/\"b\""), r#""/a/\\"b\\"""#);
        assert_eq!(exec_quote("/a/`b`"), r#""/a/\\`b\\`""#);
    }

    /// Unix only: `/x/conf` is not an absolute path on Windows.
    #[cfg(unix)]
    #[test]
    fn the_autostart_folder_follows_xdg() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |name: &str| {
                pairs
                    .iter()
                    .find(|(key, _)| *key == name)
                    .map(|(_, value)| OsString::from(value))
            }
        };
        assert_eq!(
            autostart_dir(env(&[("XDG_CONFIG_HOME", "/x/conf"), ("HOME", "/home/u")])),
            Some(PathBuf::from("/x/conf").join("autostart"))
        );
        assert_eq!(
            autostart_dir(env(&[("HOME", "/home/u")])),
            Some(PathBuf::from("/home/u").join(".config").join("autostart"))
        );
        assert_eq!(
            autostart_dir(env(&[("XDG_CONFIG_HOME", "conf"), ("HOME", "/home/u")])),
            Some(PathBuf::from("/home/u").join(".config").join("autostart"))
        );
        assert_eq!(autostart_dir(env(&[])), None);
        assert_eq!(autostart_dir(env(&[("HOME", "home/u")])), None);
        assert_eq!(
            autostart_dir(env(&[("XDG_CONFIG_HOME", "conf"), ("HOME", "home/u")])),
            None
        );
    }

    /// The file entries are plain files, so this runs on every system.
    #[test]
    fn a_file_entry_is_written_read_and_deleted() {
        let home = tempfile::tempdir().unwrap();
        let entries = [
            (
                Login::launch_agent(home.path()),
                plist_text(&exe()),
                home.path()
                    .join("Library/LaunchAgents/com.github.mattbran87.jev-tray.plist"),
            ),
            (
                Login::Autostart(home.path().join(".config/autostart/jev-tray.desktop")),
                desktop_text(&exe()),
                home.path().join(".config/autostart/jev-tray.desktop"),
            ),
        ];
        for (login, text, file) in entries {
            assert!(!login.is_on().unwrap());
            login.delete().unwrap();
            login.write(&exe()).unwrap();
            assert!(login.is_on().unwrap());
            assert_eq!(std::fs::read_to_string(&file).unwrap(), text);
            assert_eq!(login.read().unwrap(), Some(text));
            login.delete().unwrap();
            assert!(!login.is_on().unwrap());
            assert!(!file.exists());
        }
    }

    #[test]
    fn writing_again_replaces_another_path() {
        let home = tempfile::tempdir().unwrap();
        let login = Login::launch_agent(home.path());
        login.write(Path::new("/old/jev-tray")).unwrap();
        assert!(login.is_on().unwrap(), "another path still counts as on");
        login.write(&exe()).unwrap();
        assert_eq!(login.read().unwrap(), Some(plist_text(&exe())));
        let mut temp = login_file(&login).as_os_str().to_owned();
        temp.push(".tmp");
        assert!(!Path::new(&temp).exists(), "no temp file is left");
    }

    fn login_file(login: &Login) -> &Path {
        match login {
            Login::LaunchAgent(file) | Login::Autostart(file) => file,
            #[cfg(windows)]
            Login::Run { .. } => unreachable!(),
        }
    }

    /// Spec 4: any entry that exists counts as on, even one that is not text.
    #[test]
    fn a_file_entry_that_is_not_utf8_is_on() {
        let home = tempfile::tempdir().unwrap();
        for login in [
            Login::launch_agent(home.path()),
            Login::Autostart(home.path().join(".config/autostart/jev-tray.desktop")),
        ] {
            let file = login_file(&login);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, b"\xff\xfe[Desktop Entry]\n\x80").unwrap();
            assert!(login.is_on().unwrap());
            login.delete().unwrap();
            assert!(!login.is_on().unwrap());
        }
    }

    fn assert_invalid_input(result: io::Result<()>) {
        match result {
            Err(error) => assert_eq!(error.kind(), io::ErrorKind::InvalidInput),
            Ok(()) => panic!("the write did not refuse the path"),
        }
    }

    #[test]
    fn a_file_entry_refuses_a_path_with_a_control_character() {
        let home = tempfile::tempdir().unwrap();
        for login in [
            Login::launch_agent(home.path()),
            Login::Autostart(home.path().join(".config/autostart/jev-tray.desktop")),
        ] {
            for bad in [
                "/opt/a\nb/jev-tray",
                "/opt/a\rb/jev-tray",
                "/opt/a\tb/jev-tray",
            ] {
                assert_invalid_input(login.write(Path::new(bad)));
                assert!(!login.is_on().unwrap());
                assert!(!login_file(&login).parent().unwrap().exists());
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_file_entry_refuses_a_path_that_is_not_utf8() {
        use std::os::unix::ffi::OsStrExt;
        let home = tempfile::tempdir().unwrap();
        let bad = Path::new(std::ffi::OsStr::from_bytes(b"/opt/\xff/jev-tray"));
        for login in [
            Login::launch_agent(home.path()),
            Login::Autostart(home.path().join(".config/autostart/jev-tray.desktop")),
        ] {
            assert_invalid_input(login.write(bad));
            assert!(!login.is_on().unwrap());
        }
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    #[test]
    fn this_user_gets_an_autostart_file() {
        match Login::for_this_user() {
            Ok(Login::Autostart(file)) => assert!(file.ends_with("autostart/jev-tray.desktop")),
            other => panic!("{other:?}"),
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn this_user_gets_a_launch_agent() {
        match Login::for_this_user() {
            Ok(Login::LaunchAgent(file)) => {
                assert!(file.ends_with("Library/LaunchAgents/com.github.mattbran87.jev-tray.plist"))
            }
            other => panic!("{other:?}"),
        }
    }

    #[cfg(windows)]
    #[test]
    fn this_user_gets_the_run_value() {
        assert_eq!(
            Login::for_this_user().unwrap(),
            Login::Run {
                key: RUN_KEY.to_string(),
                name: "jev-tray".to_string()
            }
        );
    }

    /// A registry key of its own for one test, under `HKEY_CURRENT_USER`,
    /// never the real `Run` key. It is deleted before the test and after it.
    #[cfg(windows)]
    struct TestKey(String);

    #[cfg(windows)]
    impl TestKey {
        fn new(test: &str) -> TestKey {
            let key = format!(r"Software\jev-tray-test-{}-{test}", std::process::id());
            registry::delete_tree(&key);
            TestKey(key)
        }

        fn login(&self) -> Login {
            Login::Run {
                key: format!(r"{}\Run", self.0),
                name: NAME.to_string(),
            }
        }
    }

    #[cfg(windows)]
    impl Drop for TestKey {
        fn drop(&mut self) {
            registry::delete_tree(&self.0);
        }
    }

    #[cfg(windows)]
    #[test]
    fn a_registry_entry_is_written_read_and_deleted() {
        let key = TestKey::new("entry");
        let login = key.login();
        assert!(!login.is_on().unwrap());
        login.delete().unwrap();
        login.write(Path::new("/old/jev-tray.exe")).unwrap();
        assert!(login.is_on().unwrap());
        login.write(&exe()).unwrap();
        assert_eq!(
            login.read().unwrap(),
            Some(r#""C:\Program Files\jev\jev-tray.exe""#.to_string())
        );
        login.delete().unwrap();
        assert!(!login.is_on().unwrap());
    }

    /// Spec 4: any `jev-tray` value counts as on, whatever its type.
    #[cfg(windows)]
    #[test]
    fn a_registry_entry_of_another_type_is_on() {
        use windows_sys::Win32::System::Registry::{REG_BINARY, REG_EXPAND_SZ};
        let key = TestKey::new("types");
        let login = key.login();
        let Login::Run { key: run, name } = &login else {
            unreachable!()
        };
        let expand: Vec<u8> = "\"%LOCALAPPDATA%\\jev\\jev-tray.exe\"\0"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        for (kind, data) in [
            (REG_BINARY, &[0xff, 0xfe, 0x00][..]),
            (REG_EXPAND_SZ, &expand[..]),
        ] {
            registry::write_raw(run, name, kind, data).unwrap();
            assert!(login.is_on().unwrap(), "type {kind}");
            login.delete().unwrap();
            assert!(!login.is_on().unwrap());
        }
    }

    #[cfg(windows)]
    #[test]
    fn a_registry_entry_refuses_a_path_with_a_control_character() {
        let key = TestKey::new("control");
        let login = key.login();
        for bad in ["C:\\a\nb\\jev-tray.exe", "C:\\a\rb\\jev-tray.exe"] {
            assert_invalid_input(login.write(Path::new(bad)));
            assert!(!login.is_on().unwrap());
        }
    }
}
