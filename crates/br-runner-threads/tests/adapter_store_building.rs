mod support;

use br_runner_threads::Error;
use br_runner_threads::test_double::MemoryThreads;
use support::queue_kit::*;
use support::store_setup::*;
use support::*;

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
