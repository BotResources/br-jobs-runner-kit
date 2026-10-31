use std::time::Duration;

use br_agentic_contract::StreamSink;
use br_llm_messages::{
    AgentId, BlockId, BlockStart, ContentBlock, DeltaOp, EntryId, ModelResponse, ResponseDelta,
    ResponseEnding, ResponseMetadata, RuntimeNotice, TextBlock, TurnEntry, TurnId,
};
use br_runner_threads::test_double::MemoryThreads;
use br_runner_threads::{
    Closing, ClosureKind, ContextQuery, Error, NewThread, OpenStream, ThreadClient, ThreadContext,
};
use serde_json::json;
use uuid::Uuid;

const WAIT: Duration = Duration::from_secs(10);

pub async fn create_thread(client: &ThreadClient) -> Result<Uuid, Error> {
    let thread_id = Uuid::now_v7();
    let thread = NewThread {
        command_id: Uuid::now_v7(),
        thread_id,
        owner_id: None,
        member_ids: Vec::new(),
        title: Some("Open tickets".to_owned()),
        resources: Default::default(),
        origin: None,
        acting_user_id: None,
        wait: WAIT,
    };
    client.create_thread(&thread).await?;
    Ok(thread_id)
}

pub async fn stream_a_message(client: &ThreadClient, thread_id: Uuid) -> Result<Uuid, Error> {
    let message_id = TurnId::from_uuid(Uuid::now_v7()).expect("a turn id");
    let agent_id = AgentId::from_uuid(Uuid::now_v7()).expect("an agent id");
    let open = OpenStream {
        command_id: Uuid::now_v7(),
        thread_id,
        message_id,
        parent_message_id: None,
        agent_id,
        author_name: "Analyst".to_owned(),
        job_id: None,
        acting_user_id: None,
        wait: WAIT,
    };
    let handle = client.open_stream(&open).await?;
    let notice_id = EntryId::from_uuid(Uuid::now_v7()).expect("an entry id");
    let notice = RuntimeNotice::new(notice_id, "the run started").expect("a notice");
    handle.writer().entry(&TurnEntry::from(notice)).await?;

    let (entry, block) = (
        EntryId::from_uuid(Uuid::now_v7()).expect("an entry id"),
        BlockId::from_uuid(Uuid::now_v7()).expect("a block id"),
    );
    let (mut sink, control) = handle.response(entry)?;
    for op in [
        DeltaOp::response_started(ResponseMetadata::new()),
        DeltaOp::block_started(block, BlockStart::text()),
        DeltaOp::text_appended(block, "Three tickets"),
    ] {
        sink.send(ResponseDelta::new(message_id, entry, op)).await;
    }
    let text = TextBlock::new(block, "Three tickets are open").expect("a text block");
    let response = ModelResponse::new(
        entry,
        vec![ContentBlock::Text(text)],
        ResponseEnding::finished(),
        ResponseMetadata::new(),
    )
    .expect("a model response");
    control.complete(&response).await?;

    let closing = Closing {
        command_id: Uuid::now_v7(),
        thread_id,
        message_id: message_id.as_uuid(),
        kind: ClosureKind::Completed,
        detail: None,
        acting_user_id: None,
        wait: WAIT,
    };
    client.close_message(&closing, Some(&handle)).await?;
    Ok(message_id.as_uuid())
}

pub async fn read_context(
    client: &ThreadClient,
    thread_id: Uuid,
) -> Result<Option<ThreadContext>, Error> {
    client
        .read_context(&ContextQuery {
            thread_id,
            agent: None,
        })
        .await
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let threads = MemoryThreads::new();
    threads.answer_graphql(|_, _| Ok(json!({ "threadsContext": null })));
    let client = threads.client();
    let thread_id = create_thread(&client).await?;
    let message_id = stream_a_message(&client, thread_id).await?;
    let context = read_context(&client, thread_id).await?;
    println!("message {message_id}, context found: {}", context.is_some());
    Ok(())
}
