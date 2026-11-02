mod support;

use std::sync::{Arc, Mutex};

use br_runner_jobs::FailureKind;
use br_runner_jobs::test_double::{Fact, ScriptedJobs};
use br_runner_toolkit::{
    RunRefusal, RunnerBuilder, StartArgs, StartedRun, failure_mapping, run_start,
};
use serde_json::json;
use support::*;
use tokio_util::sync::CancellationToken;

fn sync_trigger(n: u128) -> Vec<u8> {
    call_with(trigger(n), &[("mode", json!("SYNC"))])
}

#[tokio::test(start_paused = true)]
async fn given_a_restartable_runner_and_an_agent_with_an_inbox_when_the_fields_cannot_be_written_then_the_run_fails_permanently_with_saved_state_not_made_and_no_completed_fact_is_sent()
 {
    let jobs = ScriptedJobs::new(runner_type("analyst"), "i1");
    push_mode(&jobs);
    jobs.graphql_default(writes_ok());
    jobs.push_trigger(trigger(1));
    let trap = Trap::default();
    let fields = trap.clone();
    let builder =
        RunnerBuilder::new(jobs.client(), settings(1)).with_lifecycle_hook(trap.hook_failing());
    let agent = BoomAgent {
        lookup: builder.instance_lookup(),
    };
    let args = StartArgs::new(
        boom_graph(Some(agent)),
        run_start(move |_input| {
            let trap = fields.clone();
            async move { Ok::<_, RunRefusal>(StartedRun::new(Boom { trap }, vec!["work".to_owned()])) }
        }),
        true,
    );
    let shutdown = CancellationToken::new();
    let done = tokio::spawn(builder.start(args.with_shutdown(shutdown.clone())));
    until(|| has_terminal(&jobs)).await;
    shutdown.cancel();
    done.await.expect("joined").expect("started");
    let failed = failed_fact(&jobs).expect("a failed fact");
    assert_eq!(failed.reason_code, "saved_state_not_made");
    assert_eq!(failed.kind, FailureKind::Permanent);
    assert!(
        !facts(&jobs)
            .iter()
            .any(|fact| matches!(fact, Fact::Completed { .. }))
    );
}

#[tokio::test(start_paused = true)]
async fn given_a_failure_mapping_that_panics_when_the_closing_reports_a_refused_answer_then_the_failed_fact_is_panic_and_the_mapping_was_called_once_with_answer_refused()
 {
    let jobs = ScriptedJobs::new(runner_type("analyst"), "i1");
    push_mode(&jobs);
    jobs.graphql_default(json!({
        "jobsUploadResult": { "success": false, "reason": "too_big", "message": "the answer is too big" }
    }));
    jobs.push_trigger(sync_trigger(1));
    let seen: Arc<Mutex<Vec<String>>> = Arc::default();
    let recorded = Arc::clone(&seen);
    let running = run(
        &jobs,
        |b| b,
        1,
        args(single(Behavior::Result("x")), false).with_failure_mapping(failure_mapping(
            move |failure| {
                recorded
                    .lock()
                    .expect("lock")
                    .push(failure.reason_code.to_owned());
                panic!("the mapping panics");
            },
        )),
    );
    until(|| has_terminal(&jobs)).await;
    running.stop().await;
    let failed = failed_fact(&jobs).expect("a failed fact");
    assert_eq!(failed.reason_code, "panic");
    assert!(failed.diagnostic.to_string().contains("the mapping panics"));
    assert_eq!(
        seen.lock().expect("lock").clone(),
        vec!["answer_refused".to_owned()]
    );
}
