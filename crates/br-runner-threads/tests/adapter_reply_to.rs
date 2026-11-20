//! The message a reply answers: `reply_to` names the parent of one message
//! not queued yet, by the id of that message. When the adapter queues it,
//! it takes that parent in place of the tip; no other message does. A
//! runner that writes a reply again chains it to the message it answers,
//! so the new reply is a sibling of the old one, not its child.

mod support;

use br_agentic_contract::{AiTurnRef, ThreadRef};
use br_llm_messages::{Conversation, ConversationId, InputTurn, Turn};
use br_runner_threads::test_double::MemoryThreads;
use br_runner_threads::{Error, ThreadAccessAdapter};
use support::adapter_wire::*;
use support::adapter_x::fault_of;
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

fn answer_with(w: &World, thread: &ThreadRef, turns: Vec<InputTurn>) {
    let id = ConversationId::from_uuid(thread.as_uuid()).expect("conversation id");
    let turns = turns.into_iter().map(Turn::InputTurn).collect();
    let conversation = Conversation::from_parts(id, turns).expect("conversation");
    let thread = *thread;
    w.threads
        .answer_graphql(move |_document, _variables| Ok(context_answer(thread, &conversation)));
}

/// A thread whose conversation is a question, then an earlier reply to it.
fn question_then_reply(w: &World, thread: &ThreadRef) -> (InputTurn, InputTurn) {
    let question = input_turn(vec![notice_entry("the user's question")]);
    let old_reply = input_turn(vec![notice_entry("the reply to write again")]);
    answer_with(w, thread, vec![question.clone(), old_reply.clone()]);
    (question, old_reply)
}

/// A thread that was read, and a reply bound to the question of that thread.
async fn bound_reply(w: &World, thread: &ThreadRef) -> (InputTurn, InputTurn, AiTurnRef) {
    let (question, old_reply) = question_then_reply(w, thread);
    w.access().read(thread).await.expect("a conversation");
    let reply = turn_ref(w.agent);
    w.adapter
        .reply_to(thread, reply.id, question.id())
        .await
        .expect("accepted");
    (question, old_reply, reply)
}

#[tokio::test]
async fn the_named_reply_takes_the_named_parent_not_the_tip() {
    let w = world().await;
    let thread = thread_ref();
    let (question, _old_reply, reply) = bound_reply(&w, &thread).await;

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
async fn the_reply_becomes_the_tip_and_the_parent_is_named_once() {
    let w = world().await;
    let thread = thread_ref();
    let (question, _old_reply, reply) = bound_reply(&w, &thread).await;
    let later = turn_ref(w.agent);

    w.access()
        .add_entry(&thread, reply.clone(), notice("the new reply"))
        .await
        .expect("accepted");
    w.access()
        .add_entry(&thread, reply.clone(), notice("more of the new reply"))
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
        ]
    );
}

