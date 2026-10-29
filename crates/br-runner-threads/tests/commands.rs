mod support;

use std::collections::BTreeMap;

use br_agentic_contract::ThreadRef;
use br_llm_messages::{InputTurn, Turn, TurnEntry};
use br_runner_threads::{
    Annotation, AnnotationBody, Error, PlacedAnnotation, ThreadOrigin, WholeMessage,
};
use contract_threads::{AiDelivery, CreateThread, MessageTurn, StoreMessageV2};
use support::*;
use uuid::Uuid;

fn whole(world: &World, turn: Turn, annotations: Vec<PlacedAnnotation>) -> WholeMessage {
    WholeMessage {
        command_id: v7(),
        thread: world.thread,
        parent_message_id: Some(v7()),
        agent_id: agent_id(),
        author_name: "Analyst".to_owned(),
        job_id: Some(v7()),
        acting_user_id: Some(v7()),
        impersonator_id: Some(v7()),
        turn,
        annotations,
        wait: None,
    }
}

fn ai_turn_with(entry: TurnEntry) -> Turn {
    let mut turn = empty_turn();
    turn.push_entry(entry).expect("an entry");
    turn.into()
}

#[tokio::test]
async fn creating_a_thread_sends_what_the_importer_gave_and_nothing_else() {
    let world = world();
    let (owner, member, user) = (v7(), v7(), v7());
    let mut request = new_thread(&world);
    request.owner_id = Some(owner);
    request.member_ids = vec![member];
    request.title = Some("Plan".to_owned());
    request.resources = BTreeMap::from([("kms".to_owned(), vec!["k1".to_owned()])]);
    request.acting_user_id = Some(user);
    let confirmed = world.client.create_thread(&request).await.expect("created");
    assert_eq!(confirmed.command_id, request.command_id);
    let sent = world.bus.commands();
    assert_eq!(sent.len(), 1);
    assert_eq!(
        (sent[0].aggregate, sent[0].verb, sent[0].version),
        ("thread", "create", 1)
    );
    assert_eq!(sent[0].actor, actor());
    assert_eq!(sent[0].command_id, request.command_id);
    let wire: CreateThread = serde_json::from_value(sent[0].payload.clone()).expect("wire");
    assert_eq!(wire.thread_id, world.thread.as_uuid());
    assert_eq!(wire.owner_id, Some(owner));
    assert_eq!(wire.user_ids, vec![member]);
    assert_eq!(wire.title.as_deref(), Some("Plan"));
    assert_eq!(wire.resources["kms"], vec!["k1".to_owned()]);
    assert_eq!(wire.acting_user_id, Some(user));
    assert_eq!((wire.parent_thread_id, wire.origin_block_id), (None, None));
}

#[tokio::test]
async fn a_child_thread_origin_is_the_parent_and_the_block_together() {
    let world = world();
    let parent = ThreadRef::new(v7());
    let block = v7();
    let mut request = new_thread(&world);
    request.origin = Some(ThreadOrigin {
        parent_thread: parent,
        origin_block_id: block,
    });
    world.client.create_thread(&request).await.expect("created");
    let wire: CreateThread =
        serde_json::from_value(world.bus.commands()[0].payload.clone()).expect("wire");
    assert_eq!(wire.parent_thread_id, Some(parent.as_uuid()));
    assert_eq!(wire.origin_block_id, Some(block));
}

#[tokio::test]
async fn a_thread_gets_no_agent_as_a_member() {
    let world = world();
    let mut request = new_thread(&world);
    request.member_ids = vec![v7()];
    world.client.create_thread(&request).await.expect("created");
    let wire: CreateThread =
        serde_json::from_value(world.bus.commands()[0].payload.clone()).expect("wire");
    assert_eq!(wire.user_ids, request.member_ids);
}

