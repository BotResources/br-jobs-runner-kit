//! The place of a message: a message the adapter queues on a thread names
//! no other message, whatever the adapter read or queued before. Threads
//! places it at the end of the list of the thread when it stores it.

mod support;

use br_llm_messages::{Conversation, ConversationId, Turn};
use br_runner_threads::test_double::{CommandBody, MemoryThreads};
use br_runner_threads::{MemoryQueueStore, QueueStore, StoredItem, ThreadAccessAdapter};
use contract_threads::Chunk;
use serde_json::Value;
use std::sync::Arc;
use support::adapter_wire::*;
use support::*;
use uuid::Uuid;

const ITEMS_OF_0_5_2: &str = include_str!("fixtures/queue_items_0_5_2.json");

fn sent_stores(threads: &MemoryThreads) -> Vec<Value> {
    stores(threads)
        .iter()
        .map(|store| serde_json::to_value(store).expect("json"))
        .collect()
}

fn names_no_other_message(store: &Value) -> bool {
    let mut keys: Vec<&str> = store
        .as_object()
        .expect("an object")
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    keys == ["message", "message_id", "thread_id"]
}

async fn idle(adapter: &ThreadAccessAdapter) {
    eventually("the thread queue is empty", || {
        adapter.queue_state().threads.is_empty()
    })
    .await;
}

fn id_in(value: &Value, key: &str) -> Uuid {
    serde_json::from_value(value[key].clone()).expect("an id")
}

async fn store_holding_the_items_of_0_5_2() -> (Arc<MemoryQueueStore>, Vec<Value>) {
    let items: Vec<Value> = serde_json::from_str(ITEMS_OF_0_5_2).expect("the fixture");
    let store = Arc::new(MemoryQueueStore::new());
    for item in &items {
        let thread = br_agentic_contract::ThreadRef::from_uuid(id_in(item, "thread"))
            .expect("a thread reference");
        store
            .append(StoredItem {
                thread,
                id: id_in(item, "id"),
                bytes: serde_json::to_vec(item).expect("bytes"),
            })
            .await
            .expect("kept");
    }
    (store, items)
}

#[tokio::test]
async fn a_message_queued_after_a_read_and_after_other_messages_names_no_other_message() {
    let w = world().await;
    let thread = thread_ref();
    let earlier = input_turn(vec![notice_entry("the user's question")]);
    let id = ConversationId::from_uuid(thread.as_uuid()).expect("conversation id");
    let conversation =
        Conversation::from_parts(id, vec![Turn::InputTurn(earlier)]).expect("conversation");
    w.threads
        .answer_graphql(move |_document, _variables| Ok(context_answer(thread, &conversation)));
    w.access().read(&thread).await.expect("a conversation");
    let mail = input_turn(vec![notice_entry("a mail")]);
    let (first, second) = (turn_ref(w.agent), turn_ref(w.agent));

    w.access()
        .add_input_turn(&thread, mail.clone())
        .await
        .expect("accepted");
    w.access()
        .add_entry(&thread, first.clone(), notice("first answer"))
        .await
        .expect("accepted");
    w.access()
        .add_entry(&thread, second.clone(), notice("second answer"))
        .await
        .expect("accepted");

    idle(&w.adapter).await;
    let sent = sent_stores(&w.threads);
    let ids: Vec<Uuid> = sent
        .iter()
        .map(|store| id_in(store, "message_id"))
        .collect();
    assert_eq!(
        ids,
        vec![mail.id().as_uuid(), first.id.as_uuid(), second.id.as_uuid()],
        "the messages reach Threads in the order they were queued"
    );
    assert!(sent.iter().all(names_no_other_message), "{sent:?}");
}

#[tokio::test]
async fn the_bytes_the_store_keeps_for_a_message_name_no_other_message() {
    let w = world().await;
    let thread = thread_ref();
    w.threads.pause();

    w.access()
        .add_input_turn(&thread, input_turn(vec![notice_entry("a mail")]))
        .await
        .expect("accepted");

    let kept = w.store.list().await.expect("listed");
    let item: Value = serde_json::from_slice(&kept[0].bytes).expect("json");
    assert!(names_no_other_message(&item["payload"]["Command"]["Store"]));
}

#[tokio::test]
async fn an_item_that_0_5_kept_with_a_parent_is_delivered_with_no_parent() {
    let (store, items) = store_holding_the_items_of_0_5_2().await;
    assert!(
        items[0]["payload"]["Command"]["Store"]["parent_message_id"].is_string()
            && items[1]["payload"]["Command"]["Store"]["parent_message_id"].is_string(),
        "the fixture holds a whole message and an opening that name a parent"
    );
    let threads = MemoryThreads::new();
    let recorder = Arc::new(Recorder::default());
    let agent = agent_id();

    let adapter = ThreadAccessAdapter::new(
        &threads.client(),
        settings(Arc::clone(&store), Arc::clone(&recorder), agent),
    )
    .await
    .expect("the adapter reads the items of 0.5");

    eventually("the leftovers are delivered", || {
        threads.operations().len() == 4
    })
    .await;
    idle(&adapter).await;
    let sent = sent_stores(&threads);
    assert_eq!(sent.len(), 2);
    assert!(sent.iter().all(names_no_other_message), "{sent:?}");
    assert_eq!(
        id_in(&sent[0], "message_id"),
        id_in(&items[0], "message"),
        "the whole message goes first"
    );
    assert_eq!(id_in(&sent[1], "message_id"), id_in(&items[1], "message"));
    let chunks = threads.chunks();
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].1, 0);
    assert!(matches!(chunks[0].2, Chunk::Entry { .. }));
    let commands = threads.commands();
    assert!(matches!(commands[2].body, CommandBody::Complete(_)));
    assert_eq!(
        commands
            .iter()
            .map(|command| command.id)
            .collect::<Vec<_>>(),
        [0, 1, 3]
            .iter()
            .map(|at| id_in(&items[*at], "id"))
            .collect::<Vec<_>>(),
        "each command keeps the command id it was kept under"
    );
    for _ in 0..400 {
        if store.list().await.expect("listed").is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(store.list().await.expect("listed").is_empty());
    assert!(recorder.reports().is_empty(), "{:?}", recorder.reports());
}
