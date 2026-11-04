//! The OS keyring (spec section 1, "Storage"), behind a trait so that tests
//! never touch a real credential store. `Zeroizing` wipes jevd's own copy of a
//! secret when it is dropped; copies made inside the OS keyring libraries are
//! not wiped, so this is best-effort, in line with the threat model (spec
//! section 10).

// keyring 3 falls back to an in-memory mock store on any other OS, so a key
// would be silently lost.
#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
compile_error!("jevd needs a real OS keyring: Windows, macOS, or Linux");

use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;

use zeroize::Zeroizing;

pub const SERVICE: &str = "jev";

/// The account that holds a profile's TypeSafe key.
pub fn profile_account(profile: &str) -> String {
    debug_assert!(jev_proto::valid_name(profile), "not a valid profile name");
    format!("profile:{profile}")
}

/// The account that holds a CLI client's token.
pub fn client_account(client: &str) -> String {
    debug_assert!(jev_proto::valid_name(client), "not a valid client name");
    format!("client:{client}")
}

/// A keyring failure. Each kind has a fixed message, so no secret can reach it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreError {
    /// The keyring is locked, or its service cannot be reached.
    Locked,
    /// An entry holds bytes that are not valid text.
    BadEncoding,
    /// The keyring refused the entry name or value.
    Refused,
    /// More than one entry matches the account.
    Ambiguous,
    /// Any other keyring failure.
    Other,
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            StoreError::Locked => "the keyring could not be reached; it may be locked",
            StoreError::BadEncoding => "a keyring entry is not valid text",
            StoreError::Refused => "the keyring refused the entry name or value",
            StoreError::Ambiguous => "the keyring holds more than one entry for this account",
            StoreError::Other => "the keyring failed",
        })
    }
}

impl std::error::Error for StoreError {}

pub trait KeyStore: Send + Sync {
    /// The secret for `account`, or `None` when the keyring has no entry.
    fn get(&self, account: &str) -> Result<Option<Zeroizing<String>>, StoreError>;
    fn set(&self, account: &str, secret: &str) -> Result<(), StoreError>;
    /// Deletes the entry. `false` when there was none.
    fn delete(&self, account: &str) -> Result<bool, StoreError>;
}

/// The real keyring: Windows Credential Manager, the macOS Keychain, or the
/// Secret Service on Linux. A call can block on a keyring prompt, so async
/// code runs it on a blocking thread.
pub struct OsKeyring;

fn entry(account: &str) -> Result<keyring::Entry, StoreError> {
    keyring::Entry::new(SERVICE, account).map_err(|error| store_error(&error))
}

/// The kind of each failure. The keyring's own text is never passed on,
/// because `BadEncoding` holds the stored bytes.
fn store_error(error: &keyring::Error) -> StoreError {
    match error {
        keyring::Error::PlatformFailure(_) | keyring::Error::NoStorageAccess(_) => {
            StoreError::Locked
        }
        keyring::Error::BadEncoding(_) => StoreError::BadEncoding,
        keyring::Error::TooLong(..) | keyring::Error::Invalid(..) => StoreError::Refused,
        keyring::Error::Ambiguous(_) => StoreError::Ambiguous,
        _ => StoreError::Other,
    }
}

impl KeyStore for OsKeyring {
    fn get(&self, account: &str) -> Result<Option<Zeroizing<String>>, StoreError> {
        match entry(account)?.get_password() {
            Ok(secret) => Ok(Some(Zeroizing::new(secret))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(store_error(&error)),
        }
    }

    fn set(&self, account: &str, secret: &str) -> Result<(), StoreError> {
        entry(account)?
            .set_password(secret)
            .map_err(|error| store_error(&error))
    }

    fn delete(&self, account: &str) -> Result<bool, StoreError> {
        match entry(account)?.delete_credential() {
            Ok(()) => Ok(true),
            Err(keyring::Error::NoEntry) => Ok(false),
            Err(error) => Err(store_error(&error)),
        }
    }
}

/// An in-memory keyring for the tests of `jevd` and `jev`. It counts reads,
/// and it can fail every call, as a locked keyring does.
#[derive(Default)]
pub struct MemoryKeyring {
    entries: Mutex<HashMap<String, String>>,
    failing: AtomicBool,
    reads: AtomicUsize,
}

impl MemoryKeyring {
    pub fn new() -> MemoryKeyring {
        MemoryKeyring::default()
    }

    /// While `true`, every call fails.
    pub fn set_failing(&self, failing: bool) {
        self.failing.store(failing, Ordering::SeqCst);
    }

    /// How many times `get` ran, failed calls included.
    pub fn reads(&self) -> usize {
        self.reads.load(Ordering::SeqCst)
    }

    fn check(&self) -> Result<(), StoreError> {
        if self.failing.load(Ordering::SeqCst) {
            return Err(StoreError::Locked);
        }
        Ok(())
    }

