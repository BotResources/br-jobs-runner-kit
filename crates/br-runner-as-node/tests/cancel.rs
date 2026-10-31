mod support;

use std::sync::Arc;

use br_graph_contract::{CancelToken, Cause, Event, InstanceId, State};
use br_runner_as_node::{JobSpec, Payload, RunnerAsNode};
use br_runner_jobs::SyncEnd;
use br_runner_jobs::test_double::Answer;
use support::*;

fn hung_request(source: impl br_runner_as_node::CallerSource) -> RunnerAsNode<Flow> {
    RunnerAsNode::new(
        "ask",
        runner_type(),
        |_state: Arc<State<Flow>>| std::future::pending::<Result<JobSpec, Cause>>(),
        |_state, _outcome| async { Ok::<_, Cause>(FlowUpdate::default()) },
        source,
    )
}

fn hung_response(source: impl br_runner_as_node::CallerSource) -> RunnerAsNode<Flow> {
    RunnerAsNode::new(
        "ask",
        runner_type(),
        |_state: Arc<State<Flow>>| async { Ok(JobSpec::new(Payload::Kept(content("q")))) },
        |_state, _outcome| std::future::pending::<Result<FlowUpdate, Cause>>(),
        source,
    )
}

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
    let node = hung_request(source_of(&rig.handle));
    let mut run = start(&node, rig.token.view()).await;
    run.give("q");
    assert!(stays_pending(run.events.recv()).await);

    rig.token.handle().trigger();

    assert!(matches!(run.event().await, Event::Cancelled));
    assert!(rig.script.calls().is_empty());
}

#[tokio::test]
async fn given_a_caller_source_that_waits_when_the_cancel_view_fires_then_the_event_cancelled_is_sent_with_no_cancel_handed_over()
 {
    let rig = rig();
    let node = standard(|_id: InstanceId| {
        std::future::pending::<Result<br_runner_jobs::CallerHandle, Cause>>()
    });
    let mut run = start(&node, rig.token.view()).await;
    run.give("q");
    assert!(stays_pending(run.events.recv()).await);

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
    let node = hung_response(source_of(&rig.handle));
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
    assert!(stays_pending(run.events.recv()).await);

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
async fn given_a_dropped_instance_when_the_node_waits_for_the_end_then_nothing_more_is_handed_over()
{
    let rig = rig();
    let node = standard(source_of(&rig.handle));
    let run = start(&node, rig.token.view()).await;
    run.give("q");
    let _ = the_create(&rig.script).await;

    drop(run);
    rig.token.handle().trigger();
    settle_calls().await;

    assert_eq!(kinds(&rig.script), ["end_wait", "upload", "create"]);
}