#[tokio::test]
async fn ids_of_new_objects_are_refused_before_sending_when_they_are_not_uuidv7() {
    let world = world();
    let mut request = new_thread(&world);
    request.thread = ThreadRef::new(Uuid::nil());
    assert!(matches!(
        world.client.create_thread(&request).await,
        Err(Error::InvalidInput(_))
    ));
    let mut request = new_thread(&world);
    request.command_id = support::not_v7();
    assert!(matches!(
        world.client.create_thread(&request).await,
        Err(Error::InvalidInput(_))
    ));
    let notice_turn = ai_turn_with(notice());
    let bad_annotation = PlacedAnnotation {
        after_entry: entry_id(),
        annotation: Annotation::new(
            support::not_v7(),
            AnnotationBody::Warning {
                code: "W".to_owned(),
                text: None,
            },
        ),
    };
    let message = whole(&world, notice_turn, vec![bad_annotation]);
    assert!(matches!(
        world.client.write_message(&message).await,
        Err(Error::InvalidInput(_))
    ));
    assert!(world.bus.commands().is_empty());
}

#[tokio::test]
async fn a_whole_message_carries_its_turn_annotations_and_actors_as_given() {
    let world = world();
    let entry = notice();
    let after = match &entry {
        TurnEntry::RuntimeNotice(n) => n.id(),
        _ => unreachable!(),
    };
    let artifact = v7();
    let annotations = vec![PlacedAnnotation {
        after_entry: after,
        annotation: Annotation::new(
            v7(),
            AnnotationBody::Artifact {
                artifact_id: artifact,
            },
        ),
    }];
    let turn = ai_turn_with(entry);
    let message = whole(&world, turn.clone(), annotations);
    world.client.write_message(&message).await.expect("written");
    let sent = &world.bus.commands()[0];
    assert_eq!(
        (sent.aggregate, sent.verb, sent.version),
        ("message", "store", 2)
    );
    let wire: StoreMessageV2 = serde_json::from_value(sent.payload.clone()).expect("wire");
    assert_eq!(wire.message_id, turn.id().as_uuid());
    assert_eq!(wire.parent_message_id, message.parent_message_id);
    assert_eq!(wire.job_id, message.job_id);
    assert_eq!(wire.acting_user_id, message.acting_user_id);
    assert_eq!(wire.impersonator_id, message.impersonator_id);
    let MessageTurn::Ai(ai) = wire.message else {
        panic!("an AI message");
    };
    assert_eq!(ai.agent_id, message.agent_id.as_uuid());
    assert_eq!(ai.author_name, "Analyst");
    let AiDelivery::Completed {
        turn: json,
        annotations,
    } = ai.delivery
    else {
        panic!("a whole message");
    };
    assert_eq!(json.to_turn().expect("a turn"), turn);
    assert_eq!(annotations.len(), 1);
    assert_eq!(annotations[0].annotation.body.artifact(), Some(artifact));
}

#[tokio::test]
async fn an_input_turn_of_incoming_entries_is_stored_whole_too() {
    let world = world();
    let incoming = match notice() {
        TurnEntry::RuntimeNotice(n) => br_llm_messages::IncomingEntry::from(n),
        _ => unreachable!(),
    };
    let input = InputTurn::new(turn_id(), vec![incoming]).expect("an input turn");
    let message = whole(&world, Turn::from(input.clone()), Vec::new());
    world.client.write_message(&message).await.expect("written");
    let wire: StoreMessageV2 =
        serde_json::from_value(world.bus.commands()[0].payload.clone()).expect("wire");
    let MessageTurn::Ai(ai) = wire.message else {
        panic!("an AI message");
    };
    let AiDelivery::Completed { turn, .. } = ai.delivery else {
        panic!("a whole message");
    };
    assert_eq!(turn.to_turn().expect("a turn"), Turn::from(input));
}

#[tokio::test]
async fn a_retry_is_a_new_message_with_the_same_parent_and_new_ids() {
    let world = world();
    let parent = v7();
    let mut first = whole(&world, ai_turn_with(notice()), Vec::new());
    first.parent_message_id = Some(parent);
    let mut retry = whole(&world, ai_turn_with(notice()), Vec::new());
    retry.parent_message_id = Some(parent);
    world.client.write_message(&first).await.expect("first");
    world.client.write_message(&retry).await.expect("retry");
    let wires: Vec<StoreMessageV2> = world
        .bus
        .commands()
        .iter()
        .map(|c| serde_json::from_value(c.payload.clone()).expect("wire"))
        .collect();
    assert_eq!(wires[0].parent_message_id, wires[1].parent_message_id);
    assert_ne!(wires[0].message_id, wires[1].message_id);
}
