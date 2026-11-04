//! The key cache (spec section 2, "Key cache"). A profile's key stays in
//! memory after its first request, and goes after `key-cache` with no use.
//!
//! The keyring read runs outside the cache: `lookup` claims the read, the
//! caller runs `read_key` with no lock held, and `finish` stores the result.
//! A keyring prompt then stalls only the requests of that one profile.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use jev_proto::{ErrorCode, WireError};
use zeroize::Zeroizing;

use crate::key::check_key;
use crate::keystore::{profile_account, KeyStore};

/// An in-flight read older than this counts as abandoned, so a lost ticket
/// cannot block a profile forever. It is long because a person can leave a
/// keyring prompt open for minutes. A caller whose read was cancelled calls
/// `abandon`, so this limit covers only tickets that are lost.
pub const READ_LIMIT: Duration = Duration::from_secs(600);

/// What `lookup` found.
///
/// In the daemon (plan 3), a request never calls `get`. On `Miss`, the daemon
/// runs `read_key` and `finish` in a detached task that the request does not
/// own, so a cancelled request still fills the cache. If the blocking read
/// panics (a `JoinError`), the detached task calls `abandon`.
///
/// The daemon owns a `tokio::sync::watch` that carries the outcome of each
/// read, such as `(read_id, Result<(), WireError>)`. The detached task
/// publishes its ticket's `read_id` and the outcome after `finish`, and the
/// daemon also raises the watch after `abandon`, `forget`, `clear`, and
/// `reload`. On `Wait(read)`, the request remembers `read` and awaits
/// `tokio::time::timeout(READ_LIMIT, rx.changed())`. If the watch shows that
/// `read` failed, the request replies with the same error and does not call
/// `lookup` again, so one failed read prompts once. In every other case,
/// a timeout included, it calls `lookup` again: `sweep` dropping an expired
/// mark does not raise the watch, and the watch keeps only its newest value.
/// A request that arrives after the failure gets `Miss` and reads again.
pub enum Lookup {
    /// The cached key. Its period restarts. Each hit is a separate
    /// `Zeroizing` copy, wiped when the caller drops it. A copy that the
    /// caller makes from it, such as the HTTP header value, is not wiped.
    Hit(Zeroizing<String>),
    /// No key: read it with `read_key` off the lock, then call `finish`.
    Miss(Ticket),
    /// Another read of this profile is in flight, with this id. Wait for its
    /// outcome, as above.
    Wait(u64),
}

/// Proof of a claimed read. It names the profile and the read.
#[must_use = "pass the ticket to finish or abandon"]
pub struct Ticket {
    profile: String,
    read: u64,
}

impl Ticket {
    /// The read's id, which a `Wait` for this read carries.
    pub fn read_id(&self) -> u64 {
        self.read
    }
}

/// Reads and checks a profile's key, with no cache access, so the caller can
/// run it on a blocking thread with no lock held. The result is the trimmed
/// key that `check_key` returns. A missing or blank key is `no-key`, a key of
/// a bad shape is `bad-key`, and a keyring failure is `internal`, with the
/// keyring's message. No error message holds key text.
///
/// The key check runs here, so the caller has its result before it takes a
/// rate-limit slot (spec section 4, step 3). `profile` must be a name that
/// passed `jev_proto::valid_name`.
pub fn read_key(store: &dyn KeyStore, profile: &str) -> Result<Zeroizing<String>, WireError> {
    let no_key = || {
        WireError::new(
            ErrorCode::NoKey,
            format!("profile {profile} has no key; run jev key add --profile {profile}"),
        )
    };
    let raw = store
        .get(&profile_account(profile))
        .map_err(|error| {
            WireError::new(
                ErrorCode::Internal,
                format!("cannot read the key of profile {profile}: {error}"),
            )
        })?
        .ok_or_else(no_key)?;
    let key = check_key(&raw).map_err(|code| match code {
        ErrorCode::NoKey => no_key(),
        code => WireError::new(
            code,
            format!("the key of profile {profile} is not a valid key; run jev key add --profile {profile}"),
        ),
    })?;
    Ok(Zeroizing::new(key.to_string()))
}

struct Cached {
    key: Zeroizing<String>,
    last_used: Instant,
}

/// Keys in memory. A key is wiped from memory when it leaves the cache.
///
/// A key leaves only when something calls the cache, so the daemon must call
/// `sweep` from a timer, and set the timer from `next_expiry`.
pub struct KeyCache {
    period: Duration,
    entries: HashMap<String, Cached>,
    /// The profiles with a read in flight: the read's id, and when it started.
    pending: HashMap<String, (u64, Instant)>,
    /// The id of the next read.
    next_read: u64,
}

