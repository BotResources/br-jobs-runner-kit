use std::num::NonZeroUsize;
use std::sync::Arc;

use br_agentic_contract::{
    AgentLabel, InboxContext, InboxItem, InboxPort, InstanceId, MailId, NewMail, PortError,
    ThreadRef,
};
use br_llm_messages::{Author, ConversationId};
use br_runner_jobs::{AgentBook, BookLookup, Gate, MarkGate, NodeEntry, book_lookup, mark_gate};

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

pub async fn read_the_inbox(
    entry: &NodeEntry,
    instance: InstanceId,
    thread: ThreadRef,
    label: &AgentLabel,
    book: &AgentBook,
) -> Result<usize, PortError> {
    let agent = entry.agent_id(book, label)?;
    let context = InboxContext {
        instance,
        thread,
        agent,
    };
    let inbox = entry
        .inbox_port()
        .ok_or_else(|| PortError::fault("no inbox"))?;
    let batch = inbox
        .items(&context, None, NonZeroUsize::new(20).expect("a limit"))
        .await?;
    for item in &batch.items {
        if let InboxItem::Mail(_) = item {
            inbox.mark_read(&context, item.id()).await?;
        }
    }
    Ok(batch.items.len())
}

pub async fn send_a_mail(
    entry: &NodeEntry,
    context: &InboxContext,
    conversation: ConversationId,
    to: Vec<br_llm_messages::AgentId>,
    author: Author,
) -> Result<(), PortError> {
    let inbox = entry
        .inbox_port()
        .ok_or_else(|| PortError::fault("no inbox"))?;
    let mail = NewMail {
        id: MailId::from_uuid(uuid::Uuid::now_v7()).map_err(PortError::fault)?,
        author,
        source: conversation,
        to,
        cc: Vec::new(),
        subject: "Open tickets".to_owned(),
        body: "Please summarize the open tickets".to_owned(),
        attachments: Vec::new(),
    };
    inbox.send(context, mail).await
}

fn main() {
    println!("agent_and_inbox shows the node entry, the listing, the mark and the send");
}
