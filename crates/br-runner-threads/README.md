# br-runner-threads

The Threads side of a runner. It gives the adapters of the Threads ports of `br-agentic-contract` (`ThreadPortAdapter` for the thread port, `ThreadAccessAdapter` for the thread access, with the response stream and the result stream it gives) and, under them, `ThreadClient`, a typed client of the Threads service that a runner can also use alone. The client does one operation once. The adapter keeps a durable-by-seam queue for each thread and sends again what Threads did not take for a reason that can pass. It knows no job, run, mail, node or tool.

## Install

```toml
br-runner-threads = { git = "https://github.com/BotResources/br-jobs-runner-kit", package = "br-runner-threads", tag = "br-runner-threads/v0.2.0", version = "0.2.0" }
br-runner-connection = { git = "https://github.com/BotResources/br-jobs-runner-kit", package = "br-runner-connection", tag = "br-runner-connection/v0.1.1", version = "0.1.1" }
br-agentic-contract = { git = "https://github.com/BotResources/br-llm-graph-rs", package = "br-agentic-contract", tag = "br-agentic-contract/v0.2.0", version = "0.2.0" }
br-llm-messages = { git = "https://github.com/BotResources/br-llm-messages-rs", package = "br-llm-messages", tag = "br-llm-messages/v2.1.0", version = "2.1.0" }
br-core-kernel = { git = "https://github.com/BotResources/br-rust-common", package = "br-core-kernel", tag = "br-core-kernel/v0.1.0", version = "0.1.0" }
uuid = { version = "1", features = ["v7"] }
async-nats = "0.48"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

A crate that names the types of a sibling in its public API must reach that sibling through the same git url and the same tag string, or the types are two copies: the pins above are the ones this crate is built with.

## Quickstart

This is `examples/runner_wiring.rs`. It builds the `Connection`, the `ThreadClient` and the `ThreadAccessAdapter`, writes an input turn that holds a runtime notice (Threads refuses a human message held in an input turn, see Limits), reads the mark of the mail with `entry_stored`, and ends the agent with `instance_ended`.

```rust,no_run
use std::sync::Arc;
use std::time::Duration;

use br_agentic_contract::{PortError, ThreadAccess, ThreadRef};
use br_core_kernel::ServiceAccountId;
use br_llm_messages::{AgentId, Author, EntryId, IncomingEntry, InputTurn, RuntimeNotice, TurnId};
use br_runner_connection::{Connection, FixedToken};
use br_runner_threads::{
    EntryOutcome, Error, IgnoreReports, MemoryQueueStore, NewThread, ThreadAccessAdapter,
    ThreadAccessSettings, ThreadClient, WriteContext, WriteContextFn,
};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    Read,
    LeaveUnread,
}

pub fn build_connection(
    endpoint: &str,
    token: &str,
    bus: async_nats::Client,
    service_account: ServiceAccountId,
) -> Result<Connection, br_runner_connection::Error> {
    Connection::new(
        endpoint,
        Arc::new(FixedToken::new(token)),
        bus,
        service_account,
        Duration::from_secs(30),
    )
}

pub async fn build_thread_access(
    client: &ThreadClient,
    agent: AgentId,
    author: Author,
) -> Result<ThreadAccessAdapter, Error> {
    let write_context = WriteContextFn::new(move |_thread| {
        let context = WriteContext {
            agent,
            author: author.clone(),
            job_id: None,
            acting_user_id: None,
            impersonator_id: None,
        };
        async move { Ok::<_, PortError>(context) }
    });
    let settings = ThreadAccessSettings {
        outcome_wait: Duration::from_secs(10),
        retry_wait: Duration::from_secs(1),
        store: Arc::new(MemoryQueueStore::new()),
        observer: Arc::new(IgnoreReports),
        write_context,
    };
    ThreadAccessAdapter::new(client, settings).await
}

pub async fn node_entry(
    adapter: &ThreadAccessAdapter,
    thread: &ThreadRef,
    mail: EntryId,
    turn: InputTurn,
) -> Result<Mark, PortError> {
    adapter.add_input_turn(thread, turn).await?;
    Ok(match adapter.entry_stored(thread, mail).await {
        EntryOutcome::Stored => Mark::Read,
        EntryOutcome::Dropped => Mark::LeaveUnread,
    })
}

pub async fn lifecycle_hook(adapter: &ThreadAccessAdapter, ended_agents: &[AgentId]) {
    for agent in ended_agents {
        adapter.instance_ended(*agent).await;
    }
}

