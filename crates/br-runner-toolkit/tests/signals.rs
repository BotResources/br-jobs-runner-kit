#![cfg(unix)]

mod support;

use std::time::Duration;

use br_runner_jobs::test_double::ScriptedJobs;
use br_runner_toolkit::RunnerBuilder;
use support::*;

#[tokio::test]
async fn given_process_signals_when_the_process_gets_sigterm_then_the_runner_drains_and_returns() {
    let jobs = ScriptedJobs::new("analyst", "i1");
    push_mode(&jobs);
    let runner = RunnerBuilder::new(jobs.client(), settings(1));
    let done = tokio::spawn(
        runner.start(args(single(Behavior::Result("x")), false).with_process_signals()),
    );
    tokio::time::timeout(Duration::from_secs(10), async {
        while jobs.presence_records().is_empty() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the presence was published");
    let status = std::process::Command::new("kill")
        .args(["-TERM", &std::process::id().to_string()])
        .status()
        .expect("kill ran");
    assert!(status.success());
    let report = tokio::time::timeout(Duration::from_secs(10), done)
        .await
        .expect("the runner returned")
        .expect("joined")
        .expect("started");
    assert!(report.runs.is_empty());
    assert_eq!(jobs.presence_deletions(), 1);
}
