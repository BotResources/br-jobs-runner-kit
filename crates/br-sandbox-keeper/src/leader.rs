use std::sync::{Arc, Mutex};
use std::time::Duration;

use br_runner_sandbox::{Clock, Timestamp};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

use crate::cluster::{ApiError, KubeApi, LeaseFacts, LeaseWrite};
use crate::config::LeaseSettings;
use crate::time::until;

pub const LEASE_NAME: &str = "br-sandbox-keeper";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeaseAction {
    Create,
    Renew,
    Take,
    Wait { recheck_at: Option<Timestamp> },
}

pub fn lease_action(
    current: Option<&LeaseFacts>,
    me: &str,
    now: Timestamp,
    own_duration: Duration,
) -> LeaseAction {
    let Some(lease) = current else {
        return LeaseAction::Create;
    };
    match lease.holder.as_deref() {
        Some(holder) if holder == me => LeaseAction::Renew,
        None => LeaseAction::Take,
        Some(_) => {
            let expiry = lease
                .renew_time
                .map(|renewed| renewed.plus(lease.duration.unwrap_or(own_duration)));
            match expiry {
                Some(expiry) if !expiry.has_passed(now) => LeaseAction::Wait {
                    recheck_at: Some(expiry),
                },
                _ => LeaseAction::Take,
            }
        }
    }
}

pub struct Leadership {
    held_until: Mutex<Option<Timestamp>>,
    gained: Notify,
    clock: Arc<dyn Clock>,
}

impl Leadership {
    pub fn new(clock: Arc<dyn Clock>) -> Arc<Self> {
        Arc::new(Self {
            held_until: Mutex::new(None),
            gained: Notify::new(),
            clock,
        })
    }

    pub fn is_leader(&self) -> bool {
        let held = *self
            .held_until
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        held.is_some_and(|until| !until.has_passed(self.clock.now()))
    }

    pub fn renewed(&self, until: Timestamp) {
        let was_leader = self.is_leader();
        *self
            .held_until
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(until);
        if !was_leader {
            self.gained.notify_one();
        }
    }

    pub fn lost(&self) {
        *self
            .held_until
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    }

    pub async fn gained(&self) {
        self.gained.notified().await;
    }
}

pub struct Elector {
    pub api: Arc<dyn KubeApi>,
    pub clock: Arc<dyn Clock>,
    pub leadership: Arc<Leadership>,
    pub namespace: String,
    pub identity: String,
    pub lease: LeaseSettings,
}

impl Elector {
    pub async fn run(self, shutdown: CancellationToken) {
        loop {
            let pause = self.round().await;
            tokio::select! {
                () = shutdown.cancelled() => return,
                () = tokio::time::sleep(pause) => {}
            }
        }
    }

    pub async fn round(&self) -> Duration {
        let started = self.clock.now();
        let current = match self.api.get_lease(&self.namespace, LEASE_NAME).await {
            Ok(current) => current,
            Err(error) => {
                tracing::warn!(%error, "the lease could not be read");
                return self.lease.retry_period;
            }
        };
        let action = lease_action(
            current.as_ref(),
            &self.identity,
            started,
            self.lease.duration,
        );
        let write = |transitions: i32, acquire_time: Timestamp| LeaseWrite {
            holder: self.identity.clone(),
            acquire_time,
            renew_time: started,
            duration: self.lease.duration,
            transitions,
        };
        let outcome = match (action, current.as_ref()) {
            (LeaseAction::Wait { recheck_at }, _) => {
                self.leadership.lost();
                let pause = recheck_at.map_or(self.lease.retry_period, |at| {
                    until(started, at).min(self.lease.retry_period)
                });
                return pause.max(Duration::from_millis(1));
            }
            (LeaseAction::Create, _) => {
                self.api
                    .create_lease(&self.namespace, LEASE_NAME, &write(0, started))
                    .await
            }
            (LeaseAction::Renew, Some(lease)) => {
                let acquired = lease.acquire_time.unwrap_or(started);
                self.api
                    .update_lease(
                        &self.namespace,
                        LEASE_NAME,
                        &write(lease.transitions, acquired),
                        &lease.resource_version,
                    )
                    .await
            }
            (LeaseAction::Take, Some(lease)) => {
                self.api
                    .update_lease(
                        &self.namespace,
                        LEASE_NAME,
                        &write(lease.transitions.saturating_add(1), started),
                        &lease.resource_version,
                    )
                    .await
            }
            (LeaseAction::Renew | LeaseAction::Take, None) => Err(ApiError::NotFound),
        };
        match outcome {
            Ok(()) => self
                .leadership
                .renewed(started.plus(self.lease.renew_deadline)),
            Err(ApiError::Conflict) => {
                tracing::info!("the lease was taken by another writer");
                if action != LeaseAction::Renew {
                    self.leadership.lost();
                }
            }
            Err(error) => tracing::warn!(%error, "the lease could not be written"),
        }
        self.lease.retry_period
    }
}

#[cfg(test)]
mod tests;
