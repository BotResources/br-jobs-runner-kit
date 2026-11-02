mod support;

use std::num::NonZeroUsize;

use br_runner_jobs::FailureKind;
use br_runner_jobs::test_double::ScriptedJobs;
use serde_json::json;
use support::*;

fn saved_with(steps: &[&str]) -> serde_json::Value {
    let mut reply = saved_state_reply(json!({ "fields": { "steps": steps }, "agents": [] }));
    reply
        .as_object_mut()
        .expect("object")
        .extend(writes_ok().as_object().expect("object").clone());
    reply
}

#[tokio::test(start_paused = true)]
async fn given_saved_fields_above_the_state_limit_when_a_restart_starts_then_the_run_fails_permanently_with_saved_state_invalid()
 {
    let jobs = ScriptedJobs::new(runner_type("analyst"), "i1");
    push_mode(&jobs);
    jobs.graphql_default(saved_with(&["a long step of an old run, a long one"]));
    jobs.push_trigger(restart_trigger(1));
    let running = run(
        &jobs,
        |b| b,
        1,
        args(single(Behavior::Result("x")), true)
            .with_state_limit(NonZeroUsize::new(20).expect("non zero")),
    );
    until(|| has_terminal(&jobs)).await;
    running.stop().await;
    let failed = failed_fact(&jobs).expect("a failed fact");
    assert_eq!(failed.reason_code, "saved_state_invalid");
    assert_eq!(failed.kind, FailureKind::Permanent);
}

#[tokio::test(start_paused = true)]
async fn given_saved_fields_under_the_state_limit_when_a_restart_starts_then_the_run_completes() {
    let jobs = ScriptedJobs::new(runner_type("analyst"), "i1");
    push_mode(&jobs);
    jobs.graphql_default(saved_with(&["old"]));
    jobs.push_trigger(restart_trigger(1));
    let running = run(
        &jobs,
        |b| b,
        1,
        args(single(Behavior::Result("x")), true)
            .with_state_limit(NonZeroUsize::new(200).expect("non zero")),
    );
    until(|| has_terminal(&jobs)).await;
    running.stop().await;
    assert_eq!(kinds(&jobs), vec!["started", "completed"]);
}
