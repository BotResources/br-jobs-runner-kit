mod support;

use br_runner_threads::test_double::MemoryThreads;
use br_runner_threads::{ContextContent, ContextQuery, Error};
use serde_json::{Value, json};
use support::client_graphql_x::*;
use support::*;

#[tokio::test]
async fn reading_the_context_of_a_branch_names_the_branch_tip() {
    let threads = MemoryThreads::new();
    answering(&threads, context_answer(Value::Null));
    let thread_id = v7();
    let branch_tip = v7();

    threads
        .client()
        .read_context_of_branch(
            &ContextQuery {
                thread_id,
                agent: None,
            },
            branch_tip,
        )
        .await
        .expect("read");

    let sent = graphql_operations(&threads);
    assert_eq!(sent.len(), 1);
    assert!(sent[0].0.contains("$branchTip: UUID"));
    assert!(sent[0].0.contains("branchTip: $branchTip"));
    assert_eq!(
        sent[0].1,
        json!({ "threadId": thread_id, "agentId": null, "branchTip": branch_tip })
    );
}

#[tokio::test]
async fn reading_the_context_of_a_branch_for_an_agent_names_the_agent_and_the_branch_tip() {
    let threads = MemoryThreads::new();
    answering(&threads, context_answer(Value::Null));
    let thread_id = v7();
    let branch_tip = v7();
    let agent = agent_id();

    threads
        .client()
        .read_context_of_branch(
            &ContextQuery {
                thread_id,
                agent: Some(agent),
            },
            branch_tip,
        )
        .await
        .expect("read");

    let sent = graphql_operations(&threads);
    assert_eq!(
        sent[0].1,
        json!({
            "threadId": thread_id,
            "agentId": agent.as_uuid().to_string(),
            "branchTip": branch_tip,
        })
    );
}

#[tokio::test]
async fn the_read_with_no_branch_tip_sends_no_branch_tip() {
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
    assert!(!sent[0].0.contains("branchTip"));
    assert_eq!(sent[0].1, json!({ "threadId": thread_id, "agentId": null }));
}

#[tokio::test]
async fn the_context_of_a_branch_is_read_like_the_context_of_the_thread() {
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
        .read_context_of_branch(
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
async fn a_script_answers_each_read_by_its_branch_tip() {
    let threads = MemoryThreads::new();
    let conversation = conversation_with_a_turn_of(agent_id());
    let thread_id = v7();
    let branch_tip = v7();
    let found = context_answer(json!({
        "threadId": thread_id,
        "conversation": document(&conversation),
        "projectedThread": null,
        "members": [],
        "resources": [],
    }));
    threads.answer_graphql(move |_, variables| {
        if variables.get("branchTip") == Some(&json!(branch_tip)) {
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
    let of_the_branch = client
        .read_context_of_branch(&query, branch_tip)
        .await
        .expect("read");

    assert!(of_the_thread.is_none());
    assert!(of_the_branch.is_some());
}

#[tokio::test]
async fn a_missing_thread_or_a_refusal_of_the_branch_read_reaches_the_caller() {
    let threads = MemoryThreads::new();
    answering(&threads, context_answer(Value::Null));
    let query = ContextQuery {
        thread_id: v7(),
        agent: None,
    };
    let absent = threads
        .client()
        .read_context_of_branch(&query, v7())
        .await
        .expect("read");
    assert!(absent.is_none());

    threads.answer_graphql(|_, _| Err(Error::Unauthenticated));
    let refused = threads.client().read_context_of_branch(&query, v7()).await;
    assert_eq!(refused.expect_err("failed"), Error::Unauthenticated);
}
