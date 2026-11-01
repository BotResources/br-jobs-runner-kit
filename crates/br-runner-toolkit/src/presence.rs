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

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use br_runner_jobs::PresenceStatus;
    use tokio::sync::watch;

    use super::beats;

    #[tokio::test(start_paused = true)]
    async fn given_a_publication_that_never_completes_when_the_next_beat_comes_then_it_is_dropped_and_the_beats_go_on()
     {
        let (_status, receiver) = watch::channel(PresenceStatus::Ready);
        let published = Arc::new(AtomicUsize::new(0));
        let beaten = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&published);
        let ticked = Arc::clone(&beaten);
        let task = tokio::spawn(beats(
            Duration::from_secs(30),
            receiver,
            move |_| {
                let first = counted.fetch_add(1, Ordering::SeqCst) == 0;
                async move {
                    if first {
                        std::future::pending::<()>().await;
                    }
                    Ok(())
                }
            },
            move || {
                ticked.fetch_add(1, Ordering::SeqCst);
            },
        ));
        tokio::time::sleep(Duration::from_secs(29)).await;
        assert_eq!(published.load(Ordering::SeqCst), 1);
        assert_eq!(beaten.load(Ordering::SeqCst), 0);
        tokio::time::sleep(Duration::from_secs(2)).await;
        assert_eq!(beaten.load(Ordering::SeqCst), 1);
        tokio::time::sleep(Duration::from_secs(1)).await;
        assert_eq!(published.load(Ordering::SeqCst), 2);
        task.abort();
    }
}