#[tokio::test]
async fn a_read_before_the_reply_keeps_the_named_parent() {
    let w = world().await;
    let thread = thread_ref();
    let (question, _old_reply) = question_then_reply(&w, &thread);
    let reply = turn_ref(w.agent);

    w.adapter
        .reply_to(&thread, reply.id, question.id())
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
async fn other_messages_queued_first_follow_the_tip_and_leave_the_parent_to_the_reply() {
    let w = world().await;
    let thread = thread_ref();
    let (question, old_reply, reply) = bound_reply(&w, &thread).await;
    let whole = input_turn(vec![notice_entry("a whole message")]);
    let other_agent = turn_ref(agent_id());

    w.access()
        .add_input_turn(&thread, whole.clone())
        .await
        .expect("accepted");
    w.access()
        .add_entry(&thread, other_agent.clone(), notice("another agent"))
        .await
        .expect("accepted");
    w.access()
        .add_entry(&thread, reply.clone(), notice("the new reply"))
        .await
        .expect("accepted");

    idle(&w.adapter).await;
    assert_eq!(
        parents(&w.threads),
        vec![
            (whole.id().as_uuid(), Some(old_reply.id().as_uuid())),
            (other_agent.id.as_uuid(), Some(whole.id().as_uuid())),
            (reply.id.as_uuid(), Some(question.id().as_uuid())),
        ]
    );
}

#[tokio::test]
async fn an_entry_of_a_turn_already_open_does_not_take_the_parent() {
    let w = world().await;
    let thread = thread_ref();
    let (question, old_reply) = question_then_reply(&w, &thread);
    w.access().read(&thread).await.expect("a conversation");
    let (open, reply) = (turn_ref(w.agent), turn_ref(w.agent));
    w.access()
        .add_entry(&thread, open.clone(), notice("a turn opens"))
        .await
        .expect("accepted");

    w.adapter
        .reply_to(&thread, reply.id, question.id())
        .await
        .expect("accepted");
    w.access()
        .add_entry(&thread, open.clone(), notice("more of the open turn"))
        .await
        .expect("accepted");
    w.access()
        .add_entry(&thread, reply.clone(), notice("the new reply"))
        .await
        .expect("accepted");

    idle(&w.adapter).await;
    assert_eq!(
        parents(&w.threads),
        vec![
            (open.id.as_uuid(), Some(old_reply.id().as_uuid())),
            (reply.id.as_uuid(), Some(question.id().as_uuid())),
        ]
    );
}

#[tokio::test]
async fn a_turn_that_the_adapter_already_opened_is_refused() {
    let w = world().await;
    let thread = thread_ref();
    let (question, _old_reply) = question_then_reply(&w, &thread);
    let open = turn_ref(w.agent);
    w.access()
        .add_entry(&thread, open.clone(), notice("a turn opens"))
        .await
        .expect("accepted");

    let result = w.adapter.reply_to(&thread, open.id, question.id()).await;

    assert!(matches!(fault_of(result), Error::InvalidInput(_)));
}

#[tokio::test]
async fn a_message_named_as_its_own_parent_is_refused() {
    let w = world().await;
    let reply = turn_ref(w.agent);

    let result = w.adapter.reply_to(&thread_ref(), reply.id, reply.id).await;

    assert!(matches!(fault_of(result), Error::InvalidInput(_)));
}

#[tokio::test]
async fn a_response_opened_on_the_reply_takes_the_named_parent() {
    let w = world().await;
    let thread = thread_ref();
    let (question, _old_reply, reply) = bound_reply(&w, &thread).await;

    let _stream = w
        .access()
        .open_response(&thread, reply.clone(), entry_id())
        .await
        .expect("accepted");

    idle(&w.adapter).await;
    assert_eq!(
        parents(&w.threads),
        vec![(reply.id.as_uuid(), Some(question.id().as_uuid()))]
    );
}

#[tokio::test]
async fn a_response_completed_on_the_reply_takes_the_named_parent() {
    let w = world().await;
    let thread = thread_ref();
    let (question, _old_reply, reply) = bound_reply(&w, &thread).await;

    w.access()
        .complete_response(
            &thread,
            reply.clone(),
            text_response(entry_id(), block_id(), "the new reply"),
        )
        .await
        .expect("accepted");

    idle(&w.adapter).await;
    assert_eq!(
        parents(&w.threads),
        vec![(reply.id.as_uuid(), Some(question.id().as_uuid()))]
    );
}

#[tokio::test]
async fn a_result_completed_or_opened_on_the_reply_takes_the_named_parent() {
    let w = world().await;
    let (one, two) = (thread_ref(), thread_ref());
    let (question_one, _old, completed) = bound_reply(&w, &one).await;
    let (question_two, _old, opened) = bound_reply(&w, &two).await;

    w.access()
        .complete_result(
            &one,
            completed.clone(),
            tool_results(entry_id(), block_id(), "search", "found"),
        )
        .await
        .expect("accepted");
    let _stream = w
        .access()
        .open_result(&two, opened.clone(), entry_id())
        .await
        .expect("accepted");

    idle(&w.adapter).await;
    let parents = parents(&w.threads);
    assert!(parents.contains(&(completed.id.as_uuid(), Some(question_one.id().as_uuid()))));
    assert!(parents.contains(&(opened.id.as_uuid(), Some(question_two.id().as_uuid()))));
}

#[tokio::test]
async fn a_read_after_the_reply_sets_the_tip_to_the_end_of_the_branch() {
    let w = world().await;
    let thread = thread_ref();
    let (question, _old_reply, reply) = bound_reply(&w, &thread).await;
    w.access()
        .add_entry(&thread, reply.clone(), notice("the new reply"))
        .await
        .expect("accepted");
    idle(&w.adapter).await;
    let follow_up = input_turn(vec![notice_entry("the user's follow-up")]);
    answer_with(&w, &thread, vec![question.clone(), follow_up.clone()]);
    let later = turn_ref(w.agent);

    w.access().read(&thread).await.expect("a conversation");
    w.access()
        .add_entry(&thread, later.clone(), notice("a later reply"))
        .await
        .expect("accepted");

    idle(&w.adapter).await;
    assert_eq!(
        parents(&w.threads)[1],
        (later.id.as_uuid(), Some(follow_up.id().as_uuid()))
    );
}

#[tokio::test]
async fn a_second_call_for_the_same_reply_replaces_the_first() {
    let w = world().await;
    let thread = thread_ref();
    let (_question, _old_reply, reply) = bound_reply(&w, &thread).await;
    let elsewhere = turn_id();

    w.adapter
        .reply_to(&thread, reply.id, elsewhere)
        .await
        .expect("accepted");
    w.access()
        .add_entry(&thread, reply.clone(), notice("the new reply"))
        .await
        .expect("accepted");

    idle(&w.adapter).await;
    assert_eq!(
        parents(&w.threads),
        vec![(reply.id.as_uuid(), Some(elsewhere.as_uuid()))]
    );
}

#[tokio::test]
async fn the_parent_is_named_on_its_own_thread_only() {
    let w = world().await;
    let (one, two) = (thread_ref(), thread_ref());
    let (_question, _old_reply, reply) = bound_reply(&w, &one).await;

    w.access()
        .add_entry(&two, reply.clone(), notice("the same id on another thread"))
        .await
        .expect("accepted");

    idle(&w.adapter).await;
    assert_eq!(parents(&w.threads), vec![(reply.id.as_uuid(), None)]);
}

#[tokio::test]
async fn a_reply_never_written_leaves_the_later_messages_on_the_tip() {
    let w = world().await;
    let thread = thread_ref();
    let (_question, old_reply, _never_written) = bound_reply(&w, &thread).await;
    w.adapter.instance_ended(w.agent).await;
    let whole = input_turn(vec![notice_entry("a later whole message")]);
    let later = turn_ref(w.agent);

    w.access()
        .add_input_turn(&thread, whole.clone())
        .await
        .expect("accepted");
    w.access()
        .add_entry(&thread, later.clone(), notice("a later instance"))
        .await
        .expect("accepted");

    idle(&w.adapter).await;
    assert_eq!(
        parents(&w.threads),
        vec![
            (whole.id().as_uuid(), Some(old_reply.id().as_uuid())),
            (later.id.as_uuid(), Some(whole.id().as_uuid())),
        ]
    );
}
