mod support;

use std::sync::Arc;

use br_runner_jobs::FailureKind;
use br_runner_jobs::test_double::ScriptedJobs;
use br_runner_toolkit::{
    FailureOverride, RunRefusal, StartArgs, StartedRun, failure_mapping, run_start,
};
use serde_json::json;
use support::*;

async fn failed_run(graph: Arc<br_graph_contract::Graph<Flow>>) -> br_runner_jobs::Failed {
    let jobs = ScriptedJobs::new("analyst", "i1");
    push_mode(&jobs);
    jobs.push_trigger(trigger(1));
    let running = run(&jobs, |b| b, 1, args(graph, false));
    until(|| has_terminal(&jobs)).await;
    running.stop().await;
    failed_fact(&jobs).expect("a failed fact")
}

#[tokio::test(start_paused = true)]
async fn given_a_node_that_fails_when_the_run_ends_then_the_failed_fact_has_node_failed_permanent_and_the_reason_as_diagnostic()
 {
    let failed = failed_run(single(Behavior::Fail("the model is down"))).await;
    assert_eq!(failed.reason_code, "node_failed");
    assert_eq!(failed.kind, FailureKind::Permanent);
    assert_eq!(failed.params, json!({}));
    assert_eq!(failed.retry_after_seconds, None);
    assert!(
        failed
            .diagnostic
            .as_str()
            .is_some_and(|text| text.contains("the model is down"))
    );
}

#[tokio::test(start_paused = true)]
async fn given_a_node_that_refuses_its_input_when_the_run_ends_then_the_reason_code_is_node_refused_an_input()
 {
    let failed = failed_run(single(Behavior::Refuse("bad input"))).await;
    assert_eq!(failed.reason_code, "node_refused_an_input");
}

#[tokio::test(start_paused = true)]
async fn given_a_failure_mapping_when_a_run_fails_then_the_mapping_overrides_only_what_it_gives() {
    let jobs = ScriptedJobs::new("analyst", "i1");
    push_mode(&jobs);
    jobs.push_trigger(trigger(1));
    let mapping = failure_mapping(|failure| {
        assert_eq!(failure.reason_code, "node_failed");
        assert!(failure.cause.is_some());
        FailureOverride {
            kind: Some(FailureKind::Transient),
            retry_after_seconds: Some(30),
            params: Some(json!({ "model": "m" })),
            ..FailureOverride::default()
        }
    });
    let running = run(
        &jobs,
        |b| b,
        1,
        args(single(Behavior::Fail("down")), false).with_failure_mapping(mapping),
    );
    until(|| has_terminal(&jobs)).await;
    running.stop().await;
    let failed = failed_fact(&jobs).expect("failed");
    assert_eq!(failed.kind, FailureKind::Transient);
    assert_eq!(failed.reason_code, "node_failed");
    assert_eq!(failed.retry_after_seconds, Some(30));
    assert_eq!(failed.params, json!({ "model": "m" }));
}

#[tokio::test(start_paused = true)]
async fn given_a_failure_mapping_that_panics_when_a_run_fails_then_the_failed_fact_has_the_reason_code_panic()
 {
    let jobs = ScriptedJobs::new("analyst", "i1");
    push_mode(&jobs);
    jobs.push_trigger(trigger(1));
    let mapping = failure_mapping(|_| panic!("the mapping panics"));
    let running = run(
        &jobs,
        |b| b,
        1,
        args(single(Behavior::Fail("down")), false).with_failure_mapping(mapping),
    );
    until(|| has_terminal(&jobs)).await;
    running.stop().await;
    assert_eq!(failed_fact(&jobs).expect("failed").reason_code, "panic");
}

fn refusing_args(refusal: RunRefusal) -> StartArgs<Flow> {
    StartArgs::new(
        single(Behavior::Result("x")),
        run_start(move |_input| {
            let refusal = refusal.clone();
            async move { Err::<StartedRun<Flow>, _>(refusal) }
        }),
        false,
    )
}

#[tokio::test(start_paused = true)]
async fn given_a_refusal_of_the_run_start_function_when_it_carries_fields_then_they_reach_jobs_and_the_mapping_is_not_called()
 {
    let jobs = ScriptedJobs::new("analyst", "i1");
    push_mode(&jobs);
    jobs.push_trigger(trigger(1));
    let refusal = RunRefusal::new("quota_over")
        .with_kind(FailureKind::Transient)
        .with_params(json!({ "limit": 3 }))
        .with_diagnostic(json!("over"))
        .with_retry_after_seconds(60);
    let mapping = failure_mapping(|_| panic!("the mapping must not be called"));
    let running = run(
        &jobs,
        |b| b,
        1,
        refusing_args(refusal).with_failure_mapping(mapping),
    );
    until(|| has_terminal(&jobs)).await;
    running.stop().await;
    let failed = failed_fact(&jobs).expect("failed");
    assert_eq!(failed.reason_code, "quota_over");
    assert_eq!(failed.kind, FailureKind::Transient);
    assert_eq!(failed.params, json!({ "limit": 3 }));
    assert_eq!(failed.diagnostic, json!("over"));
    assert_eq!(failed.retry_after_seconds, Some(60));
}

