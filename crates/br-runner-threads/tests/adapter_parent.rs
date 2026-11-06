//! The parent of a message: each message the adapter queues on a thread
//! takes the tip of that thread (the last turn of the last read, then each
//! message queued since) as its parent, so Threads reads the thread whole.

mod support;

use br_llm_messages::{Conversation, ConversationId, Turn};
use br_runner_threads::ThreadAccessAdapter;
use br_runner_threads::test_double::MemoryThreads;
use support::adapter_wire::*;
use support::*;
use uuid::Uuid;

fn answer_with(threads: &MemoryThreads, conversation: Conversation) {
    let thread = br_agentic_contract::ThreadRef::from_uuid(conversation.id().as_uuid())
        .expect("a thread reference");
    threads.answer_graphql(move |_document, _variables| Ok(context_answer(thread, &conversation)));
}

fn conversation_of(thread: Uuid, turns: Vec<Turn>) -> Conversation {
    let id = ConversationId::from_uuid(thread).expect("conversation id");
    Conversation::from_parts(id, turns).expect("conversation")
}

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

#[tokio::test]
async fn a_message_on_a_thread_never_read_nor_written_starts_a_tree() {
    let w = world().await;
    let thread = thread_ref();
    let turn = input_turn(vec![notice_entry("first")]);

    w.access()
        .add_input_turn(&thread, turn.clone())
        .await
        .expect("accepted");

    idle(&w.adapter).await;
    assert_eq!(parents(&w.threads), vec![(turn.id().as_uuid(), None)]);
}

#[tokio::test]
async fn after_a_read_the_next_message_follows_the_last_turn_of_the_conversation() {
    let w = world().await;
    let thread = thread_ref();
    let earlier = input_turn(vec![notice_entry("the user's question")]);
    let latest = input_turn(vec![notice_entry("the user's follow-up")]);
    answer_with(
        &w.threads,
        conversation_of(
            thread.as_uuid(),
            vec![Turn::InputTurn(earlier), Turn::InputTurn(latest.clone())],
        ),
    );
    w.access().read(&thread).await.expect("a conversation");
    let reply = turn_ref(w.agent);

    w.access()
        .add_entry(&thread, reply.clone(), notice("the reply begins"))
        .await
        .expect("accepted");

    idle(&w.adapter).await;
    assert_eq!(
        parents(&w.threads),
        vec![(reply.id.as_uuid(), Some(latest.id().as_uuid()))]
    );
}

#[tokio::test]
async fn each_message_queued_follows_the_one_queued_before_it() {
    let w = world().await;
    let thread = thread_ref();
    answer_with(&w.threads, conversation_of(thread.as_uuid(), Vec::new()));
    w.access()
        .read(&thread)
        .await
        .expect("an empty conversation");
    let opening = input_turn(vec![notice_entry("the opening")]);
    let (first, second) = (turn_ref(w.agent), turn_ref(w.agent));

    w.access()
        .add_input_turn(&thread, opening.clone())
        .await
        .expect("accepted");
    w.access()
        .add_entry(&thread, first.clone(), notice("first answer"))
        .await
        .expect("accepted");
    w.access()
        .add_entry(&thread, first.clone(), notice("more of the first answer"))
        .await
        .expect("accepted");
    w.access()
        .add_entry(&thread, second.clone(), notice("second answer"))
        .await
        .expect("accepted");

    idle(&w.adapter).await;
    assert_eq!(
        parents(&w.threads),
        vec![
            (opening.id().as_uuid(), None),
            (first.id.as_uuid(), Some(opening.id().as_uuid())),
            (second.id.as_uuid(), Some(first.id.as_uuid())),
        ],
        "an entry of an open turn is a chunk, not a message: it moves no tip"
    );
}

#[tokio::test]
async fn a_later_read_replaces_the_tip() {
    let w = world().await;
    let thread = thread_ref();
    let written = input_turn(vec![notice_entry("written here")]);
    w.access()
        .add_input_turn(&thread, written.clone())
        .await
        .expect("accepted");
    idle(&w.adapter).await;
    let elsewhere = input_turn(vec![notice_entry("written by someone else")]);
    answer_with(
        &w.threads,
        conversation_of(
            thread.as_uuid(),
            vec![
                Turn::InputTurn(written.clone()),
                Turn::InputTurn(elsewhere.clone()),
            ],
        ),
    );
    w.access().read(&thread).await.expect("a conversation");
    let reply = turn_ref(w.agent);

    w.access()
        .add_entry(&thread, reply.clone(), notice("the reply"))
        .await
        .expect("accepted");

    idle(&w.adapter).await;
    assert_eq!(
        parents(&w.threads)[1],
        (reply.id.as_uuid(), Some(elsewhere.id().as_uuid()))
    );
}

#[tokio::test]
async fn each_thread_has_its_own_tip() {
    let w = world().await;
    let (one, two) = (thread_ref(), thread_ref());
    let on_one = input_turn(vec![notice_entry("on one")]);
    w.access()
        .add_input_turn(&one, on_one.clone())
        .await
        .expect("accepted");
    let (on_two, again_on_one) = (turn_ref(w.agent), turn_ref(w.agent));

    w.access()
        .add_entry(&two, on_two.clone(), notice("on two"))
        .await
        .expect("accepted");
    w.access()
        .add_entry(&one, again_on_one.clone(), notice("on one again"))
        .await
        .expect("accepted");

    idle(&w.adapter).await;
    let parents = parents(&w.threads);
    assert!(parents.contains(&(on_two.id.as_uuid(), None)));
    assert!(parents.contains(&(again_on_one.id.as_uuid(), Some(on_one.id().as_uuid()))));
}

#[tokio::test]
async fn the_tip_outlives_the_end_of_an_instance() {
    let w = world().await;
    let thread = thread_ref();
    let before = turn_ref(w.agent);
    w.access()
        .add_entry(&thread, before.clone(), notice("before the end"))
        .await
        .expect("accepted");
    w.adapter.instance_ended(w.agent).await;
    let after = turn_ref(w.agent);

    w.access()
        .add_entry(&thread, after.clone(), notice("a later instance"))
        .await
        .expect("accepted");

    idle(&w.adapter).await;
    assert_eq!(
        parents(&w.threads)[1],
        (after.id.as_uuid(), Some(before.id.as_uuid()))
    );
}
