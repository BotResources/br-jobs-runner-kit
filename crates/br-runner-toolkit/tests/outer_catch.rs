mod support;

use br_runner_jobs::test_double::ScriptedJobs;
use br_runner_toolkit::{RunRefusal, RunnerBuilder, StartArgs, StartedRun, run_start};
use serde_json::json;
use support::*;
use tokio_util::sync::CancellationToken;

#[tokio::test(start_paused = true)]
async fn given_a_sync_run_whose_fields_panic_when_serialized_then_the_failed_fact_is_panic_and_the_slot_serves_the_next_run()
 {
    let jobs = ScriptedJobs::new("analyst", "i1");
    push_mode(&jobs);
    jobs.graphql_default(writes_ok());
    jobs.push_trigger(call_with(trigger(1), &[("mode", json!("SYNC"))]));
    let trap = Trap::default();
    let fields = trap.clone();
    let args = StartArgs::new(
        boom_graph(None),
        run_start(move |_input| {
            let trap = fields.clone();
            async move { Ok::<_, RunRefusal>(StartedRun::new(Boom { trap }, vec!["work".to_owned()])) }
        }),
        false,
    );
    let shutdown = CancellationToken::new();
    let runner =
        RunnerBuilder::new(jobs.client(), settings(1)).with_lifecycle_hook(trap.hook_panicking());
    let done = tokio::spawn(runner.start(args.with_shutdown(shutdown.clone())));
    until(|| has_terminal(&jobs)).await;
    let failed = failed_fact(&jobs).expect("a failed fact");
    assert_eq!(failed.reason_code, "panic");
    assert!(
        failed
            .diagnostic
            .to_string()
            .contains("cannot be serialized")
    );
    jobs.push_trigger(call_with(trigger(2), &[("mode", json!("SYNC"))]));
    until(|| kinds(&jobs).contains(&"completed")).await;
    assert_eq!(
        kinds(&jobs),
        vec!["started", "failed", "started", "completed"]
    );
    assert!(
        jobs.presence_records()
            .iter()
            .all(|record| record.capacity == 1)
    );
    shutdown.cancel();
    done.await.expect("joined").expect("started");
}
