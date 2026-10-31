use br_llm_messages::{AITurn, Conversation, Turn};
use br_runner_threads::test_double::{MemoryThreads, Operation};
use br_runner_threads::{ContextQuery, Error};
use serde_json::{Value, json};

use super::ids::*;

pub fn conversation_with_a_turn_of(agent: br_llm_messages::AgentId) -> Conversation {
    let mut conversation = Conversation::new(conversation_id());
    conversation
        .push_turn(Turn::InputTurn(input_turn(vec![human_entry("hello")])))
        .expect("input turn");
    let mut turn = AITurn::open(turn_id(), agent, author("Analyst"));
    turn.push_entry(notice("working")).expect("entry");
    conversation.push_turn(Turn::AITurn(turn)).expect("ai turn");
    conversation
}

pub fn document(conversation: &Conversation) -> Value {
    serde_json::from_str(&conversation.to_json().expect("json")).expect("a document")
}

pub fn context_answer(fields: Value) -> Value {
    json!({ "threadsContext": fields })
}

pub fn members_and_resources() -> (Value, Value) {
    (
        json!([
            { "userId": v7(), "displayName": "Ada", "inRoster": true },
            { "userId": v7(), "displayName": null, "inRoster": false }
        ]),
        json!([{ "key": "kms", "ids": ["a", "b"] }]),
    )
}

pub fn answering(threads: &MemoryThreads, answer: Value) {
    threads.answer_graphql(move |_, _| Ok(answer.clone()));
}

pub fn graphql_operations(threads: &MemoryThreads) -> Vec<(String, Value)> {
    threads
        .operations()
        .into_iter()
        .filter_map(|operation| match operation {
            Operation::Graphql {
                document,
                variables,
            } => Some((document, variables)),
            _ => None,
        })
        .collect()
}

pub async fn unreadable_context(
    query_agent: Option<br_llm_messages::AgentId>,
    answer: Value,
) -> Error {
    let threads = MemoryThreads::new();
    answering(&threads, answer);
    threads
        .client()
        .read_context(&ContextQuery {
            thread_id: v7(),
            agent: query_agent,
        })
        .await
        .expect_err("unreadable")
}
