# br-runner-jobs

The adapters that bind the jobs service to the ports of a runner. It gives the adapter of the agent port (`AgentPortAdapter`) and the adapter of the incoming port (`IncomingPortAdapter`), the two node ports of `br-agentic-contract`, for a runner that builds the ports of a node itself with no harness; the adapter of the mail port of `br-agentic-harness-contract` (`MailPortAdapter`), for a runner that uses the harness; the node entry that makes them (`NodeEntry`); and one seam, the mark gate (`MarkGate`, with the ready gate `mark_gate_stored` and the maker `mark_gate`), which serves the incoming adapter only. It holds no client: the typed client of the jobs service, the Work, the delivery queue of a Run, the agent book, the caller handle, the mail handle, the feeds and their errors are the library `br-runner-jobs-client`, and this crate depends on it. In 0.2.0 this crate held all of that; the move is the split of 0.3.0. A runner takes the incoming adapter or the mail adapter for an agent, according to whether it uses the harness; the agent port is the same in both cases.

## Install

```toml
br-runner-jobs = { git = "https://github.com/BotResources/br-jobs-runner-kit", package = "br-runner-jobs", tag = "br-runner-jobs/v0.4.0", version = "0.4.0" }
br-runner-jobs-client = { git = "https://github.com/BotResources/br-jobs-runner-kit", package = "br-runner-jobs-client", tag = "br-runner-jobs-client/v0.1.1", version = "0.1.1" }
br-agentic-contract = { git = "https://github.com/BotResources/br-llm-graph-rs", package = "br-agentic-contract", tag = "br-agentic-contract/v0.3.0", version = "0.3.0" }
br-agentic-harness-contract = { git = "https://github.com/BotResources/br-llm-graph-rs", package = "br-agentic-harness-contract", tag = "br-agentic-harness-contract/v0.1.0", version = "0.1.0" }
br-llm-messages = { git = "https://github.com/BotResources/br-llm-messages-rs", package = "br-llm-messages", tag = "br-llm-messages/v2.1.0", version = "2.1.0" }
```

A crate that names the types of a sibling in its public API must reach that sibling through the same git url and the same tag string, or the types are two copies: the pins above are the ones this crate is built with. `BookLookup`, `book_lookup`, `AgentBook`, `Gate` and `Error` are types of `br-runner-jobs-client`; this crate re-exports none of them.

## Quickstart

This is `examples/agent_and_incoming.rs`. It makes a node entry for a node that has an inbox from the book lookup, the agent label and a mark gate, gets the agent id from the entry, lists the unread items of that agent through the adapter of the incoming port, and marks each mail as read.

```rust,no_run
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
```

This is `examples/agent_and_mail.rs`. It makes the node entry of a runner that uses the harness, from the book lookup and the agent label, with no mark gate. It lists the items through the adapter of the mail port, gives the documents of each direct mail, sends a reply and marks each item as read.

```rust,no_run
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
```

## Surface

| Item | What it is |
|---|---|
| `NodeEntry::new(node, lookup)` | the entry of a node with no inbox; `incoming_port()` and `mail_port()` give `None` |
| `NodeEntry::with_inbox(node, lookup, label, gate)` | the entry of a node that has an inbox, for a runner with no harness; the gate is required; it gives the incoming adapter and no mail adapter |
| `NodeEntry::with_mail(node, lookup, label)` | the entry of a node that has an inbox, for a runner that uses the harness; it takes no gate (never pass `mark_gate_stored`); it gives the mail adapter and no incoming adapter |
| `NodeEntry::agent_port()` | the `AgentPortAdapter` |
| `NodeEntry::incoming_port()` | the `IncomingPortAdapter`, for an entry made with a gate |
| `NodeEntry::mail_port()` | the `MailPortAdapter` (the `MailPort` of `br-agentic-harness-contract`), for an entry made without a gate |
| `NodeEntry::agent_id(book, label)` | the agent id of a key in a book, the same agent as the agent port gives |
| `MarkGate`, `GateFuture` | an async function of (thread reference, entry id) that ends `Stored` or `Dropped` (`Gate` of `br-runner-jobs-client`) |
| `mark_gate(f)` | makes a `MarkGate` from an async function |
| `mark_gate_stored()` | a gate that ends `Stored` at once and waits for no store |

## Rules the code keeps

**Agent port**

- It gives the identity of the agent of an instance id and a label, at once, and waits for nothing of jobs. It gives no author name.
- The first call for a key mints the id (UUIDv7) in the agent book, makes the agent with an inbox for the label that holds the inbox and without one for any other, and puts its registration in the delivery queue. Calls for one key at the same time get the one agent. A later call gives the same id, also for an agent that the book was made with at a restart.
- The call fails only for an error of the book lookup, as a fault of the instance.

**Incoming port**

