mod support;

use std::sync::{Arc, Mutex};

use br_graph_contract::{Cause, Input, Node, PROGRAMMATIC, State};
use br_runner_as_node::{JobSpec, Payload, RunnerAsNode};
use br_runner_jobs::test_double::{Answer, MemoryCall};
use br_runner_jobs::{CallMode, PayloadForm, SyncEnd};
use serde_json::json;
use support::*;

fn payload_node(
    payload: Payload,
    config: Option<serde_json::Value>,
    source: impl br_runner_as_node::CallerSource,
) -> RunnerAsNode<Flow> {
    RunnerAsNode::new(
        "ask",
        runner_type(),
        move |_state: Arc<State<Flow>>| {
            let payload = payload.clone();
            let config = config.clone();
            async move {
                let spec = JobSpec::new(payload);
                Ok(match config {
                    Some(config) => spec.with_config(config),
                    None => spec,
                })
            }
        },
        |_state, outcome| {
            let update = describe(&outcome);
            async move { Ok::<_, Cause>(update) }
        },
        source,
    )
}

#[tokio::test]
async fn given_the_node_when_it_is_asked_what_it_is_then_it_is_a_programmatic_runner_call_with_its_name()
 {
    let rig = rig();
    let node = standard(source_of(&rig.handle));

    assert_eq!(node.name(), "ask");
    assert_eq!(node.kind(), PROGRAMMATIC);
    assert_eq!(node.subkind(), "runner_call");
}

#[tokio::test]
async fn given_an_inline_payload_when_the_instance_runs_then_no_upload_is_handed_over_and_the_create_holds_the_content_and_the_config()
 {
    let rig = rig();
    let node = payload_node(
        Payload::Inline(content("inline")),
        Some(json!({ "depth": 3 })),
        source_of(&rig.handle),
    );
    let mut run = start(&node, rig.token.view()).await;

    run.give("q");
    let create = the_create(&rig.script).await;

    assert_eq!(kinds(&rig.script), ["end_wait", "create"]);
    assert_eq!(create.payload, PayloadForm::Inline(content("inline")));
    assert_eq!(create.config, Some(json!({ "depth": 3 })));
    assert_eq!(create.max_attempts, None);
    rig.script.end(
        create.job_id,
        Answer::Now(SyncEnd::Cancelled {
            message: "x".to_owned(),
        }),
    );
    update_of(run.event().await);
}

#[tokio::test]
async fn given_a_document_reference_when_the_instance_runs_then_no_upload_is_handed_over_and_the_create_holds_the_reference()
 {
    let rig = rig();
    let node = payload_node(
        Payload::Reference {
            service: "documents".to_owned(),
            document_id: "doc-7".to_owned(),
        },
        None,
        source_of(&rig.handle),
    );
    let run = start(&node, rig.token.view()).await;

    run.give("q");
    let create = the_create(&rig.script).await;

    assert_eq!(kinds(&rig.script), ["end_wait", "create"]);
    assert_eq!(
        create.payload,
        PayloadForm::Reference {
            service: "documents".to_owned(),
            document_id: "doc-7".to_owned(),
        }
    );
    assert_eq!(create.config, None);
}

#[tokio::test]
async fn given_a_maximum_attempt_count_when_the_instance_runs_then_the_create_carries_it_as_given()
{
    let rig = rig();
    let node = standard(source_of(&rig.handle)).with_max_attempts(4);
    let run = start(&node, rig.token.view()).await;

    run.give("q");
    let create = the_create(&rig.script).await;

    assert_eq!(create.max_attempts, Some(4));
    assert_eq!(create.mode, CallMode::Sync);
}

#[tokio::test]
async fn given_two_states_in_the_queue_when_the_instance_starts_then_the_request_function_gets_the_last_one()
 {
    let rig = rig();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&seen);
    let node = RunnerAsNode::new(
        "ask",
        runner_type(),
        move |state: Arc<State<Flow>>| {
            sink.lock()
                .expect("lock")
                .push(state.fields().question.clone());
            async move { Ok(JobSpec::new(Payload::Inline(content("q")))) }
        },
        |_state, outcome| {
            let update = describe(&outcome);
            async move { Ok::<_, Cause>(update) }
        },
        source_of(&rig.handle),
    );
    let run = start(&node, rig.token.view()).await;

    run.input.give(Input::new(state_of("first"))).expect("open");
    run.input
        .give(Input::new(state_of("second")))
        .expect("open");
    let _ = the_create(&rig.script).await;

    assert_eq!(*seen.lock().expect("lock"), ["second"]);
    assert!(run.input.give(Input::new(state_of("late"))).is_err());
}

#[tokio::test]
async fn given_the_response_function_when_it_runs_then_it_gets_the_state_the_request_function_got()
{
    let rig = rig();
    let node = RunnerAsNode::new(
        "ask",
        runner_type(),
        |_state: Arc<State<Flow>>| async { Ok(JobSpec::new(Payload::Inline(content("q")))) },
        |state: Arc<State<Flow>>, _outcome| {
            let note = state.fields().question.clone();
            async move {
                Ok::<_, Cause>(FlowUpdate {
                    answer: None,
                    note: Some(note),
                })
            }
        },
        source_of(&rig.handle),
    );
    let mut run = start(&node, rig.token.view()).await;
    run.give("the question");
    let create = the_create(&rig.script).await;

    rig.script.end(
        create.job_id,
        Answer::Now(SyncEnd::Cancelled {
            message: "x".to_owned(),
        }),
    );
    let update = update_of(run.event().await);

    assert_eq!(update.note.as_deref(), Some("the question"));
}

#[tokio::test]
async fn given_two_instances_of_one_node_when_they_run_then_each_makes_a_new_child_job_with_new_command_ids_all_uuidv7()
 {
    let rig = rig();
    let node = standard(source_of(&rig.handle));
    let first = start(&node, rig.token.view()).await;
    let second = start(&node, rig.token.view()).await;

    first.give("a");
    second.give("b");
    within(async {
        while rig
            .script
            .calls()
            .iter()
            .filter(|call| matches!(call, MemoryCall::Create(_)))
            .count()
            < 2
        {
            tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        }
    })
    .await;

    let creates: Vec<_> = rig
        .script
        .calls()
        .into_iter()
        .filter_map(|call| match call {
            MemoryCall::Create(input) => Some(input),
            _ => None,
        })
        .collect();
    assert_ne!(creates[0].job_id, creates[1].job_id);
    assert_ne!(creates[0].command_id, creates[1].command_id);
    for create in &creates {
        assert_eq!(create.job_id.get_version_num(), 7);
        assert_eq!(create.command_id.get_version_num(), 7);
    }
}
