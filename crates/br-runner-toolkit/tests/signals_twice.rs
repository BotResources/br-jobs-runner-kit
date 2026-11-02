#![cfg(unix)]

mod support;

use std::time::{Duration, Instant};

use br_runner_jobs::PresenceStatus;
use br_runner_jobs::test_double::ScriptedJobs;
use br_runner_toolkit::{RunnerBuilder, Settings};
use support::*;

fn kill_term() {
    let status = std::process::Command::new("kill")
        .args(["-TERM", &std::process::id().to_string()])
        .status()
        .expect("kill ran");
    assert!(status.success());
}

fn settings_with_a_drain_of_two_seconds() -> Settings {
    Settings::builder()
        .runner_version("1.0.0")
        .max_concurrent_runs(1)
        .ack_wait(Duration::from_secs(30))
        .drain_timeout(Duration::from_secs(2))
        .cancel_grace(Duration::from_secs(1))
        .claim_wait(Duration::from_secs(5))
        .outcome_wait(Duration::from_secs(10))
        .retry_wait(Duration::from_secs(1))
        .build()
        .expect("settings")
}

async fn real_until(mut condition: impl FnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(10), async {
        while !condition() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the condition came true");
}

#[tokio::test]
async fn given_a_drain_that_a_signal_began_when_a_second_signal_comes_then_the_drain_goes_on_until_its_time_ends()
 {
    let jobs = ScriptedJobs::new(runner_type("analyst"), "i1");
    push_mode(&jobs);
    jobs.push_trigger(trigger(1));
    let runner = RunnerBuilder::new(jobs.client(), settings_with_a_drain_of_two_seconds());
    let done = tokio::spawn(
        runner.start(args(single(Behavior::UntilCancelled), false).with_process_signals()),
    );
    real_until(|| kinds(&jobs) == vec!["started"]).await;
    kill_term();
    real_until(|| {
        jobs.presence_records().last().map(|record| record.status) == Some(PresenceStatus::Draining)
    })
    .await;
    let second = Instant::now();
    kill_term();
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(!done.is_finished());
    let report = tokio::time::timeout(Duration::from_secs(10), done)
        .await
        .expect("the runner returned")
        .expect("joined")
        .expect("started");
    assert!(second.elapsed() >= Duration::from_secs(1));
    assert_eq!(report.runs.len(), 1);
    assert!(report.runs[0].aborted);
    assert_eq!(jobs.presence_deletions(), 1);
}