#[tokio::test(start_paused = true)]
async fn given_a_refusal_with_no_diagnostic_when_it_is_sent_then_the_diagnostic_is_empty_and_the_kind_permanent()
 {
    let jobs = ScriptedJobs::new("analyst", "i1");
    push_mode(&jobs);
    jobs.push_trigger(trigger(1));
    let running = run(&jobs, |b| b, 1, refusing_args(RunRefusal::new("no")));
    until(|| has_terminal(&jobs)).await;
    running.stop().await;
    let failed = failed_fact(&jobs).expect("failed");
    assert_eq!(failed.diagnostic, json!(""));
    assert_eq!(failed.kind, FailureKind::Permanent);
    assert_eq!(failed.params, json!({}));
}

#[tokio::test(start_paused = true)]
async fn given_a_run_start_function_that_panics_when_the_run_is_taken_then_the_run_fails_with_panic_and_the_slot_is_free()
 {
    let jobs = ScriptedJobs::new("analyst", "i1");
    push_mode(&jobs);
    jobs.push_trigger(trigger(1));
    jobs.push_trigger(trigger(2));
    let panicking = StartArgs::new(
        single(Behavior::Result("x")),
        run_start(|_input| async {
            if true {
                panic!("the function panics");
            }
            Ok::<StartedRun<Flow>, RunRefusal>(StartedRun::new(Flow::default(), Vec::new()))
        }),
        false,
    );
    let running = run(&jobs, |b| b, 1, panicking);
    until(|| kinds(&jobs).iter().filter(|k| **k == "failed").count() == 2).await;
    running.stop().await;
    assert_eq!(failed_fact(&jobs).expect("failed").reason_code, "panic");
}

#[tokio::test(start_paused = true)]
async fn given_fields_over_the_state_limit_when_the_run_starts_then_the_run_fails_with_start_refused()
 {
    let jobs = ScriptedJobs::new("analyst", "i1");
    push_mode(&jobs);
    jobs.push_trigger(trigger(1));
    let big = StartArgs::new(
        single(Behavior::Result("x")),
        run_start(|_input| async {
            Ok::<_, RunRefusal>(StartedRun::new(
                Flow {
                    steps: vec!["x".repeat(500)],
                },
                vec!["work".to_owned()],
            ))
        }),
        false,
    )
    .with_state_limit(std::num::NonZeroUsize::new(100).expect("non zero"));
    let running = run(&jobs, |b| b, 1, big);
    until(|| has_terminal(&jobs)).await;
    running.stop().await;
    assert_eq!(
        failed_fact(&jobs).expect("failed").reason_code,
        "start_refused"
    );
}

#[tokio::test(start_paused = true)]
async fn given_a_start_with_unknown_first_nodes_when_the_executor_refuses_then_the_run_fails_with_start_refused()
 {
    let jobs = ScriptedJobs::new("analyst", "i1");
    push_mode(&jobs);
    jobs.push_trigger(trigger(1));
    let unknown = StartArgs::new(
        single(Behavior::Result("x")),
        run_start(|_input| async {
            Ok::<_, RunRefusal>(StartedRun::new(Flow::default(), vec!["missing".to_owned()]))
        }),
        false,
    );
    let running = run(&jobs, |b| b, 1, unknown);
    until(|| has_terminal(&jobs)).await;
    running.stop().await;
    assert_eq!(
        failed_fact(&jobs).expect("failed").reason_code,
        "start_refused"
    );
}

#[tokio::test(start_paused = true)]
async fn given_a_run_start_function_that_panics_when_a_trigger_arrives_then_the_run_fails_with_panic_and_the_next_run_is_served()
 {
    let jobs = ScriptedJobs::new("analyst", "i1");
    push_mode(&jobs);
    jobs.push_trigger(trigger(1));
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = std::sync::Arc::clone(&calls);
    let args = StartArgs::new(
        single(Behavior::Result("x")),
        run_start(move |_input| {
            let first = counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0;
            async move {
                if first {
                    panic!("the start panics");
                }
                Ok::<_, RunRefusal>(StartedRun::new(Flow::default(), vec!["work".to_owned()]))
            }
        }),
        false,
    );
    let running = run(&jobs, |b| b, 1, args);
    until(|| has_terminal(&jobs)).await;
    assert_eq!(failed_fact(&jobs).expect("failed").reason_code, "panic");
    jobs.push_trigger(trigger(2));
    until(|| kinds(&jobs).contains(&"completed")).await;
    running.stop().await;
}
