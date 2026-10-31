mod support;

use std::sync::Arc;

use br_graph_contract::{Cause, InstanceId, State};
use br_runner_as_node::{Error, JobSpec, Payload, RunnerAsNode};
use br_runner_jobs::test_double::Answer;
use br_runner_jobs::{Error as JobsError, Removal, SyncEnd};
use support::*;

#[derive(Debug, thiserror::Error)]
#[error("the author's own error")]
struct Own;

fn fault_of(cause: &Cause) -> &Error {
    cause
        .downcast_ref::<Error>()
        .expect("an error of the library")
}

#[tokio::test]
async fn given_a_request_function_that_fails_when_the_instance_runs_then_the_event_failed_carries_the_error_unchanged_and_no_call_is_made()
 {
    let rig = rig();
    let node = RunnerAsNode::new(
        "ask",
        runner_type(),
        |_state: Arc<State<Flow>>| async { Err::<JobSpec, Cause>(Box::new(Own)) },
        |_state, outcome| {
            let update = describe(&outcome);
            async move { Ok::<_, Cause>(update) }
        },
        source_of(&rig.handle),
    );
    let mut run = start(&node, rig.token.view()).await;

    run.give("q");
    let cause = error_of(run.event().await);

    assert!(cause.downcast_ref::<Own>().is_some());
    assert!(rig.script.calls().is_empty());
}

#[tokio::test]
async fn given_a_response_function_that_fails_when_the_runner_ends_then_the_event_failed_carries_the_error_unchanged()
 {
    let rig = rig();
    let node = RunnerAsNode::new(
        "ask",
        runner_type(),
        |_state: Arc<State<Flow>>| async { Ok(JobSpec::new(Payload::Inline(content("q")))) },
        |_state, _outcome| async { Err::<FlowUpdate, Cause>(Box::new(Own)) },
        source_of(&rig.handle),
    );
    let mut run = start(&node, rig.token.view()).await;
    run.give("q");
    let create = the_create(&rig.script).await;

    rig.script.end(
        create.job_id,
        Answer::Now(SyncEnd::Cancelled {
            message: "x".to_owned(),
        }),
    );
    let cause = error_of(run.event().await);

    assert!(cause.downcast_ref::<Own>().is_some());
}

#[tokio::test]
async fn given_a_caller_source_that_fails_when_the_instance_runs_then_the_event_failed_holds_its_error_as_the_source_and_no_call_is_made()
 {
    let node = standard(|_id: InstanceId| async {
        Err::<br_runner_jobs::CallerHandle, Cause>(Box::new(Own))
    });
    let token = br_graph_contract::CancelToken::new();
    let mut run = start(&node, token.view()).await;

    run.give("q");
    let cause = error_of(run.event().await);

    let error = fault_of(&cause);
    assert!(matches!(error, Error::CallerSource(_)));
    let source = std::error::Error::source(error).expect("a source");
    assert!(source.downcast_ref::<Own>().is_some());
}

#[tokio::test]
async fn given_a_caller_handle_that_cannot_take_the_upload_when_the_instance_runs_then_the_event_failed_is_an_upload_fault_with_no_create_and_no_cancel()
 {
    let rig = rig();
    rig.script
        .upload(Answer::Fail(JobsError::InvalidInput("no room".to_owned())));
    let node = standard(source_of(&rig.handle));
    let mut run = start(&node, rig.token.view()).await;

    run.give("q");
    let cause = error_of(run.event().await);

    assert!(matches!(fault_of(&cause), Error::Upload(_)));
    assert_eq!(kinds(&rig.script), ["end_wait", "upload"]);
}

#[tokio::test]
async fn given_a_caller_handle_that_cannot_take_the_create_when_the_instance_runs_then_the_event_failed_is_a_create_fault_and_no_cancel_is_handed_over()
 {
    let rig = rig();
    rig.script
        .create(Answer::Fail(JobsError::InvalidInput("no room".to_owned())));
    let node = standard(source_of(&rig.handle));
    let mut run = start(&node, rig.token.view()).await;

    run.give("q");
    let cause = error_of(run.event().await);

    assert!(matches!(fault_of(&cause), Error::Create(_)));
    assert_eq!(kinds(&rig.script), ["end_wait", "upload", "create"]);
}

#[tokio::test]
async fn given_a_create_removed_after_a_report_when_the_node_waits_then_the_event_failed_is_a_create_removed_fault_and_no_cancel_is_handed_over()
 {
    let removals = [
        (
            Removal::Failed(JobsError::Unreadable("a frame".to_owned())),
            true,
        ),
        (Removal::DependsOnRemoved, false),
        (Removal::LeftUnsent, false),
    ];
    for (removal, has_source) in removals {
        let rig = rig();
        rig.script.create(Answer::Now(Err(removal)));
        let node = standard(source_of(&rig.handle));
        let mut run = start(&node, rig.token.view()).await;

        run.give("q");
        let cause = error_of(run.event().await);

        let error = fault_of(&cause);
        assert!(matches!(error, Error::CreateRemoved { .. }));
        assert_eq!(std::error::Error::source(error).is_some(), has_source);
        assert_eq!(kinds(&rig.script), ["end_wait", "upload", "create"]);
    }
}

#[tokio::test]
async fn given_an_end_wait_that_gives_an_error_when_the_node_waits_then_the_cancel_is_handed_over_once_and_the_event_failed_follows()
 {
    let rig = rig();
    let node = standard(source_of(&rig.handle));
    let mut run = start(&node, rig.token.view()).await;
    run.give("q");
    let create = the_create(&rig.script).await;

    rig.script.end(
        create.job_id,
        Answer::Fail(JobsError::InvalidInput("the feed broke".to_owned())),
    );
    let cause = error_of(run.event().await);

    assert!(matches!(fault_of(&cause), Error::EndWait(_)));
    assert_eq!(
        kinds(&rig.script),
        ["end_wait", "upload", "create", "cancel"]
    );
}

#[tokio::test]
async fn given_a_request_function_that_panics_when_the_instance_runs_then_the_instance_ends_with_no_event_and_no_call_is_made()
 {
    let rig = rig();
    let node = RunnerAsNode::new(
        "ask",
        runner_type(),
        |_state: Arc<State<Flow>>| async { panic!("the author's function panicked") },
        |_state, outcome| {
            let update = describe(&outcome);
            async move { Ok::<_, Cause>(update) }
        },
        source_of(&rig.handle),
    );
    let mut run = start(&node, rig.token.view()).await;

    run.give("q");

    assert!(within(run.events.recv()).await.is_err());
    assert!(rig.script.calls().is_empty());
}
