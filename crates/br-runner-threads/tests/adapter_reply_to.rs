//! The message a reply answers: `reply_to` names the parent of the next
//! message the adapter queues on a thread, in place of the tip, once. A
//! runner that writes a reply again chains it to the message it answers,
//! so the new reply is a sibling of the old one, not its child.

mod support;

use br_agentic_contract::ThreadRef;
use br_llm_messages::{Conversation, ConversationId, InputTurn, Turn};
use br_runner_threads::ThreadAccessAdapter;
use br_runner_threads::test_double::MemoryThreads;
use support::adapter_wire::*;
use support::*;
use uuid::Uuid;

fn parents(threads: &MemoryThreads) -> Vec<(Uuid, Option<Uuid>)> {
    stores(threads)
        .iter()
        .map(|store| (store.message_id, store.parent_message_id))
        .collect()
}

async fn idle(adapter: &ThreadAccessAdapter) {
    eventually("the thread queue is empty", || {
        adapter.queue_state().threads.is_empty()
    })
    .await;
}

/// A thread whose conversation is a question, then an earlier reply to it.
fn question_then_reply(w: &World, thread: &ThreadRef) -> (InputTurn, InputTurn) {
    let question = input_turn(vec![notice_entry("the user's question")]);
    let old_reply = input_turn(vec![notice_entry("the reply to write again")]);
    let id = ConversationId::from_uuid(thread.as_uuid()).expect("conversation id");
    let conversation = Conversation::from_parts(
        id,
        vec![
            Turn::InputTurn(question.clone()),
            Turn::InputTurn(old_reply.clone()),
        ],
    )
    .expect("conversation");
    let thread = *thread;
    w.threads
        .answer_graphql(move |_document, _variables| Ok(context_answer(thread, &conversation)));
    (question, old_reply)
}

#[tokio::test]
async fn the_next_message_takes_the_named_message_as_parent_not_the_tip() {
    let w = world().await;
    let thread = thread_ref();
    let (question, old_reply) = question_then_reply(&w, &thread);
    w.access().read(&thread).await.expect("a conversation");
    let reply = turn_ref(w.agent);

    w.adapter
        .reply_to(&thread, question.id())
        .await
        .expect("accepted");
    w.access()
        .add_entry(&thread, reply.clone(), notice("the new reply"))
        .await
        .expect("accepted");

    idle(&w.adapter).await;
    let parents = parents(&w.threads);
    assert_eq!(
        parents,
        vec![(reply.id.as_uuid(), Some(question.id().as_uuid()))]
    );
    assert_ne!(parents[0].1, Some(old_reply.id().as_uuid()));
}

#[tokio::test]
async fn it_holds_for_one_message_then_the_tip_is_the_parent_again() {
    let w = world().await;
    let thread = thread_ref();
    let (question, _old_reply) = question_then_reply(&w, &thread);
    w.access().read(&thread).await.expect("a conversation");
    let (reply, later) = (turn_ref(w.agent), turn_ref(w.agent));

    w.adapter
        .reply_to(&thread, question.id())
        .await
        .expect("accepted");
    w.access()
        .add_entry(&thread, reply.clone(), notice("the new reply"))
        .await
        .expect("accepted");
    w.access()
        .add_entry(&thread, later.clone(), notice("a later message"))
        .await
        .expect("accepted");

    idle(&w.adapter).await;
    assert_eq!(
        parents(&w.threads),
        vec![
            (reply.id.as_uuid(), Some(question.id().as_uuid())),
            (later.id.as_uuid(), Some(reply.id.as_uuid())),
        ],
        "the new reply became the tip"
    );
}

#[tokio::test]
async fn a_read_before_the_message_keeps_the_named_message() {
    let w = world().await;
    let thread = thread_ref();
    let (question, _old_reply) = question_then_reply(&w, &thread);
    let reply = turn_ref(w.agent);

    w.adapter
        .reply_to(&thread, question.id())
        .await
        .expect("accepted");
    w.access().read(&thread).await.expect("a conversation");
    w.access()
        .add_entry(&thread, reply.clone(), notice("the new reply"))
        .await
        .expect("accepted");

    idle(&w.adapter).await;
    assert_eq!(
        parents(&w.threads),
        vec![(reply.id.as_uuid(), Some(question.id().as_uuid()))]
    );
}

#[tokio::test]
async fn an_input_turn_takes_the_named_message_too() {
    let w = world().await;
    let thread = thread_ref();
    let (question, _old_reply) = question_then_reply(&w, &thread);
    w.access().read(&thread).await.expect("a conversation");
    let whole = input_turn(vec![notice_entry("a whole message")]);

    w.adapter
        .reply_to(&thread, question.id())
        .await
        .expect("accepted");
    w.access()
        .add_input_turn(&thread, whole.clone())
        .await
        .expect("accepted");

    idle(&w.adapter).await;
    assert_eq!(
        parents(&w.threads),
        vec![(whole.id().as_uuid(), Some(question.id().as_uuid()))]
    );
}

#[tokio::test]
async fn a_second_call_replaces_the_first() {
    let w = world().await;
    let thread = thread_ref();
    let (question, old_reply) = question_then_reply(&w, &thread);
    let reply = turn_ref(w.agent);

    w.adapter
        .reply_to(&thread, question.id())
        .await
        .expect("accepted");
    w.adapter
        .reply_to(&thread, old_reply.id())
        .await
        .expect("accepted");
    w.access()
        .add_entry(&thread, reply.clone(), notice("the new reply"))
        .await
        .expect("accepted");

    idle(&w.adapter).await;
    assert_eq!(
        parents(&w.threads),
        vec![(reply.id.as_uuid(), Some(old_reply.id().as_uuid()))]
    );
}

#[tokio::test]
async fn it_names_the_parent_on_its_own_thread_only() {
    let w = world().await;
    let (one, two) = (thread_ref(), thread_ref());
    let (question, _old_reply) = question_then_reply(&w, &one);
    let (on_two, on_one) = (turn_ref(w.agent), turn_ref(w.agent));

    w.adapter
        .reply_to(&one, question.id())
        .await
        .expect("accepted");
    w.access()
        .add_entry(&two, on_two.clone(), notice("on the other thread"))
        .await
        .expect("accepted");
    w.access()
        .add_entry(&one, on_one.clone(), notice("the new reply"))
        .await
        .expect("accepted");

    idle(&w.adapter).await;
    let parents = parents(&w.threads);
    assert!(parents.contains(&(on_two.id.as_uuid(), None)));
    assert!(parents.contains(&(on_one.id.as_uuid(), Some(question.id().as_uuid()))));
}
