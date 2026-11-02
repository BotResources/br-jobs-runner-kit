use std::time::Duration;

use crate::error::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArrivalSettings {
    pub ack_wait: Duration,
    pub inactivity_limit: Duration,
}

impl ArrivalSettings {
    pub(crate) fn check(&self) -> Result<(), Error> {
        if self.ack_wait.is_zero() {
            return Err(Error::invalid("the arrival acknowledgement wait is zero"));
        }
        if self.inactivity_limit.is_zero() {
            return Err(Error::invalid("the arrival inactivity limit is zero"));
        }
        Ok(())
    }
}

#[cfg(test)]
pub(crate) fn settings_of_tests() -> ArrivalSettings {
    ArrivalSettings {
        ack_wait: Duration::from_secs(10),
        inactivity_limit: Duration::from_secs(120),
    }
}