    fn entries(&self) -> std::sync::MutexGuard<'_, HashMap<String, String>> {
        self.entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl KeyStore for MemoryKeyring {
    fn get(&self, account: &str) -> Result<Option<Zeroizing<String>>, StoreError> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        self.check()?;
        Ok(self
            .entries()
            .get(account)
            .map(|secret| Zeroizing::new(secret.clone())))
    }

    fn set(&self, account: &str, secret: &str) -> Result<(), StoreError> {
        self.check()?;
        self.entries()
            .insert(account.to_string(), secret.to_string());
        Ok(())
    }

    fn delete(&self, account: &str) -> Result<bool, StoreError> {
        self.check()?;
        Ok(self.entries().remove(account).is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accounts_follow_the_spec() {
        assert_eq!(SERVICE, "jev");
        assert_eq!(profile_account("work"), "profile:work");
        assert_eq!(client_account("cc-jev-adhd"), "client:cc-jev-adhd");
    }

    #[test]
    fn the_memory_keyring_stores_and_deletes() {
        let store = MemoryKeyring::new();
        assert!(store.get("profile:work").unwrap().is_none());
        store.set("profile:work", "tsk-1").unwrap();
        store.set("profile:work", "tsk-2").unwrap();
        assert_eq!(
            store.get("profile:work").unwrap().expect("stored").as_str(),
            "tsk-2"
        );
        assert!(store.delete("profile:work").unwrap());
        assert!(!store.delete("profile:work").unwrap());
        assert!(store.get("profile:work").unwrap().is_none());
        assert_eq!(store.reads(), 3);
    }

    #[test]
    fn a_failing_memory_keyring_fails_every_call() {
        let store = MemoryKeyring::new();
        store.set("profile:work", "tsk-1").unwrap();
        store.set_failing(true);
        assert_eq!(store.get("profile:work").unwrap_err(), StoreError::Locked);
        assert_eq!(
            store.set("profile:work", "tsk-2").unwrap_err(),
            StoreError::Locked
        );
        assert_eq!(
            store.delete("profile:work").unwrap_err(),
            StoreError::Locked
        );
        store.set_failing(false);
        assert_eq!(
            store.get("profile:work").unwrap().expect("kept").as_str(),
            "tsk-1"
        );
    }

    #[test]
    fn a_failed_read_still_counts() {
        let store = MemoryKeyring::new();
        assert_eq!(store.reads(), 0);
        store.set_failing(true);
        assert!(store.get("profile:work").is_err());
        assert_eq!(store.reads(), 1);
    }

    #[test]
    fn each_store_error_has_a_fixed_message() {
        let cases = [
            (
                StoreError::Locked,
                "the keyring could not be reached; it may be locked",
            ),
            (StoreError::BadEncoding, "a keyring entry is not valid text"),
            (
                StoreError::Refused,
                "the keyring refused the entry name or value",
            ),
            (
                StoreError::Ambiguous,
                "the keyring holds more than one entry for this account",
            ),
            (StoreError::Other, "the keyring failed"),
        ];
        for (error, text) in cases {
            assert_eq!(error.to_string(), text);
            assert!(!text.contains('{') && !text.contains('}'), "{text}");
        }
    }

    #[test]
    fn keyring_errors_map_to_fixed_kinds() {
        let boxed = || -> Box<dyn std::error::Error + Send + Sync> { "tsk-secret".into() };
        let cases = [
            (keyring::Error::PlatformFailure(boxed()), StoreError::Locked),
            (keyring::Error::NoStorageAccess(boxed()), StoreError::Locked),
            (
                keyring::Error::BadEncoding(b"tsk-secret".to_vec()),
                StoreError::BadEncoding,
            ),
            (
                keyring::Error::TooLong("name".to_string(), 1),
                StoreError::Refused,
            ),
            (
                keyring::Error::Invalid("name".to_string(), "why".to_string()),
                StoreError::Refused,
            ),
            (keyring::Error::Ambiguous(Vec::new()), StoreError::Ambiguous),
            (keyring::Error::NoEntry, StoreError::Other),
        ];
        for (error, kind) in cases {
            assert_eq!(store_error(&error), kind, "{error}");
        }
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "not a valid profile name")]
    fn a_bad_profile_name_is_a_bug() {
        profile_account("Work:x");
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "not a valid client name")]
    fn a_bad_client_name_is_a_bug() {
        client_account("");
    }

    #[test]
    #[ignore = "touches the real OS keyring; run by hand with --ignored"]
    fn the_os_keyring_round_trips() {
        let account = format!("test:{}", std::process::id());
        let store = OsKeyring;
        store.set(&account, "secret-value").unwrap();
        assert_eq!(
            store.get(&account).unwrap().expect("stored").as_str(),
            "secret-value"
        );
        assert!(store.delete(&account).unwrap());
        assert!(store.get(&account).unwrap().is_none());
        assert!(!store.delete(&account).unwrap());
    }
}