pub fn mail_turn(text: &str) -> Result<(EntryId, InputTurn), br_llm_messages::Error> {
    let mail = EntryId::from_uuid(Uuid::now_v7())?;
    let notice = RuntimeNotice::new(mail, text)?;
    let turn = InputTurn::new(
        TurnId::from_uuid(Uuid::now_v7())?,
        vec![IncomingEntry::from(notice)],
    )?;
    Ok((mail, turn))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let [_, endpoint, nats_url, token, account] = args.as_slice() else {
        return Err("usage: runner_wiring <endpoint> <nats-url> <token> <account-uuid>".into());
    };
    let bus = async_nats::connect(nats_url).await?;
    let connection = build_connection(endpoint, token, bus, ServiceAccountId(account.parse()?))?;
    let client = ThreadClient::new(&connection, Duration::from_secs(10)).await?;
    let agent = AgentId::from_uuid(Uuid::now_v7())?;
    let adapter = build_thread_access(&client, agent, Author::new("Analyst")?).await?;
    let thread = ThreadRef::from_uuid(Uuid::now_v7())?;
    let creation = NewThread {
        command_id: Uuid::now_v7(),
        thread_id: thread.as_uuid(),
        owner_id: None,
        member_ids: Vec::new(),
        title: None,
        resources: Default::default(),
        origin: None,
        acting_user_id: None,
        wait: Duration::from_secs(10),
    };
    client.create_thread(&creation).await?;
    let (mail, turn) = mail_turn("Please summarize the open tickets")?;
    let mark = node_entry(&adapter, &thread, mail, turn).await?;
    lifecycle_hook(&adapter, &[agent]).await;
    println!("the mail is marked: {mark:?}");
    Ok(())
}
```

The other examples are in `examples/` and run in the tests: `runner_ports` (the two adapters given to a node builder, the concrete adapter kept by the runner), `agent_turn_through_adapter` (a turn through the thread queue, one streamed response completed and one discarded), `queue_not_draining` (`queue_state` and the observer), `close_left_open_turn` (close a message by its id), `client_operations` (the client without a node).

## Surface

| Item | Role |
|---|---|
| `ThreadClient::new(connection, check_wait)` | Builds the client on a `Connection`. Checks the three streams that Threads uses; sends nothing else. |
| `ThreadClient::create_thread`, `write_message`, `open_stream`, `close_message`, `read_context`, `set_title`, `fail_title_job`, `request_upload` | The operations. Each sends once and keeps nothing once it ends, except the stream handle. |
| `StreamHandle`, `StreamWriter` | The open message of a stream and the clones of its write side: `delta`, `entry`, `annotation`, `keepalive`, `response(entry)`. |
| `ResponseSink`, `ResponseControl` | The stream sink of one model response over a handle, and the object that completes or discards it. |
| `Closing`, `ClosureKind` | A closing request: Completed, BudgetExhausted, Failed, Interrupted or Cancelled. |
| `ThreadAccessAdapter::new(client, ThreadAccessSettings)` | The adapter of the thread access port, with the thread queue. Also `queue_state()`, `entry_stored(thread, entry)` and `instance_ended(agent)`, which are not methods of the port. |
| `ThreadPortAdapter::new(find)` | The adapter of the thread port: `find` gives the thread id of a snapshot. |
| `WriteContext`, `WriteContextFn` | The seam that gives agent, author, job, acting user and impersonator for the writes of a thread. |
| `QueueStore`, `MemoryQueueStore`, `StoredItem` | The seam that keeps the items of the thread queue (`append`, `list`, `remove`) and its in-memory implementation. |
| `QueueObserver`, `QueueReport`, `ReportKind`, `ItemKind`, `IgnoreReports` | The seam that receives the reports of the thread queue. |
| `QueueState`, `ThreadQueueState`, `OldestItem`, `FailedAttempt` | The state of the thread queue, read at once. |
| `Error`, `Failure`, `Cause`, `Refusal`, `CredentialFailure`, `CredentialKind` | The single error type and its parts. All are `#[non_exhaustive]`: match with a final `_` arm. |
| `test_double::MemoryThreads`, `test_double::refusal`, `test_double::failure` (feature `test-double`) | A client over memory that records the commands, the chunks and the GraphQL operations and answers as the caller scripts; `refusal(code, message, command_id)` and `failure(cause, text)` build the `Refusal` and the `Failure` that a script returns. |

## Bus access

The commands and their outcomes go through `br-util-nats-fabric` on the bus connection of the `Connection`: the client gives the Fabric typed coordinates only and writes no subject, stream or bucket name. Chunks go on the streaming stream of `contract-threads` as `ChunkFrame` messages with a message id; `async-nats` is named in one module only, `src/chunk_lane/`. At building the client checks that the command stream, the event stream and the streaming stream exist: a missing one is `InfraMissing`, and a check that does not end within the check wait is `Unavailable`. This crate creates no stream, no bucket and no consumer: the outcome wait is a plain subscription on the event subjects of the event stream.

## Rules the code keeps

