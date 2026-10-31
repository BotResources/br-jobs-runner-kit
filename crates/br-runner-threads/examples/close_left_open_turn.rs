use std::sync::Arc;
use std::time::Duration;

use br_agentic_contract::{AiTurnRef, PortError, ThreadAccess, ThreadRef};
use br_llm_messages::{AgentId, Author, EntryId, RuntimeNotice, TurnEntry, TurnId};
use br_runner_threads::test_double::MemoryThreads;
use br_runner_threads::{
    Closing, ClosureKind, Confirmed, Error, IgnoreReports, MemoryQueueStore, ThreadAccessAdapter,
    ThreadAccessSettings, ThreadClient, WriteContext, WriteContextFn,
};
use uuid::Uuid;

pub async fn leave_a_turn_open(
    adapter: &ThreadAccessAdapter,
    thread: &ThreadRef,
    turn: &AiTurnRef,
) -> Result<(), PortError> {
    let entry = EntryId::from_uuid(Uuid::now_v7()).expect("an entry id");
    let notice = RuntimeNotice::new(entry, "the run started").expect("a notice");
    adapter
        .add_entry(thread, turn.clone(), TurnEntry::from(notice))
        .await?;
    adapter.instance_ended(turn.agent).await;
    Ok(())
}

pub async fn close_by_message_id(
    client: &ThreadClient,
    thread: &ThreadRef,
    message_id: Uuid,
) -> Result<Confirmed, Error> {
    let closing = Closing {
        command_id: Uuid::now_v7(),
        thread_id: thread.as_uuid(),
        message_id,
        kind: ClosureKind::Failed,
        detail: Some("the run was lost with its runner".to_owned()),
        acting_user_id: None,
        wait: Duration::from_secs(10),
    };
    client.close_message(&closing, None).await
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let threads = MemoryThreads::new();
    let client = threads.client();
    let agent = AgentId::from_uuid(Uuid::now_v7())?;
    let author = Author::new("Analyst")?;
    let write_context = WriteContextFn::new({
        let author = author.clone();
        move |_thread| {
            let context = WriteContext {
                agent,
                author: author.clone(),
                job_id: None,
                acting_user_id: None,
                impersonator_id: None,
            };
            async move { Ok::<_, PortError>(context) }
        }
    });
    let settings = ThreadAccessSettings {
        outcome_wait: Duration::from_secs(10),
        retry_wait: Duration::from_secs(1),
        store: Arc::new(MemoryQueueStore::new()),
        observer: Arc::new(IgnoreReports),
        write_context,
    };
    let adapter = ThreadAccessAdapter::new(&client, settings).await?;
    let thread = ThreadRef::from_uuid(Uuid::now_v7())?;
    let turn = AiTurnRef {
        id: TurnId::from_uuid(Uuid::now_v7())?,
        agent,
        author,
    };
    leave_a_turn_open(&adapter, &thread, &turn).await?;
    let confirmed = close_by_message_id(&client, &thread, turn.id.as_uuid()).await?;
    println!("the left-open turn is now {:?}", confirmed.status);
    Ok(())
}
