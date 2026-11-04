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
            .filter(|home| !home.is_empty())
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME is not set"))?;
        Ok(Login::launch_agent(Path::new(&home)))
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

    /// True when the entry exists, whatever path it names.
    pub fn is_on(&self) -> io::Result<bool> {
        Ok(self.read()?.is_some())
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
    /// there already gets the new path.
    pub fn write(&self, exe: &Path) -> io::Result<()> {
        match self {
            Login::LaunchAgent(file) => write_file(file, &plist_text(exe)),
            Login::Autostart(file) => write_file(file, &desktop_text(exe)),
            #[cfg(windows)]
            Login::Run { key, name } => registry::write(key, name, &run_value(exe)),
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

fn write_file(file: &Path, text: &str) -> io::Result<()> {
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(file, text)
}

/// The Linux autostart folder: `$XDG_CONFIG_HOME/autostart`, or
/// `$HOME/.config/autostart`. The XDG rules ignore a relative
/// `XDG_CONFIG_HOME`. `var` stands in for the environment.
pub fn autostart_dir(var: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let set = |name: &str| var(name).filter(|value| !value.is_empty());
    if let Some(config) = set("XDG_CONFIG_HOME").map(PathBuf::from) {
        if config.is_absolute() {
            return Some(config.join("autostart"));
        }
    }
    set("HOME").map(|home| PathBuf::from(home).join(".config").join("autostart"))
}

/// The registry string: the full path of `jev-tray.exe`, quoted.
pub fn run_value(exe: &Path) -> String {
    format!("\"{}\"", exe.display())
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
    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegDeleteKeyValueW, RegGetValueW, RegSetValueExW, HKEY,
        HKEY_CURRENT_USER, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ, RRF_RT_REG_SZ,
    };

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(Some(0)).collect()
    }

    fn failed(status: u32) -> io::Error {
        io::Error::from_raw_os_error(status as i32)
    }

    pub fn read(key: &str, name: &str) -> io::Result<Option<String>> {
        let (key, name) = (wide(key), wide(name));
        let mut size: u32 = 0;
        // SAFETY: both names end in a zero; a null buffer asks for the size.
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut size,
            )
        };
        if status == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        if status != ERROR_SUCCESS {
            return Err(failed(status));
        }
        let mut buffer = vec![0u16; (size as usize).div_ceil(2)];
        let mut size = (buffer.len() * 2) as u32;
        // SAFETY: as above, and the buffer holds `size` bytes.
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
        if status == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        if status != ERROR_SUCCESS {
            return Err(failed(status));
        }
        let chars = (size as usize / 2).min(buffer.len());
        let text = &buffer[..chars];
        let end = text.iter().position(|&c| c == 0).unwrap_or(text.len());
        Ok(Some(String::from_utf16_lossy(&text[..end])))
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
            run_value(Path::new(r"C:\Program Files\jev\jev-tray.exe")),
            r#""C:\Program Files\jev\jev-tray.exe""#
        );
    }

    #[test]
    fn the_plist_runs_the_path_at_load() {
        let text = plist_text(Path::new("/Applications/jev & co/jev-tray"));
        assert!(text.contains("<string>com.github.mattbran87.jev-tray</string>"));
        assert!(text.contains("<string>/Applications/jev &amp; co/jev-tray</string>"));
        assert!(text.contains("<key>RunAtLoad</key>\n\t<true/>"));
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

    /// Uses a test key, never the real `Run` key.
    #[cfg(windows)]
    #[test]
    fn a_registry_entry_is_written_read_and_deleted() {
        const TEST_KEY: &str = r"Software\jev-tray-test";
        struct Cleanup;
        impl Drop for Cleanup {
            fn drop(&mut self) {
                registry::delete_tree(TEST_KEY);
            }
        }
        let _cleanup = Cleanup;
        let login = Login::Run {
            key: format!(r"{TEST_KEY}\Run"),
            name: NAME.to_string(),
        };
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
}
