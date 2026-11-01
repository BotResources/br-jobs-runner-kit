use std::future::Future;
use std::sync::Arc;

use tokio::task::JoinHandle;

use crate::shared::Shared;
use crate::supervise::{Restart, supervised};

pub(crate) struct LoopTasks {
    pub presence: JoinHandle<()>,
    pub feeds: [JoinHandle<()>; 2],
    pub slots: Vec<JoinHandle<()>>,
}

pub(crate) struct LoopBodies<P, M, C, S> {
    pub presence: P,
    pub dispatch_mode: M,
    pub cancel_orders: C,
    pub slot: S,
}

pub(crate) fn spawn_loop<P, PF, M, MF, C, CF, S, SF>(
    shared: &Arc<Shared>,
    slots: u32,
    bodies: LoopBodies<P, M, C, S>,
) -> LoopTasks
where
    P: Fn() -> PF + Send + 'static,
    PF: Future<Output = ()> + Send + 'static,
    M: Fn() -> MF + Send + 'static,
    MF: Future<Output = ()> + Send + 'static,
    C: Fn() -> CF + Send + 'static,
    CF: Future<Output = ()> + Send + 'static,
    S: Fn() -> SF + Clone + Send + 'static,
    SF: Future<Output = ()> + Send + 'static,
{
    let presence = supervised(
        Arc::clone(shared),
        "presence",
        Restart::AtOnce,
        bodies.presence,
    );
    let feeds = [
        supervised(
            Arc::clone(shared),
            "dispatch mode",
            Restart::AfterBeat,
            bodies.dispatch_mode,
        ),
        supervised(
            Arc::clone(shared),
            "cancel orders",
            Restart::AfterBeat,
            bodies.cancel_orders,
        ),
    ];
    let slots = (0..slots)
        .map(|_| {
            supervised(
                Arc::clone(shared),
                "slot",
                Restart::AfterBeat,
                bodies.slot.clone(),
            )
        })
        .collect();
    LoopTasks {
        presence,
        feeds,
        slots,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use br_runner_jobs::test_double::ScriptedJobs;

    use super::{LoopBodies, LoopTasks, spawn_loop};
    use crate::instances::Instances;
    use crate::shared::Shared;
    use crate::unit_support::shared;

    struct Probe {
        starts: Arc<AtomicUsize>,
        serving: Arc<AtomicUsize>,
    }

    impl Probe {
        fn new() -> Self {
            Self {
                starts: Arc::new(AtomicUsize::new(0)),
                serving: Arc::new(AtomicUsize::new(0)),
            }
        }

        fn body(&self, panics: usize) -> impl Fn() -> BodyFuture + Clone + Send + 'static {
            let (starts, serving) = (Arc::clone(&self.starts), Arc::clone(&self.serving));
            move || {
                let (starts, serving) = (Arc::clone(&starts), Arc::clone(&serving));
                Box::pin(async move {
                    if starts.fetch_add(1, Ordering::SeqCst) < panics {
                        panic!("injected");
                    }
                    serving.fetch_add(1, Ordering::SeqCst);
                    std::future::pending::<()>().await;
                })
            }
        }

        fn starts(&self) -> usize {
            self.starts.load(Ordering::SeqCst)
        }

        fn serving(&self) -> usize {
            self.serving.load(Ordering::SeqCst)
        }
    }

    type BodyFuture = std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>;

    fn world() -> Arc<Shared> {
        let jobs = ScriptedJobs::new("analyst", "i1");
        shared(&jobs, None, &Arc::new(Instances::default()))
    }

    fn spawned(
        shared: &Arc<Shared>,
        slots: u32,
        presence: &Probe,
        mode: &Probe,
        cancel: &Probe,
        slot: &Probe,
    ) -> LoopTasks {
        spawn_loop(
            shared,
            slots,
            LoopBodies {
                presence: presence.body(1),
                dispatch_mode: mode.body(1),
                cancel_orders: cancel.body(1),
                slot: slot.body(1),
            },
        )
    }

    async fn settle() {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    fn stop(tasks: LoopTasks) {
        tasks.presence.abort();
        tasks.feeds.iter().for_each(|task| task.abort());
        tasks.slots.iter().for_each(|task| task.abort());
    }

    #[tokio::test(start_paused = true)]
    async fn given_a_presence_that_panics_when_no_beat_comes_then_it_is_started_again_at_once() {
        let shared = world();
        let (presence, mode, cancel, slot) =
            (Probe::new(), Probe::new(), Probe::new(), Probe::new());
        let tasks = spawned(&shared, 1, &presence, &mode, &cancel, &slot);
        settle().await;
        assert_eq!(presence.starts(), 2);
        assert_eq!(presence.serving(), 1);
        stop(tasks);
    }

    #[tokio::test(start_paused = true)]
    async fn given_a_presence_that_panics_twice_in_a_row_then_it_is_started_again_each_time_at_once()
     {
        let shared = world();
        let presence = Probe::new();
        let tasks = spawn_loop(
            &shared,
            1,
            LoopBodies {
                presence: presence.body(2),
                dispatch_mode: Probe::new().body(0),
                cancel_orders: Probe::new().body(0),
                slot: Probe::new().body(0),
            },
        );
        settle().await;
        assert_eq!(presence.starts(), 3);
        assert_eq!(presence.serving(), 1);
        stop(tasks);
    }

    #[tokio::test(start_paused = true)]
    async fn given_a_feed_that_panics_then_it_is_started_again_only_after_the_next_beat() {
        let shared = world();
        let (presence, mode, cancel, slot) =
            (Probe::new(), Probe::new(), Probe::new(), Probe::new());
        let tasks = spawned(&shared, 1, &presence, &mode, &cancel, &slot);
        settle().await;
        assert_eq!((mode.starts(), cancel.starts()), (1, 1));
        shared.beat_now();
        settle().await;
        assert_eq!((mode.starts(), cancel.starts()), (2, 2));
        assert_eq!((mode.serving(), cancel.serving()), (1, 1));
        stop(tasks);
    }

    #[tokio::test(start_paused = true)]
    async fn given_one_slot_that_panics_when_the_beat_comes_then_the_slots_serving_are_the_capacity_again()
     {
        let shared = world();
        let (presence, mode, cancel, slot) =
            (Probe::new(), Probe::new(), Probe::new(), Probe::new());
        let tasks = spawned(&shared, 3, &presence, &mode, &cancel, &slot);
        settle().await;
        assert_eq!(slot.serving(), 2);
        shared.beat_now();
        settle().await;
        assert_eq!(slot.serving(), 3);
        assert_eq!(tasks.slots.len(), 3);
        stop(tasks);
    }
}
