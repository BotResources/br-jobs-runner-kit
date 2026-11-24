use std::time::Duration;

use br_runner_jobs_client::test_double::Call;
use br_runner_threads::Error;
use br_runner_toolkit::HostEvent;
use br_runner_wiring_example::{Variant, World, id, run_in_host};
use tokio::task::JoinHandle;
use tokio::time::{Instant, sleep};

const VARIANTS: [Variant; 2] = [Variant::Harness, Variant::Gate];

type Hosted = JoinHandle<Result<br_runner_toolkit::HostRun, String>>;

fn start(world: &World) -> Hosted {
    let world = world.clone();
    tokio::spawn(async move { run_in_host(&world).await.map_err(|e| e.to_string()) })
}

fn marked(world: &World) -> bool {
    world
        .jobs
        .calls()
        .iter()
        .any(|call| matches!(call, Call::MarkRead { item_ids, .. } if item_ids == &vec![id(101)]))
}

async fn until(what: &str, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !condition() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        sleep(Duration::from_millis(10)).await;
    }
}

async fn settle() {
    sleep(Duration::from_millis(400)).await;
}

fn world(variant: Variant) -> World {
    World::new(variant, Duration::from_secs(5)).expect("a world")
}

fn impatient(variant: Variant) -> World {
    World::new(variant, Duration::from_millis(800)).expect("a world")
}

#[tokio::test]
async fn a_mail_is_marked_read_after_the_thread_entry_is_stored() {
    for variant in VARIANTS {
        let world = world(variant);

        let run = start(&world);
        until("the mark", || marked(&world)).await;
        let finished = run.await.expect("joined").expect("a run");

        assert!(
            !world.threads.commands().is_empty(),
            "{variant:?}: no thread write"
        );
        assert!(!finished.events.is_empty(), "{variant:?}: no host event");
    }
}

#[tokio::test]
async fn a_mail_stays_unread_while_the_thread_write_is_not_taken() {
    for variant in VARIANTS {
        let world = world(variant);
        world.threads.pause();

        let run = start(&world);
        until("the thread write", || {
            !world.threads.operations().is_empty()
        })
        .await;
        settle().await;
        assert!(!marked(&world), "{variant:?}: marked before the store");

        world.threads.resume();
        until("the mark", || marked(&world)).await;
        run.await.expect("joined").expect("a run");
    }
}

#[tokio::test]
async fn a_mail_stays_unread_when_the_thread_store_fails() {
    for variant in VARIANTS {
        let world = impatient(variant);
        world
            .threads
            .answer_commands(|_| Err(Error::InvalidInput("refused".to_owned())));

        let run = start(&world);
        until("the thread write", || {
            !world.threads.operations().is_empty()
        })
        .await;
        settle().await;
        let finished = run.await.expect("joined");

        assert!(!marked(&world), "{variant:?}: marked after a failed store");
        assert!(finished.is_ok(), "{variant:?}: the host run failed");
    }
}

#[tokio::test]
async fn the_toolkit_drives_the_run_and_the_node_works_inside_it() {
    for variant in VARIANTS {
        let world = world(variant);

        let run = start(&world);
        until("the mark", || marked(&world)).await;
        let finished = run.await.expect("joined").expect("a run");

        let events = &finished.events;
        assert!(
            events
                .iter()
                .any(|e| matches!(e, HostEvent::TriggerAcknowledged)),
            "{variant:?}: the toolkit took no trigger: {events:?}"
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, HostEvent::PresencePublished { .. })),
            "{variant:?}: no presence"
        );
        assert_eq!(world.observed.outcomes(), vec!["read".to_owned()]);
        assert!(
            world
                .jobs
                .calls()
                .iter()
                .any(|call| matches!(call, Call::Register { .. })),
            "{variant:?}: the agent book was not registered"
        );
    }
}
