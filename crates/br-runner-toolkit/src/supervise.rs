use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;

use futures::FutureExt;
use tokio::task::JoinHandle;

use crate::panic::text_of;
use crate::shared::Shared;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Restart {
    AtOnce,
    AfterBeat,
}

pub(crate) fn supervised<F, Fut>(
    shared: Arc<Shared>,
    name: &'static str,
    restart: Restart,
    make: F,
) -> JoinHandle<()>
where
    F: Fn() -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    tokio::spawn(async move {
        loop {
            match AssertUnwindSafe(make()).catch_unwind().await {
                Ok(()) => return,
                Err(payload) => {
                    tracing::error!(
                        task = name,
                        panic = %text_of(payload),
                        "a task of the loop ended by a panic and is started again"
                    );
                    match restart {
                        Restart::AtOnce => tokio::task::yield_now().await,
                        Restart::AfterBeat => {
                            tokio::select! {
                                () = shared.next_beat() => {}
                                () = shared.drain.cancelled() => {}
                            }
                        }
                    }
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use br_runner_jobs::test_double::ScriptedJobs;

    use super::{Restart, supervised};
    use crate::instances::Instances;
    use crate::unit_support::shared;

    #[tokio::test(start_paused = true)]
    async fn given_a_task_that_panics_once_when_it_is_supervised_at_once_then_it_is_started_again_and_ends_normally()
     {
        let jobs = ScriptedJobs::new("analyst", "i1");
        let shared = shared(&jobs, None, &Arc::new(Instances::default()));
        let starts = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&starts);
        let handle = supervised(shared, "probe", Restart::AtOnce, move || {
            let counted = Arc::clone(&counted);
            async move {
                if counted.fetch_add(1, Ordering::SeqCst) == 0 {
                    panic!("first start");
                }
            }
        });
        handle.await.expect("joined");
        assert_eq!(starts.load(Ordering::SeqCst), 2);
    }

    #[tokio::test(start_paused = true)]
    async fn given_a_task_that_panics_when_it_is_supervised_after_a_beat_then_it_waits_for_the_beat()
     {
        let jobs = ScriptedJobs::new("analyst", "i1");
        let shared = shared(&jobs, None, &Arc::new(Instances::default()));
        let starts = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&starts);
        let handle = supervised(
            Arc::clone(&shared),
            "probe",
            Restart::AfterBeat,
            move || {
                let counted = Arc::clone(&counted);
                async move {
                    if counted.fetch_add(1, Ordering::SeqCst) == 0 {
                        panic!("first start");
                    }
                }
            },
        );
        tokio::time::sleep(Duration::from_secs(5)).await;
        assert_eq!(starts.load(Ordering::SeqCst), 1);
        shared.beat_now();
        handle.await.expect("joined");
        assert_eq!(starts.load(Ordering::SeqCst), 2);
    }
}
