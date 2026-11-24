use std::num::NonZeroUsize;
use std::sync::Arc;

use br_agentic_contract::{
    AgentLabel, IncomingContext, IncomingItem, InstanceId, MailItem, NewMail, PortError, ThreadRef,
};
use br_agentic_harness_contract::MailPort;
use br_runner_jobs::NodeEntry;
use br_runner_jobs_client::{AgentBook, BookLookup, book_lookup};

pub fn lookup_of(book: Arc<AgentBook>) -> Arc<dyn BookLookup> {
    book_lookup(move |_instance| {
        let book = Arc::clone(&book);
        async move { Ok(book) }
    })
}

pub fn build_node_entry(book: Arc<AgentBook>, label: AgentLabel) -> NodeEntry {
    NodeEntry::with_mail("analyst", lookup_of(book), label)
}

pub async fn read_send_and_mark(
    entry: &NodeEntry,
    instance: InstanceId,
    thread: ThreadRef,
    label: &AgentLabel,
    book: &AgentBook,
    reply: NewMail,
) -> Result<usize, PortError> {
    let agent = entry.agent_id(book, label)?;
    let context = IncomingContext {
        instance,
        thread,
        agent,
    };
    let mail = entry
        .mail_port()
        .ok_or_else(|| PortError::fault("no inbox"))?;
    let batch = mail
        .items(&context, None, NonZeroUsize::new(20).expect("a limit"))
        .await?;
    for item in &batch.items {
        if let IncomingItem::Mail(MailItem::To { id, .. }) = item {
            let documents = mail.documents(&context, *id).await?;
            println!("{} documents", documents.len());
        }
    }
    mail.send(&context, reply).await?;
    for item in &batch.items {
        mail.mark_read(&context, item.id()).await?;
    }
    Ok(batch.items.len())
}

pub fn main() {
    println!(
        "agent_and_mail shows the node entry for the harness, the listing, send, documents and mark"
    );
}
