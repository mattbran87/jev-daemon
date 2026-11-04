//! A sliding window of requests for each client (spec section 4, step 4).
//! Counts live in memory, so an idle exit or a restart resets them.

use crate::units::Limit;
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

#[derive(Debug, Default)]
pub struct RateLimiter {
    seen: HashMap<String, VecDeque<Instant>>,
}

impl RateLimiter {
    pub fn new() -> RateLimiter {
        RateLimiter::default()
    }

    /// Ok, and count the request, when `client` is under `limit`.
    /// Else Err with the whole seconds until a slot frees up, at least 1.
    pub fn check(&mut self, client: &str, limit: Limit, now: Instant) -> Result<(), u64> {
        let times = self.seen.entry(client.to_string()).or_default();
        while let Some(&first) = times.front() {
            if now.saturating_duration_since(first) >= limit.window {
                times.pop_front();
            } else {
                break;
            }
        }
        if times.len() >= limit.count as usize {
            // The entry whose expiry brings the length down to count - 1. After a
            // lowered limit that is not the front.
            let first = times[times.len() - limit.count as usize];
            let wait: Duration = limit
                .window
                .saturating_sub(now.saturating_duration_since(first));
            return Err(wait.as_secs() + u64::from(wait.subsec_nanos() > 0));
        }
        times.push_back(now);
        Ok(())
    }

    /// Drop a client's counts, for example after `revoke`.
    pub fn forget(&mut self, client: &str) {
        self.seen.remove(client);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::DEFAULT_RATE_LIMIT;

    fn limit(text: &str) -> Limit {
        text.parse().unwrap()
    }

    #[test]
    fn the_121st_request_in_a_window_is_refused() {
        let mut limiter = RateLimiter::new();
        let t0 = Instant::now();
        for _ in 0..120 {
            assert_eq!(limiter.check("a", DEFAULT_RATE_LIMIT, t0), Ok(()));
        }
        assert_eq!(limiter.check("a", DEFAULT_RATE_LIMIT, t0), Err(60));
    }

    #[test]
    fn the_window_slides() {
        let mut limiter = RateLimiter::new();
        let t0 = Instant::now();
        for _ in 0..120 {
            limiter.check("a", DEFAULT_RATE_LIMIT, t0).unwrap();
        }
        assert!(limiter
            .check("a", DEFAULT_RATE_LIMIT, t0 + Duration::from_secs(59))
            .is_err());
        assert_eq!(
            limiter.check("a", DEFAULT_RATE_LIMIT, t0 + Duration::from_secs(60)),
            Ok(())
        );
    }

    #[test]
    fn retry_after_rounds_up_and_never_exceeds_the_window() {
        let mut limiter = RateLimiter::new();
        let t0 = Instant::now();
        limiter.check("a", limit("1/60"), t0).unwrap();
        assert_eq!(
            limiter.check("a", limit("1/60"), t0 + Duration::from_millis(59_500)),
            Err(1)
        );
        assert_eq!(
            limiter.check("a", limit("1/60"), t0 + Duration::from_millis(500)),
            Err(60)
        );
    }

    #[test]
    fn clients_count_apart() {
        let mut limiter = RateLimiter::new();
        let t0 = Instant::now();
        limiter.check("a", limit("1/60"), t0).unwrap();
        assert_eq!(limiter.check("b", limit("1/60"), t0), Ok(()));
    }

    #[test]
    fn an_override_changes_the_count() {
        let mut limiter = RateLimiter::new();
        let t0 = Instant::now();
        for _ in 0..5 {
            limiter.check("a", limit("5/60"), t0).unwrap();
        }
        assert!(limiter.check("a", limit("5/60"), t0).is_err());
        assert_eq!(limiter.check("a", limit("6/60"), t0), Ok(()));
    }

    #[test]
    fn lowered_limit_reports_the_true_wait() {
        let mut limiter = RateLimiter::new();
        let t0 = Instant::now();
        for seconds in [0, 10, 20, 30, 40] {
            limiter
                .check("a", limit("5/60"), t0 + Duration::from_secs(seconds))
                .unwrap();
        }
        // Five entries, new count 2: the t0+30s entry must expire first, at t0+90s.
        assert_eq!(
            limiter.check("a", limit("2/60"), t0 + Duration::from_secs(45)),
            Err(45)
        );
    }

    #[test]
    fn a_refused_call_uses_no_slot() {
        let mut limiter = RateLimiter::new();
        let t0 = Instant::now();
        assert_eq!(limiter.check("a", limit("1/60"), t0), Ok(()));
        assert!(limiter
            .check("a", limit("1/60"), t0 + Duration::from_secs(30))
            .is_err());
        assert_eq!(
            limiter.check("a", limit("1/60"), t0 + Duration::from_secs(60)),
            Ok(())
        );
    }

    #[test]
    fn forget_clears_a_client() {
        let mut limiter = RateLimiter::new();
        let t0 = Instant::now();
        limiter.check("a", limit("1/60"), t0).unwrap();
        limiter.forget("a");
        assert_eq!(limiter.check("a", limit("1/60"), t0), Ok(()));
    }
}
