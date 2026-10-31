use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use br_runner_jobs::{Capacity, Error, PresenceStatus};
use tokio::sync::watch;
use tokio::time::{Interval, MissedTickBehavior, interval};

use crate::shared::Shared;

fn beat_period(ttl: Duration) -> Duration {
    (ttl / 3).max(Duration::from_millis(1))
}

pub(crate) async fn keep(shared: Arc<Shared>, ttl: Duration, capacity: Capacity) {
    let status = shared.status.subscribe();
    let publishing = Arc::clone(&shared);
    beats(
        beat_period(ttl),
        status,
        move |current| {
            let shared = Arc::clone(&publishing);
            async move {
                shared
                    .calls
                    .publish_presence(shared.settings.runner_version(), current, capacity)
                    .await
            }
        },
        move || shared.beat_now(),
    )
    .await;
}

pub(crate) async fn beats<P, Fut>(
    period: Duration,
    mut status: watch::Receiver<PresenceStatus>,
    publish: P,
    on_beat: impl Fn(),
) where
    P: Fn(PresenceStatus) -> Fut,
    Fut: Future<Output = Result<(), Error>>,
{
    let mut ticks = interval(period);
    ticks.set_missed_tick_behavior(MissedTickBehavior::Delay);
    ticks.tick().await;
    loop {
        let current = *status.borrow_and_update();
        tokio::select! {
            result = publish(current) => {
                if let Err(error) = result {
                    tracing::error!(%error, "the presence could not be published");
                }
                wait_for_beat(&mut ticks, &mut status).await;
            }
            _ = ticks.tick() => {
                tracing::warn!("a presence publication was still running at the next beat, it is dropped");
            }
        }
        on_beat();
    }
}

async fn wait_for_beat(ticks: &mut Interval, status: &mut watch::Receiver<PresenceStatus>) {
    tokio::select! {
        _ = ticks.tick() => {}
        _ = status.changed() => {}
    }
}
