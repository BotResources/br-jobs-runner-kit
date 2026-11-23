use std::num::NonZeroUsize;
use std::sync::Arc;

use br_agentic_contract::{
    AgentLabel, IncomingContext, IncomingItem, IncomingPort, InstanceId, PortError, ThreadRef,
};
use br_runner_jobs::{MarkGate, NodeEntry, mark_gate};
use br_runner_jobs_client::{AgentBook, BookLookup, Gate, book_lookup};

pub fn lookup_of(book: Arc<AgentBook>) -> Arc<dyn BookLookup> {
    book_lookup(move |_instance| {
        let book = Arc::clone(&book);
        async move { Ok(book) }
    })
}

pub fn store_then_release() -> MarkGate {
    mark_gate(|_thread, _entry| async { Gate::Stored })
}

pub fn build_node_entry(book: Arc<AgentBook>, label: AgentLabel) -> NodeEntry {
    NodeEntry::with_inbox("analyst", lookup_of(book), label, store_then_release())
}

pub async fn read_the_incoming_items(
    entry: &NodeEntry,
    instance: InstanceId,
    thread: ThreadRef,
    label: &AgentLabel,
    book: &AgentBook,
) -> Result<usize, PortError> {
    let agent = entry.agent_id(book, label)?;
    let context = IncomingContext {
        instance,
        thread,
        agent,
    };
    let incoming = entry
        .incoming_port()
        .ok_or_else(|| PortError::fault("no inbox"))?;
    let batch = incoming
        .items(&context, None, NonZeroUsize::new(20).expect("a limit"))
        .await?;
    for item in &batch.items {
        if let IncomingItem::Mail(_) = item {
            incoming.mark_read(&context, item.id()).await?;
        }
    }
    Ok(batch.items.len())
}

fn main() {
    println!("agent_and_incoming shows the node entry, the agent id, the listing and the mark");
}