- A wait that is zero is `InvalidInput`. A command id, the id of a new thread, an annotation id or an artifact id that is not a UUIDv7 is `InvalidInput`.
- A command is published once with its command id and the client waits for its outcome: a confirmation is the result, a refusal is `Refused` with the code, the parameters and the command id. A refusal read from a command outcome has an empty message and no reason. A command published while its stream is missing is `Unavailable`; a command whose outcome did not come within the wait is `Lost`; a command whose outcome cannot be read as a known one, a refusal code this crate does not know included, is `Unreadable`.
- A chunk is numbered from 0 by the handle, in the order the calls take it, with no gap even when many tasks write; it returns when the bus has taken it. A keepalive is numbered like any other chunk and is counted in the seal. A chunk the bus did not take within the wait breaks the handle: that chunk and every later one is `StreamBroken`.
- Closing table: `Completed` sends a complete, with the seal of the chunks, and needs a handle that holds at least one chunk; `BudgetExhausted`, `Failed` and `Interrupted` send a fail; `Cancelled` sends a cancel. The seal is sent for the other kinds only when the handle is healthy and holds a chunk. The commands carry no closure and no detail: the detail of a `Failed` is checked and not sent.
- A closing goes after every chunk sent before it. After it the handle, its writers and its sinks take no chunk (`StreamClosed`). A closing that Threads refuses, or that ends `Unavailable`, frees the handle again; a closing that is waiting, lost or unreadable keeps the handle closed to chunks, and a new closing can still be sent; a confirmed closing spends the handle and a second closing is `StreamClosed`.
- The thread queue of the adapter is first in, first out for each thread, with one delivery task for each thread: a stuck thread blocks its own later items and no other thread. Every write operation of the port returns when the item is in the store; none waits for Threads. On one thread, `complete_response`, `open_response`, `open_result`, `close_turn`, `add_entry`, `complete_result`, `add_input_turn` and `discard_response` run one at a time: no other of these operations goes between the deltas that `complete_response` queues. The write-context function of an operation is called before the operation takes its place on the thread, so it may itself write on the same adapter and thread. `read` waits until the items accepted on that thread before it have ended, leftovers of the store included, with no limit, then asks Threads for the context of the thread with the thread id and no agent; `entry_stored` and `instance_ended` wait in the same way (see below). The adapter sends no parent message id with any message it writes, so Threads reads the thread from its latest message alone.
- Sending again: a command is sent again, with the same command id, on `Unavailable`, `Lost` and `Unreadable`; a chunk is sent again, with the same number, on `StreamBroken`. Any other error is final: the item is removed from the store, reported as `ItemRefused`, and the entries it held are `Dropped`. A refused opening drops the later items of its message unsent (`ItemDroppedUnsent`). A command that Threads confirms as `FAILED` (a closing, a whole message or an opening) is delivered and reported as `ClosingFailed`, with the kind of the item in the report; a closing that ends for good with an error is reported as `ClosingFailed` too. `queue_state` gives `last_failure` of a thread until an item of that thread is delivered; delivery clears it.
- `entry_stored` gives `Stored` when the item that holds the entry has ended without a refusal (and for a thread or an entry that the adapter holds nothing about), and `Dropped` when an item of that entry was removed after a report. For a command, `Stored` means that Threads took it. For a chunk, `Stored` means only that the bus took it: a chunk that Threads later refuses is known only when a closing that carries a seal is refused. `instance_ended(agent)` waits for the items accepted before the call on the threads that the agent used, then drops the states and open objects of that agent; it queues nothing.
- A response sink that is re-opened with the same entry, or discarded, queues the delta that ends the earlier response as interrupted. A response that is completed queues the deltas that the draft still lacks, then the completion, and the result is checked by folding the deltas on a copy of the draft.
- Dropping the adapter stops its tasks and reports every waiting item as `Unsent`; the items stay in the store. An item whose append to the store ends after the drop is reported as `Unsent` and is not delivered. A write that a response sink still holds after the drop is refused with `Store`.
- Building the adapter fails with `Store` when the store cannot list, when a kept item cannot be read, or when a kept item does not match the id it is kept under; nothing is sent or removed in that case.
- The adapter never reads the conversation back except in `read`. An error that this crate produces reaches the caller as `PortError::Fault` with the error of this crate as its cause; the errors of the write-context function and of the thread function of `ThreadPortAdapter` reach the caller as they are, `PortError::Refused` included.
- The dependency graph of the crate holds `br-llm-clients-core`, through `br-agentic-contract`, and no other library of the `br-llm-clients` family.

## Limits of this version

