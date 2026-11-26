use std::collections::BTreeMap;
use std::time::Duration;

use br_runner_sandbox::Timestamp;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClaimRate {
    pub claims: u32,
    pub bucket: Duration,
}

impl ClaimRate {
    pub fn per_second(&self) -> f64 {
        f64::from(self.claims) / self.bucket.as_secs_f64()
    }
}

#[derive(Debug, Clone)]
pub struct ClaimWindow {
    window: Duration,
    bucket: Duration,
    counts: BTreeMap<u64, u32>,
}

fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX).max(1)
}

fn now_millis(now: Timestamp) -> u64 {
    now.to_system_time()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| u64::try_from(since.as_millis()).unwrap_or(u64::MAX))
}

impl ClaimWindow {
    pub fn new(window: Duration, bucket: Duration) -> Self {
        Self {
            window,
            bucket,
            counts: BTreeMap::new(),
        }
    }

    fn index(&self, now: Timestamp) -> u64 {
        now_millis(now) / millis(self.bucket)
    }

    fn prune(&mut self, now: Timestamp) {
        let window_start = now_millis(now).saturating_sub(millis(self.window));
        let bucket = millis(self.bucket);
        self.counts
            .retain(|index, _| (index + 1).saturating_mul(bucket) > window_start);
    }

    pub fn record(&mut self, now: Timestamp) {
        self.prune(now);
        let slot = self.counts.entry(self.index(now)).or_insert(0);
        *slot = slot.saturating_add(1);
    }

    pub fn peak(&mut self, now: Timestamp) -> ClaimRate {
        self.prune(now);
        ClaimRate {
            claims: self.counts.values().copied().max().unwrap_or(0),
            bucket: self.bucket,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::at;

    fn window() -> ClaimWindow {
        ClaimWindow::new(Duration::from_secs(300), Duration::from_secs(10))
    }

    #[test]
    fn a_window_with_no_claim_has_a_peak_of_zero() {
        assert_eq!(window().peak(at(1000)).claims, 0);
    }

    #[test]
    fn the_peak_is_the_busiest_bucket_not_the_sum() {
        let mut claims = window();
        for second in [1000, 1001, 1002] {
            claims.record(at(second));
        }
        claims.record(at(1015));
        claims.record(at(1025));
        let peak = claims.peak(at(1030));
        assert_eq!(peak.claims, 3);
        assert_eq!(peak.bucket, Duration::from_secs(10));
        assert!((peak.per_second() - 0.3).abs() < 1e-9);
    }

    #[test]
    fn a_bucket_leaves_the_window_when_its_end_is_before_the_window_start() {
        let mut claims = window();
        claims.record(at(1000));
        assert_eq!(claims.peak(at(1000 + 289)).claims, 1);
        assert_eq!(claims.peak(at(1000 + 309)).claims, 1);
        assert_eq!(claims.peak(at(1000 + 310)).claims, 0);
    }

    #[test]
    fn a_cold_start_has_no_rate() {
        let mut claims = window();
        assert_eq!(claims.peak(at(5)).claims, 0);
        assert_eq!(claims.peak(at(0)).claims, 0);
    }
}
