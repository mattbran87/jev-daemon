//! The settings file (spec section 5). The CLI writes it; jevd reads it on
//! start and on `reload`. It never holds a key.

use crate::files::write_atomic;
use crate::units::{Limit, Period, DEFAULT_RATE_LIMIT};
use jev_proto::valid_name;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Duration;

pub const SETTING_NAMES: [&str; 5] = [
    "idle-timeout",
    "key-cache",
    "rate-limit",
    "redact-strict",
    "max-connections",
];

const MAX_CONNECTIONS_LIMIT: u32 = 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub idle_timeout: Period,
    pub key_cache: Duration,
    pub rate_limit: Limit,
    pub redact_strict: bool,
    pub max_connections: u32,
    /// Profile names. The keyring cannot list entries on every OS.
    pub profiles: Vec<String>,
}

/// The result of a proposed change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub settings: Settings,
    /// True when the change weakens protection. The CLI then needs a terminal and a typed `y`.
    pub loosens: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", default, deny_unknown_fields)]
struct Raw {
    idle_timeout: Option<String>,
    key_cache: Option<String>,
    rate_limit: Option<String>,
    redact_strict: Option<bool>,
    max_connections: Option<u32>,
    profiles: Option<Vec<String>>,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            // jevd uses little memory and no CPU while it waits, so by default it keeps running.
            idle_timeout: Period::Off,
            key_cache: Duration::from_secs(2 * 3600),
            rate_limit: DEFAULT_RATE_LIMIT,
            redact_strict: false,
            max_connections: 32,
            profiles: Vec::new(),
        }
    }
}

fn on_off(value: bool) -> String {
    String::from(if value { "on" } else { "off" })
}

