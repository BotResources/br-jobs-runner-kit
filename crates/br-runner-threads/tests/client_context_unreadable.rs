mod support;

use br_runner_threads::Error;
use serde_json::{Value, json};
use support::client_graphql_x::*;
use support::*;

#[tokio::test]
async fn an_answer_without_the_context_field_is_unreadable() {
    let error = unreadable_context(None, json!({})).await;

    assert!(matches!(error, Error::Unreadable(_)), "{error:?}");
}

#[tokio::test]
async fn a_context_with_malformed_members_is_unreadable() {
    let conversation = conversation_with_a_turn_of(agent_id());
    let answer = context_answer(json!({
        "threadId": v7(),
        "conversation": document(&conversation),
        "members": [{ "userId": "not a uuid", "inRoster": true }],
        "resources": [],
    }));

    let error = unreadable_context(None, answer).await;

    assert!(matches!(error, Error::Unreadable(_)), "{error:?}");
}

#[tokio::test]
async fn a_context_with_malformed_resources_is_unreadable() {
    let conversation = conversation_with_a_turn_of(agent_id());
    let answer = context_answer(json!({
        "threadId": v7(),
        "conversation": document(&conversation),
        "members": [],
        "resources": [{ "key": "kms" }],
    }));

    let error = unreadable_context(None, answer).await;

    assert!(matches!(error, Error::Unreadable(_)), "{error:?}");
}

#[tokio::test]
async fn a_conversation_document_that_the_message_library_refuses_is_unreadable() {
    let answer = context_answer(json!({
        "threadId": v7(),
        "conversation": { "schema": "not a conversation", "turns": 7 },
        "members": [],
        "resources": [],
    }));

    let error = unreadable_context(None, answer).await;

    assert!(matches!(error, Error::Unreadable(_)), "{error:?}");
}

#[tokio::test]
async fn a_projected_document_that_the_message_library_refuses_is_unreadable() {
    let answer = context_answer(json!({
        "threadId": v7(),
        "projectedThread": { "schema": "not a projection" },
        "members": [],
        "resources": [],
    }));

    let error = unreadable_context(Some(agent_id()), answer).await;

    assert!(matches!(error, Error::Unreadable(_)), "{error:?}");
}

#[tokio::test]
async fn a_context_with_no_document_for_the_request_is_unreadable() {
    let agent = agent_id();
    let conversation = conversation_with_a_turn_of(agent);
    let projected: Value =
        serde_json::from_str(&conversation.project_for(agent).to_json().expect("json"))
            .expect("a document");
    let only_conversation = context_answer(json!({
        "threadId": v7(),
        "conversation": document(&conversation),
        "projectedThread": null,
        "members": [],
        "resources": [],
    }));
    let only_projected = context_answer(json!({
        "threadId": v7(),
        "conversation": null,
        "projectedThread": projected,
        "members": [],
        "resources": [],
    }));

    let for_agent = unreadable_context(Some(agent), only_conversation).await;
    let for_nobody = unreadable_context(None, only_projected).await;

    assert!(matches!(for_agent, Error::Unreadable(_)), "{for_agent:?}");
    assert!(matches!(for_nobody, Error::Unreadable(_)), "{for_nobody:?}");
}
