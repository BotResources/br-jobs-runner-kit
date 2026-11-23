mod support;

use std::sync::Arc;

use br_graph_contract::Graph;
use br_runner_jobs_client::CancelEvent;
use br_runner_jobs_client::test_double::ScriptedJobs;
use support::*;
use tracing::Level;

async fn settled(graph: Arc<Graph<Flow>>, cancel: bool) -> (Logs, Vec<&'static str>) {
    let (logs, _guard) = Logs::capture();
    let jobs = ScriptedJobs::new(runner_type("analyst"), "i1");
    push_mode(&jobs);
    jobs.push_trigger(trigger(1));
    let running = run(&jobs, |b| b, 1, args(graph, false));
    if cancel {
        until(|| kinds(&jobs) == vec!["started"]).await;
        jobs.push_cancel(Ok(CancelEvent::OrderMade { run_id: id(101) }));
    }
    until(|| has_terminal(&jobs)).await;
    running.stop().await;
    (logs, kinds(&jobs))
}

fn waits_logged(logs: &Logs) -> usize {
    logs.containing(Level::WARN, "its last observations")
}

#[tokio::test(start_paused = true)]
async fn given_a_run_whose_graph_ends_when_it_settles_then_the_runner_logs_no_wait_for_the_last_observations_of_the_executor()
 {
    let (logs, kinds) = settled(single(Behavior::Result("x")), false).await;
    assert_eq!(kinds, vec!["started", "completed"]);
    assert_eq!(waits_logged(&logs), 0);
}

#[tokio::test(start_paused = true)]
async fn given_a_run_whose_graph_fails_when_it_settles_then_the_runner_logs_no_wait_for_the_last_observations_of_the_executor()
 {
    let (logs, kinds) = settled(single(Behavior::Fail("down")), false).await;
    assert_eq!(kinds, vec!["started", "failed"]);
    assert_eq!(waits_logged(&logs), 0);
}

#[tokio::test(start_paused = true)]
async fn given_a_run_that_a_cancel_order_ends_when_it_settles_then_the_runner_logs_no_wait_for_the_last_observations_of_the_executor()
 {
    let (logs, kinds) = settled(single(Behavior::UntilCancelled), true).await;
    assert_eq!(kinds, vec!["started", "failed"]);
    assert_eq!(waits_logged(&logs), 0);
}
