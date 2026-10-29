mod support;

use br_llm_messages::{Conversation, ConversationId};
use br_runner_threads::{ContextContent, ContextQuery, Error};
use serde_json::{Value, json};
use support::*;

fn context_data(conversation: &Conversation, agent: Option<br_llm_messages::AgentId>) -> Value {
    let conversation_json: Value =
        serde_json::from_str(&conversation.to_json().expect("json")).expect("value");
    let projected: Value = agent.map_or(Value::Null, |agent| {
        serde_json::from_str(&conversation.project_for(agent).to_json().expect("json"))
            .expect("value")
    });
    json!({ "data": { "threadsContext": {
        "threadId": v7(),
        "conversation": conversation_json,
        "projectedThread": projected,
        "members": [
            { "userId": v7(), "displayName": "Ada", "inRoster": true },
            { "userId": v7(), "displayName": null, "inRoster": false }
        ],
        "resources": [ { "key": "artifacts", "ids": ["a", "b"] } ]
    } } })
}

fn conversation() -> Conversation {
    Conversation::new(ConversationId::from_uuid(v7()).expect("a conversation id"))
}

#[tokio::test]
async fn a_context_without_an_agent_gives_the_conversation_and_the_members_as_they_are() {
    let world = world();
    world
        .graphql
        .reply_with(reply(200, context_data(&conversation(), None)));
    let context = world
        .client
        .read_context(&query(&world, false))
        .await
        .expect("read")
        .expect("found");
    assert!(matches!(context.content, ContextContent::Conversation(_)));
    assert_eq!(context.members.len(), 2);
    assert_eq!(context.members[0].display_name.as_deref(), Some("Ada"));
    assert!(context.members[0].in_roster);
    assert_eq!(context.members[1].display_name, None);
    assert!(!context.members[1].in_roster);
    assert_eq!(context.resources[0].key, "artifacts");
    assert_eq!(context.resources[0].ids, ["a", "b"]);
}

#[tokio::test]
async fn a_context_for_an_agent_gives_the_projected_thread_and_sends_the_agent() {
    let world = world();
    let agent = agent_id();
    world
        .graphql
        .reply_with(reply(200, context_data(&conversation(), Some(agent))));
    let query = ContextQuery {
        thread: world.thread,
        agent: Some(agent),
    };
    let context = world
        .client
        .read_context(&query)
        .await
        .expect("read")
        .expect("found");
    assert!(matches!(context.content, ContextContent::Projected(_)));
    let sent: Value = serde_json::from_slice(&world.graphql.bodies()[0]).expect("json");
    assert_eq!(
        sent["variables"]["agentId"],
        json!(agent.as_uuid().to_string())
    );
    assert_eq!(sent["variables"]["threadId"], json!(world.thread.as_uuid()));
}

#[tokio::test]
async fn a_null_context_is_none_and_a_bad_document_is_unreadable_with_the_stable_reason() {
    let world = world();
    world
        .graphql
        .reply_with(reply(200, json!({ "data": { "threadsContext": null } })));
    assert!(
        world
            .client
            .read_context(&query(&world, false))
            .await
            .expect("read")
            .is_none()
    );

    let mut bad = context_data(&conversation(), None);
    bad["data"]["threadsContext"]["conversation"] = json!({ "schema": "nope" });
    world.graphql.reply_with(reply(200, bad));
    match world.client.read_context(&query(&world, false)).await {
        Err(Error::Unreadable(reason)) => assert!(!reason.is_empty()),
        other => panic!("unreadable, not {other:?}"),
    }
}

#[tokio::test]
async fn a_context_with_no_document_for_the_request_is_unreadable() {
    let world = world();
    world
        .graphql
        .reply_with(reply(200, context_data(&conversation(), None)));
    assert!(matches!(
        world.client.read_context(&query(&world, true)).await,
        Err(Error::Unreadable(_))
    ));
}
