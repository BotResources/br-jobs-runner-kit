mod support;

use br_runner_jobs::PresenceStatus;
use br_runner_jobs::test_double::{Delivery, ScriptedJobs};
use support::*;

#[tokio::test(start_paused = true)]
async fn given_a_trigger_in_push_mode_when_the_graph_ends_then_started_acknowledged_completed_in_order()
 {
    let jobs = ScriptedJobs::new("analyst", "i1");
    push_mode(&jobs);
    jobs.push_trigger(trigger(1));
    let running = run(
        &jobs,
        |b| b,
        2,
        args(single(Behavior::Result("done")), false),
    );
    until(|| has_terminal(&jobs)).await;
    assert_eq!(kinds(&jobs), vec!["started", "completed"]);
    assert_eq!(jobs.deliveries(), vec![Delivery::Ack]);
    let report = running.stop().await;
    assert!(report.runs.is_empty());
    assert_eq!(jobs.presence_deletions(), 1);
    let records = jobs.presence_records();
    assert_eq!(records[0].status, PresenceStatus::Ready);
    assert_eq!(records[0].capacity, 2);
    assert_eq!(records[0].runner_version, "1.0.0");
    assert_eq!(
        records.last().expect("a record").status,
        PresenceStatus::Draining
    );
}
