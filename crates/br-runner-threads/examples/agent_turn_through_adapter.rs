use std::sync::Arc;
use std::time::Duration;

use br_agentic_contract::{AiTurnRef, PortError, ThreadAccess, ThreadRef};
use br_llm_messages::{
    AgentId, Author, BlockId, BlockStart, ClosureReason, ContentBlock, DeltaOp, EntryId,
    ModelResponse, ResponseDelta, ResponseEnding, ResponseMetadata, TextBlock, TurnClosure, TurnId,
};
use br_runner_threads::test_double::MemoryThreads;
use br_runner_threads::{
    IgnoreReports, MemoryQueueStore, ThreadAccessAdapter, ThreadAccessSettings, WriteContext,
    WriteContextFn,
};
use uuid::Uuid;

pub struct TurnEntries {
    pub discarded: EntryId,
    pub completed: EntryId,
}

fn block_id() -> BlockId {
    BlockId::from_uuid(Uuid::now_v7()).expect("a UUIDv7 is a block id")
}

fn entry_id() -> EntryId {
    EntryId::from_uuid(Uuid::now_v7()).expect("a UUIDv7 is an entry id")
}

fn streamed_deltas(turn: TurnId, entry: EntryId, block: BlockId, text: &str) -> Vec<ResponseDelta> {
    [
        DeltaOp::response_started(ResponseMetadata::new()),
        DeltaOp::block_started(block, BlockStart::text()),
        DeltaOp::text_appended(block, text),
    ]
    .into_iter()
    .map(|op| ResponseDelta::new(turn, entry, op))
    .collect()
}

pub fn whole_response(entry: EntryId, block: BlockId, text: &str) -> ModelResponse {
    let text = TextBlock::new(block, text).expect("a text block");
    ModelResponse::new(
        entry,
        vec![ContentBlock::Text(text)],
        ResponseEnding::finished(),
        ResponseMetadata::new(),
    )
    .expect("a model response")
}

pub fn build_thread_access_settings(agent: AgentId, author: Author) -> ThreadAccessSettings {
    ThreadAccessSettings {
        outcome_wait: Duration::from_secs(10),
        retry_wait: Duration::from_secs(1),
        store: Arc::new(MemoryQueueStore::new()),
        observer: Arc::new(IgnoreReports),
        write_context: WriteContextFn::new(move |_thread| {
            let context = WriteContext {
                agent,
                author: author.clone(),
                job_id: None,
                acting_user_id: None,
                impersonator_id: None,
            };
            async move { Ok::<_, PortError>(context) }
        }),
    }
}

pub async fn discard_then_complete_then_close(
    access: &dyn ThreadAccess,
    thread: &ThreadRef,
    turn: &AiTurnRef,
) -> Result<TurnEntries, PortError> {
    let entries = TurnEntries {
        discarded: entry_id(),
        completed: entry_id(),
    };
    let (cut_block, block) = (block_id(), block_id());
    let mut cut = access
        .open_response(thread, turn.clone(), entries.discarded)
        .await?;
    for delta in streamed_deltas(turn.id, entries.discarded, cut_block, "The tick") {
        cut.send(delta).await;
    }
    access.discard_response(thread, entries.discarded).await?;
    let mut sink = access
        .open_response(thread, turn.clone(), entries.completed)
        .await?;
    for delta in streamed_deltas(turn.id, entries.completed, block, "Three tickets") {
        sink.send(delta).await;
    }
    let response = whole_response(entries.completed, block, "Three tickets are open");
    access
        .complete_response(thread, turn.clone(), response)
        .await?;
    let closure = TurnClosure::new(entry_id(), ClosureReason::completed());
    access.close_turn(thread, turn.clone(), closure).await?;
    Ok(entries)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let threads = MemoryThreads::new();
    let agent = AgentId::from_uuid(Uuid::now_v7())?;
    let author = Author::new("Analyst")?;
    let settings = build_thread_access_settings(agent, author.clone());
    let adapter = ThreadAccessAdapter::new(&threads.client(), settings).await?;
    let turn = AiTurnRef {
        id: TurnId::from_uuid(Uuid::now_v7())?,
        agent,
        author,
    };
    let thread = ThreadRef::from_uuid(Uuid::now_v7())?;
    discard_then_complete_then_close(&adapter, &thread, &turn).await?;
    adapter.instance_ended(agent).await;
    println!("{} chunks reached the double", threads.chunks().len());
    Ok(())
}
