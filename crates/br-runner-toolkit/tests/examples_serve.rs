mod support;

#[path = "../examples/serve_runs.rs"]
#[allow(dead_code)]
mod serve_runs;

use br_runner_jobs::test_double::ScriptedJobs;
use tokio_util::sync::CancellationToken;

use support::{has_terminal, kinds, push_mode, trigger, until};

#[tokio::test(start_paused = true)]
async fn the_serve_runs_example_takes_a_run_and_reports_it() {
    let jobs = ScriptedJobs::new("analyst", "i1");
    push_mode(&jobs);
    jobs.push_trigger(trigger(1));
    let shutdown = CancellationToken::new();
    let done = tokio::spawn(serve_runs::serve(jobs.client(), shutdown.clone()));
    until(|| has_terminal(&jobs)).await;
    shutdown.cancel();
    let report = done.await.expect("joined").expect("served");
    assert!(report.runs.is_empty());
    assert_eq!(kinds(&jobs), vec!["started", "completed"]);
}
