//! Setting values: periods such as `2h`, and rate limits such as `120/60`.

use std::fmt;
use std::str::FromStr;
use std::time::Duration;

/// A period in minutes or hours, or `off`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Period {
    Off,
    After(Duration),
}

const MAX_PERIOD_NUMBER: u64 = 10_000;

impl FromStr for Period {
    type Err = String;

    fn from_str(text: &str) -> Result<Period, String> {
        if text == "off" {
            return Ok(Period::Off);
        }
        let problem = || format!("{text:?}: use minutes or hours, such as 30m or 2h, or off");
        let (number, seconds_each) = if let Some(number) = text.strip_suffix('h') {
            (number, 3600)
        } else if let Some(number) = text.strip_suffix('m') {
            (number, 60)
        } else {
            return Err(problem());
        };
        if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
            return Err(problem());
        }
        let count: u64 = number.parse().map_err(|_| problem())?;
        if count == 0 || count > MAX_PERIOD_NUMBER {
            return Err(problem());
        }
        Ok(Period::After(Duration::from_secs(count * seconds_each)))
    }
}

impl fmt::Display for Period {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Period::Off => write!(f, "off"),
            Period::After(duration) if duration.as_secs() % 3600 == 0 => {
                write!(f, "{}h", duration.as_secs() / 3600)
            }
            Period::After(duration) => write!(f, "{}m", duration.as_secs() / 60),
        }
    }
}

impl Period {
    /// True when `self` keeps jevd or a key around longer than `other`.
    pub fn looser_than(self, other: Period) -> bool {
        match (self, other) {
            (Period::Off, Period::Off) => false,
            (Period::Off, Period::After(_)) => true,
            (Period::After(_), Period::Off) => false,
            (Period::After(new), Period::After(old)) => new > old,
        }
    }
}

/// A rate limit: at most `count` requests in any `window`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limit {
    pub count: u32,
    pub window: Duration,
}

pub const DEFAULT_RATE_LIMIT: Limit = Limit {
    count: 120,
    window: Duration::from_secs(60),
};

const MAX_LIMIT_COUNT: u32 = 999_999;
const MAX_LIMIT_SECONDS: u64 = 9_999_999;

impl FromStr for Limit {
    type Err = String;

    fn from_str(text: &str) -> Result<Limit, String> {
        let problem = || format!("{text:?}: use <count>/<seconds>, such as 120/60");
        let (count, seconds) = text.split_once('/').ok_or_else(problem)?;
        let digits = |part: &str| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit());
        if !digits(count) || !digits(seconds) {
            return Err(problem());
        }
        let count: u32 = count.parse().map_err(|_| problem())?;
        let seconds: u64 = seconds.parse().map_err(|_| problem())?;
        if count == 0 || count > MAX_LIMIT_COUNT || seconds == 0 || seconds > MAX_LIMIT_SECONDS {
            return Err(problem());
        }
        Ok(Limit {
            count,
            window: Duration::from_secs(seconds),
        })
    }
}

impl fmt::Display for Limit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.count, self.window.as_secs())
    }
}

impl Limit {
    /// True when `self` allows a bigger burst or a higher average rate than `other`.
    pub fn looser_than(self, other: Limit) -> bool {
        let new_rate = u64::from(self.count) * other.window.as_secs();
        let old_rate = u64::from(other.count) * self.window.as_secs();
        self.count > other.count || new_rate > old_rate
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn period(text: &str) -> Period {
        text.parse().unwrap()
    }

    fn limit(text: &str) -> Limit {
        text.parse().unwrap()
    }

    #[test]
    fn periods_parse_and_print() {
        assert_eq!(period("2h"), Period::After(Duration::from_secs(7200)));
        assert_eq!(period("30m"), Period::After(Duration::from_secs(1800)));
        assert_eq!(period("off"), Period::Off);
        assert_eq!(period("120m").to_string(), "2h");
        assert_eq!(period("90m").to_string(), "90m");
        assert_eq!(period("off").to_string(), "off");
        for bad in [
            "", "2", "h", "0h", "-1h", "+1h", "2d", "2 h", "1.5h", "2hé", "10001h",
        ] {
            assert!(bad.parse::<Period>().is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_longer_period_or_off_loosens() {
        assert!(period("3h").looser_than(period("2h")));
        assert!(period("off").looser_than(period("2h")));
        assert!(!period("1h").looser_than(period("2h")));
        assert!(!period("2h").looser_than(period("2h")));
        assert!(!period("2h").looser_than(period("off")));
        assert!(!period("off").looser_than(period("off")));
    }

    #[test]
    fn limits_parse_and_print() {
        assert_eq!(limit("120/60"), DEFAULT_RATE_LIMIT);
        assert_eq!(limit("5/1").to_string(), "5/1");
        for bad in [
            "",
            "120",
            "/60",
            "120/",
            "0/60",
            "120/0",
            "a/60",
            "1000000/60",
            "120/10000000",
            "120/60/1",
        ] {
            assert!(bad.parse::<Limit>().is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_higher_count_or_rate_loosens() {
        assert!(limit("200/60").looser_than(limit("120/60")));
        assert!(limit("120/30").looser_than(limit("120/60")));
        assert!(limit("130/600").looser_than(limit("120/60")));
        assert!(!limit("60/60").looser_than(limit("120/60")));
        assert!(!limit("120/120").looser_than(limit("120/60")));
        assert!(!limit("120/60").looser_than(limit("120/60")));
    }
}
