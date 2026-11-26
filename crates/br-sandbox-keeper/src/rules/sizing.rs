use br_runner_sandbox::Timestamp;
use chrono_tz::Tz;

use crate::config::PoolConfig;
use crate::time::seconds_of_day;

use super::rate::ClaimRate;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Desired {
    pub raw: u64,
    pub clamped: u32,
}

impl Desired {
    pub fn above_max(&self, pool: &PoolConfig) -> bool {
        self.raw > u64::from(pool.max_replicas)
    }
}

pub fn floor_now(pool: &PoolConfig, now: Timestamp, zone: Tz) -> u32 {
    let second = seconds_of_day(now, zone);
    pool.floors
        .iter()
        .filter(|window| window.from_second <= second && second < window.to_second)
        .map(|window| window.floor)
        .max()
        .unwrap_or(0)
}

fn ceil_claims(rate: ClaimRate, pool: &PoolConfig) -> u128 {
    let bucket = rate.bucket.as_millis().max(1);
    let needed = u128::from(rate.claims) * pool.refill.as_millis();
    needed.div_ceil(bucket)
}

pub fn desired_size(rate: ClaimRate, pool: &PoolConfig, floor: u32) -> Desired {
    let reactive = ceil_claims(rate, pool) + u128::from(pool.margin);
    let raw = u64::try_from(reactive.max(u128::from(floor))).unwrap_or(u64::MAX);
    let clamped = raw.clamp(u64::from(pool.min_replicas), u64::from(pool.max_replicas));
    Desired {
        raw,
        clamped: u32::try_from(clamped).unwrap_or(u32::MAX),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::config::FloorWindow;
    use crate::testing;

    fn pool(floors: Vec<FloorWindow>) -> PoolConfig {
        let mut pool = testing::pool();
        pool.floors = floors;
        pool
    }

    fn window(from: u32, to: u32, floor: u32) -> FloorWindow {
        FloorWindow {
            from_second: from,
            to_second: to,
            floor,
        }
    }

    fn rate(claims: u32) -> ClaimRate {
        ClaimRate {
            claims,
            bucket: Duration::from_secs(10),
        }
    }

    #[test]
    fn with_no_load_only_the_margin_and_the_floor_stay() {
        let pool = pool(Vec::new());
        assert_eq!(desired_size(rate(0), &pool, 0).clamped, 2);
        assert_eq!(desired_size(rate(0), &pool, 7).clamped, 7);
    }

    #[test]
    fn the_peak_rate_times_the_refill_time_is_rounded_up_and_the_margin_is_added() {
        let pool = pool(Vec::new());
        assert_eq!(desired_size(rate(1), &pool, 0).raw, 1 + 2);
        assert_eq!(desired_size(rate(10), &pool, 0).raw, 3 + 2);
        assert_eq!(desired_size(rate(11), &pool, 0).raw, 4 + 2);
        assert_eq!(desired_size(rate(3), &pool, 0).raw, 1 + 2);
    }

    #[test]
    fn a_rate_that_float_arithmetic_would_round_up_is_not_rounded_up() {
        let mut pool = pool(Vec::new());
        pool.refill = Duration::from_secs(10);
        pool.margin = 0;
        let seconds_per_claim = ClaimRate {
            claims: 3,
            bucket: Duration::from_secs(10),
        };
        assert_eq!(desired_size(seconds_per_claim, &pool, 0).raw, 3);
    }

    #[test]
    fn the_floor_wins_over_a_smaller_reactive_size() {
        let pool = pool(Vec::new());
        assert_eq!(desired_size(rate(10), &pool, 15).raw, 15);
        assert_eq!(desired_size(rate(100), &pool, 15).raw, 30 + 2);
    }

    #[test]
    fn the_size_is_clamped_between_min_and_max_and_raw_keeps_the_value_before() {
        let mut pool = pool(Vec::new());
        pool.min_replicas = 4;
        let low = desired_size(rate(0), &pool, 0);
        assert_eq!((low.raw, low.clamped), (2, 4));
        let high = desired_size(rate(1000), &pool, 0);
        assert_eq!(high.clamped, 20);
        assert!(high.above_max(&pool));
        assert!(!low.above_max(&pool));
        let exact = desired_size(rate(60), &pool, 0);
        assert_eq!((exact.raw, exact.clamped), (20, 20));
        assert!(!exact.above_max(&pool));
    }

    #[test]
    fn a_zero_refill_leaves_the_margin_and_the_floor() {
        let mut pool = pool(Vec::new());
        pool.refill = Duration::ZERO;
        assert_eq!(desired_size(rate(50), &pool, 0).raw, 2);
    }

    #[test]
    fn the_floor_now_is_the_highest_of_the_windows_in_force() {
        let pool = pool(vec![
            window(0, 3600, 4),
            window(1800, 7200, 9),
            window(7200, 10_000, 30),
        ]);
        let zero = |hour: &str| Timestamp::parse(&format!("2026-03-01T{hour}Z")).unwrap();
        assert_eq!(floor_now(&pool, zero("00:00:00"), Tz::UTC), 4);
        assert_eq!(floor_now(&pool, zero("00:45:00"), Tz::UTC), 9);
        assert_eq!(floor_now(&pool, zero("01:00:00"), Tz::UTC), 9);
        assert_eq!(floor_now(&pool, zero("01:59:59"), Tz::UTC), 9);
        assert_eq!(floor_now(&pool, zero("02:00:00"), Tz::UTC), 30);
        assert_eq!(floor_now(&pool, zero("02:46:40"), Tz::UTC), 0);
    }

    #[test]
    fn with_no_window_in_force_the_floor_is_zero_and_the_zone_counts() {
        let in_force = pool(vec![window(9 * 3600, 17 * 3600, 5)]);
        let utc_morning = Timestamp::parse("2026-07-01T07:30:00Z").unwrap();
        assert_eq!(floor_now(&in_force, utc_morning, Tz::UTC), 0);
        assert_eq!(floor_now(&in_force, utc_morning, Tz::Europe__Paris), 5);
        assert_eq!(floor_now(&pool(Vec::new()), utc_morning, Tz::UTC), 0);
    }
}
