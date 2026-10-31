use br_agentic_contract::ThreadRef;
use br_llm_messages::{
    BlockId, Conversation, DeltaOp, EntryId, ModelResponse, ResponseDelta, ResponseEnding,
    ResponseMetadata, TextBlock, Turn, TurnEntry, TurnId,
};
use br_runner_threads::test_double::{CommandBody, MemoryThreads};
use contract_threads::{AiDelivery, AiTurn, Chunk, MessageTurn, StoreMessageV2};
use serde_json::{Value, json};
use uuid::Uuid;

use super::ids::{input_turn, notice_entry};

pub fn stores(threads: &MemoryThreads) -> Vec<StoreMessageV2> {
    threads
        .commands()
        .into_iter()
        .filter_map(|command| match command.body {
            CommandBody::Store(store) => Some(store),
            _ => None,
        })
        .collect()
}

pub fn ai_of(store: &StoreMessageV2) -> &AiTurn {
    match &store.message {
        MessageTurn::Ai(turn) => turn,
        MessageTurn::User(_) => panic!("a user message, not an AI message"),
    }
}

pub fn is_streaming(store: &StoreMessageV2) -> bool {
    matches!(ai_of(store).delivery, AiDelivery::Streaming)
}

pub fn chunks_of(threads: &MemoryThreads, message: Uuid) -> Vec<(u64, Chunk)> {
    threads
        .chunks()
        .into_iter()
        .filter(|(id, _, _)| *id == message)
        .map(|(_, seq, chunk)| (seq, chunk))
        .collect()
}

pub fn deltas_of(threads: &MemoryThreads, message: Uuid) -> Vec<ResponseDelta> {
    chunks_of(threads, message)
        .into_iter()
        .filter_map(|(_, chunk)| match chunk {
            Chunk::Delta { delta } => Some(delta.to_delta().expect("a delta")),
            _ => None,
        })
        .collect()
}

pub fn entry_of(chunk: &Chunk) -> TurnEntry {
    match chunk {
        Chunk::Entry { entry } => entry.to_entry().expect("an entry"),
        other => panic!("not an entry chunk: {other:?}"),
    }
}

pub fn text_response(entry: EntryId, block: BlockId, text: &str) -> ModelResponse {
    ModelResponse::new(
        entry,
        vec![br_llm_messages::ContentBlock::Text(
            TextBlock::new(block, text).expect("text"),
        )],
        ResponseEnding::finished(),
        ResponseMetadata::new(),
    )
    .expect("response")
}

pub fn started(turn: TurnId, entry: EntryId) -> ResponseDelta {
    ResponseDelta::new(
        turn,
        entry,
        DeltaOp::response_started(ResponseMetadata::new()),
    )
}

pub fn inserted(turn: TurnId, entry: EntryId, block: BlockId, text: &str) -> ResponseDelta {
    ResponseDelta::new(
        turn,
        entry,
        DeltaOp::block_inserted(br_llm_messages::ContentBlock::Text(
            TextBlock::new(block, text).expect("text"),
        )),
    )
}

pub fn folded(deltas: &[ResponseDelta], turn: TurnId, entry: EntryId) -> Option<ModelResponse> {
    let mut draft = br_llm_messages::ResponseDraft::new(turn, entry);
    for delta in deltas {
        draft.apply(delta).expect("the delta applies");
    }
    draft.into_response()
}

pub fn context_answer(thread: ThreadRef, conversation: &Conversation) -> Value {
    let document: Value =
        serde_json::from_str(&conversation.to_json().expect("conversation json")).expect("json");
    json!({
        "threadsContext": {
            "threadId": thread.as_uuid(),
            "conversation": document,
            "projectedThread": null,
            "members": [],
            "resources": [],
        }
    })
}

pub fn small_conversation(id: Uuid) -> Conversation {
    let id = br_llm_messages::ConversationId::from_uuid(id).expect("conversation id");
    Conversation::from_parts(
        id,
        vec![Turn::InputTurn(input_turn(vec![notice_entry("earlier")]))],
    )
    .expect("conversation")
}
