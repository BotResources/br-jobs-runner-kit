use std::time::Duration;

/// How long the library tries to publish one record: the number of attempts,
/// the wait of one attempt, and the pause after the first failed attempt. Each
/// later pause is twice the one before it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PublishSettings {
    attempts: u32,
    wait: Duration,
    pause: Duration,
}

/// The only error of the library: settings that cannot publish.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SettingsError {
    #[error("the number of attempts of a publish is zero")]
    ZeroAttempts,
    #[error("the wait of one publish attempt is zero")]
    ZeroWait,
}

impl PublishSettings {
    /// Refuses zero attempts and a zero wait. A zero pause is accepted.
    pub fn new(attempts: u32, wait: Duration, pause: Duration) -> Result<Self, SettingsError> {
        if attempts == 0 {
            return Err(SettingsError::ZeroAttempts);
        }
        if wait.is_zero() {
            return Err(SettingsError::ZeroWait);
        }
        Ok(Self {
            attempts,
            wait,
            pause,
        })
    }

    pub fn attempts(&self) -> u32 {
        self.attempts
    }

    pub fn wait(&self) -> Duration {
        self.wait
    }

    pub fn pause(&self) -> Duration {
        self.pause
    }
}

impl Default for PublishSettings {
    /// 3 attempts, a wait of 2 seconds, a first pause of 200 milliseconds.
    fn default() -> Self {
        Self {
            attempts: 3,
            wait: Duration::from_secs(2),
            pause: Duration::from_millis(200),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_is_three_attempts_two_seconds_and_a_pause_of_200_ms() {
        let settings = PublishSettings::default();
        assert_eq!(settings.attempts(), 3);
        assert_eq!(settings.wait(), Duration::from_secs(2));
        assert_eq!(settings.pause(), Duration::from_millis(200));
    }

    #[test]
    fn zero_attempts_or_a_zero_wait_is_refused() {
        let s = Duration::from_secs(1);
        assert_eq!(
            PublishSettings::new(0, s, s),
            Err(SettingsError::ZeroAttempts)
        );
        assert_eq!(
            PublishSettings::new(1, Duration::ZERO, s),
            Err(SettingsError::ZeroWait)
        );
        assert_eq!(
            PublishSettings::new(0, Duration::ZERO, s),
            Err(SettingsError::ZeroAttempts)
        );
    }

    #[test]
    fn a_zero_pause_is_accepted_and_the_values_are_kept() {
        let settings =
            PublishSettings::new(5, Duration::from_millis(30), Duration::ZERO).expect("valid");
        assert_eq!(settings.attempts(), 5);
        assert_eq!(settings.wait(), Duration::from_millis(30));
        assert_eq!(settings.pause(), Duration::ZERO);
    }
}
