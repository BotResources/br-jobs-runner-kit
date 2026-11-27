use std::sync::{Arc, Mutex};
use std::time::Duration;

use br_runner_sandbox::{Clock, Timestamp};

#[derive(Clone)]
pub struct ManualClock {
    now: Arc<Mutex<Timestamp>>,
}

impl ManualClock {
    pub fn at(now: Timestamp) -> Self {
        Self {
            now: Arc::new(Mutex::new(now)),
        }
    }

    pub fn advance(&self, span: Duration) {
        let mut now = self.now.lock().expect("clock");
        *now = now.plus(span);
    }
}

impl Clock for ManualClock {
    fn now(&self) -> Timestamp {
        *self.now.lock().expect("clock")
    }
}
