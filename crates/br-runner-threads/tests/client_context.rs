mod support;

use br_runner_threads::test_double;
use br_runner_threads::test_double::MemoryThreads;
use br_runner_threads::{Cause, ContextContent, ContextQuery, Error};
use serde_json::{Value, json};
use support::client_graphql_x::*;
use support::*;

#[tokio::test]
async fn reading_a_context_asks_for_the_thread_without_an_agent() {
    let threads = MemoryThreads::new();
    answering(&threads, context_answer(Value::Null));
    let thread_id = v7();

    threads
        .client()
        .read_context(&ContextQuery {
            thread_id,
            agent: None,
        })
        .await
        .expect("read");

    let sent = graphql_operations(&threads);
    assert_eq!(sent.len(), 1);
    assert!(sent[0].0.contains("threadsContext"));
    assert_eq!(sent[0].1, json!({ "threadId": thread_id, "agentId": null }));
}

#[tokio::test]
async fn reading_a_context_for_an_agent_names_the_agent() {
    let threads = MemoryThreads::new();
    answering(&threads, context_answer(Value::Null));
    let thread_id = v7();
    let agent = agent_id();

    threads
        .client()
        .read_context(&ContextQuery {
            thread_id,
            agent: Some(agent),
        })
        .await
        .expect("read");

    let sent = graphql_operations(&threads);
    assert_eq!(
        sent[0].1,
        json!({ "threadId": thread_id, "agentId": agent.as_uuid().to_string() })
    );
}

#[tokio::test]
async fn a_missing_thread_is_an_absent_value() {
    let threads = MemoryThreads::new();
    answering(&threads, context_answer(Value::Null));

    let context = threads
        .client()
        .read_context(&ContextQuery {
            thread_id: v7(),
            agent: None,
        })
        .await
        .expect("read");

    assert!(context.is_none());
}

#[tokio::test]
async fn with_no_agent_the_context_holds_the_conversation_the_members_and_the_resources() {
    let threads = MemoryThreads::new();
    let conversation = conversation_with_a_turn_of(agent_id());
    let (members, resources) = members_and_resources();
    let thread_id = v7();
    answering(
        &threads,
        context_answer(json!({
            "threadId": thread_id,
            "conversation": document(&conversation),
            "projectedThread": null,
            "members": members,
            "resources": resources,
        })),
    );

    let context = threads
        .client()
        .read_context(&ContextQuery {
            thread_id,
            agent: None,
        })
        .await
        .expect("read")
        .expect("a context");

    assert_eq!(context.thread_id, thread_id);
    let ContextContent::Conversation(read) = context.content else {
        panic!("not a conversation");
    };
    assert_eq!(
        read.to_json().expect("json"),
        conversation.to_json().expect("json")
    );
    assert_eq!(context.members.len(), 2);
    assert_eq!(context.members[0].display_name.as_deref(), Some("Ada"));
    assert!(context.members[0].in_roster);
    assert_eq!(context.members[1].display_name, None);
    assert!(!context.members[1].in_roster);
    assert_eq!(context.resources.len(), 1);
    assert_eq!(context.resources[0].key, "kms");
    assert_eq!(
        context.resources[0].ids,
        vec!["a".to_owned(), "b".to_owned()]
    );
}

#[tokio::test]
async fn with_an_agent_the_context_holds_the_projected_thread_of_that_agent() {
    let threads = MemoryThreads::new();
    let agent = agent_id();
    let projected = conversation_with_a_turn_of(agent).project_for(agent);
    let (members, resources) = members_and_resources();
    let thread_id = v7();
    let projected_document: Value =
        serde_json::from_str(&projected.to_json().expect("json")).expect("a document");
    answering(
        &threads,
        context_answer(json!({
            "threadId": thread_id,
            "conversation": null,
            "projectedThread": projected_document,
            "members": members,
            "resources": resources,
        })),
    );

    let context = threads
        .client()
        .read_context(&ContextQuery {
            thread_id,
            agent: Some(agent),
        })
        .await
        .expect("read")
        .expect("a context");

    let ContextContent::Projected(read) = context.content else {
        panic!("not a projected thread");
    };
    assert_eq!(
        read.to_json().expect("json"),
        projected.to_json().expect("json")
    );
    assert_eq!(read.agent(), agent);
}

#[tokio::test]
async fn a_failure_of_the_graphql_request_reaches_the_caller_unchanged() {
    let failures = [
        Error::Unavailable(test_double::failure(Cause::NoListener, "none")),
        Error::Lost(test_double::failure(Cause::WaitFired, "no reply")),
        Error::Unauthenticated,
    ];
    for failure in failures {
        let threads = MemoryThreads::new();
        let expected = failure.clone();
        threads.answer_graphql(move |_, _| Err(failure.clone()));

        let result = threads
            .client()
            .read_context(&ContextQuery {
                thread_id: v7(),
                agent: None,
            })
            .await;

        assert_eq!(result.expect_err("failed"), expected);
    }
}
