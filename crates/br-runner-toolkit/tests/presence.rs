mod support;

use std::time::Duration;

use br_runner_jobs::test_double::{Call, ScriptedJobs};
use br_runner_jobs::{Error, PresenceStatus};
use support::*;

fn presence_calls(jobs: &ScriptedJobs) -> Vec<&'static str> {
    jobs.calls()
        .into_iter()
        .filter_map(|call| match call {
            Call::PresencePut => Some("put"),
            Call::PresenceDelete => Some("delete"),
            _ => None,
        })
        .collect()
}

#[tokio::test(start_paused = true)]
async fn given_a_time_to_live_of_ninety_seconds_when_the_runner_runs_then_the_presence_is_published_at_once_and_then_every_thirty_seconds()
 {
    let jobs = ScriptedJobs::new("analyst", "i1");
    push_mode(&jobs);
    let running = run(&jobs, |b| b, 3, args(single(Behavior::Result("x")), false));
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(jobs.presence_records().len(), 1);
    tokio::time::sleep(Duration::from_secs(28)).await;
    assert_eq!(jobs.presence_records().len(), 1);
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert_eq!(jobs.presence_records().len(), 2);
    tokio::time::sleep(Duration::from_secs(30)).await;
    let records = jobs.presence_records();
    assert_eq!(records.len(), 3);
    for record in &records {
        assert_eq!(record.status, PresenceStatus::Ready);
        assert_eq!(record.capacity, 3);
        assert_eq!(record.runner_version, "1.0.0");
    }
    running.stop().await;
}

#[tokio::test(start_paused = true)]
async fn given_a_publication_that_fails_when_the_beat_comes_then_the_next_beat_publishes_again() {
    let jobs = ScriptedJobs::new("analyst", "i1");
    push_mode(&jobs);
    jobs.fail_next_presence(Error::InfraMissing("the bucket".to_owned()));
    let running = run(&jobs, |b| b, 1, args(single(Behavior::Result("x")), false));
    tokio::time::sleep(Duration::from_secs(70)).await;
    assert_eq!(jobs.presence_records().len(), 3);
    running.stop().await;
}

#[tokio::test(start_paused = true)]
async fn given_a_draining_runner_when_the_drain_ends_then_the_presence_is_deleted_once_after_the_last_publication()
 {
    let jobs = ScriptedJobs::new("analyst", "i1");
    push_mode(&jobs);
    let running = run(&jobs, |b| b, 1, args(single(Behavior::Result("x")), false));
    tokio::time::sleep(Duration::from_secs(40)).await;
    let report = running.stop().await;
    assert!(report.runs.is_empty());
    let calls = presence_calls(&jobs);
    assert_eq!(calls.last(), Some(&"delete"));
    assert_eq!(calls.iter().filter(|call| **call == "delete").count(), 1);
    tokio::time::sleep(Duration::from_secs(300)).await;
    assert_eq!(presence_calls(&jobs), calls);
}
