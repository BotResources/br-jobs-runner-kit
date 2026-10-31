mod support;

use std::sync::Arc;

use br_agentic_contract::ThreadAccess;
use br_runner_threads::test_double::{CommandBody, MemoryThreads};
use br_runner_threads::{Error, ReportKind, ThreadAccessAdapter};
use support::queue_kit::*;
use support::queue_store::*;
use support::*;

fn store_error(text: &str) -> Error {
    Error::Store(text.to_owned())
}

struct Setup {
    store: Arc<FlakyStore>,
    recorder: Arc<Recorder>,
    agent: br_llm_messages::AgentId,
}

impl Setup {
    fn new() -> Self {
        Self {
            store: FlakyStore::new(),
            recorder: Arc::new(Recorder::default()),
            agent: agent_id(),
        }
    }

    async fn adapter_on(&self, threads: &MemoryThreads) -> Result<ThreadAccessAdapter, Error> {
        build_adapter(
            &threads.client(),
            self.store.clone(),
            self.recorder.clone(),
            self.agent,
        )
        .await
    }
}

async fn leave_inputs_in_the_store(
    setup: &Setup,
    thread: &br_agentic_contract::ThreadRef,
    count: usize,
) -> Vec<uuid::Uuid> {
    let threads = MemoryThreads::new();
    threads.pause();
    let adapter = setup.adapter_on(&threads).await.expect("adapter");
    let mut messages = Vec::new();
    for _ in 0..count {
        let turn = input_turn(vec![human_entry("left")]);
        messages.push(turn.id().as_uuid());
        adapter
            .add_input_turn(thread, turn)
            .await
            .expect("accepted");
    }
    drop(adapter);
    messages
}

#[tokio::test]
async fn the_items_left_in_a_store_are_delivered_first_and_in_order_by_a_new_adapter() {
    let setup = Setup::new();
    let thread = thread_ref();
    let mut expected = leave_inputs_in_the_store(&setup, &thread, 3).await;
    assert_eq!(setup.store.kept().await.len(), 3);

    let threads = MemoryThreads::new();
    let adapter = setup.adapter_on(&threads).await.expect("adapter");
    let turn = input_turn(vec![human_entry("later")]);
    expected.push(turn.id().as_uuid());
    adapter
        .add_input_turn(&thread, turn)
        .await
        .expect("accepted");

    eventually("four delivered", || threads.commands().len() == 4).await;
    assert_eq!(command_messages(&threads), expected);
    settled_over(&adapter, setup.store.as_ref()).await;
}

#[tokio::test]
async fn an_accepted_item_is_in_the_store_when_the_call_returns_under_its_own_id() {
    let setup = Setup::new();
    let threads = MemoryThreads::new();
    threads.pause();
    let adapter = setup.adapter_on(&threads).await.expect("adapter");
    let thread = thread_ref();

    for _ in 0..2 {
        let turn = input_turn(vec![human_entry("kept")]);
        adapter
            .add_input_turn(&thread, turn)
            .await
            .expect("accepted");
    }

    let kept = setup.store.kept().await;
    assert_eq!(kept.len(), 2);
    assert!(kept.iter().all(|item| item.thread == thread));
    assert_ne!(kept[0].id, kept[1].id);
    assert!(kept.iter().all(|item| item.id.get_version_num() == 7));
    eventually("the head was attempted", || threads.commands().len() == 1).await;
    assert_eq!(threads.commands()[0].id, kept[0].id);
    threads.resume();
    eventually("both delivered", || threads.commands().len() == 2).await;
    assert_eq!(threads.commands()[1].id, kept[1].id);
    settled_over(&adapter, setup.store.as_ref()).await;
}

#[tokio::test]
async fn a_store_that_cannot_append_refuses_the_operation_with_a_store_error() {
    let setup = Setup::new();
    let threads = MemoryThreads::new();
    let adapter = setup.adapter_on(&threads).await.expect("adapter");
    setup.store.allow_appends(0);
    let thread = thread_ref();

    let refused = adapter
        .add_input_turn(&thread, input_turn(vec![human_entry("lost")]))
        .await
        .expect_err("refused");

    assert_eq!(
        fault_of(refused),
        store_error("the test store refuses to append")
    );
    assert!(adapter.queue_state().threads.is_empty());
    assert!(threads.operations().is_empty());
    assert!(setup.store.kept().await.is_empty());
}

#[tokio::test]
async fn when_a_later_append_of_an_operation_fails_the_items_already_kept_stay_queued() {
    let setup = Setup::new();
    let threads = MemoryThreads::new();
    let adapter = setup.adapter_on(&threads).await.expect("adapter");
    let thread = thread_ref();
    let turn = turn_ref(setup.agent);
    setup.store.allow_appends(1);

    let refused = adapter
        .add_entry(&thread, turn.clone(), notice("a"))
        .await
        .expect_err("refused");

    assert_eq!(
        fault_of(refused),
        store_error("the test store refuses to append")
    );
    eventually("the opening that was kept is delivered", || {
        threads.commands().len() == 1
    })
    .await;
    assert!(matches!(threads.commands()[0].body, CommandBody::Store(_)));
    assert_eq!(message_of(&threads.commands()[0]), turn.id.as_uuid());
    quiet().await;
    assert!(threads.chunks().is_empty());
}

