mod support;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock, Weak};

use br_agentic_contract::{AiTurnRef, ThreadAccess, ThreadRef};
use br_runner_threads::test_double::MemoryThreads;
use br_runner_threads::{MemoryQueueStore, ThreadAccessAdapter, WriteContextFn};
use support::adapter_x::*;
use support::queue_kit::within;
use support::*;

async fn adapter_whose_context_function_writes_on_the_same_thread()
-> (Arc<ThreadAccessAdapter>, MemoryThreads, AiTurnRef) {
    let agent = agent_id();
    let handle: Arc<OnceLock<Weak<ThreadAccessAdapter>>> = Arc::default();
    let reentered = Arc::new(AtomicBool::new(false));
    let inner_turn = turn_ref(agent);
    let function = {
        let handle = Arc::clone(&handle);
        let inner_turn = inner_turn.clone();
        WriteContextFn::new(move |thread: ThreadRef| {
            let handle = Arc::clone(&handle);
            let reentered = Arc::clone(&reentered);
            let inner_turn = inner_turn.clone();
            async move {
                if !reentered.swap(true, Ordering::SeqCst) {
                    let adapter = handle.get().and_then(Weak::upgrade).expect("the adapter");
                    adapter
                        .add_entry(&thread, inner_turn, notice("from the context function"))
                        .await?;
                }
                Ok(context_of(agent))
            }
        })
    };
    let threads = MemoryThreads::new();
    let adapter = Arc::new(
        ThreadAccessAdapter::new(
            &threads.client(),
            settings_for(
                Arc::new(MemoryQueueStore::new()),
                Arc::new(Recorder::default()),
                function,
            ),
        )
        .await
        .expect("adapter"),
    );
    handle.set(Arc::downgrade(&adapter)).expect("set once");
    (adapter, threads, inner_turn)
}

#[tokio::test]
async fn a_context_function_that_writes_on_the_same_thread_does_not_block_opening_a_response() {
    let (adapter, threads, inner_turn) =
        adapter_whose_context_function_writes_on_the_same_thread().await;
    let other_turn = turn_ref(inner_turn.agent);

    let opened = within(
        "open_response",
        adapter.open_response(&thread_ref(), other_turn, entry_id()),
    )
    .await;

    assert!(opened.is_ok());
    eventually("the writes of both turns", || threads.commands().len() == 2).await;
}

#[tokio::test]
async fn a_context_function_that_writes_on_the_same_thread_does_not_block_opening_a_result() {
    let (adapter, threads, inner_turn) =
        adapter_whose_context_function_writes_on_the_same_thread().await;
    let other_turn = turn_ref(inner_turn.agent);

    let opened = within(
        "open_result",
        adapter.open_result(&thread_ref(), other_turn, entry_id()),
    )
    .await;

    assert!(opened.is_ok());
    eventually("the writes of both turns", || threads.commands().len() == 2).await;
}
