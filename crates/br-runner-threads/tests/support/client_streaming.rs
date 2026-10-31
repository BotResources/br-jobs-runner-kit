use br_agentic_contract::StreamSink;
use br_llm_messages::{
    BlockId, DeltaOp, EntryId, InterruptionKind, ModelResponse, ResponseDelta, ResponseMetadata,
};
use br_runner_threads::test_double::MemoryThreads;
use br_runner_threads::{OpenStream, ResponseControl, ResponseSink, StreamHandle};

use super::client_x::*;
use super::ids::*;

pub struct Streaming {
    pub threads: MemoryThreads,
    pub handle: StreamHandle,
    pub open: OpenStream,
    pub entry: EntryId,
    pub sink: ResponseSink,
    pub control: ResponseControl,
}

pub async fn streaming() -> Streaming {
    let threads = MemoryThreads::new();
    let (handle, open) = opened(&threads.client()).await;
    let entry = entry_id();
    let (sink, control) = handle.response(entry).expect("a pair");
    Streaming {
        threads,
        handle,
        open,
        entry,
        sink,
        control,
    }
}

impl Streaming {
    pub fn delta(&self, op: DeltaOp) -> ResponseDelta {
        ResponseDelta::new(self.open.message_id, self.entry, op)
    }

    pub async fn send(&mut self, op: DeltaOp) {
        let delta = self.delta(op);
        self.sink.send(delta).await;
    }

    pub async fn stream_partial_text(&mut self, block: BlockId, text: &str) {
        self.send(DeltaOp::response_started(ResponseMetadata::new()))
            .await;
        self.send(DeltaOp::block_started(
            block,
            br_llm_messages::BlockStart::text(),
        ))
        .await;
        self.send(DeltaOp::text_appended(block, text)).await;
    }

    pub fn sent(&self) -> Vec<ResponseDelta> {
        deltas_sent(&self.threads)
    }

    pub fn folded(&self) -> Option<ModelResponse> {
        folded(self.open.message_id, self.entry, &self.sent())
    }
}

pub fn kinds(deltas: &[ResponseDelta]) -> Vec<&'static str> {
    deltas
        .iter()
        .map(|delta| match delta.op() {
            DeltaOp::ResponseStarted { .. } => "started",
            DeltaOp::BlockStarted { .. } => "block_started",
            DeltaOp::TextAppended { .. } => "text_appended",
            DeltaOp::BlockInserted { .. } => "inserted",
            DeltaOp::BlockFinalized { .. } => "finalized",
            DeltaOp::ResponseMetadataUpdated { .. } => "metadata",
            DeltaOp::ResponseCompleted { .. } => "completed",
            DeltaOp::ResponseInterrupted { .. } => "interrupted",
            _ => "other",
        })
        .collect()
}

pub fn interrupted_cancelled(delta: &ResponseDelta) -> bool {
    matches!(
        delta.op(),
        DeltaOp::ResponseInterrupted { reason, .. }
            if reason.kind() == InterruptionKind::Cancelled && reason.detail().is_none()
    )
}
