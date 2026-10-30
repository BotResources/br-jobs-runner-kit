mod support;

use std::time::Duration;

use br_llm_messages::{Turn, TurnEntry};
use br_runner_threads::transport::{Answer, Confirmed, Reply};
use br_runner_threads::{Error, MessageStatus, PlacedAnnotation, Refusal, WholeMessage};
use serde_json::{Map, json};
use support::*;

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
async fn a_confirmation_of_a_message_command_carries_the_status() {
    let world = world();
    world
        .bus
        .answer_with(|c| confirmed(c.command_id, Some(MessageStatus::Failed)));
    let message = whole(&world, ai_turn_with(notice()), Vec::new());
    let answered = world.client.write_message(&message).await.expect("written");
    assert_eq!(answered.status, Some(MessageStatus::Failed));
}

#[tokio::test]
async fn a_refusal_gives_the_code_the_params_and_the_command_id() {
    let world = world();
    let params: Map<String, serde_json::Value> = json!({ "error": "reused_identity" })
        .as_object()
        .cloned()
        .expect("params");
    let wanted = params.clone();
    world.bus.answer_with(move |c| {
        Reply::Answer(Answer::Refused(Refusal {
            code: "KEY_REUSED".to_owned(),
            params: wanted.clone(),
            command_id: Some(c.command_id),
        }))
    });
    let request = new_thread(&world);
    let refused = world
        .client
        .create_thread(&request)
        .await
        .expect_err("refused");
    assert_eq!(
        refused,
        Error::Refused(Refusal {
            code: "KEY_REUSED".to_owned(),
            params,
            command_id: Some(request.command_id),
        })
    );
}

#[tokio::test]
async fn a_command_with_no_answer_is_answer_lost_after_the_wait_and_waits_without_one() {
    let world = world();
    world.bus.answer_with(|_| Reply::Never);
    let mut request = new_thread(&world);
    request.wait = Some(Duration::from_millis(30));
    assert!(matches!(
        world.client.create_thread(&request).await,
        Err(Error::AnswerLost(_))
    ));
    request.wait = None;
    request.command_id = v7();
    let waiting = world.client.create_thread(&request);
    assert!(
        tokio::time::timeout(Duration::from_millis(100), waiting)
            .await
            .is_err(),
        "with no wait the operation keeps waiting"
    );
}

#[tokio::test]
async fn nothing_is_sent_when_the_bus_is_unavailable_and_the_error_says_so() {
    let world = world();
    world
        .bus
        .answer_with(|_| Reply::Unavailable("not connected".to_owned()));
    let result = world.client.create_thread(&new_thread(&world)).await;
    assert_eq!(result, Err(Error::Unavailable("not connected".to_owned())));
    assert!(world.bus.commands().is_empty());
}

#[tokio::test]
async fn a_lost_answer_leaves_the_resend_to_the_importer_under_the_same_id() {
    let world = world();
    world.bus.answer_with(|_| Reply::Lost);
    let request = new_thread(&world);
    assert!(matches!(
        world.client.create_thread(&request).await,
        Err(Error::AnswerLost(_))
    ));
    world.bus.answer_with(|c| confirmed(c.command_id, None));
    let confirmed = world.client.create_thread(&request).await.expect("resent");
    assert_eq!(
        confirmed,
        Confirmed {
            command_id: request.command_id,
            status: None
        }
    );
    let sent = world.bus.commands();
    assert_eq!(sent[0].command_id, sent[1].command_id);
}

#[tokio::test]
async fn the_client_adds_no_author_rule_and_does_not_compare_the_agent_with_the_turn() {
    let world = world();
    let message = whole(&world, ai_turn_with(notice()), Vec::new());
    assert_ne!(
        message.agent_id.as_uuid(),
        match &message.turn {
            Turn::AITurn(turn) => turn.agent().as_uuid(),
            _ => unreachable!(),
        }
    );
    world.client.write_message(&message).await.expect("written");
    assert_eq!(world.bus.commands().len(), 1);
}