| What | State |
|---|---|
| Chunk errors | The chunk path gives `StreamBroken` or `InvalidInput` only, never `Unavailable`. |
| Closure on the wire | The complete, fail and cancel commands this crate sends carry no closure reason and no detail. The detail of a `Failed` closing is checked and not sent. Threads reads a `BudgetExhausted` or an `Interrupted` closing as failed, so a conversation read after a restart shows such a closing as failed and not as `BudgetExhausted` or `Interrupted`. |
| Keepalive | `keepalive` publishes a keepalive chunk; when to send one is the caller's. The adapter sends none, although the client has `keepalive`: when a tool call lasts longer than the silence time of Threads on a thread queue with no item, Threads fails the open message and refuses the items delivered later, and this is learned only when a closing that carries a seal is refused. |
| Read and restart | The adapter sends no parent message id, so Threads reads a thread that the adapter wrote from its latest message alone. `read`, a restart included, gives that conversation. |
| What `read` omits | `read` gives the context as Threads gives it for the whole active branch. It leaves out annotations, cancelled messages, interrupted responses, messages that failed before their first entry and deleted messages. |
| Discard | `discard_response` of a response whose sink queued a delta keeps the partial response in its message, ended as interrupted; `read` leaves it out, so a response dropped by `discard` never comes back in `read`. A response whose sink queued no delta leaves nothing. |
| Entry id of an interrupted response | A response opened again or completed with the entry id of a response that was interrupted is sent as given. If Threads refuses it, this is known only when a closing that carries a seal is refused. |
| Writer of an input turn | The message of an input turn is written under the agent and the author name of the `WriteContext` of its thread, not under the author of the input. |
| Human message in an AI turn | A human message given as an entry of an AI turn is sent as given. |
| Seal refused | A closing whose seal does not match what Threads took in is refused and reported as `ClosingFailed`. The message stays open until the silence rule of Threads fails it or a closing by its message id is sent through the client. |
| One adapter, one store | One adapter, with one store, serves the whole runner process. A store serves one adapter at a time: an adapter built on a store sends every item the store holds, those of another running adapter on it included. |
| Port future dropped | With a durable store, dropping a port future while the append of its item is in flight can leave the item in the store but not in the thread queue or in the chunk count of the adapter. The item is not sent before the next restart, and the next chunk of that message may reuse its number. |
| Chunk result | `Stored` from `entry_stored` for a chunk means only that the bus took it. A chunk that Threads later refuses is known only when a closing that carries a seal is refused. |
| Chunk sent twice | A chunk is sent again under the same number after `StreamBroken`. The result of a number that reaches Threads twice is unknown. |
| Outcome lost | A command that Threads applied but whose outcome was `Lost` is sent again. If Threads then answers `Refused`, the refusal is final. For an opening, the later chunks and the closing of that message are dropped unsent. |
| Unknown refusal code | A refusal code that this crate does not know reads as `Unreadable`. It is sent again with no maximum, so the thread queue stays blocked on it until the importer's side changes. |
| Input turn with a human message | Threads refuses a human message held in an input turn; the refusal is final and the entries are `Dropped`. |
| Turn opened elsewhere | A turn opened by another instance or another process has no state in this adapter. The adapter queues an opening for it, Threads refuses that opening, and the later items of that turn are dropped unsent. |
| Re-open refused | When the re-open of a response is refused with `Store`, the earlier response stays open and its sink drops the deltas it is sent later. |
| Left open turn | A turn left open by `instance_ended` stays streaming until a closing by its message id or until the silence rule of Threads fails it. |
| Kept items | The stored bytes of a queue item are not stable across versions of this crate. |
| Upload | `request_upload` returns the access as Threads gives it and uploads nothing. |
| Missing command stream with a core listener | When the command stream is missing and a core NATS subscriber listens on the command subjects, the publish is not acknowledged and the command ends `Lost`, not `Unavailable`. |
| Wait while Threads is down | `read`, `entry_stored` and `instance_ended` wait with no limit for the items of the thread queue to end. |
| Store | `MemoryQueueStore` keeps nothing across a restart; a durable backend is a `QueueStore` the importer gives. |

## Why

| Thing | Why it is the way it is |
|---|---|
| `async-nats` in `chunk_lane` | A chunk frame goes on the streaming stream with a message id, and the acknowledgment is awaited inside the outcome wait; the one module that names `async-nats` keeps this out of the rest of the crate, and every other bus use goes through the Fabric |
| closing table in the client | The adapter and a direct importer close in the same way; the caller gives a reason and the client picks the command |
| one delivery task for each thread | The order of items of a thread is the order of the chunks and of the closing; threads do not wait for each other |
| unknown `br-runner-connection` error is `Lost` | The Connection reports `Unavailable` when nothing was sent and `Lost` when the request may have reached Threads. A variant of its error that this crate does not know cannot be placed on that line, so it is read as `Lost`: the command may have reached Threads, and it is sent again under the same command id |
| `Drop` reports `Unsent` | The items stay in the store, so a restarted adapter sends them; the observer learns that this instance did not |
| `instance_ended` is not in the port | The port of the contract has no end-of-life call; a runner can keep the concrete adapter and call it from its lifecycle hook |