#[tokio::test]
async fn the_number_of_a_chunk_that_the_store_did_not_keep_is_not_used() {
    let setup = Setup::new();
    let threads = MemoryThreads::new();
    let adapter = setup.adapter_on(&threads).await.expect("adapter");
    let thread = thread_ref();
    let turn = turn_ref(setup.agent);
    adapter
        .add_entry(&thread, turn.clone(), notice("first"))
        .await
        .expect("accepted");
    setup.store.allow_appends(0);
    adapter
        .add_entry(&thread, turn.clone(), notice("refused"))
        .await
        .expect_err("refused");
    setup.store.allow_all_appends();
    adapter
        .add_entry(&thread, turn.clone(), notice("second"))
        .await
        .expect("accepted");

    eventually("two chunks delivered", || threads.chunks().len() == 2).await;
    let seqs: Vec<u64> = threads.chunks().iter().map(|c| c.1).collect();
    assert_eq!(seqs, vec![0, 1]);
}

#[tokio::test]
async fn a_failed_remove_is_reported_counts_as_ended_and_the_next_adapter_sends_the_item_again() {
    let setup = Setup::new();
    let threads = MemoryThreads::new();
    let adapter = setup.adapter_on(&threads).await.expect("adapter");
    let thread = thread_ref();
    setup.store.fail_remove(true);
    let turn = input_turn(vec![human_entry("sent")]);
    let message = turn.id().as_uuid();
    adapter
        .add_input_turn(&thread, turn)
        .await
        .expect("accepted");

    eventually("the failure is reported", || {
        setup
            .recorder
            .reports()
            .iter()
            .any(|r| r.kind == ReportKind::RemoveFailed)
    })
    .await;

    let report = &setup.recorder.reports()[0];
    assert_eq!(report.kind, ReportKind::RemoveFailed);
    assert_eq!(
        report.error,
        Some(store_error("the test store refuses to remove"))
    );
    assert_eq!(report.message_id, message);
    assert_eq!(report.thread, thread);
    assert_eq!(report.item_id, threads.commands()[0].id);
    assert!(adapter.queue_state().threads.is_empty());
    let kept = setup.store.kept().await;
    assert_eq!(kept.len(), 1);
    drop(adapter);

    setup.store.fail_remove(false);
    let next_threads = MemoryThreads::new();
    let next = setup.adapter_on(&next_threads).await.expect("adapter");
    eventually("the item is sent again", || {
        next_threads.commands().len() == 1
    })
    .await;
    assert_eq!(next_threads.commands()[0], threads.commands()[0]);
    settled_over(&next, setup.store.as_ref()).await;
}

#[tokio::test]
async fn building_fails_with_a_store_error_when_the_store_cannot_list_and_sends_and_removes_nothing()
 {
    let setup = Setup::new();
    let thread = thread_ref();
    leave_inputs_in_the_store(&setup, &thread, 2).await;
    setup.store.fail_list(true);
    let threads = MemoryThreads::new();

    let failed = setup
        .adapter_on(&threads)
        .await
        .err()
        .expect("building fails");

    assert_eq!(failed, store_error("the test store refuses to list"));
    quiet().await;
    assert!(threads.operations().is_empty());
    assert_eq!(setup.store.kept().await.len(), 2);
}

#[tokio::test]
async fn building_fails_with_a_store_error_when_a_kept_item_cannot_be_read_and_sends_and_removes_nothing()
 {
    let setup = Setup::new();
    let thread = thread_ref();
    leave_inputs_in_the_store(&setup, &thread, 1).await;
    setup
        .store
        .keep_raw(br_runner_threads::StoredItem {
            thread,
            id: v7(),
            bytes: b"not an item".to_vec(),
        })
        .await;
    let threads = MemoryThreads::new();

    let failed = setup
        .adapter_on(&threads)
        .await
        .err()
        .expect("building fails");

    assert!(matches!(failed, Error::Store(_)));
    quiet().await;
    assert!(threads.operations().is_empty());
    assert_eq!(setup.store.kept().await.len(), 2);
}

#[tokio::test]
async fn building_fails_with_a_store_error_when_a_kept_item_is_not_under_the_id_it_names() {
    let setup = Setup::new();
    let thread = thread_ref();
    leave_inputs_in_the_store(&setup, &thread, 1).await;
    let mut item = setup.store.kept().await.remove(0);
    item.id = v7();
    let other = Setup::new();
    other.store.keep_raw(item).await;
    let threads = MemoryThreads::new();

    let failed = other
        .adapter_on(&threads)
        .await
        .err()
        .expect("building fails");

    assert!(matches!(failed, Error::Store(_)));
    assert!(threads.operations().is_empty());
    assert_eq!(other.store.kept().await.len(), 1);
}
