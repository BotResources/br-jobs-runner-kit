use std::time::Duration;

use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum BackoffConfigError {
    #[error("base delay must be non-zero")]
    ZeroBase,
    #[error("cap must be at or above the base delay")]
    CapBelowBase,
}

#[derive(Debug)]
pub struct Backoff {
    base: Duration,
    cap: Duration,
    attempts: u32,
}

impl Backoff {
    pub fn new(base: Duration, cap: Duration) -> Result<Self, BackoffConfigError> {
        if base.is_zero() {
            return Err(BackoffConfigError::ZeroBase);
        }
        if cap < base {
            return Err(BackoffConfigError::CapBelowBase);
        }
        Ok(Self {
            base,
            cap,
            attempts: 0,
        })
    }

    pub fn next_delay(&mut self) -> Duration {
        let factor = 2u32.saturating_pow(self.attempts);
        self.attempts = self.attempts.saturating_add(1);
        self.base.saturating_mul(factor).min(self.cap)
    }

    pub fn reset(&mut self) {
        self.attempts = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn backoff() -> Backoff {
        Backoff::new(Duration::from_secs(1), Duration::from_secs(30)).expect("valid config")
    }

    #[test]
    fn given_zero_base_when_new_then_rejected() {
        let result = Backoff::new(Duration::ZERO, Duration::from_secs(1));
        assert_eq!(result.unwrap_err(), BackoffConfigError::ZeroBase);
    }

    #[test]
    fn given_cap_below_base_when_new_then_rejected() {
        let result = Backoff::new(Duration::from_secs(2), Duration::from_secs(1));
        assert_eq!(result.unwrap_err(), BackoffConfigError::CapBelowBase);
    }

    #[test]
    fn given_consecutive_failures_when_asking_delay_then_doubles_up_to_cap() {
        let mut backoff = backoff();
        assert_eq!(backoff.next_delay(), Duration::from_secs(1));
        assert_eq!(backoff.next_delay(), Duration::from_secs(2));
        assert_eq!(backoff.next_delay(), Duration::from_secs(4));
        assert_eq!(backoff.next_delay(), Duration::from_secs(8));
        assert_eq!(backoff.next_delay(), Duration::from_secs(16));
        assert_eq!(backoff.next_delay(), Duration::from_secs(30));
        assert_eq!(backoff.next_delay(), Duration::from_secs(30));
    }

    #[test]
    fn given_reset_when_asking_delay_then_starts_from_base_again() {
        let mut backoff = backoff();
        backoff.next_delay();
        backoff.next_delay();
        backoff.reset();
        assert_eq!(backoff.next_delay(), Duration::from_secs(1));
    }

    #[test]
    fn given_many_failures_when_asking_delay_then_never_overflows() {
        let mut backoff = backoff();
        for _ in 0..100 {
            assert!(backoff.next_delay() <= Duration::from_secs(30));
        }
    }
}
