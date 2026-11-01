use std::sync::{Arc, Mutex};
use std::time::Duration;

use br_graph_executor::{Observation, Observer, OutcomeKind};
use br_runner_jobs::DeliveryQueue;
use br_runner_jobs::test_double::ScriptedJobs;
use tokio::sync::Notify;
use tokio::task::JoinHandle;

use tracing::Level;

use super::settle;
use crate::exec_observer::ExecObserver;
use crate::hook::{InstanceEnded, lifecycle_hook};
use crate::instances::Instances;
use crate::record::RunRecord;
use crate::unit_support::logs::Logs;
use crate::unit_support::{book_with_agent, instance, queue_of, record, shared, work};

struct Fixture {
    jobs: Arc<ScriptedJobs>,
    queue: Arc<DeliveryQueue>,
    record: Arc<RunRecord>,
    instances: Arc<Instances>,
    observer: ExecObserver,
    calls: Arc<Mutex<Vec<InstanceEnded>>>,
    gate: Arc<Notify>,
    settling: JoinHandle<()>,
}

async fn ticks() {
    for _ in 0..30 {
        tokio::task::yield_now().await;
    }
}

async fn settling(expect_observations: bool) -> Fixture {
    let jobs = ScriptedJobs::new("analyst", "i1");
    let calls: Arc<Mutex<Vec<InstanceEnded>>> = Arc::default();
    let gate = Arc::new(Notify::new());
    let seen = Arc::clone(&calls);
    let held = Arc::clone(&gate);
    let hook = lifecycle_hook(move |ended| {
        let seen = Arc::clone(&seen);
        let held = Arc::clone(&held);
        async move {
            seen.lock().expect("lock").push(ended);
            held.notified().await;
        }
    });
    let instances = Arc::new(Instances::default());
    let shared = shared(&jobs, Some(hook), &instances);
    let work = work(&jobs, 1);
    let queue: Arc<DeliveryQueue> = Arc::new(queue_of(&work).await);
    let record = record(&work);
    record.set_book(Arc::new(book_with_agent(&work, &queue, "work")));
    if expect_observations {
        record.expect_observations_end();
    }
    let observer = ExecObserver {
        shared: Arc::clone(&shared),
        record: Arc::clone(&record),
    };
    let settling = tokio::spawn({
        let (record, queue) = (Arc::clone(&record), Arc::clone(&queue));
        async move { settle(&shared, &record, &queue).await }
    });
    ticks().await;
    Fixture {
        jobs,
        queue,
        record,
        instances,
        observer,
        calls,
        gate,
        settling,
    }
}

fn created(n: u128) -> Observation {
    Observation::InstanceCreated {
        instance: instance(n),
        node: "work".to_owned(),
        kind: "programmatic".to_owned(),
        subkind: "worker".to_owned(),
    }
}

#[tokio::test]
async fn given_an_instance_reported_after_the_executor_returned_when_the_run_settles_then_the_settle_waits_for_the_end_of_the_observations_and_for_the_hook_of_that_instance_which_gets_the_live_book()
 {
    let fixture = settling(true).await;
    assert!(!fixture.settling.is_finished());
    fixture.observer.observe(created(7));
    ticks().await;
    assert!(!fixture.settling.is_finished());
    assert!(fixture.instances.holds(instance(7)));
    assert!(fixture.calls.lock().expect("lock").is_empty());
    fixture
        .observer
        .observe(Observation::Outcome(OutcomeKind::RunEnded));
    ticks().await;
    {
        let calls = fixture.calls.lock().expect("lock");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].instance_id, instance(7));
        assert_eq!(calls[0].node, "work");
        assert_eq!(calls[0].agents.len(), 1);
        assert_eq!(calls[0].agents[0].key.label, "main");
    }
    assert!(!fixture.settling.is_finished());
    fixture.gate.notify_one();
    tokio::time::timeout(Duration::from_secs(5), fixture.settling)
        .await
        .expect("the settle ends once the hook returned")
        .expect("joined");
}

#[tokio::test]
async fn given_an_executor_whose_observer_is_dropped_before_any_outcome_when_the_run_settles_then_the_settle_does_not_wait_any_longer()
 {
    let fixture = settling(true).await;
    assert!(!fixture.settling.is_finished());
    drop(fixture.observer);
    tokio::time::timeout(Duration::from_secs(5), fixture.settling)
        .await
        .expect("the settle ends")
        .expect("joined");
    assert!(fixture.calls.lock().expect("lock").is_empty());
    assert!(fixture.record.running_hooks().is_empty());
}

#[tokio::test]
async fn given_a_run_whose_executor_did_not_start_when_the_run_settles_then_the_settle_does_not_wait_for_observations()
 {
    let fixture = settling(false).await;
    tokio::time::timeout(Duration::from_secs(5), fixture.settling)
        .await
        .expect("the settle ends with no observation")
        .expect("joined");
}

#[tokio::test]
async fn given_observations_that_are_not_over_when_the_run_settles_then_one_warning_names_the_wait_once()
 {
    let (logs, _guard) = Logs::capture();
    let fixture = settling(true).await;
    assert_eq!(logs.containing(Level::WARN, "its last observations"), 1);
    assert_eq!(
        logs.containing(Level::WARN, &fixture.record.run_id.to_string()),
        1
    );
    fixture
        .observer
        .observe(Observation::Outcome(OutcomeKind::RunEnded));
    tokio::time::timeout(Duration::from_secs(5), fixture.settling)
        .await
        .expect("the settle ends")
        .expect("joined");
    assert_eq!(logs.containing(Level::WARN, "its last observations"), 1);
}

#[tokio::test]
async fn given_observations_that_are_already_over_when_the_run_settles_then_no_wait_is_logged() {
    let (logs, _guard) = Logs::capture();
    let jobs = ScriptedJobs::new("analyst", "i1");
    let instances = Arc::new(Instances::default());
    let shared = shared(&jobs, None, &instances);
    let work = work(&jobs, 1);
    let queue = queue_of(&work).await;
    let record = record(&work);
    record.expect_observations_end();
    record.end_observations();
    settle(&shared, &record, &queue).await;
    assert_eq!(logs.containing(Level::WARN, "its last observations"), 0);
}

#[tokio::test]
async fn given_a_write_handed_over_while_the_settle_waits_for_the_observations_when_they_end_then_the_settle_does_not_wait_for_that_write()
 {
    let fixture = settling(true).await;
    fixture.jobs.pause();
    let _write = fixture
        .queue
        .step_started(0, "late".to_owned(), chrono::Utc::now());
    ticks().await;
    assert!(!fixture.queue.state().waiting.is_empty());
    fixture
        .observer
        .observe(Observation::Outcome(OutcomeKind::RunEnded));
    tokio::time::timeout(Duration::from_secs(5), fixture.settling)
        .await
        .expect("the settle does not wait for a write handed over after the executor returned")
        .expect("joined");
}
