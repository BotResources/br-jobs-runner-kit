use br_agentic_contract::{AiTurnRef, ThreadRef};
use br_llm_messages::{
    AgentId, Author, BlockId, ContentBlock, ConversationId, EntryId, HumanId, HumanMessage,
    IncomingEntry, InputTurn, ModelResponse, OpaqueJson, PlainBlock, RequestId, ResponseEnding,
    ResponseMetadata, RuntimeNotice, RuntimeToolCall, RuntimeToolResult, RuntimeToolResults,
    TaskId, TaskStarted, TextBlock, ToolName, TurnEntry, TurnId, Uuid,
};

pub fn v7() -> Uuid {
    Uuid::now_v7()
}

pub fn entry_id() -> EntryId {
    EntryId::from_uuid(v7()).expect("entry id")
}

pub fn turn_id() -> TurnId {
    TurnId::from_uuid(v7()).expect("turn id")
}

pub fn block_id() -> BlockId {
    BlockId::from_uuid(v7()).expect("block id")
}

pub fn agent_id() -> AgentId {
    AgentId::from_uuid(v7()).expect("agent id")
}

pub fn conversation_id() -> ConversationId {
    ConversationId::from_uuid(v7()).expect("conversation id")
}

pub fn thread_ref() -> ThreadRef {
    ThreadRef::from_uuid(v7()).expect("thread ref")
}

pub fn author(name: &str) -> Author {
    Author::new(name).expect("author")
}

pub fn turn_ref(agent: AgentId) -> AiTurnRef {
    AiTurnRef {
        id: turn_id(),
        agent,
        author: author("Analyst"),
    }
}

pub fn text_block(text: &str) -> PlainBlock {
    PlainBlock::Text(TextBlock::new(block_id(), text).expect("text block"))
}

pub fn human_entry(text: &str) -> IncomingEntry {
    let message = HumanMessage::new(
        entry_id(),
        HumanId::from_uuid(v7()).expect("human id"),
        vec![text_block(text)],
    )
    .expect("human message");
    IncomingEntry::from(message)
}

pub fn input_turn(entries: Vec<IncomingEntry>) -> InputTurn {
    InputTurn::new(turn_id(), entries).expect("input turn")
}

pub fn notice(text: &str) -> TurnEntry {
    TurnEntry::from(RuntimeNotice::new(entry_id(), text).expect("notice"))
}

pub fn notice_entry(text: &str) -> IncomingEntry {
    IncomingEntry::from(RuntimeNotice::new(entry_id(), text).expect("notice"))
}

pub fn task_started() -> TurnEntry {
    TurnEntry::from(TaskStarted::new(
        entry_id(),
        RequestId::from_uuid(v7()).expect("request id"),
        TaskId::from_uuid(v7()).expect("task id"),
    ))
}

pub fn response_text(entry: EntryId, text: &str) -> ModelResponse {
    ModelResponse::new(
        entry,
        vec![ContentBlock::Text(
            TextBlock::new(block_id(), text).expect("text"),
        )],
        ResponseEnding::finished(),
        ResponseMetadata::new(),
    )
    .expect("response")
}

pub fn response_call(entry: EntryId, call: BlockId, name: &str) -> ModelResponse {
    let call = RuntimeToolCall::new(
        call,
        ToolName::new(name).expect("tool name"),
        OpaqueJson::parse("{}").expect("json"),
    )
    .expect("call");
    ModelResponse::new(
        entry,
        vec![ContentBlock::RuntimeToolCall(call)],
        ResponseEnding::tool_use(),
        ResponseMetadata::new(),
    )
    .expect("response")
}

pub fn tool_results(entry: EntryId, call: BlockId, name: &str, text: &str) -> RuntimeToolResults {
    let result = RuntimeToolResult::new(
        block_id(),
        call,
        ToolName::new(name).expect("tool name"),
        false,
        vec![text_block(text)],
    )
    .expect("result");
    RuntimeToolResults::new(entry, vec![result]).expect("results")
}
