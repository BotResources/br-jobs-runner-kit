mod support;

use std::sync::Arc;
use std::time::Duration;

use br_runner_threads::test_double::MemoryThreads;
use br_runner_threads::{Error, MemoryQueueStore, QueueStore, StoredItem, ThreadAccessAdapter};
use support::adapter_x::*;
use support::*;

async fn build(
    threads: &MemoryThreads,
    store: Arc<dyn QueueStore>,
    change: impl FnOnce(&mut br_runner_threads::ThreadAccessSettings),
) -> Result<ThreadAccessAdapter, Error> {
    let mut settings = settings_for(
        store,
        Arc::new(Recorder::default()),
        fixed_context(context_of(agent_id())),
    );
    change(&mut settings);
    ThreadAccessAdapter::new(&threads.client(), settings).await
}

#[tokio::test]
async fn a_zero_outcome_wait_is_refused_with_invalid_input() {
    let threads = MemoryThreads::new();

    let built = build(&threads, Arc::new(MemoryQueueStore::new()), |settings| {
        settings.outcome_wait = Duration::ZERO;
    })
    .await;

    assert!(matches!(built.err(), Some(Error::InvalidInput(_))));
}

#[tokio::test]
async fn a_zero_wait_between_attempts_is_refused_with_invalid_input() {
    let threads = MemoryThreads::new();

    let built = build(&threads, Arc::new(MemoryQueueStore::new()), |settings| {
        settings.retry_wait = Duration::ZERO;
    })
    .await;

    assert!(matches!(built.err(), Some(Error::InvalidInput(_))));
}

#[tokio::test]
async fn waits_above_zero_build_an_adapter_that_sends_nothing_by_itself() {
    let threads = MemoryThreads::new();

    let built = build(&threads, Arc::new(MemoryQueueStore::new()), |settings| {
        settings.outcome_wait = Duration::from_millis(1);
        settings.retry_wait = Duration::from_millis(1);
    })
    .await;

    let Ok(adapter) = built else {
        panic!("an adapter");
    };
    assert!(adapter.queue_state().threads.is_empty());
    assert!(threads.operations().is_empty());
}

#[tokio::test]
async fn a_store_that_cannot_list_fails_the_building_with_store_and_delivers_nothing() {
    let threads = MemoryThreads::new();
    let flaky = FlakyStore::new();
    flaky.fail_list();

    let built = build(&threads, flaky, |_settings| {}).await;

    assert_eq!(
        built.err(),
        Some(Error::Store("the store cannot list".to_owned()))
    );
    assert!(threads.operations().is_empty());
}

#[tokio::test]
async fn a_kept_item_that_cannot_be_read_fails_the_building_with_store_and_removes_nothing() {
    let threads = MemoryThreads::new();
    let store = Arc::new(MemoryQueueStore::new());
    store
        .append(StoredItem {
            thread: thread_ref(),
            id: v7(),
            bytes: b"not an item".to_vec(),
        })
        .await
        .expect("kept");

    let built = build(&threads, store.clone(), |_settings| {}).await;

    assert!(matches!(built.err(), Some(Error::Store(_))));
    assert_eq!(store.list().await.expect("list").len(), 1);
    assert!(threads.operations().is_empty());
}
