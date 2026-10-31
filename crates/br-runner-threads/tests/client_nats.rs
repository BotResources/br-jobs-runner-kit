mod support;

use br_core_events::EventMetadata;
use br_core_integration::IntegrationCommand;
use br_core_kernel::Actor;
use br_runner_threads::{Cause, Confirmed, Error, MessageStatus};
use br_util_nats_fabric::command_subject;
use contract_threads::{CreateThread, ReasonCode, StoreMessageV2, ThreadsCommand};
use serde_json::{Value, json};
use support::nats_fixtures::*;
use support::nats_responder::{Reply, Responder};
use support::nats_server::Server;
use support::nats_streams::*;
use uuid::Uuid;

async fn infra() -> (Server, async_nats::Client) {
    let server = Server::start();
    let bus = server.client().await;
    create_all_streams(&bus).await;
    (server, bus)
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn a_created_thread_is_confirmed_and_its_command_reaches_the_command_stream() {
    let (_server, bus) = infra().await;
    let _responder = Responder::start(&bus).await;
    let client = built(&bus).await;
    let request = new_thread();

    let confirmed = client.create_thread(&request).await.expect("confirmed");

    assert_eq!(
        confirmed,
        Confirmed {
            command_id: request.command_id,
            status: None
        }
    );
    let commands = stored(&bus, COMMANDS).await;
    assert_eq!(commands.len(), 1);
    assert_eq!(
        commands[0].subject,
        command_subject(&CreateThread::coords())
    );
    let envelope: IntegrationCommand<CreateThread> = commands[0].json();
    assert_eq!(envelope.command_id, request.command_id);
    assert_eq!(envelope.command_type, "thread.create");
    assert_eq!(envelope.payload.thread_id, request.thread_id);
    assert_eq!(envelope.payload.owner_id, request.owner_id);
    assert_eq!(envelope.payload.user_ids, request.member_ids);
    assert_eq!(envelope.payload.title, request.title);
    assert_eq!(envelope.payload.resources, request.resources);
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn a_whole_message_is_confirmed_with_its_status_and_stored_as_a_store_command() {
    let (_server, bus) = infra().await;
    let _responder = Responder::start(&bus).await;
    let client = built(&bus).await;
    let message = whole_message(Uuid::now_v7());

    let confirmed = client.write_message(&message).await.expect("confirmed");

    assert_eq!(confirmed.command_id, message.command_id);
    assert_eq!(confirmed.status, Some(MessageStatus::Completed));
    let commands = stored(&bus, COMMANDS).await;
    assert_eq!(commands.len(), 1);
    assert_eq!(
        commands[0].subject,
        command_subject(&StoreMessageV2::coords())
    );
    let envelope: IntegrationCommand<StoreMessageV2> = commands[0].json();
    assert_eq!(envelope.payload.thread_id, message.thread_id);
    assert_eq!(envelope.payload.message_id, message.turn.id().as_uuid());
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn a_refused_thread_command_is_refused_with_its_code_params_and_command_id() {
    let (_server, bus) = infra().await;
    let responder = Responder::start(&bus).await;
    let client = built(&bus).await;
    let request = new_thread();
    let params = json!({ "user_ids": ["u-1"] });
    let params = params.as_object().cloned().expect("an object");
    responder.script(
        request.command_id,
        [Reply::Refuse(ReasonCode::KeyReused, params.clone())],
    );

    let result = client.create_thread(&request).await;

    match result {
        Err(Error::Refused(refusal)) => {
            assert_eq!(refusal.code, "KEY_REUSED");
            assert_eq!(refusal.params, params);
            assert_eq!(refusal.command_id, Some(request.command_id));
        }
        other => panic!("Refused, not {other:?}"),
    }
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn a_refused_message_command_is_refused_with_its_code_and_command_id() {
    let (_server, bus) = infra().await;
    let responder = Responder::start(&bus).await;
    let client = built(&bus).await;
    let message = whole_message(Uuid::now_v7());
    responder.script(
        message.command_id,
        [Reply::Refuse(ReasonCode::ThreadNotOpen, Default::default())],
    );

    let result = client.write_message(&message).await;

    match result {
        Err(Error::Refused(refusal)) => {
            assert_eq!(refusal.code, "THREAD_NOT_OPEN");
            assert_eq!(refusal.command_id, Some(message.command_id));
        }
        other => panic!("Refused, not {other:?}"),
    }
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn no_outcome_within_the_wait_is_lost_and_the_command_still_reached_the_bus() {
    let (_server, bus) = infra().await;
    let responder = Responder::start(&bus).await;
    let client = built(&bus).await;
    let mut request = new_thread();
    request.wait = std::time::Duration::from_millis(300);
    responder.script(request.command_id, [Reply::Silence]);

    let result = client.create_thread(&request).await;

    match result {
        Err(Error::Lost(failure)) => assert_eq!(failure.cause, Cause::WaitFired),
        other => panic!("Lost, not {other:?}"),
    }
    assert_eq!(responder.received_of(request.command_id).len(), 1);
    assert_eq!(stored_count(&bus, COMMANDS).await, 1);
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn an_outcome_that_names_another_command_is_not_taken_for_the_answer() {
    let (_server, bus) = infra().await;
    let responder = Responder::start(&bus).await;
    let client = built(&bus).await;
    let request = new_thread();
    responder.script(request.command_id, [Reply::StrayThenConfirm]);

    let confirmed = client.create_thread(&request).await.expect("confirmed");

    assert_eq!(confirmed.command_id, request.command_id);
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn an_answer_that_cannot_be_read_is_unreadable() {
    let (_server, bus) = infra().await;
    let responder = Responder::start(&bus).await;
    let client = built(&bus).await;
    let thread_request = new_thread();
    let message_request = whole_message(Uuid::now_v7());
    responder.script(thread_request.command_id, [Reply::Unreadable]);
    responder.script(message_request.command_id, [Reply::Unreadable]);

    let thread_result = client.create_thread(&thread_request).await;
    let message_result = client.write_message(&message_request).await;

    assert!(matches!(thread_result, Err(Error::Unreadable(_))));
    assert!(matches!(message_result, Err(Error::Unreadable(_))));
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn concurrent_commands_each_get_their_own_answer_when_answers_come_back_reversed() {
    let (_server, bus) = infra().await;
    let responder = Responder::batching(&bus, 6).await;
    let client = built(&bus).await;
    let requests: Vec<_> = (0..6).map(|_| new_thread()).collect();
    for request in requests.iter().step_by(2) {
        let refusal = Reply::Refuse(ReasonCode::KeyReused, Default::default());
        responder.script(request.command_id, [refusal]);
    }

    let results = futures::future::join_all(requests.iter().map(|r| client.create_thread(r))).await;

    for (index, (request, result)) in requests.iter().zip(results).enumerate() {
        match (index % 2, result) {
            (0, Err(Error::Refused(refusal))) => {
                assert_eq!(refusal.command_id, Some(request.command_id), "{index}")
            }
            (1, Ok(confirmed)) => assert_eq!(confirmed.command_id, request.command_id, "{index}"),
            (_, other) => panic!("request {index}: {other:?}"),
        }
    }
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn a_stored_command_names_the_service_account_as_actor_and_its_id_as_correlation() {
    let (_server, bus) = infra().await;
    let _responder = Responder::start(&bus).await;
    let client = built(&bus).await;
    let request = new_thread();

    client.create_thread(&request).await.expect("confirmed");

    let commands = stored(&bus, COMMANDS).await;
    let raw: Value = commands[0].json();
    let metadata: EventMetadata =
        serde_json::from_value(raw["metadata"].clone()).expect("the metadata reads");
    assert_eq!(metadata.actor, Actor::Service(account()));
    assert_eq!(metadata.correlation_id, request.command_id);
    assert_eq!(metadata.causation_id, None);
}