impl KeyCache {
    pub fn new(period: Duration) -> KeyCache {
        KeyCache {
            period,
            entries: HashMap::new(),
            pending: HashMap::new(),
            next_read: 0,
        }
    }

    /// Sweeps, then returns a hit (and restarts its period), a miss with a
    /// ticket (and marks the profile in flight), or `Wait` when a read of the
    /// profile started less than `READ_LIMIT` ago. See `Lookup` for how the
    /// daemon uses each answer.
    pub fn lookup(&mut self, profile: &str, now: Instant) -> Lookup {
        self.sweep(now);
        if let Some(cached) = self.entries.get_mut(profile) {
            cached.last_used = now;
            return Lookup::Hit(cached.key.clone());
        }
        if let Some(&(read, _)) = self.pending.get(profile) {
            return Lookup::Wait(read);
        }
        let read = self.next_read;
        self.next_read = self.next_read.wrapping_add(1);
        self.pending.insert(profile.to_string(), (read, now));
        Lookup::Miss(Ticket {
            profile: profile.to_string(),
            read,
        })
    }

    /// Whether the in-flight mark of the ticket's profile is this read's.
    fn owns(&self, ticket: &Ticket) -> bool {
        self.pending.get(&ticket.profile).map(|pending| pending.0) == Some(ticket.read)
    }

    /// Ends a read. If the profile's in-flight mark is still this read's, it
    /// clears the mark and stores an `Ok` key. A failure is never stored.
    /// Otherwise it changes nothing: a `forget`, `clear`, or `reload` of the
    /// profile ran since the claim, or the read passed `READ_LIMIT`, and a mark
    /// there now belongs to a newer read. A read that ends after its mark
    /// expired and was dropped is not stored, but its result still goes back
    /// to the caller. Returns the result unchanged in all cases, so the caller
    /// can reply with it.
    pub fn finish(
        &mut self,
        ticket: Ticket,
        result: Result<Zeroizing<String>, WireError>,
        now: Instant,
    ) -> Result<Zeroizing<String>, WireError> {
        if !self.owns(&ticket) {
            return result;
        }
        self.pending.remove(&ticket.profile);
        if let Ok(key) = &result {
            self.entries.insert(
                ticket.profile,
                Cached {
                    key: key.clone(),
                    last_used: now,
                },
            );
        }
        result
    }

    /// Ends a read with no result, for a caller whose read was cancelled. It
    /// clears the in-flight mark only if the mark is still this read's.
    pub fn abandon(&mut self, ticket: Ticket) {
        if self.owns(&ticket) {
            self.pending.remove(&ticket.profile);
        }
    }

    /// `lookup`, `read_key`, and `finish` in one call, for single-threaded
    /// callers (tests, the CLI). The daemon does not call it (see `Lookup`).
    /// On `Wait` it reads without caching. Each key it returns is a separate
    /// `Zeroizing` copy; a copy that the caller makes from it, such as the
    /// HTTP header value, is not wiped.
    ///
    /// As with `read_key`, the key check runs before the caller takes a
    /// rate-limit slot (spec section 4, step 3), and `profile` must be a name
    /// that passed `jev_proto::valid_name`.
    pub fn get(
        &mut self,
        profile: &str,
        store: &dyn KeyStore,
        now: Instant,
    ) -> Result<Zeroizing<String>, WireError> {
        match self.lookup(profile, now) {
            Lookup::Hit(key) => Ok(key),
            Lookup::Miss(ticket) => {
                let result = read_key(store, profile);
                self.finish(ticket, result, now)
            }
            Lookup::Wait(_) => read_key(store, profile),
        }
    }

    /// Wipes every key whose last use is a full period ago, and drops every
    /// in-flight mark that started `READ_LIMIT` or more ago. A key is wiped
    /// only when something calls the cache, so the daemon must call this from
    /// a timer, set from `next_expiry`.
    pub fn sweep(&mut self, now: Instant) {
        let period = self.period;
        self.entries
            .retain(|_, cached| now.saturating_duration_since(cached.last_used) < period);
        self.pending
            .retain(|_, pending| now.saturating_duration_since(pending.1) < READ_LIMIT);
    }

