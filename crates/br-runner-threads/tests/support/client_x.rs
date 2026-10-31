use br_runner_threads::test_double;
use std::collections::BTreeMap;
use std::time::Duration;

use br_llm_messages::{
    AITurn, BlockId, BlockStart, ContentBlock, DeltaOp, EntryId, ModelName, ModelResponse,
    ResponseDelta, ResponseDraft, ResponseEnding, ResponseMetadata, TextBlock, Turn, TurnId,
};
use br_runner_threads::test_double::{Command, CommandBody, MemoryThreads};
use br_runner_threads::{
    Closing, ClosureKind, Error, NewThread, OpenStream, StreamHandle, ThreadClient, WholeMessage,
};
use contract_threads::{AiTurn, Chunk, MessageTurn, StoreMessageV2};
use uuid::Uuid;

use super::ids::{agent_id, author, turn_id, v7};

pub const WAIT: Duration = Duration::from_secs(5);

pub fn new_thread() -> NewThread {
    NewThread {
        command_id: v7(),
        thread_id: v7(),
        owner_id: None,
        member_ids: Vec::new(),
        title: None,
        resources: BTreeMap::new(),
        origin: None,
        acting_user_id: None,
        wait: WAIT,
    }
}

pub fn ai_message(thread_id: Uuid) -> WholeMessage {
    let agent = agent_id();
    WholeMessage {
        command_id: v7(),
        thread_id,
        parent_message_id: None,
        agent_id: agent,
        author_name: "Analyst".to_owned(),
        job_id: None,
        acting_user_id: None,
        impersonator_id: None,
        turn: Turn::AITurn(AITurn::open(turn_id(), agent, author("Analyst"))),
        annotations: Vec::new(),
        wait: WAIT,
    }
}

pub fn open_request(thread_id: Uuid) -> OpenStream {
    OpenStream {
        command_id: v7(),
        thread_id,
        message_id: turn_id(),
        parent_message_id: None,
        agent_id: agent_id(),
        author_name: "Analyst".to_owned(),
        job_id: None,
        acting_user_id: None,
        impersonator_id: None,
        wait: WAIT,
    }
}

pub async fn opened(client: &ThreadClient) -> (StreamHandle, OpenStream) {
    let open = open_request(v7());
    let handle = client.open_stream(&open).await.expect("the stream opens");
    (handle, open)
}

pub fn closing_of(open: &OpenStream, kind: ClosureKind) -> Closing {
    Closing {
        command_id: v7(),
        thread_id: open.thread_id,
        message_id: open.message_id.as_uuid(),
        kind,
        detail: (kind == ClosureKind::Failed).then(|| "the run failed".to_owned()),
        acting_user_id: None,
        wait: WAIT,
    }
}

pub fn started(turn: TurnId, entry: EntryId) -> ResponseDelta {
    ResponseDelta::new(
        turn,
        entry,
        DeltaOp::response_started(ResponseMetadata::new()),
    )
}

pub fn text_block_started(turn: TurnId, entry: EntryId, block: BlockId) -> ResponseDelta {
    ResponseDelta::new(
        turn,
        entry,
        DeltaOp::block_started(block, BlockStart::text()),
    )
}

pub fn text_appended(turn: TurnId, entry: EntryId, block: BlockId, text: &str) -> ResponseDelta {
    ResponseDelta::new(turn, entry, DeltaOp::text_appended(block, text))
}

pub fn chunk_seqs(threads: &MemoryThreads) -> Vec<u64> {
    threads.chunks().iter().map(|(_, seq, _)| *seq).collect()
}

pub fn chunk_list(threads: &MemoryThreads) -> Vec<Chunk> {
    threads.chunks().into_iter().map(|(_, _, c)| c).collect()
}

pub fn last_command(threads: &MemoryThreads) -> Command {
    threads.commands().pop().expect("a command was sent")
}

pub fn not_v7() -> Uuid {
    Uuid::from_u128(0x1234_5678_9abc_4def_8123_4567_89ab_cdef)
}

pub fn contiguous(count: usize) -> Vec<u64> {
    (0..count as u64).collect()
}

pub fn refusal_error() -> Error {
    Error::Refused(test_double::refusal(
        "thread-closed",
        "the thread is closed",
        Some(Uuid::now_v7()),
    ))
}

pub fn text_response(entry: EntryId, blocks: &[(BlockId, &str)]) -> ModelResponse {
    let content = blocks
        .iter()
        .map(|(id, text)| ContentBlock::Text(TextBlock::new(*id, *text).expect("text block")))
        .collect();
    ModelResponse::new(
        entry,
        content,
        ResponseEnding::finished(),
        ResponseMetadata::new(),
    )
    .expect("response")
}

pub fn deltas_sent(threads: &MemoryThreads) -> Vec<ResponseDelta> {
    chunk_list(threads)
        .into_iter()
        .filter_map(|chunk| match chunk {
            Chunk::Delta { delta } => Some(delta.to_delta().expect("a delta reads back")),
            _ => None,
        })
        .collect()
}

pub fn folded(turn: TurnId, entry: EntryId, deltas: &[ResponseDelta]) -> Option<ModelResponse> {
    let mut draft = ResponseDraft::new(turn, entry);
    for delta in deltas {
        draft.apply(delta).expect("the sent deltas fold");
    }
    draft.into_response()
}

pub fn store_of(body: &CommandBody) -> &StoreMessageV2 {
    match body {
        CommandBody::Store(store) => store,
        other => panic!("not a store command: {other:?}"),
    }
}

pub fn ai_of(store: &StoreMessageV2) -> &AiTurn {
    match &store.message {
        MessageTurn::Ai(turn) => turn,
        other => panic!("not an AI message: {other:?}"),
    }
}

pub fn text_response_by_model(
    entry: EntryId,
    block: BlockId,
    text: &str,
    model: &str,
) -> ModelResponse {
    ModelResponse::new(
        entry,
        vec![ContentBlock::Text(
            TextBlock::new(block, text).expect("text block"),
        )],
        ResponseEnding::finished(),
        ResponseMetadata::new().with_model(ModelName::new(model).expect("model name")),
    )
    .expect("response")
}
