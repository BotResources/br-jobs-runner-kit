use std::time::Duration;

use br_runner_sandbox::{Clock, Timestamp};
use tokio::time::Instant;

pub struct TokioClock {
    base: Timestamp,
    start: Instant,
}

impl TokioClock {
    pub fn starting_at(base: Timestamp) -> Self {
        Self {
            base,
            start: Instant::now(),
        }
    }
}

impl Clock for TokioClock {
    fn now(&self) -> Timestamp {
        let elapsed: Duration = self.start.elapsed();
        self.base.plus(elapsed)
    }
}
