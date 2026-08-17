use std::time::{Duration, Instant};

use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresenceAction {
    PublishReady,
    PublishDraining,
    Delete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NextDue {
    Now,
    At(Instant),
    Never,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum PresenceConfigError {
    #[error("heartbeat interval must be non-zero")]
    ZeroInterval,
    #[error("heartbeat interval must be strictly below the presence ttl")]
    IntervalNotBelowTtl,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Ready,
    Draining,
    Closed,
}

#[derive(Debug)]
pub struct PresenceSession {
    interval: Duration,
    phase: Phase,
    last_publish: Option<Instant>,
}

impl PresenceSession {
    pub fn new(ttl: Duration, heartbeat_interval: Duration) -> Result<Self, PresenceConfigError> {
        if heartbeat_interval.is_zero() {
            return Err(PresenceConfigError::ZeroInterval);
        }
        if heartbeat_interval >= ttl {
            return Err(PresenceConfigError::IntervalNotBelowTtl);
        }
        Ok(Self {
            interval: heartbeat_interval,
            phase: Phase::Ready,
            last_publish: None,
        })
    }

    pub fn poll(&mut self, now: Instant) -> Option<PresenceAction> {
        let action = match self.phase {
            Phase::Ready => PresenceAction::PublishReady,
            Phase::Draining => PresenceAction::PublishDraining,
            Phase::Closed => return None,
        };
        match self.next_due() {
            NextDue::Now => {}
            NextDue::At(due) if now >= due => {}
            NextDue::At(_) | NextDue::Never => return None,
        }
        self.last_publish = Some(now);
        Some(action)
    }

    pub fn next_due(&self) -> NextDue {
        if self.phase == Phase::Closed {
            return NextDue::Never;
        }
        match self.last_publish {
            None => NextDue::Now,
            Some(last) => NextDue::At(last + self.interval),
        }
    }

    pub fn begin_drain(&mut self) {
        if self.phase == Phase::Ready {
            self.phase = Phase::Draining;
            self.last_publish = None;
        }
    }

    pub fn is_draining(&self) -> bool {
        self.phase == Phase::Draining
    }

    pub fn close(&mut self) -> Option<PresenceAction> {
        if self.phase == Phase::Closed {
            return None;
        }
        self.phase = Phase::Closed;
        Some(PresenceAction::Delete)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> PresenceSession {
        PresenceSession::new(Duration::from_secs(30), Duration::from_secs(10))
            .expect("valid config")
    }

    #[test]
    fn given_interval_at_or_above_ttl_when_new_then_rejected() {
        let result = PresenceSession::new(Duration::from_secs(10), Duration::from_secs(10));
        assert_eq!(
            result.unwrap_err(),
            PresenceConfigError::IntervalNotBelowTtl
        );
    }

    #[test]
    fn given_zero_interval_when_new_then_rejected() {
        let result = PresenceSession::new(Duration::from_secs(10), Duration::ZERO);
        assert_eq!(result.unwrap_err(), PresenceConfigError::ZeroInterval);
    }

    #[test]
    fn given_fresh_session_when_polled_then_publishes_ready_immediately() {
        let mut session = session();
        assert_eq!(session.next_due(), NextDue::Now);
        assert_eq!(
            session.poll(Instant::now()),
            Some(PresenceAction::PublishReady)
        );
    }

    #[test]
    fn given_recent_publish_when_polled_before_interval_then_silent() {
        let mut session = session();
        let start = Instant::now();
        session.poll(start);
        assert_eq!(session.poll(start + Duration::from_secs(5)), None);
        assert_eq!(
            session.next_due(),
            NextDue::At(start + Duration::from_secs(10))
        );
    }

    #[test]
    fn given_elapsed_interval_when_polled_then_heartbeats_again() {
        let mut session = session();
        let start = Instant::now();
        session.poll(start);
        let later = start + Duration::from_secs(10);
        assert_eq!(session.poll(later), Some(PresenceAction::PublishReady));
    }

    #[test]
    fn given_drain_when_polled_then_publishes_draining_immediately() {
        let mut session = session();
        let start = Instant::now();
        session.poll(start);
        session.begin_drain();
        assert!(session.is_draining());
        assert_eq!(session.next_due(), NextDue::Now);
        assert_eq!(
            session.poll(start + Duration::from_secs(1)),
            Some(PresenceAction::PublishDraining)
        );
    }

    #[test]
    fn given_closed_session_when_polled_then_nothing_ever() {
        let mut session = session();
        assert_eq!(session.close(), Some(PresenceAction::Delete));
        assert_eq!(session.close(), None);
        assert_eq!(session.next_due(), NextDue::Never);
        assert_eq!(session.poll(Instant::now()), None);
    }

    #[test]
    fn given_draining_session_when_drained_again_then_no_heartbeat_reset() {
        let mut session = session();
        let start = Instant::now();
        session.begin_drain();
        session.poll(start);
        session.begin_drain();
        assert_eq!(session.poll(start + Duration::from_secs(1)), None);
    }
}
