use std::time::Duration;

use br_runner_sandbox::Timestamp;
use chrono_tz::Tz;

pub fn elapsed(now: Timestamp, since: Timestamp) -> Duration {
    now.to_system_time()
        .duration_since(since.to_system_time())
        .unwrap_or_default()
}

pub fn until(now: Timestamp, at: Timestamp) -> Duration {
    elapsed(at, now)
}

pub fn seconds_of_day(now: Timestamp, zone: Tz) -> u32 {
    use chrono::{DateTime, Timelike, Utc};
    let utc: DateTime<Utc> = now.to_system_time().into();
    let local = utc.with_timezone(&zone);
    local.num_seconds_from_midnight()
}

pub fn earliest(a: Option<Timestamp>, b: Option<Timestamp>) -> Option<Timestamp> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, None) => a,
        (None, b) => b,
    }
}

#[cfg(test)]
pub(crate) fn at(seconds: u64) -> Timestamp {
    Timestamp::from_system_time(std::time::UNIX_EPOCH + Duration::from_secs(seconds))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elapsed_is_the_distance_and_never_negative() {
        assert_eq!(elapsed(at(100), at(40)), Duration::from_secs(60));
        assert_eq!(elapsed(at(40), at(100)), Duration::ZERO);
    }

    #[test]
    fn until_is_the_distance_to_a_later_instant() {
        assert_eq!(until(at(40), at(100)), Duration::from_secs(60));
        assert_eq!(until(at(100), at(40)), Duration::ZERO);
    }

    #[test]
    fn seconds_of_day_follow_the_zone() {
        let noon_utc = Timestamp::parse("2026-07-01T12:00:00Z").unwrap();
        assert_eq!(seconds_of_day(noon_utc, Tz::UTC), 12 * 3600);
        assert_eq!(seconds_of_day(noon_utc, Tz::Europe__Paris), 14 * 3600);
        let winter = Timestamp::parse("2026-01-01T12:00:00Z").unwrap();
        assert_eq!(seconds_of_day(winter, Tz::Europe__Paris), 13 * 3600);
    }

    #[test]
    fn earliest_takes_the_smaller_of_the_present_values() {
        assert_eq!(earliest(Some(at(5)), Some(at(3))), Some(at(3)));
        assert_eq!(earliest(None, Some(at(3))), Some(at(3)));
        assert_eq!(earliest(Some(at(5)), None), Some(at(5)));
        assert_eq!(earliest(None, None), None);
    }
}
