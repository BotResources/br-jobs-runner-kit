mod support;

use br_llm_messages::{Turn, TurnId};
use br_runner_threads::test_double::MemoryThreads;
use br_runner_threads::{Annotation, AnnotationBody, Error, MessageStatus, PlacedAnnotation};
use contract_threads::AiDelivery;
use support::client_x::*;
use support::*;

#[tokio::test]
async fn writing_a_message_sends_a_completed_ai_message_with_its_values() {
    let threads = MemoryThreads::new();
    let thread_id = v7();
    let mut message = ai_message(thread_id);
    let (parent, job, acting, impersonator) = (v7(), v7(), v7(), v7());
    message.parent_message_id = Some(parent);
    message.job_id = Some(job);
    message.acting_user_id = Some(acting);
    message.impersonator_id = Some(impersonator);
    message.author_name = "Analyst".to_owned();

    let confirmed = threads
        .client()
        .write_message(&message)
        .await
        .expect("confirmed");

    let command = last_command(&threads);
    let store = store_of(&command.body);
    let ai = ai_of(store);
    assert_eq!(command.id, message.command_id);
    assert_eq!(store.thread_id, thread_id);
    assert_eq!(store.message_id, message.turn.id().as_uuid());
    assert_eq!(store.parent_message_id, Some(parent));
    assert_eq!(store.job_id, Some(job));
    assert_eq!(store.acting_user_id, Some(acting));
    assert_eq!(store.impersonator_id, Some(impersonator));
    assert_eq!(ai.agent_id, message.agent_id.as_uuid());
    assert_eq!(ai.author_name, "Analyst");
    let AiDelivery::Completed { turn, annotations } = &ai.delivery else {
        panic!("not a completed delivery");
    };
    assert_eq!(turn.to_turn().expect("the turn reads back"), message.turn);
    assert!(annotations.is_empty());
    assert_eq!(confirmed.status, Some(MessageStatus::Completed));
}

#[tokio::test]
async fn a_message_with_no_parent_starts_the_tree() {
    let threads = MemoryThreads::new();
    let message = ai_message(v7());

    threads
        .client()
        .write_message(&message)
        .await
        .expect("confirmed");

    let command = last_command(&threads);
    let store = store_of(&command.body);
    assert_eq!(store.parent_message_id, None);
    assert_eq!(store.job_id, None);
    assert_eq!(store.acting_user_id, None);
    assert_eq!(store.impersonator_id, None);
}

#[tokio::test]
async fn an_input_turn_is_written_whole_under_the_id_of_the_turn() {
    let threads = MemoryThreads::new();
    let mut message = ai_message(v7());
    let input = input_turn(vec![human_entry("hello")]);
    let turn_id: TurnId = input.id();
    message.turn = Turn::InputTurn(input);

    threads
        .client()
        .write_message(&message)
        .await
        .expect("confirmed");

    let command = last_command(&threads);
    let store = store_of(&command.body);
    assert_eq!(store.message_id, turn_id.as_uuid());
    let AiDelivery::Completed { turn, .. } = &ai_of(store).delivery else {
        panic!("not a completed delivery");
    };
    assert_eq!(turn.to_turn().expect("reads back"), message.turn);
}

#[tokio::test]
async fn annotations_travel_with_the_entry_they_follow() {
    let threads = MemoryThreads::new();
    let mut message = ai_message(v7());
    let after = entry_id();
    let annotation = Annotation::new(
        v7(),
        AnnotationBody::Warning {
            code: "slow".to_owned(),
            text: Some("took long".to_owned()),
        },
    );
    message.annotations = vec![PlacedAnnotation {
        after_entry: after,
        annotation: annotation.clone(),
    }];

    threads
        .client()
        .write_message(&message)
        .await
        .expect("confirmed");

    let command = last_command(&threads);
    let AiDelivery::Completed { annotations, .. } = &ai_of(store_of(&command.body)).delivery else {
        panic!("not a completed delivery");
    };
    assert_eq!(annotations.len(), 1);
    assert_eq!(annotations[0].after_entry_id, after.as_uuid());
    assert_eq!(annotations[0].annotation.id, annotation.id);
}

#[tokio::test]
async fn every_annotation_body_is_sent_as_its_wire_kind() {
    let threads = MemoryThreads::new();
    let mut message = ai_message(v7());
    let bodies = [
        AnnotationBody::Artifact { artifact_id: v7() },
        AnnotationBody::ThreadReference { thread_id: v7() },
        AnnotationBody::JobReference { job_id: v7() },
        AnnotationBody::Error {
            code: "boom".to_owned(),
            text: None,
        },
        AnnotationBody::Warning {
            code: "careful".to_owned(),
            text: None,
        },
    ];
    message.annotations = bodies
        .iter()
        .map(|body| PlacedAnnotation {
            after_entry: entry_id(),
            annotation: Annotation::new(v7(), body.clone()),
        })
        .collect();

    threads
        .client()
        .write_message(&message)
        .await
        .expect("confirmed");

    let command = last_command(&threads);
    let AiDelivery::Completed { annotations, .. } = &ai_of(store_of(&command.body)).delivery else {
        panic!("not a completed delivery");
    };
    let kinds: Vec<_> = annotations.iter().map(|a| a.annotation.kind()).collect();
    use contract_threads::AnnotationKind as Kind;
    assert_eq!(
        kinds,
        vec![
            Kind::Artifact,
            Kind::ThreadReference,
            Kind::JobReference,
            Kind::Error,
            Kind::Warning
        ]
    );
}

#[tokio::test]
async fn an_annotation_id_that_is_not_a_uuidv7_refuses_the_message_before_sending() {
    let threads = MemoryThreads::new();
    let mut message = ai_message(v7());
    message.annotations = vec![PlacedAnnotation {
        after_entry: entry_id(),
        annotation: Annotation::new(
            not_v7(),
            AnnotationBody::Error {
                code: "boom".to_owned(),
                text: None,
            },
        ),
    }];

    let result = threads.client().write_message(&message).await;

    assert!(matches!(result, Err(Error::InvalidInput(_))), "{result:?}");
    assert!(threads.operations().is_empty());
}

#[tokio::test]
async fn opening_a_stream_sends_a_streaming_message_with_no_turn_and_no_annotation() {
    let threads = MemoryThreads::new();
    let client = threads.client();
    let mut open = open_request(v7());
    let (parent, job, acting) = (v7(), v7(), v7());
    open.parent_message_id = Some(parent);
    open.job_id = Some(job);
    open.acting_user_id = Some(acting);

    let handle = client.open_stream(&open).await.expect("the handle");

    let command = last_command(&threads);
    let store = store_of(&command.body);
    let ai = ai_of(store);
    assert_eq!(command.id, open.command_id);
    assert_eq!(store.message_id, open.message_id.as_uuid());
    assert_eq!(handle.message_id(), open.message_id.as_uuid());
    assert_eq!(store.thread_id, open.thread_id);
    assert_eq!(store.parent_message_id, Some(parent));
    assert_eq!(store.job_id, Some(job));
    assert_eq!(store.acting_user_id, Some(acting));
    assert_eq!(store.impersonator_id, None);
    assert_eq!(ai.agent_id, open.agent_id.as_uuid());
    assert_eq!(ai.author_name, open.author_name);
    assert_eq!(ai.delivery, AiDelivery::Streaming);
    assert!(threads.chunks().is_empty());
}