impl Settings {
    /// Read the file, or the defaults when it does not exist.
    pub fn load(path: &Path) -> Result<Settings, String> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Settings::default())
            }
            Err(error) => return Err(format!("cannot read {}: {error}", path.display())),
        };
        // Not `{error}`: its Display prints the offending source line, and a
        // key pasted into the file must never reach a log.
        let raw: Raw = toml::from_str(&text).map_err(|error| {
            let line = error.span().map_or(1, |span| {
                text[..span.start.min(text.len())].matches('\n').count() + 1
            });
            // A fixed text: some messages print the value.
            format!(
                "{}: line {line}: the file is not valid settings TOML",
                path.display()
            )
        })?;
        let mut settings = Settings::default();
        let fields = [
            ("idle-timeout", raw.idle_timeout),
            ("key-cache", raw.key_cache),
            ("rate-limit", raw.rate_limit),
            ("redact-strict", raw.redact_strict.map(on_off)),
            (
                "max-connections",
                raw.max_connections.map(|n| n.to_string()),
            ),
        ];
        for (name, value) in fields {
            if let Some(value) = value {
                settings = settings
                    .with_change(name, &value)
                    .map_err(|_| format!("{}: the value of {name} cannot be read", path.display()))?
                    .settings;
            }
        }
        for profile in raw.profiles.unwrap_or_default() {
            settings
                .add_profile(&profile)
                .map_err(|_| format!("{}: a profile name cannot be read", path.display()))?;
        }
        Ok(settings)
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let raw = Raw {
            idle_timeout: Some(self.idle_timeout.to_string()),
            key_cache: Some(Period::After(self.key_cache).to_string()),
            rate_limit: Some(self.rate_limit.to_string()),
            redact_strict: Some(self.redact_strict),
            max_connections: Some(self.max_connections),
            profiles: Some(self.profiles.clone()),
        };
        let text = toml::to_string(&raw).map_err(|error| error.to_string())?;
        write_atomic(path, &text)
            .map_err(|error| format!("cannot write {}: {error}", path.display()))
    }

    pub fn get(&self, name: &str) -> Option<String> {
        Some(match name {
            "idle-timeout" => self.idle_timeout.to_string(),
            "key-cache" => Period::After(self.key_cache).to_string(),
            "rate-limit" => self.rate_limit.to_string(),
            "redact-strict" => on_off(self.redact_strict),
            "max-connections" => self.max_connections.to_string(),
            _ => return None,
        })
    }

    /// Every setting and its value, in `SETTING_NAMES` order.
    pub fn list(&self) -> Vec<(&'static str, String)> {
        SETTING_NAMES
            .iter()
            .map(|name| {
                (
                    *name,
                    self.get(name).expect("every listed name has a value"),
                )
            })
            .collect()
    }

    /// The settings after `name = value`, and whether that loosens protection.
    pub fn with_change(&self, name: &str, value: &str) -> Result<Change, String> {
        let mut settings = self.clone();
        let loosens = match name {
            "idle-timeout" => {
                let period: Period = value.parse()?;
                settings.idle_timeout = period;
                period.looser_than(self.idle_timeout)
            }
            "key-cache" => {
                let Period::After(duration) = value.parse::<Period>()? else {
                    return Err("key-cache cannot be off".to_string());
                };
                settings.key_cache = duration;
                duration > self.key_cache
            }
            "rate-limit" => {
                let limit: Limit = value.parse()?;
                settings.rate_limit = limit;
                limit.looser_than(self.rate_limit)
            }
            "redact-strict" => {
                let strict = match value {
                    "on" => true,
                    "off" => false,
                    _ => return Err(format!("{value:?}: use on or off")),
                };
                settings.redact_strict = strict;
                self.redact_strict && !strict
            }
            "max-connections" => {
                let count: u32 = value
                    .parse()
                    .ok()
                    .filter(|count| (1..=MAX_CONNECTIONS_LIMIT).contains(count))
                    .ok_or_else(|| {
                        format!("{value:?}: use a number from 1 to {MAX_CONNECTIONS_LIMIT}")
                    })?;
                settings.max_connections = count;
                false
            }
            _ => return Err(format!("no setting named {name:?}")),
        };
        Ok(Change { settings, loosens })
    }

    /// Add a profile name. Adding a name that exists does nothing.
    pub fn add_profile(&mut self, name: &str) -> Result<(), String> {
        if !valid_name(name) {
            return Err(format!(
                "{name:?}: profile names use lowercase letters, digits, and hyphens"
            ));
        }
        if !self.profiles.iter().any(|profile| profile == name) {
            self.profiles.push(name.to_string());
            self.profiles.sort();
        }
        Ok(())
    }

    /// Remove a profile name. False when it was not there.
    pub fn remove_profile(&mut self, name: &str) -> bool {
        let before = self.profiles.len();
        self.profiles.retain(|profile| profile != name);
        self.profiles.len() != before
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(settings: &Settings, name: &str, value: &str) -> Change {
        settings.with_change(name, value).unwrap()
    }

    #[test]
    fn defaults_match_the_spec() {
        let settings = Settings::default();
        assert_eq!(settings.get("idle-timeout").unwrap(), "off");
        assert_eq!(settings.get("key-cache").unwrap(), "2h");
        assert_eq!(settings.get("rate-limit").unwrap(), "120/60");
        assert_eq!(settings.get("redact-strict").unwrap(), "off");
        assert_eq!(settings.get("max-connections").unwrap(), "32");
        assert_eq!(settings.get("nope"), None);
        let names: Vec<_> = settings.list().into_iter().map(|(name, _)| name).collect();
        assert_eq!(names, SETTING_NAMES);
    }

    #[test]
    fn a_parse_error_names_the_line_and_never_the_source_text() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            "idle-timeout = \"2h\"
key = \"tsk-secret-value-123\"
",
        )
        .unwrap();
        let error = Settings::load(&path).unwrap_err();
        assert!(!error.contains("tsk-secret-value-123"), "{error}");
        assert!(error.contains("line 2"), "{error}");
    }

    #[test]
    fn load_errors_never_repeat_a_value() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let cases = [
            (
                "rate-limit = \"tsk-secret-value-123\"\n",
                "rate-limit",
                "tsk-secret-value-123",
            ),
            (
                "profiles = [\"TSK_secret_value_123\"]\n",
                "profile",
                "TSK_secret_value_123",
            ),
            ("rate-limit = 123456789\n", "line 1", "123456789"),
        ];
        for (text, names, secret) in cases {
            std::fs::write(&path, text).unwrap();
            let error = Settings::load(&path).unwrap_err();
            assert!(!error.contains(secret), "{error}");
            assert!(error.contains(names), "{error}");
        }
    }

    #[test]
    fn a_missing_file_gives_the_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let loaded = Settings::load(&dir.path().join("settings.toml")).unwrap();
        assert_eq!(loaded, Settings::default());
    }

    #[test]
    fn settings_round_trip_through_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.toml");
        let mut settings = change(&Settings::default(), "idle-timeout", "off").settings;
        settings = change(&settings, "rate-limit", "10/60").settings;
        settings = change(&settings, "redact-strict", "on").settings;
        settings.add_profile("work").unwrap();
        settings.save(&path).unwrap();
        assert_eq!(Settings::load(&path).unwrap(), settings);
        assert!(!std::fs::read_to_string(&path).unwrap().contains("tsk"));
    }

    #[test]
    fn a_bad_file_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.toml");
        std::fs::write(&path, "rate-limit = \"fast\"\n").unwrap();
        assert!(Settings::load(&path).is_err());
    }

    #[test]
    fn loosening_changes_are_marked() {
        // An idle stop of 2h, so a longer one and `off` count as looser.
        let base = change(&Settings::default(), "idle-timeout", "2h").settings;
        let loosening = [
            ("rate-limit", "200/60"),
            ("rate-limit", "120/30"),
            ("idle-timeout", "off"),
            ("idle-timeout", "3h"),
            ("key-cache", "3h"),
        ];
        for (name, value) in loosening {
            assert!(change(&base, name, value).loosens, "{name} {value}");
        }
        let tightening = [
            ("rate-limit", "60/60"),
            ("idle-timeout", "1h"),
            ("key-cache", "30m"),
            ("redact-strict", "on"),
            ("max-connections", "200"),
            ("rate-limit", "120/60"),
        ];
        for (name, value) in tightening {
            assert!(!change(&base, name, value).loosens, "{name} {value}");
        }
        let strict = change(&base, "redact-strict", "on").settings;
        assert!(change(&strict, "redact-strict", "off").loosens);
    }

    #[test]
    fn bad_changes_are_errors() {
        let base = Settings::default();
        for (name, value) in [
            ("key-cache", "off"),
            ("rate-limit", "fast"),
            ("redact-strict", "yes"),
            ("max-connections", "0"),
            ("max-connections", "1025"),
            ("idle-timeout", "2d"),
            ("no-such-setting", "1"),
        ] {
            assert!(base.with_change(name, value).is_err(), "{name} {value}");
        }
    }

    #[test]
    fn profiles_stay_sorted_and_unique() {
        let mut settings = Settings::default();
        settings.add_profile("work").unwrap();
        settings.add_profile("default").unwrap();
        settings.add_profile("work").unwrap();
        assert_eq!(settings.profiles, ["default", "work"]);
        assert!(settings.add_profile("Bad Name").is_err());
        assert!(settings.remove_profile("work"));
        assert!(!settings.remove_profile("work"));
        assert_eq!(settings.profiles, ["default"]);
    }

    #[test]
    fn an_unchanged_value_does_not_loosen() {
        let base = Settings::default();
        assert!(!change(&base, "key-cache", "2h").loosens);
        assert!(!change(&base, "idle-timeout", "off").loosens);
        assert!(!change(&base, "redact-strict", "off").loosens);
        let strict = change(&base, "redact-strict", "on").settings;
        assert!(!change(&strict, "redact-strict", "on").loosens);
    }

    #[test]
    fn max_connections_accepts_both_ends_of_its_range() {
        let base = Settings::default();
        assert_eq!(
            change(&base, "max-connections", "1")
                .settings
                .max_connections,
            1
        );
        assert_eq!(
            change(&base, "max-connections", "1024")
                .settings
                .max_connections,
            1024
        );
    }

    #[test]
    fn every_setting_round_trips_through_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.toml");
        let mut settings = Settings::default();
        for (name, value) in [
            ("idle-timeout", "30m"),
            ("key-cache", "45m"),
            ("rate-limit", "7/9"),
            ("redact-strict", "on"),
            ("max-connections", "5"),
        ] {
            settings = change(&settings, name, value).settings;
        }
        settings.save(&path).unwrap();
        let loaded = Settings::load(&path).unwrap();
        assert_eq!(loaded, settings);
        for (name, value) in [("key-cache", "45m"), ("max-connections", "5")] {
            assert_eq!(loaded.get(name).unwrap(), value);
        }
    }

    #[test]
    fn a_file_that_is_not_toml_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.toml");
        std::fs::write(&path, "this is [not toml").unwrap();
        assert!(Settings::load(&path).is_err());
    }

    #[test]
    fn a_hand_edited_file_with_a_bad_key_or_value_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.toml");
        for text in [
            "redact_strict = true\n",
            "key-cache = \"off\"\n",
            "max-connections = 0\n",
        ] {
            std::fs::write(&path, text).unwrap();
            assert!(Settings::load(&path).is_err(), "{text:?}");
        }
    }
}
