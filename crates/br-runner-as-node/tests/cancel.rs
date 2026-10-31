mod support;

use std::sync::Arc;

use br_graph_contract::{CancelToken, Cause, Event};
use br_runner_jobs::test_double::Answer;
use br_runner_jobs::{Removal, SyncEnd};
use support::*;
use tokio::sync::Notify;

#[tokio::test]
async fn given_no_input_yet_when_the_cancel_view_fires_then_the_event_cancelled_is_sent_with_no_call()
 {
    let rig = rig();
    let node = standard(source_of(&rig.handle));
    let mut run = start(&node, rig.token.view()).await;

    rig.token.handle().trigger();

    assert!(matches!(run.event().await, Event::Cancelled));
    assert!(rig.script.calls().is_empty());
}

#[tokio::test]
async fn given_a_request_function_that_waits_when_the_cancel_view_fires_then_the_wait_ends_with_the_event_cancelled_and_no_call()
 {
    let rig = rig();
    let waiting = Arc::new(Notify::new());
    let node = hung_request(source_of(&rig.handle), Arc::clone(&waiting));
    let mut run = start(&node, rig.token.view()).await;
    run.give("q");
    started(&waiting).await;

    rig.token.handle().trigger();

    assert!(matches!(run.event().await, Event::Cancelled));
    assert!(rig.script.calls().is_empty());
}

#[tokio::test]
async fn given_a_caller_source_that_waits_when_the_cancel_view_fires_then_the_event_cancelled_is_sent_with_no_cancel_handed_over()
 {
    let rig = rig();
    let waiting = Arc::new(Notify::new());
    let (held, _gone) = probe();
    let node = standard(hung_source(held, Arc::clone(&waiting)));
    let mut run = start(&node, rig.token.view()).await;
    run.give("q");
    started(&waiting).await;

    rig.token.handle().trigger();

    assert!(matches!(run.event().await, Event::Cancelled));
    assert!(rig.script.calls().is_empty());
}

#[tokio::test]
async fn given_a_delivered_create_when_the_cancel_view_fires_then_one_cancel_follows_the_create_and_the_event_cancelled_is_sent()
 {
    let rig = rig();
    let node = standard(source_of(&rig.handle));
    let mut run = start(&node, rig.token.view()).await;
    run.give("q");
    let create = the_create(&rig.script).await;

    rig.token.handle().trigger();

    assert!(matches!(run.event().await, Event::Cancelled));
    assert_eq!(
        kinds(&rig.script),
        ["end_wait", "upload", "create", "cancel"]
    );
    assert!(matches!(
        rig.script.calls().last(),
        Some(br_runner_jobs::test_double::MemoryCall::Cancel { job_id, .. }) if *job_id == create.job_id
    ));
}

#[tokio::test]
async fn given_a_create_whose_end_is_pending_when_the_cancel_view_fires_then_the_cancel_is_handed_over_and_the_event_cancelled_is_sent()
 {
    let rig = rig();
    rig.script.create(Answer::Never);
    let node = standard(source_of(&rig.handle));
    let mut run = start(&node, rig.token.view()).await;
    run.give("q");
    let _ = the_create(&rig.script).await;

    rig.token.handle().trigger();

    assert!(matches!(run.event().await, Event::Cancelled));
    assert_eq!(
        kinds(&rig.script),
        ["end_wait", "upload", "create", "cancel"]
    );
}

#[tokio::test]
async fn given_a_response_function_that_waits_when_the_cancel_view_fires_then_the_event_cancelled_is_sent_with_the_finish_kept_and_no_cancel()
 {
    let rig = rig();
    let waiting = Arc::new(Notify::new());
    let node = hung_response(source_of(&rig.handle), Arc::clone(&waiting));
    let mut run = start(&node, rig.token.view()).await;
    run.give("q");
    let create = the_create(&rig.script).await;
    rig.script.end(
        create.job_id,
        Answer::Now(SyncEnd::Completed {
            inline: Some(content("done")),
            result_ref: None,
        }),
    );
    started(&waiting).await;

    rig.token.handle().trigger();

    assert!(matches!(run.event().await, Event::Cancelled));
    assert_eq!(
        kinds(&rig.script),
        ["end_wait", "upload", "create", "finish"]
    );
}

#[tokio::test]
async fn given_the_cancel_view_fired_before_the_instance_starts_when_a_state_comes_then_the_event_cancelled_is_sent_with_no_call()
 {
    let rig = rig();
    let token = CancelToken::new();
    token.handle().trigger();
    let node = standard(source_of(&rig.handle));
    let mut run = start(&node, token.view()).await;

    run.give("q");

    assert!(matches!(run.event().await, Event::Cancelled));
    assert!(rig.script.calls().is_empty());
}

#[tokio::test]
async fn given_a_create_that_ended_refused_when_the_cancel_view_fires_before_the_end_is_heard_then_the_event_cancelled_is_sent_with_no_cancel_handed_over()
 {
    let rig = rig();
    rig.script.create(Answer::Now(Err(Removal::Refused(
        a_jobs_refusal("unknown_runner_type").await,
    ))));
    let trigger = rig.token.handle().clone();
    let handle = rig.handle.clone();
    let node = standard(move |_id: br_graph_contract::InstanceId| {
        let trigger = trigger.clone();
        let handle = handle.clone();
        async move {
            trigger.trigger();
            Ok::<_, Cause>(handle)
        }
    });
    let mut run = start(&node, rig.token.view()).await;

    run.give("q");

    assert!(matches!(run.event().await, Event::Cancelled));
    assert_eq!(kinds(&rig.script), ["end_wait", "upload", "create"]);
}

#[tokio::test]
async fn given_a_create_removed_after_a_report_when_the_cancel_view_fires_before_the_end_is_heard_then_the_event_cancelled_is_sent_with_no_cancel_handed_over()
 {
    let rig = rig();
    rig.script.create(Answer::Now(Err(Removal::LeftUnsent)));
    let trigger = rig.token.handle().clone();
    let handle = rig.handle.clone();
    let node = standard(move |_id: br_graph_contract::InstanceId| {
        let trigger = trigger.clone();
        let handle = handle.clone();
        async move {
            trigger.trigger();
            Ok::<_, Cause>(handle)
        }
    });
    let mut run = start(&node, rig.token.view()).await;

    run.give("q");

    assert!(matches!(run.event().await, Event::Cancelled));
    assert_eq!(kinds(&rig.script), ["end_wait", "upload", "create"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn given_the_cancel_view_triggered_at_the_instant_of_the_end_when_many_instances_run_then_a_cancel_and_a_finish_never_both_reach_the_same_child_job()
 {
    for _ in 0..200 {
        let rig = rig();
        let node = standard(source_of(&rig.handle));
        let mut run = start(&node, rig.token.view()).await;
        run.give("q");
        let create = the_create(&rig.script).await;
        let trigger = rig.token.handle().clone();
        let racer = tokio::spawn(async move { trigger.trigger() });

        rig.script
            .end(create.job_id, Answer::Now(completed("done")));
        racer.await.expect("the racer ended");
        let event = run.event().await;

        let kinds = kinds(&rig.script);
        assert!(matches!(event, Event::Cancelled | Event::Result(_)));
        assert!(!(kinds.contains(&"finish") && kinds.contains(&"cancel")));
    }
}
