mod support;

#[path = "../examples/call_a_runner_as_node.rs"]
#[allow(dead_code)] // the main of the example is unused in the test crate
mod call_a_runner_as_node;

use br_graph_contract::{CancelToken, Event, Input, InstanceId, Node, State};
use br_runner_jobs::SyncEnd;
use br_runner_jobs::test_double::Answer;
use call_a_runner_as_node::{Review, call_a_runner_as_node};
use support::*;
use uuid::Uuid;

#[tokio::test]
async fn the_example_node_calls_the_runner_and_puts_the_answer_in_the_state() {
    let rig = rig();
    let node = call_a_runner_as_node(source_of(&rig.handle)).expect("a node");
    let token = CancelToken::new();
    let id = InstanceId::new(Uuid::now_v7()).expect("a UUIDv7");
    let instance = node.create(token.view(), id).await.expect("an instance");
    let (input, mut events) = instance.into_parts();
    let state = State::create(Review {
        question: "count the tickets".to_owned(),
        answer: None,
    })
    .expect("a state");
    input
        .give(Input::new(std::sync::Arc::new(state.clone())))
        .expect("open");
    let create = the_create(&rig.script).await;
    rig.script.end(
        create.job_id,
        Answer::Now(SyncEnd::Completed {
            inline: Some(content("42 tickets")),
            result_ref: None,
        }),
    );

    let event = within(events.recv()).await.expect("an event");

    let Event::Result(update) = event else {
        panic!("expected the event Result");
    };
    let next = state.apply(update).expect("the update applies");
    assert_eq!(next.fields().answer.as_deref(), Some("42 tickets"));
    assert_eq!(node.name(), "ask_the_analyst");
}

#[tokio::test]
async fn the_example_node_fails_the_instance_when_the_runner_fails() {
    let rig = rig();
    let node = call_a_runner_as_node(source_of(&rig.handle)).expect("a node");
    let token = CancelToken::new();
    let id = InstanceId::new(Uuid::now_v7()).expect("a UUIDv7");
    let instance = node.create(token.view(), id).await.expect("an instance");
    let (input, mut events) = instance.into_parts();
    let state = State::create(Review {
        question: "q".to_owned(),
        answer: None,
    })
    .expect("a state");
    input
        .give(Input::new(std::sync::Arc::new(state)))
        .expect("open");
    let create = the_create(&rig.script).await;
    rig.script.end(
        create.job_id,
        Answer::Now(SyncEnd::Failed {
            report: None,
            message: "the analyst broke".to_owned(),
        }),
    );

    let event = within(events.recv()).await.expect("an event");

    let Event::Failed(cause) = event else {
        panic!("expected the event Failed");
    };
    assert_eq!(cause.to_string(), "the analyst broke");
}