- The context holds the instance, the thread and the agent id. A call for an agent that does not hold the inbox of the node entry in the book of the instance is an error of the port (a fault, not a refusal). No error of this port is a fault of the instance.
- Listing reads the unread items of the address of the agent, in the order in which jobs gives them, in the batches and with the cursor that the port states. Jobs gives the items oldest first by the sequence number it gives each item when it stores it, which is not the order of the item ids; the adapter keeps that order and never sorts or compares item ids. One listing makes one read of the unread items and one read of the mail for each new direct mail (to get its recipients), all inside the wait of the call; a listing that gets no answer is an error of the port. The cursor holds the seq (the sequence number of jobs) and the item id of the last item of the page. When the item of the cursor is no longer among the unread items and the adapter holds no earlier read, the listing goes on from the position that the item held in the order of jobs: it gives the items of a fresh read whose seq is above the seq of the cursor, and never compares item ids; the instance skips an item its conversation holds. It filters none by thread. A direct mail is the mail item of a recipient in `to`, a cc notice the mail item of a recipient in `cc`, a call notice and a mail notice a runtime notice with the item id as its entry id. The adapter gives no human message.
- A direct mail with no source conversation, a blank notice or author, an id that is not a UUIDv7, a cursor that is not of the form `{seq}:{item_id}` (a plain item id is one), an item or a call notice of a kind that this library does not know, and an unreadable item are errors of the port. A listing that gets no answer from jobs is an error of the port.
- An item of an unknown kind or a call notice of an unknown type from jobs is an error of the port. The adapter never gives a default text for it.
- The listing leaves out every item that this agent marked through its book, for the life of the book. The inbox of a new agent whose registration is still queued is empty and calls jobs for nothing; an agent that the book was made with at a restart is read at once.
- Marking returns at once. The key is the agent id and the entry id (the mail id for a mail item, the item id for a notice). The adapter maps it to the item id of jobs with the map of the book. An unknown pair, or an item already marked, is not an error. The marker waits for the gate in the delivery queue, outside its order: `Stored` joins it to the end of the queue, `Dropped` removes it after a report and the item stays unread in jobs, out of the listing, and a later Run gives it again.
- For a call notice "Run completed" of a child Job, the finish of that child Job is handed over into the queue first (command id: the item id), then the marker is held, so a drained call cannot pass the marker and lose the finish.

**Mail port**

- The context is the `IncomingContext`; a call for an agent that does not hold the inbox of the node entry is an error of the port (a fault), for all four operations. The adapter has no operation to wait for an item.
- Listing and marking are those of the incoming adapter: the same items, batches, cursor, entry ids and book map. An item that one adapter listed can be marked through the other.
- Marking returns at once and does not wait for jobs. The marker joins the delivery queue with no gate: the harness tells the mail port to mark a mail only after the stored signal of its history port answered `Stored`, and this crate does not read that signal. A mail that the harness does not mark stays unread in jobs. For a call notice "Run completed" the finish of the child Job is handed over first, then the marker, before the call returns. An item already read or not held is not an error.
- Sending returns when the adapter accepted the mail, which is at once, not when jobs has it. The mail value carries no sender (`NewMail` has no sender field in `br-agentic-harness-contract` 0.1.0), so the sender rule of the intent has no case to apply, and the adapter always sends as the agent of the context. A mail with no recipient in `to`, or with a recipient listed twice in `to` and `cc` together, is refused with a reason and nothing is handed over. Each document of the mail is sent as an attachment with the document reference as its text content (`text/plain`). A second send of the same mail id is handed over again; jobs makes the write idempotent by the mail id.
- Documents: one read of the mail and one fetch for each attachment, each bounded by the reply wait of the connection; a call with no answer is an error of the port. Only a mail that has the agent in `to` gives documents; any other mail is refused with a reason. A mail with no attachment gives none.

**Errors and ids**

- The adapters have no error type. Every failure is an error of the port with the error of `br-runner-jobs-client` as its cause. They give no Passport, token or Job id of the Work.

## Limits of this version

- The incoming adapter has no send, no documents and no wait for an item: the incoming port of `br-agentic-contract` 0.3.0 has none. The mail adapter has no wait either.
- A send, a marker and a finish that the delivery queue holds are in memory: a crash before delivery loses them, though the agent was told the mail was sent.
- A restart for an item left unread has no limit: an entry that the thread access always refuses makes the marker `Dropped` in each Run.
- What the gate waits for is the importer's. A gate that ends `Stored` too early can lose a mail on a crash.

## Why

| Thing | Why it is the way it is |
|---|---|
| the crate holds no client | adapters and logic libraries must not depend on each other; they share `br-runner-jobs-client` |
| the node entry is the only way to make an adapter | the node name and the inbox label must be the same for the agent port and the incoming port or the mail port |
| the mail adapter takes no gate | with the harness the stored signal is awaited by the harness before it tells the mail port to mark; a gate here would wait for nothing |
| an entry gives only the adapter of its own use | an entry made with a gate for a runner with no harness must not give a mail adapter that marks with no gate; an entry made without a gate gives no incoming adapter |
| the cursor holds the seq and the item id of the last item | the seq is the order of jobs and the client 0.1.1 gives it, so a gone item still has its position; the item id finds the item when it is still unread; a comparison of item ids is not the order of jobs |
| the adapter keeps the seqs of its kept read | the kept read of the agent book holds items with no seq, and the cursor of the next page needs the seq of its last item |
| a held read marker waits outside the order | the marker waits for the store of its entry, and a gate that takes time must not hold unrelated writes |
| the finish goes in front of its marker | a `drained()` call must not return between the marker and the finish of the call it closes |
| the crate does not name the bus or a subject | the bus is the client's; CI checks that the Cargo.toml of this crate does not list a bus crate directly (the client carries the bus) and that the source names no subject |
| the crate does not depend on `br-runner-threads` | the importer maps the result of `entry_stored` to the gate type |
| an unknown item kind or call notice type is an error of the port | an item that cannot be made into what the port states is an error, never a default text |
