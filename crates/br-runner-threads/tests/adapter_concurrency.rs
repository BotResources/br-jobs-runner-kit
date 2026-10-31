mod support;

use std::sync::Arc;

use br_agentic_contract::{PortFuture, ThreadRef};
use br_llm_messages::{ClosureReason, TurnClosure};
use br_runner_threads::test_double::{CommandBody, Operation};
use br_runner_threads::{Error, MemoryQueueStore, QueueStore, StoredItem};
use support::adapter_wire::*;
use support::adapter_x::*;
use support::*;
use uuid::Uuid;

struct YieldingStore {
    inner: MemoryQueueStore,
}

impl QueueStore for YieldingStore {
    fn append<'a>(&'a self, item: StoredItem) -> PortFuture<'a, Result<(), Error>> {
        Box::pin(async move {
            for _ in 0..3 {
                tokio::task::yield_now().await;
            }
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
async fn a_closing_on_the_same_turn_waits_for_a_completing_response_to_queue_all_its_deltas() {
    let rig = rig_with_store(Arc::new(YieldingStore {
        inner: MemoryQueueStore::new(),
    }))
    .await;
    let thread = thread_ref();
    let turn = turn_ref(rig.agent);
    let (entry, block) = (entry_id(), block_id());
    let response = text_response(entry, block, "the answer");
    let closure = TurnClosure::new(entry_id(), ClosureReason::cancelled());

    let (completed, closed) = tokio::join!(
        rig.access()
            .complete_response(&thread, turn.clone(), response.clone()),
        rig.access().close_turn(&thread, turn.clone(), closure),
    );

    assert!(completed.is_ok(), "{completed:?}");
    assert!(closed.is_ok(), "{closed:?}");
    rig.idle().await;
    let deltas = deltas_of(&rig.threads, turn.id.as_uuid());
    assert_eq!(folded(&deltas, turn.id, entry), Some(response));
    let operations = rig.threads.operations();
    let Some(Operation::Command(last)) = operations.last() else {
        panic!("the closing is the last operation: {operations:?}");
    };
    assert!(matches!(last.body, CommandBody::Fail(_)));
}