    /// The earliest instant a cached key expires, so the daemon can sleep
    /// until then. `None` when no key is cached.
    pub fn next_expiry(&self) -> Option<Instant> {
        self.entries
            .values()
            .filter_map(|cached| cached.last_used.checked_add(self.period))
            .min()
    }

    /// Wipes one profile's key and its in-flight mark, for `jev key remove`.
    pub fn forget(&mut self, profile: &str) {
        self.entries.remove(profile);
        self.pending.remove(profile);
    }

    /// Wipes every key and every in-flight mark, for exit.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.pending.clear();
    }

    /// Sets the period and clears every key, for `reload`.
    pub fn reload(&mut self, period: Duration) {
        self.period = period;
        self.clear();
    }

    /// The number of cached keys. In-flight reads do not count.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keystore::MemoryKeyring;
    use jev_proto::WireCode;

    const SECOND: Duration = Duration::from_secs(1);
    const MINUTE: Duration = Duration::from_secs(60);
    const HOUR: Duration = Duration::from_secs(3600);

    fn store_with(key: &str) -> MemoryKeyring {
        let store = MemoryKeyring::new();
        store.set(&profile_account("work"), key).unwrap();
        store
    }

    fn key(text: &str) -> Result<Zeroizing<String>, WireError> {
        Ok(Zeroizing::new(text.to_string()))
    }

    fn ticket(lookup: Lookup) -> Ticket {
        match lookup {
            Lookup::Miss(ticket) => ticket,
            Lookup::Hit(_) => panic!("expected a miss, got a hit"),
            Lookup::Wait(_) => panic!("expected a miss, got a wait"),
        }
    }

    fn hit(lookup: Lookup) -> Zeroizing<String> {
        match lookup {
            Lookup::Hit(key) => key,
            Lookup::Miss(_) => panic!("expected a hit, got a miss"),
            Lookup::Wait(_) => panic!("expected a hit, got a wait"),
        }
    }

    #[test]
    fn the_first_request_reads_the_keyring_and_later_ones_do_not() {
        let store = store_with("tsk-work");
        let mut cache = KeyCache::new(2 * HOUR);
        let t0 = Instant::now();
        assert_eq!(cache.get("work", &store, t0).unwrap().as_str(), "tsk-work");
        assert_eq!(
            cache.get("work", &store, t0 + HOUR).unwrap().as_str(),
            "tsk-work"
        );
        assert_eq!(store.reads(), 1);
    }

    #[test]
    fn a_key_unused_for_the_whole_period_is_read_again() {
        let store = store_with("tsk-work");
        let mut cache = KeyCache::new(2 * HOUR);
        let t0 = Instant::now();
        cache.get("work", &store, t0).unwrap();
        cache.get("work", &store, t0 + 2 * HOUR).unwrap();
        assert_eq!(store.reads(), 2);
    }

    #[test]
    fn each_use_restarts_the_period() {
        let store = store_with("tsk-work");
        let mut cache = KeyCache::new(2 * HOUR);
        let t0 = Instant::now();
        cache.get("work", &store, t0).unwrap();
        cache.get("work", &store, t0 + 90 * MINUTE).unwrap();
        cache.get("work", &store, t0 + 180 * MINUTE).unwrap();
        assert_eq!(store.reads(), 1);
        cache.get("work", &store, t0 + 300 * MINUTE).unwrap();
        assert_eq!(store.reads(), 2);
    }

    #[test]
    fn the_stored_key_is_trimmed() {
        let store = store_with("  tsk-work\n");
        assert_eq!(read_key(&store, "work").unwrap().as_str(), "tsk-work");
        let mut cache = KeyCache::new(2 * HOUR);
        assert_eq!(
            cache.get("work", &store, Instant::now()).unwrap().as_str(),
            "tsk-work"
        );
    }

    #[test]
    fn a_missing_or_blank_key_is_no_key() {
        let empty = MemoryKeyring::new();
        let error = KeyCache::new(2 * HOUR)
            .get("work", &empty, Instant::now())
            .unwrap_err();
        assert_eq!(error.code, WireCode::Known(ErrorCode::NoKey));
        assert!(
            error.message.contains("jev key add --profile work"),
            "{}",
            error.message
        );
        let blank = store_with("   ");
        let error = KeyCache::new(2 * HOUR)
            .get("work", &blank, Instant::now())
            .unwrap_err();
        assert_eq!(error.code, WireCode::Known(ErrorCode::NoKey));
    }

    #[test]
    fn a_bad_key_is_not_kept() {
        let store = store_with("a b");
        let mut cache = KeyCache::new(2 * HOUR);
        let t0 = Instant::now();
        let error = cache.get("work", &store, t0).unwrap_err();
        assert_eq!(error.code, WireCode::Known(ErrorCode::BadKey));
        assert!(!error.message.contains("a b"), "{}", error.message);
        assert!(cache.is_empty());
        store.set(&profile_account("work"), "tsk-fixed").unwrap();
        assert_eq!(cache.get("work", &store, t0).unwrap().as_str(), "tsk-fixed");
    }

    #[test]
    fn a_keyring_failure_is_internal_and_not_kept() {
        let store = store_with("tsk-work");
        store.set_failing(true);
        let mut cache = KeyCache::new(2 * HOUR);
        let t0 = Instant::now();
        let error = cache.get("work", &store, t0).unwrap_err();
        assert_eq!(error.code, WireCode::Known(ErrorCode::Internal));
        assert!(error.message.contains("locked"), "{}", error.message);
        assert!(cache.is_empty());
        store.set_failing(false);
        assert_eq!(cache.get("work", &store, t0).unwrap().as_str(), "tsk-work");
    }

    #[test]
    fn forget_and_clear_wipe_keys() {
        let store = store_with("tsk-work");
        store.set(&profile_account("home"), "tsk-home").unwrap();
        let mut cache = KeyCache::new(2 * HOUR);
        let t0 = Instant::now();
        assert_eq!(cache.get("home", &store, t0).unwrap().as_str(), "tsk-home");
        assert_eq!(cache.get("work", &store, t0).unwrap().as_str(), "tsk-work");
        assert_eq!(cache.len(), 2);
        cache.forget("work");
        assert_eq!(cache.len(), 1);
        cache.get("work", &store, t0).unwrap();
        assert_eq!(store.reads(), 3);
        cache.clear();
        assert!(cache.is_empty());
    }

    #[test]
    fn sweep_wipes_only_keys_past_the_period() {
        let store = store_with("tsk-work");
        store.set(&profile_account("home"), "tsk-home").unwrap();
        let mut cache = KeyCache::new(2 * HOUR);
        let t0 = Instant::now();
        cache.get("work", &store, t0).unwrap();
        cache.get("home", &store, t0 + HOUR).unwrap();
        cache.sweep(t0 + 2 * HOUR);
        assert_eq!(cache.len(), 1);
        cache.get("home", &store, t0 + 2 * HOUR).unwrap();
        assert_eq!(store.reads(), 2);
        // `reload` sets the period and also clears every key.
        cache.reload(MINUTE);
        assert!(cache.is_empty());
        let t1 = t0 + 2 * HOUR;
        cache.get("home", &store, t1).unwrap();
        assert_eq!(store.reads(), 3);
        cache.sweep(t1 + MINUTE - SECOND);
        assert_eq!(cache.len(), 1);
        cache.sweep(t1 + MINUTE);
        assert!(cache.is_empty());
    }

    #[test]
    fn a_miss_marks_the_profile_so_a_second_lookup_waits() {
        let mut cache = KeyCache::new(2 * HOUR);
        let t0 = Instant::now();
        let _ticket = ticket(cache.lookup("work", t0));
        assert!(matches!(cache.lookup("work", t0 + MINUTE), Lookup::Wait(_)));
        let _other = ticket(cache.lookup("home", t0 + MINUTE));
        assert!(cache.is_empty());
    }

    #[test]
    fn a_wait_carries_the_id_of_the_read_in_flight() {
        let mut cache = KeyCache::new(2 * HOUR);
        let t0 = Instant::now();
        let home = ticket(cache.lookup("home", t0));
        let old = ticket(cache.lookup("work", t0));
        assert!(matches!(cache.lookup("home", t0), Lookup::Wait(id) if id == home.read_id()));
        assert!(matches!(cache.lookup("work", t0), Lookup::Wait(id) if id == old.read_id()));
        cache.forget("work");
        let new = ticket(cache.lookup("work", t0));
        assert_ne!(new.read_id(), old.read_id());
        assert!(matches!(cache.lookup("work", t0), Lookup::Wait(id) if id == new.read_id()));
        cache.abandon(home);
        cache.abandon(old);
        cache.abandon(new);
    }

    #[test]
    fn after_finish_the_next_lookup_is_a_hit() {
        let mut cache = KeyCache::new(2 * HOUR);
        let t0 = Instant::now();
        let claim = ticket(cache.lookup("work", t0));
        let returned = cache.finish(claim, key("tsk-work"), t0 + MINUTE).unwrap();
        assert_eq!(returned.as_str(), "tsk-work");
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.next_expiry(), Some(t0 + MINUTE + 2 * HOUR));
        assert_eq!(hit(cache.lookup("work", t0 + MINUTE)).as_str(), "tsk-work");
    }

    #[test]
    fn a_wait_turns_into_a_miss_after_the_read_limit() {
        let mut cache = KeyCache::new(2 * HOUR);
        let t0 = Instant::now();
        let _lost = ticket(cache.lookup("work", t0));
        assert!(matches!(
            cache.lookup("work", t0 + READ_LIMIT - SECOND),
            Lookup::Wait(_)
        ));
        let _retry = ticket(cache.lookup("work", t0 + READ_LIMIT));
        assert!(matches!(
            cache.lookup("work", t0 + READ_LIMIT + SECOND),
            Lookup::Wait(_)
        ));
    }

    #[test]
    fn a_read_open_for_minutes_still_holds_the_profile() {
        // A person can leave a keyring prompt open for minutes.
        let mut cache = KeyCache::new(2 * HOUR);
        let t0 = Instant::now();
        let _claim = ticket(cache.lookup("work", t0));
        assert!(matches!(
            cache.lookup("work", t0 + 9 * MINUTE),
            Lookup::Wait(_)
        ));
        let _retry = ticket(cache.lookup("work", t0 + 10 * MINUTE));
    }

    #[test]
    fn a_failed_finish_stores_nothing_and_the_next_lookup_is_a_miss() {
        let mut cache = KeyCache::new(2 * HOUR);
        let t0 = Instant::now();
        let claim = ticket(cache.lookup("work", t0));
        let failure = WireError::new(ErrorCode::Internal, "the keyring is locked".to_string());
        let error = cache.finish(claim, Err(failure), t0).unwrap_err();
        assert_eq!(error.code, WireCode::Known(ErrorCode::Internal));
        assert_eq!(error.message, "the keyring is locked");
        assert!(cache.is_empty());
        let _retry = ticket(cache.lookup("work", t0));
    }

    #[test]
    fn forget_between_miss_and_finish_keeps_the_key_out() {
        let mut cache = KeyCache::new(2 * HOUR);
        let t0 = Instant::now();
        let old = ticket(cache.lookup("work", t0));
        cache.forget("work");
        let new = ticket(cache.lookup("work", t0));
        assert_eq!(
            cache.finish(old, key("tsk-old"), t0).unwrap().as_str(),
            "tsk-old"
        );
        assert!(cache.is_empty());
        // The stale ticket leaves the newer read's mark in place.
        assert!(matches!(cache.lookup("work", t0), Lookup::Wait(_)));
        cache.finish(new, key("tsk-new"), t0).unwrap();
        assert_eq!(hit(cache.lookup("work", t0)).as_str(), "tsk-new");
    }

    #[test]
    fn clear_between_miss_and_finish_keeps_the_key_out() {
        let mut cache = KeyCache::new(2 * HOUR);
        let t0 = Instant::now();
        let old = ticket(cache.lookup("work", t0));
        cache.clear();
        let new = ticket(cache.lookup("work", t0));
        assert_eq!(
            cache.finish(old, key("tsk-old"), t0).unwrap().as_str(),
            "tsk-old"
        );
        assert!(cache.is_empty());
        assert!(matches!(cache.lookup("work", t0), Lookup::Wait(_)));
        cache.finish(new, key("tsk-new"), t0).unwrap();
        assert_eq!(hit(cache.lookup("work", t0)).as_str(), "tsk-new");
    }

    #[test]
    fn reload_between_miss_and_finish_keeps_the_key_out() {
        let mut cache = KeyCache::new(2 * HOUR);
        let t0 = Instant::now();
        let old = ticket(cache.lookup("work", t0));
        cache.reload(HOUR);
        let _new = ticket(cache.lookup("work", t0));
        cache.finish(old, key("tsk-old"), t0).unwrap();
        assert!(cache.is_empty());
    }

    #[test]
    fn forgetting_one_profile_leaves_another_profiles_read_alone() {
        let mut cache = KeyCache::new(MINUTE);
        let t0 = Instant::now();
        let claim = ticket(cache.lookup("home", t0));
        cache.forget("work");
        cache.finish(claim, key("tsk-home"), t0).unwrap();
        assert_eq!(hit(cache.lookup("home", t0)).as_str(), "tsk-home");
        // The key goes after the period, and no mark is left behind.
        let _retry = ticket(cache.lookup("home", t0 + MINUTE));
    }

    #[test]
    fn a_ticket_past_the_read_limit_does_not_touch_the_newer_read() {
        let mut cache = KeyCache::new(2 * HOUR);
        let t0 = Instant::now();
        let old = ticket(cache.lookup("work", t0));
        let new = ticket(cache.lookup("work", t0 + READ_LIMIT));
        let late = t0 + READ_LIMIT + SECOND;
        assert_eq!(
            cache.finish(old, key("tsk-old"), late).unwrap().as_str(),
            "tsk-old"
        );
        assert!(cache.is_empty());
        assert!(matches!(cache.lookup("work", late), Lookup::Wait(_)));
        cache.finish(new, key("tsk-new"), late).unwrap();
        assert_eq!(hit(cache.lookup("work", late)).as_str(), "tsk-new");
    }

    #[test]
    fn abandon_clears_only_its_own_mark() {
        let mut cache = KeyCache::new(2 * HOUR);
        let t0 = Instant::now();
        let claim = ticket(cache.lookup("work", t0));
        cache.abandon(claim);
        assert!(cache.is_empty());
        let old = ticket(cache.lookup("work", t0));
        cache.forget("work");
        let _new = ticket(cache.lookup("work", t0));
        cache.abandon(old);
        assert!(matches!(cache.lookup("work", t0), Lookup::Wait(_)));
    }

    #[test]
    fn sweep_drops_marks_at_the_read_limit() {
        let mut cache = KeyCache::new(2 * HOUR);
        let t0 = Instant::now();
        let work = ticket(cache.lookup("work", t0));
        let home = ticket(cache.lookup("home", t0 + SECOND));
        let late = t0 + READ_LIMIT;
        cache.sweep(late);
        // The work mark is gone, so its read is not stored.
        cache.finish(work, key("tsk-work"), late).unwrap();
        cache.finish(home, key("tsk-home"), late).unwrap();
        assert_eq!(cache.len(), 1);
        assert_eq!(hit(cache.lookup("home", late)).as_str(), "tsk-home");
    }

    #[test]
    fn next_expiry_is_the_oldest_key_plus_the_period() {
        let store = store_with("tsk-work");
        store.set(&profile_account("home"), "tsk-home").unwrap();
        let mut cache = KeyCache::new(2 * HOUR);
        assert_eq!(cache.next_expiry(), None);
        let t0 = Instant::now();
        cache.get("work", &store, t0).unwrap();
        cache.get("home", &store, t0 + HOUR).unwrap();
        let _in_flight = ticket(cache.lookup("other", t0 + HOUR));
        assert_eq!(cache.next_expiry(), Some(t0 + 2 * HOUR));
        cache.forget("work");
        assert_eq!(cache.next_expiry(), Some(t0 + 3 * HOUR));
    }

    #[test]
    fn get_on_wait_reads_the_key_and_stores_nothing() {
        let store = store_with("tsk-work");
        let mut cache = KeyCache::new(2 * HOUR);
        let t0 = Instant::now();
        let claim = ticket(cache.lookup("work", t0));
        assert_eq!(cache.get("work", &store, t0).unwrap().as_str(), "tsk-work");
        assert_eq!(store.reads(), 1);
        assert!(cache.is_empty());
        assert!(matches!(cache.lookup("work", t0), Lookup::Wait(_)));
        cache.finish(claim, key("tsk-work"), t0).unwrap();
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn forgetting_an_unknown_profile_is_fine() {
        let store = store_with("tsk-work");
        let mut cache = KeyCache::new(2 * HOUR);
        let t0 = Instant::now();
        cache.get("work", &store, t0).unwrap();
        cache.forget("nobody");
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn a_now_earlier_than_the_last_use_does_not_panic() {
        let store = store_with("tsk-work");
        let mut cache = KeyCache::new(2 * HOUR);
        let t0 = Instant::now();
        cache.get("work", &store, t0 + HOUR).unwrap();
        assert_eq!(cache.get("work", &store, t0).unwrap().as_str(), "tsk-work");
        cache.sweep(t0);
        assert_eq!(cache.len(), 1);
        assert_eq!(store.reads(), 1);
        let _claim = ticket(cache.lookup("home", t0 + HOUR));
        assert!(matches!(cache.lookup("home", t0), Lookup::Wait(_)));
    }
}
