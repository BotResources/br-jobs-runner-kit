use std::sync::Arc;
use std::time::Duration;

use br_runner_jobs::{Capacity, PresenceStatus};
use tokio::time::{MissedTickBehavior, interval};

use crate::shared::Shared;

fn beat_period(ttl: Duration) -> Duration {
    (ttl / 3).max(Duration::from_millis(1))
}

pub(crate) async fn keep(shared: Arc<Shared>, ttl: Duration, capacity: Capacity) {
    let mut ticks = interval(beat_period(ttl));
    ticks.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut status = shared.status.subscribe();
    ticks.tick().await;
    loop {
        let current = *status.borrow_and_update();
        let publication =
            shared
                .calls
                .publish_presence(shared.settings.runner_version(), current, capacity);
        tokio::select! {
            result = publication => {
                if let Err(error) = result {
                    tracing::error!(%error, "the presence could not be published");
                }
                wait_for_beat(&mut ticks, &mut status).await;
            }
            _ = ticks.tick() => {
                tracing::warn!("a presence publication was still running at the next beat, it is dropped");
            }
        }
        shared.beat_now();
    }
}

async fn wait_for_beat(
    ticks: &mut tokio::time::Interval,
    status: &mut tokio::sync::watch::Receiver<PresenceStatus>,
) {
    tokio::select! {
        _ = ticks.tick() => {}
        _ = status.changed() => {}
    }
}
