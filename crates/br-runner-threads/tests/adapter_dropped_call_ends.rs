mod support;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use br_agentic_contract::{PortFuture, ThreadRef};
use br_runner_threads::{Error, MemoryQueueStore, QueueStore, StoredItem};
use support::adapter_wire::*;
use support::adapter_x::*;
use support::gate_store::{GateStore, dropped};
use support::queue_kit::pends;
use support::*;
use uuid::Uuid;

struct PanicsOnce {
    inner: MemoryQueueStore,
    armed: AtomicBool,
}

impl QueueStore for PanicsOnce {
    fn append<'a>(&'a self, item: StoredItem) -> PortFuture<'a, Result<(), Error>> {
        Box::pin(async move {
            assert!(!self.armed.swap(false, Ordering::SeqCst), "the store broke");
            self.inner.append(item).await
        })
    }

    fn list<'a>(&'a self) -> PortFuture<'a, Result<Vec<StoredItem>, Error>> {
        self.inner.list()
    }

    fn remove<'a>(&'a self, thread: &'a ThreadRef, id: Uuid) -> PortFuture<'a, Result<(), Error>> {
        self.inner.remove(thread, id)
    }
}

#[tokio::test]
async fn a_panic_in_the_store_reaches_the_caller_and_leaves_the_thread_usable() {
    let store = Arc::new(PanicsOnce {
        inner: MemoryQueueStore::new(),
        armed: AtomicBool::new(true),
    });
    let agent = agent_id();
    let rig = Arc::new(rig_over(store, fixed_context(context_of(agent)), agent).await);
    let (thread, turn) = (thread_ref(), turn_ref(agent));

    let calling = Arc::clone(&rig);
    let (called_thread, called_turn) = (thread, turn.clone());
    let outcome = tokio::spawn(async move {
        calling
            .access()
            .add_entry(&called_thread, called_turn, notice("first"))
            .await
    })
    .await;

    assert!(outcome.expect_err("the call panicked").is_panic());
    rig.access()
        .add_entry(&thread, turn.clone(), notice("second"))
        .await
        .expect("the thread takes the next call");
    rig.idle().await;
    assert_eq!(chunks_of(&rig.threads, turn.id.as_uuid()).len(), 1);
}

#[tokio::test]
async fn instance_ended_waits_for_the_entry_of_a_dropped_call_that_is_still_being_kept() {
    let store = GateStore::after_free_appends(1);
    let agent = agent_id();
    let rig = rig_over(store.clone(), fixed_context(context_of(agent)), agent).await;
    let (thread, turn) = (thread_ref(), turn_ref(agent));
    dropped(
        &store,
        rig.access()
            .add_entry(&thread, turn.clone(), notice("first")),
    )
    .await;

    let early = pends(rig.adapter.instance_ended(agent)).await;
    store.release_appends();
    rig.adapter.instance_ended(agent).await;

    assert!(early, "instance_ended ended while the append was parked");
    assert_eq!(chunks_of(&rig.threads, turn.id.as_uuid()).len(), 1);
}
