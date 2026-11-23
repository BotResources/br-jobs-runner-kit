mod support;

use br_runner_threads::test_double::MemoryThreads;
use br_runner_threads::{ContextContent, ContextQuery, Error};
use serde_json::{Value, json};
use support::client_graphql_x::*;
use support::*;

#[tokio::test]
async fn reading_the_context_up_to_a_message_names_that_message() {
    let threads = MemoryThreads::new();
    answering(&threads, context_answer(Value::Null));
    let thread_id = v7();
    let up_to = v7();

    threads
        .client()
        .read_context_up_to(
            &ContextQuery {
                thread_id,
                agent: None,
            },
            up_to,
        )
        .await
        .expect("read");

    let sent = graphql_operations(&threads);
    assert_eq!(sent.len(), 1);
    assert!(sent[0].0.contains("$upTo: UUID"));
    assert!(sent[0].0.contains("upTo: $upTo"));
    assert!(!sent[0].0.contains("branchTip"));
    assert_eq!(
        sent[0].1,
        json!({ "threadId": thread_id, "agentId": null, "upTo": up_to })
    );
}

#[tokio::test]
async fn reading_the_context_up_to_a_message_for_an_agent_names_the_agent_and_the_message() {
    let threads = MemoryThreads::new();
    answering(&threads, context_answer(Value::Null));
    let thread_id = v7();
    let up_to = v7();
    let agent = agent_id();

    threads
        .client()
        .read_context_up_to(
            &ContextQuery {
                thread_id,
                agent: Some(agent),
            },
            up_to,
        )
        .await
        .expect("read");

    let sent = graphql_operations(&threads);
    assert_eq!(
        sent[0].1,
        json!({
            "threadId": thread_id,
            "agentId": agent.as_uuid().to_string(),
            "upTo": up_to,
        })
    );
}

#[tokio::test]
async fn the_read_of_the_whole_thread_names_no_message() {
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
    assert!(!sent[0].0.contains("upTo"));
    assert!(!sent[0].0.contains("branchTip"));
    assert_eq!(sent[0].1, json!({ "threadId": thread_id, "agentId": null }));
}

#[tokio::test]
async fn the_context_up_to_a_message_is_read_like_the_context_of_the_thread() {
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
        .read_context_up_to(
            &ContextQuery {
                thread_id,
                agent: None,
            },
            v7(),
        )
        .await
        .expect("read")
        .expect("a context");

    let ContextContent::Conversation(read) = context.content else {
        panic!("not a conversation");
    };
    assert_eq!(
        read.to_json().expect("json"),
        conversation.to_json().expect("json")
    );
    assert_eq!(context.thread_id, thread_id);
    assert_eq!(context.members.len(), 2);
    assert_eq!(context.resources[0].key, "kms");
}

#[tokio::test]
async fn a_script_answers_each_read_by_the_message_it_names() {
    let threads = MemoryThreads::new();
    let conversation = conversation_with_a_turn_of(agent_id());
    let thread_id = v7();
    let up_to = v7();
    let found = context_answer(json!({
        "threadId": thread_id,
        "conversation": document(&conversation),
        "projectedThread": null,
        "members": [],
        "resources": [],
    }));
    threads.answer_graphql(move |_, variables| {
        if variables.get("upTo") == Some(&json!(up_to)) {
            Ok(found.clone())
        } else {
            Ok(context_answer(Value::Null))
        }
    });
    let client = threads.client();
    let query = ContextQuery {
        thread_id,
        agent: None,
    };

    let of_the_thread = client.read_context(&query).await.expect("read");
    let up_to_the_message = client
        .read_context_up_to(&query, up_to)
        .await
        .expect("read");

    assert!(of_the_thread.is_none());
    assert!(up_to_the_message.is_some());
}

#[tokio::test]
async fn a_missing_thread_or_a_refusal_of_the_read_up_to_a_message_reaches_the_caller() {
    let threads = MemoryThreads::new();
    answering(&threads, context_answer(Value::Null));
    let query = ContextQuery {
        thread_id: v7(),
        agent: None,
    };
    let absent = threads
        .client()
        .read_context_up_to(&query, v7())
        .await
        .expect("read");
    assert!(absent.is_none());

    threads.answer_graphql(|_, _| Err(Error::Unauthenticated));
    let refused = threads.client().read_context_up_to(&query, v7()).await;
    assert_eq!(refused.expect_err("failed"), Error::Unauthenticated);
}

#[tokio::test]
async fn a_named_message_that_the_thread_does_not_hold_gives_an_empty_conversation() {
    let threads = MemoryThreads::new();
    let thread_id = v7();
    let empty = br_llm_messages::Conversation::from_parts(
        br_llm_messages::ConversationId::from_uuid(thread_id).expect("conversation id"),
        Vec::new(),
    )
    .expect("an empty conversation");
    answering(
        &threads,
        context_answer(json!({
            "threadId": thread_id,
            "conversation": document(&empty),
            "projectedThread": null,
            "members": [],
            "resources": [],
        })),
    );

    let context = threads
        .client()
        .read_context_up_to(
            &ContextQuery {
                thread_id,
                agent: None,
            },
            v7(),
        )
        .await
        .expect("read")
        .expect("the thread exists");

    let ContextContent::Conversation(read) = context.content else {
        panic!("not a conversation");
    };
    assert!(read.turns().is_empty());
}
