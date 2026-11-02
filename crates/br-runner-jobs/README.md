# br-runner-jobs

The jobs side of a runner. It gives `JobsClient`, a typed client of the jobs service (triggers, claims, presence, cancel orders, dispatch mode); `Work`, one received Run with every call that belongs to it; `DeliveryQueue`, a queue of writes for each Run that sends again what jobs did not take for a reason that can pass; `AgentBook`, `CallerHandle` and `MailHandle`; and the adapters of the agent port and of the inbox port of `br-agentic-contract` (`AgentPortAdapter` and `InboxPortAdapter`, both given by `NodeEntry`). The client does one operation once. The delivery queue is the only part that sends a write again. The crate knows no graph, no agent loop, no thread and no tool: the importer decides when to call.

## Install

```toml
br-runner-jobs = { git = "https://github.com/BotResources/br-jobs-runner-kit", package = "br-runner-jobs", tag = "br-runner-jobs/v0.2.0", version = "0.2.0" }
br-runner-connection = { git = "https://github.com/BotResources/br-jobs-runner-kit", package = "br-runner-connection", tag = "br-runner-connection/v0.1.1", version = "0.1.1" }
br-agentic-contract = { git = "https://github.com/BotResources/br-llm-graph-rs", package = "br-agentic-contract", tag = "br-agentic-contract/v0.2.0", version = "0.2.0" }
br-llm-messages = { git = "https://github.com/BotResources/br-llm-messages-rs", package = "br-llm-messages", tag = "br-llm-messages/v2.1.0", version = "2.1.0" }
br-core-kernel = { git = "https://github.com/BotResources/br-rust-common", package = "br-core-kernel", tag = "br-core-kernel/v0.1.0", version = "0.1.0" }
br-core-auth = { git = "https://github.com/BotResources/br-rust-common", package = "br-core-auth", tag = "br-core-auth/v0.1.0", version = "0.1.0" }
contract-jobs = { git = "https://github.com/BotResources/svc-jobs", package = "contract-jobs", tag = "contract-jobs/v0.8.0", version = "0.8.0", features = ["integration"] }
uuid = { version = "1", features = ["v7"] }
serde_json = "1"
async-nats = "0.48"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

A crate that names the types of a sibling in its public API must reach that sibling through the same git url and the same tag string, or the types are two copies: the pins above are the ones this crate is built with. `contract-jobs` is the wire contract of the jobs service; the crate takes every subject, stream, bucket, frame and id of that wire from it.

## Quickstart

This is `examples/run_one_work.rs`. It builds the `Connection` and the `JobsClient`, opens the trigger feed and reads one `Work`, makes the delivery queue (its first write is the started fact), acknowledges the trigger, makes the agent book and a node entry and takes one agent id from it (its registration joins the queue), reads the payload, hands over the answer and the saved state (which names that agent), hands over the completed fact, and waits until the queue holds nothing.

```rust,no_run
use std::sync::Arc;
use std::time::Duration;

use br_agentic_contract::AgentLabel;
use br_core_kernel::ServiceAccountId;
use br_runner_connection::{Connection, FixedToken};
use br_runner_jobs::{
    AgentBook, Content, DeliveryQueue, Error, IgnoreReports, JobsClient, NodeEntry, QueueSettings,
    Removal, RunnerType, Work, book_lookup,
};
use serde_json::json;
use uuid::Uuid;

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

pub async fn build_client(
    runner_type: RunnerType,
    instance_key: &str,
    connection: Connection,
) -> Result<JobsClient, Error> {
    JobsClient::new(
        runner_type,
        instance_key,
        Duration::from_secs(10),
        connection,
    )
    .await
}

pub async fn start_queue(work: &Work) -> Result<DeliveryQueue, Box<dyn std::error::Error>> {
    let settings = QueueSettings {
        start_id: Uuid::now_v7(),
        outcome_wait: Duration::from_secs(10),
        retry_wait: Duration::from_secs(1),
        observer: Arc::new(IgnoreReports),
    };
    let (queue, started) = work.delivery_queue(settings).await?;
    started.await.map_err(removal_text)?;
    Ok(queue)
}

pub fn node_entry_of(book: &Arc<AgentBook>) -> NodeEntry {
    let book = Arc::clone(book);
    NodeEntry::new(
        "analyst",
        book_lookup(move |_instance| {
            let book = Arc::clone(&book);
            async move { Ok(book) }
        }),
    )
}

pub async fn run_one_work(work: Work) -> Result<String, Box<dyn std::error::Error>> {
    let queue = start_queue(&work).await?;
    work.ack().await?;
    let book = Arc::new(work.agent_book(&queue, Vec::new(), false)?);
    let label = AgentLabel::new("writer")?;
    let agent = node_entry_of(&book).agent_id(&book, &label)?;
    let payload = work.payload().await?;
    let answer = Content {
        message: format!("received: {}", payload.message),
        attachments: Vec::new(),
    };
    let answered = queue.answer(answer);
    let saved = queue.saved_state(json!({ "steps_done": 1, "agents": [agent.as_uuid()] }));
    answered.await.map_err(removal_text)?;
    saved.await.map_err(removal_text)?;
    queue.completed().await.map_err(removal_text)?;
    let left = queue.drained().await;
    Ok(format!("{} writes left", left.waiting.len()))
}

fn removal_text(removal: Removal) -> String {
    format!("a write was removed: {removal:?}")
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let [
        _,
        endpoint,
        nats_url,
        token,
        account,
        runner_type,
        instance_key,
    ] = args.as_slice()
    else {
        return Err(
            "usage: run_one_work <endpoint> <nats-url> <token> <account-uuid> <runner-type> <instance-key>"
                .into(),
        );
    };
    let bus = async_nats::connect(nats_url).await?;
    let connection = build_connection(endpoint, token, bus, ServiceAccountId(account.parse()?))?;
    let client = build_client(RunnerType::new(runner_type)?, instance_key, connection).await?;
    let mut triggers = client.triggers(Duration::from_secs(30)).await?;
    let work = triggers
        .next()
        .await
        .map_err(|unreadable| unreadable.error)?;
    println!("{}", run_one_work(work).await?);
    Ok(())
}
```

`QueueSettings` has four required fields and no default: the start id, the outcome wait, the wait between two attempts and the observer. A zero wait is `InvalidInput`. An observer that does nothing is `IgnoreReports`.

The other two examples are in `examples/` and run in the tests: `call_a_runner` (upload a payload, create a child Job, open the end wait, read a Result, finish the child Job, all through the `CallerHandle`) and `agent_and_inbox` (a `NodeEntry` with an inbox, the agent id, a listing, a mark behind a gate, and a send).

## Surface

| Item | Role |
|---|---|
| `JobsClient::new(runner_type, instance_key, reply_wait, connection)` | Builds the client of one Instance of one runner type (a `RunnerType`) on a `Connection`. Checks the streams and buckets that the wire needs; sends nothing else. |
| `RunnerType` | The runner type as a value that the wire accepts: `RunnerType::new(text)` is `InvalidInput` for an empty text, a dot, a space, `*`, `>` or a text that is too long. It has `as_str`, `Display`, `FromStr` and `TryFrom<&str>`, and compares with a `str`. `CreateChildInput`, `CreateChild`, `ListedAgent` and `JobsClient::runner_type()` hold one. |
| `JobsClient::triggers(ack_wait)`, `claim(claim_id, wait)` | Receive work: the trigger feed (`TriggerFeed::next` gives a `Work`) or one claim for a free slot (a `Work`, nothing, or `Refused`). |
| `JobsClient::dispatch_modes()`, `cancel_orders()` | Feeds (`Feed<ModeEvent>`, `Feed<CancelEvent>`) of the dispatch mode of the runner type and of the cancel orders. |
| `JobsClient::publish_presence(runner_version, status, capacity)`, `delete_presence()`, `presence_time_to_live()` | The presence of the Instance and the time to live of the presence bucket. Also `runner_type()` and `instance_key()`. |
| `Feed<T>`, `TriggerFeed`, `Unreadable` | The feeds. `Unreadable` is the error event of the trigger feed: the error with the Job id and the Run id when they can be read. |
| `Work` | One received Run. Reads: `job_id`, `run_id`, `attempt`, `config`, `parent_job_id`, `initial_job_id`, `passport`, `triggered_by`, `payload_form`, `call`, `restart`, `unknown_fields`. Calls: `ack`, `give_back`, `terminate`, `payload`, `saved_state`, `read_result`, `cancel_own_job`, `log`. |
| `Work::delivery_queue(settings)`, `mail_handle()`, `arrival_feed()`, `agent_book(queue, restored, can_restart)` | Make the objects of the Run: the queue with the end of its started fact, the mail handle, the arrival feed and the agent book. |
| `DeliveryQueue` | The writes of a Run: `answer`, `saved_state`, `plan_declared`, `step_started`, `completed`, `failed`, `drained`, `state`. Each hand-over returns at once with a `WriteEnd`. |
| `QueueSettings`, `QueueObserver`, `IgnoreReports`, `QueueReport`, `QueueState`, `LastFailure`, `WriteEnd`, `WriteResult`, `Removal`, `WriteName`, `WriteKind`, `Gate` | The settings, the seam that receives the reports, the state read at once, the end of a write, the cause of a removal, the name of a write (kind and ids only) and the result of a mark gate (`Stored` or `Dropped`). |
| `AgentBook`, `AgentKey`, `RestoredAgent`, `BookedAgent` | The agents of one Run by key (node name and agent label). `agents`, `caller_handle`, and the three calls that take the arrival feed into the book: `arrival`, `arrival_ended`, `arrival_open_again`. |
| `CallerHandle`, `CreateChildInput`, `EndWait` | The calls of the caller side: `upload_payload`, `create_child`, `read_result`, `finish`, `cancel`, `end_wait`. `EndWait` is a future of the end of one sync call. |
| `MailHandle` | The mail calls of a Work, made at once and never queued: `send_mail`, `unread`, `read_mail`, `attachment`, `mark_read`, `list_agents`. |
| `ArrivalFeed`, `Arrival` | The feed of arrivals of a Run: an item arrived, the end of a sync call, or an unreadable event. |
| `NodeEntry`, `AgentPortAdapter`, `InboxPortAdapter` | The adapters of the agent port and of the inbox port. `NodeEntry::new` makes an entry with no inbox, `with_inbox` an entry with an inbox label and a mark gate; `agent_port()`, `inbox_port()` and `agent_id(book, label)` give the objects. |
| `BookLookup`, `book_lookup`, `MarkGate`, `GateFuture`, `mark_gate`, `mark_gate_stored` | The seams of the adapters: the async lookup from an instance id to its `AgentBook`, and the mark gate (an async function of thread reference and entry id that ends `Stored` or `Dropped`; `mark_gate_stored()` ends `Stored` at once). |
| `Content`, `ContentAttachment`, `PayloadForm`, `CallMode`, `Restart`, `SavedState`, `ResultRef`, `SyncEnd`, `FailureReportView` | The values of a Run and of a call: content, the three forms of a payload, the call mode, the restart data, the saved state, the reference to a Result and the end of a sync call. |
| `Failed`, `FailureKind`, `LogLevel`, `PresenceStatus`, `Capacity`, `DispatchMode`, `ModeEvent`, `CancelEvent` | The failed fact (the kind is required), the log level, the presence and its capacity (1 or more), and the events of the two feeds. |
| `Mail`, `MailAttachment`, `NewMailInput`, `NewMailAttachment`, `UnreadItem`, `CallNotice`, `CallNoticeKind`, `ItemArrived`, `ItemKind`, `ListedAgent`, `AgentDeclaration` | The mail and agent values. |
| `Error`, `Failure`, `Cause`, `Refusal`, `RefusalReason`, `Operation`, `CredentialFailure`, `CredentialKind` | The single error type and its parts. All are `#[non_exhaustive]`: match with a final `_` arm. `Error::refused(operation, reason, message)`, `Refusal::new`, `Failure::new` and `CredentialFailure::new` make the failures, so that code that stands in for the client (a test of the importer, a double) can build them. `Refusal::reason` is a `RefusalReason`: one case for each word of the wire that `contract-jobs` names (the claim, the runner surface, the commands and the creation) and `MailNotYours`; `Unknown(UnknownWord)` is a word that this version does not know. An `UnknownWord` can only be made by `RefusalReason::from_word` (also `From<&str>` and `From<String>`), which gives the typed case for every known word, so `Unknown` never holds a known word and a `match` on the cases is safe. A `RefusalReason` compares with a `str` and prints as its word. |
| `test_double::*` (feature `test-double`) | `ScriptedJobs` (a `JobsClient` over memory that records the calls as `Call` and answers as the test scripts; the started request is `Call::Started` and is answered by `answer_started`), `trigger_json`, `work_from_trigger`, `Delivery`, `PresenceRecord`, `memory_caller`, `MemoryScript`, `MemoryCall`, `Answer`, `result_ref`, and the types `ChildCommand`, `CreateChild`, `Fact`, `LogRecord` that the recorded calls hold. |

## Bus access

The module `src/runner_transport/` is the one module of the library code that names `async-nats` (examples and tests use it as a client). It carries the `jobs.*` runner transport (the trigger stream, the status and log publishes, the claim request-reply, the `JOBS_CANCEL` and `JOBS_PRESENCE` buckets) and the jobs requests (the registration of agents, the agent list and the read marker, as request-reply) on the bus connection that the `Connection` gives back. The started fact is a request on `jobs.runner.started.{runner_type}` with one reply (`StartedRequest` and `SurfaceReply`). The other facts of a Run are publishes on the status stream, acknowledged by the broker. The subjects, the stream and bucket names, the frames and the ids come from `contract-jobs`: this crate makes none. `integration.*` traffic and the published language go through `br-util-nats-fabric`, which `runner_transport` builds on the bus connection of the `Connection`: the child Job commands (create, finish, cancel) with their outcome events and the dispatch mode. The one exception is the arrival feed: `runner_transport` reads `integration.evt.jobs.run.item_arrived.v2` on the stream `INTEGRATION_EVT` through an ephemeral consumer of its own that starts at new messages (see Limits and Why). Streams and buckets are bound, not created, and a missing one is `InfraMissing`. The one durable consumer that this crate creates is the one of the triggers of the runner type, named `runner_jobs_trigger_` and the runner type. It creates no stream, no bucket and no other durable consumer; the watches, the subscriptions and the consumers that the feeds and the outcome waits open are ephemeral, and the server removes an arrival consumer that nobody pulls after 120 seconds. A server that ran version 0.1.0 keeps the durable arrival consumers that version created, named `runner_jobs_arrivals_{length of the runner type}_{runner type}_{instance key}`; this version neither uses nor deletes them, and an operator deletes them.

## Rules the code keeps

**Making the client and the feeds**

- A runner type or an instance key that the wire does not accept (empty, a dot, a space, `*`, `>`, or too long) is `InvalidInput`: for the runner type at `RunnerType::new`, for the instance key at `JobsClient::new`. So is a zero reply wait, a zero acknowledgement wait and a zero claim wait. The crate reads no environment variable and no file, and has no default for a wait that the importer gives; the two fixed durations of the crate are the look for an outcome before a resend (see the delivery queue) and the 120 seconds after which the server removes an arrival consumer that nobody pulls (see Limits).
- Making the client checks the three streams of the runner wire, the two buckets, the published-language bucket, and the event stream and the command stream of the integration bus. A missing stream or bucket, or a presence bucket with no time to live, is `InfraMissing`. A check that gets no answer within the reply wait, or a bus connection that is not connected, is `Unavailable`.
- Every feed ends only when the importer drops it or with an error. A single event that cannot be read is an `Unreadable` error event and the feed goes on, except on the dispatch mode feed, where an entry that is not JSON gives `Unreadable` and the feed then ends with `Lost`. Any other error ends the feed, and `Feed::next` gives that error again on every later call. A feed whose consumer or watch fails is `Lost`, or `InfraMissing` when its stream or bucket is gone. The crate never opens a feed again; a cut of the bus session is repaired by the transport, and the crate repeats no call for it. The arrival feed has two more ways to end, both `Lost`: its consumer is gone from the server (a deleted consumer, or a cut longer than 120 seconds), and a pull that misses its heartbeat for about 30 seconds while the check of the consumer fails (what a cut of that length does; this path is tested with a scripted pull, not with a real cut).
- The dispatch mode feed gives the mode at open, then each change; it does not repeat a value it just gave. A runner type with no entry, also after the entry is deleted, gives `ModeEvent::NoEntry`. An entry that cannot be read is an `Unreadable` event, never a default mode; an entry that is not JSON is followed by the end of the feed with `Lost`.
- The cancel feed gives the entries that exist, then `ReplayComplete` once for each open (an empty bucket gives it at once), then `OrderMade` and `OrderDeleted`. A key that is not a Run id is ignored. A deletion is reported only after the replay is complete. An order that cannot be read, or that names another Run than its key, is an `Unreadable` event. The crate does not act on an order.
- The trigger feed uses one durable consumer for the runner type, shared by all its Instances: explicit acknowledgement, the acknowledgement wait given at the open, every trigger of the stream from the start, only the triggers of the runner type, no limit on deliveries and no limit on triggers in flight. A consumer that exists already with another configuration is made equal to this one, so the acknowledgement wait of the last Instance that opened the feed applies. The consumer pulls in batches of one message.
- `ack`, `give_back` and `terminate` return when the broker has confirmed the settlement. Jobs fails as permanent the Run of a terminated trigger that has not started; if the advisory of the broker is lost, the Run stays pending. `give_back` and `terminate` take the `Work`. A `Work` that comes from a claim has no trigger: these three calls are `InvalidInput`.
- A trigger that is not JSON, that is not an object, that lacks a field of the wire, that has no Passport or a Passport that cannot be read, or a background call with no caller address, is terminated by the crate and given as an `Unreadable` error event, never as a `Work`. The error event holds the Job id and the Run id when they can be read. If the termination itself fails, nothing is reported for it and the broker delivers the trigger again after the acknowledgement wait. A Passport with a field that the `Passport` type of `br-core-auth` does not know cannot be read. The first error of the trigger consumer ends the feed: `TriggerFeed::next` gives it again afterwards.
- A claim is one request. The caller gives the claim id (not nil) and the wait. The crate waits for the claim wait plus the reply wait of the bus; no reply by then is `Lost`. The reply is a `Work`, nothing (`Ok(None)`), or `Refused` for the claim with the reason as the wire names it and an empty message; a busy refusal carries the delay in `retry_after_ms`. A reply that is not a JSON object or that cannot be read is `Unreadable`. A new claim is the importer's choice.
- A `Work` keeps the fields of the trigger, or of the `run` object of a claim reply, that the crate does not know (`unknown_fields`). Unknown fields at the top of a claim reply are not kept. The wire version of a trigger or of a claim reply is not checked.

**Work**

- A `Work` comes only from a trigger or from a claim reply that the client received. No public constructor makes one from raw values, except the test double. Every call of a `Work` carries the Job and the Run of that `Work`; the caller cannot give others.
- The Passport is the whole Passport as `br-core-auth` defines it. `payload`, `saved_state` and `read_result` are GraphQL reads; the crate does not check the form of the payload, the restart flag or the call mode before the call and gives the reply of jobs as it is. `read_result` takes the reference that the end of a sync call gave and a keep flag that the importer sets. A read is not sent again.
- `log` publishes a line on the bus without waiting for the broker. A failure to send is returned and the line is not published again. The level is typed. The step index and the id of the line are optional and come from the importer.
- `cancel_own_job` sends the cancel command of the Job of the `Work` with a command id that the importer gives, and returns when jobs accepts it or refuses it with its reason. It does not go through the delivery queue.
- `delivery_queue` is made once for a `Work`, and `agent_book` once: a second making is `InvalidInput`. Making the queue needs an async runtime, and hands over the started fact as its first write. A zero wait and a missing runtime are `InvalidInput` and spend nothing: the queue can be made again.

**Delivery queue of a Run**

- The queue holds the writes of one Run in hand-over order: the started fact, the registrations of agents, the mails and the released read markers of the adapters, the uploads, creates, finishes and cancels of child Jobs, the answer, the saved state, and the plan declared, step started, completed and failed facts. A task of the queue, on the async runtime of the importer, sends them. The facts that are publishes (plan declared, step started, completed, failed) carry a message id that the crate makes when the fact is handed over, and a resend carries the same id, so the broker drops a repeat inside the duplicate window of the status stream. The started fact is a request: it carries the start id that the importer gives, and a resend carries the same start id. The crate sets no cap, no deadline, no budget and no maximum size.
- A hand-over returns at once and gives the `WriteEnd` of that write. A write ends in one of two ways: delivered, or removed with a cause: `Refused` (with the refusal of jobs), `Failed` (with a final error), `DependsOnRemoved` or `LeftUnsent`. A hand-over is an error only for what the crate checks before it (a plan with no step); after the hand-over, nothing becomes an error of the call. Making the queue refuses a zero wait.
- The queue is first in first out: a write is sent only when every earlier write has ended. Each attempt is bounded by the outcome wait; an attempt with no outcome by then is `Lost`.
- `Unavailable`, `Lost`, `Unreadable` and a `Credential` error of the kind "fetch failed" are sent again, identical, after the wait between attempts, with no maximum. A refusal `instance_unknown` of the started fact is also sent again. Every other failure is final: `Unauthenticated`, `Credential` of the kind "no usable credential", `InvalidInput`, `InfraMissing` (for example a missing command stream), and every other refusal, among them the refusals of the started fact for a Run held by another Instance or under another start id, reserved by a claim for another Instance, or ended (a resend after the end of the Run is one of these). A final refusal of jobs removes the write with the cause `Refused`; any other final error removes it with the cause `Failed`.
- A write that keeps failing with an error that is sent again holds every later write, with no maximum. The observer and `state()` show it; what to do is the importer's choice. The crate declares no stall, writes no log and takes no action.
- Dependencies between writes: every write but the started fact depends on the started fact; a mail depends on the registration of its sender, and a read marker on the registration of its agent (the registrations of the recipients are not waited for); a background create depends on the registration of the calling agent; a create depends on the upload for the same Job id when one was handed over; a finish or a cancel depends on the create of that Job id when one is in this queue; the completed fact depends on the answer and on the saved state. A write whose dependency ended removed is removed with the cause `DependsOnRemoved`, at once when it is handed over after that end. A create whose upload was refused is removed with the cause `Refused` and the refusal of the upload. Every other write is independent.
- A read marker from the inbox adapter waits outside the order of the queue until its mark gate ends. `Stored`: the marker joins the end of the queue, and for the notice of a completed call the finish of that child Job (command id is the item id of the notice) joins just before it. `Dropped`: the marker is removed with the cause `DependsOnRemoved`. A marker whose dependency was removed already is removed when its gate ends, not before. A held marker is listed by `state()`, and counts for `drained()`.
- `drained()` returns when every write handed over before the call has ended, held read markers included, and gives the state of the queue; it sets no timeout and loses nothing if abandoned. `state()` gives, at once, the writes waiting (named by kind and ids), the instant the oldest was handed over, and the last failed attempt with its error; that failure is cleared when its write ends. A write is listed until the report of its end has returned.
- The observer receives every failed attempt that will be sent again (`Retrying`), every removal and every delivery, naming the write by its kind and ids only. The report of a refused mail carries the text of jobs. `report` is a plain synchronous call. The queue task makes it; a write that is removed at once (handed over after the drop, or after the end of its dependency) is reported by the hand-over call itself, on the thread of the caller; the writes left at a drop are reported by the queue task. The report of a registration that the agent book makes is made after the book has released its lock.
- Dropping the `DeliveryQueue` stops delivery. The writes left, held read markers included, are reported as `LeftUnsent` when the queue task next runs; a write handed over after the drop ends at once with `LeftUnsent`. If the async runtime stops before the queue task has run for the first time, the writes left are reported all the same and `drained()` returns. The queue lives in memory and the crate writes no file for it.
- "Delivered" means that jobs accepted the write. For the plan declared, step started, completed and failed facts, it means that the broker acknowledged the publish; for the started fact, that jobs replied accepted.
- The child Job commands (create, finish, cancel) are sent as integration commands carrying the command id. The producer of a create is the runner type. A create carries the Passport of its `Work`, the Job of the `Work` as parent and its Initial Job. Finish and cancel carry the Run and the instance key of the `Work`. For each of these commands the crate opens one subscription for the outcome event before the first publish and keeps it for all the attempts of that write. Before it sends the command again, it looks for the outcome for a short fixed time (20 ms, or a quarter of what is left of the attempt wait when that is less, so that the command is sent again within the wait); an outcome found ends the write and nothing is sent again.

**Agent book, caller handle and end wait**

- The agent book holds the agents of one Run by key. `agent_book` takes the agents of the previous Run that the importer restored (key, agent id, whether it holds an inbox) and one flag "can restart" for the whole book. The agent ids of the previous Run come from `Work::saved_state()` (`SavedState::agent_ids`); the importer pairs each with its key and gives it as a `RestoredAgent`. A restored agent id that is not a UUIDv7, and a key or an id given twice, is `InvalidInput`, and the book is not spent. Making the book waits for nothing: the registrations go into the queue right after the started fact.
- The first call for a key, by `AgentPortAdapter` or by `NodeEntry::agent_id`, mints a UUIDv7, makes the agent (with an inbox for the label that holds the inbox of the entry, without one for any other label) and puts its registration in the queue. Calls for the same key give the same id, also at the same time.
- `CallerHandle` writes into the queue of the Run. `upload_payload`, `create_child`, `finish` and `cancel` return at once with the end of the write; the importer hands the upload over before its create and the cancel after its create. `create_child` takes the Job id, the command id, the runner type, the call mode (sync, or background with the address of the calling agent), the configuration, the form of the payload and an optional maximum attempt count. The parent Job, the Initial Job and the Passport are those of the `Work` and cannot be set. The crate checks no size and gives the refusal of jobs with its reason.
- In sync mode the end of the call arrives on the arrival feed: completed (the Result inline when jobs gives it inline, else a reference), failed (the report and the message), or cancelled (the message). The importer gives each event to `AgentBook::arrival`, which routes each end to the `EndWait` of its child Job id.
- An `EndWait` is local: it sends nothing, cannot be refused and can be opened before the create. It gives, in this order: `BookDropped` when the book is dropped; the outcome the book holds for the child Job id; the `InfraMissing` that the book keeps; the `InfraMissing` of an end of the feed that came after the wait was opened; else it stays pending. The book keeps one outcome for each child Job id, an end or `Unreadable`: the first one stays. An `Unreadable` event that names a child Job id ends the wait of that id; one that names none ends no wait.
- `arrival_ended` ignores an `Unreadable` error. A feed that ends with `InfraMissing`, given to it, ends every open wait with `InfraMissing`; the book keeps it until `arrival_open_again` is called. A feed that ends with `Lost` ends no wait: the waits stay pending until the importer opens the feed again. When the book is dropped every open wait gives `BookDropped`, and so does a wait opened later: the caller handle holds the book weakly, and its hand-overs still go to the queue.

**Mail handle and arrival feed**

- `MailHandle` calls are single calls made at once: send, read the unread items of an address, read one mail, fetch an attachment, set the read marker, list the agents of the Tree. A mail with attachments uploads each attachment, then sends the mail. The unread items come in the order jobs gives, with no order added. A refusal carries the reason and the text of jobs; the lines of a `badAddresses` refusal are appended to the message.
- `MailHandle::unread` and the inbox listing of an address decode the items one by one, and an item that cannot be read makes the read `Unreadable` as a whole. A reply that is malformed as a whole is `Unreadable`. An item of the kind `MAIL_NOTICE` is an `UnreadItem::MailNotice` (the text is the `message` of the item, required; the mail id is optional). A call notice of the type `JOB_CANCELLED` is a `CallNoticeKind::JobCancelled` (the child Job id and the message are required for the end of a sync call), and one of the type `REFUSED` is a `CallNoticeKind::Refused` (the child Job id is optional: jobs gives none when it could not read the id of the refused create). A notice type or an item kind that the crate does not know is not an error: it is kept as `CallNoticeKind::Unknown` (with the type text of the wire in `notice_type`), `ItemKind::Unknown` or `UnreadItem::Unknown`, and the inbox adapter makes a runtime notice from it.
- The arrival feed is opened first, then one `jobsUnread` replay read of the whole Run is made, then the live events follow. The replay is the one read of the unread items of the whole Run (no address given, which jobs answers with the items of every address of the Run and its run-level notices). It gives one `Arrival::Item` for each unread item, and an end for each run-level call notice. The importer removes duplicates by item id or by child Job id. The replay and the run-level read decode each item alone: an item that cannot be read gives an `Arrival::Unreadable` that names its child Job id when it can be read, the other items of the same read are given, and the bad item ends only the wait of its own child Job. A reply that is malformed as a whole gives one `Arrival::Unreadable` with no child Job id, and the feed goes on. Any other error of the replay read fails the open. An event for a run-level call notice (an item with no address) makes the feed read the unread items of the whole Run once, to learn the end of the sync call from the notice. If that read fails with an error other than `Unreadable`, the feed ends with that error after the item event was given, so a transient `Lost` or `Unavailable` on that read ends the feed and the importer opens it again.
- The arrival feed gives the address, the item id and the kind of item (`DirectMail`, `CcNotice`, `CallNotice` or `MailNotice`); it carries no content. Each feed has its own ephemeral consumer, which sees the doorbell events of every Run and keeps those of its own Run. Every open feed reads every event of the platform, so an event that cannot be read is given to each open feed as an `Unreadable` event, and each feed goes on.

**Adapters**

- `NodeEntry` is made for one node, from the async lookup of the `AgentBook` of an instance. An entry with an inbox takes the label that holds the inbox and a mark gate. Calls that need the book await the lookup; an error of the lookup is an error of the port. Every error of this crate reaches the port as `PortError::Fault` with that error as cause; a refusal that the port states is `PortError::Refused`.
- `AgentPortAdapter` gives the agent id of a key at once. It gives no author name.
- `InboxPortAdapter` serves only an agent that holds the inbox of its node entry; any other call is a fault. Listing gives every unread item of the address of the agent, with no filter by thread, in the batches and with the cursor that the port states. The cursor is an item id. A listing that needs several batches keeps one snapshot read for the agent and cuts the next batches from it; a cursor that the snapshot does not hold causes a new read. The listing and the waits leave out every item that this agent marked, for the life of the book, whatever the end of its marker. A new agent whose registration is still queued has an empty inbox and the listing calls jobs for nothing; an agent restored by the book is read at once.
- A direct mail becomes a mail to the agent with the `to` and `cc` recipients that `read_mail` gives (one `read_mail` for each direct mail, kept in the book); it needs the agent in `to`, a source conversation and an author name that is not blank. A cc notice becomes a cc mail with the mail id and the subject. A call notice, a mail notice (its text is the text of jobs) and an item of an unknown kind become a runtime notice with the item id as its entry id. An item that cannot be made into what the port states is a fault of the whole listing.
- `wait` registers for the events of the address and for the end of the registration of its agent, then reads the unread items; it returns when a read holds an item that this agent has not marked, and reads again at each event of that address. It ends with a fault when the importer gave the end of the arrival feed to the book, when the book is dropped, when the book keeps the end of the feed with `InfraMissing` and the first read holds no item, and when a read gets no answer from jobs. The crate reads nothing again.
- `mark_read` returns at once for every kind of item. Its key is the agent id and the entry id: the mail id for a mail, the item id for a notice. A pair that the adapter did not list, or that is marked already, is not an error and changes nothing. The marker is held until the mark gate of the node entry ends. For a call notice of a completed call, the finish of that child Job goes into the queue before the marker.
- `send` first asks the mail for its own check (`NewMail::refusal` of `br-agentic-contract`: a recipient in `to`, no recipient listed twice in `to` and `cc` together); a mail that fails it is `PortError::Refused`. A mail whose id the book took already returns success and changes nothing. Any other mail goes into the queue with the agent id as sender and returns success: the adapter has taken the mail, jobs has not accepted it yet. If jobs refuses it, the observer gets the report with the text of jobs.
- `documents` reads the mail at the address of the agent and fetches its attachments. A mail of which the agent is not a recipient in `to`, and a refusal of jobs with the reason `mail_not_yours`, are a refused request of the port with the reason; every other error, and a read that gets no answer, is a fault.
- The text of a runtime notice for a call is a fixed text of this crate, in English, that names the child Job (accepted, queue position, started, completed with the Result or its reference, failed with the reason code, and the type text of a notice that the crate does not know). A cancelled call notice and a refused call notice have their texts too ("was cancelled", "was refused"), followed by the message of jobs.

**Errors**

- `Refused` names the operation and the reason (a `RefusalReason`: the word of the wire as a typed case, or `Unknown(UnknownWord)` for a word that this version does not know), and carries the text of jobs. `Unauthenticated`: the token was refused. `Credential`: the credential gave no token (`NoUsable`, `FetchFailed`) and nothing was sent. `Unavailable`: the service received nothing, with a cause (`WaitFired`, `Transport`, `NoListener`). `Lost`: the call was sent and no usable answer came, so it may have been applied. `InfraMissing`: a stream or bucket is absent. `Unreadable`: a reply or event of jobs that cannot be read; the crate fails closed and never falls back to a default. `BookDropped`: an end wait ended because the book was dropped. `InvalidInput`: a bad value, or a second making.
- An error of `br-runner-connection` that this crate does not know is read as `Lost`.

**Secrets and text forms**

- The GraphQL calls carry the bearer token of the runner and the ids of the `Work`: the Job, the Run and the instance key. They never carry the Passport.
- `Work` gives the Passport and the three Job ids (its Job, its parent Job and its Initial Job) through its accessors only. Its `Debug` and `Display` forms show none of them; `Debug` shows the attempt, the call mode and the restart data (`restart_of_run_id` only). The recorded `CreateChild` of the test double shows its Passport as hidden. Outside the `test-double` feature, no other object (client, feed, handle, book, queue, adapter, error, report, state) shows the token, the Passport or those ids in its `Debug` or `Display` form; tests search for them (the text forms of the `Work` and the error texts, the book, and the reports of the queue). Two texts are carried as they were given and are not scrubbed: the text of a `Credential` error, as the credential gives it, and the message of a `Refused` error, as the service gave it. The `Unreadable` error event is not a `Work`: its `Debug` form shows the Job id and the Run id of the trigger that could not be read. `JobsClient` and `TriggerFeed` have no `Display` form.
- The crate mints the agent id of the first call for a key and the message id of each status publish (a UUIDv7 made when the fact is handed over). The start id comes from the importer. Every other id that it sends comes from the importer, or is derived (see Limits).

**Dependency graph**

- The dependency graph of the crate holds `br-llm-clients-core`, through `br-agentic-contract`, and no other library of the `br-llm-clients` family. The CI reads the resolved dependency graph and fails when another library of that family is reachable from a normal or build dependency, or from a dev-dependency that is not on the allowed list of dev-dependencies.

## Limits of this version

| What | State |
|---|---|
| Started fact | It is a request on `jobs.runner.started.{runner_type}` (the Run id, the instance key and the start id) with one reply. Delivered means that jobs replied accepted. A refusal is typed (`RefusalReason`): the queue sends the request again, with the same start id, after `Lost`, `Unavailable`, `Unreadable` and the refusal `instance_unknown`; every other refusal is final and removes the started fact with the cause `Refused`, and every write behind it with `DependsOnRemoved`. The crate publishes no started fact on the status stream. |
| Facts without an id | The wire carries no fact id for the step started, the completed and the failed facts; the message id that the publish carries is not a fact id, and it drops a repeat only inside the duplicate window of the status stream, which the deployment sets. After that window a fact sent again is a second fact. The plan declared fact carries the optional declaration id the importer gives. |
| Outcome of a child create, finish and cancel | It is read from an event of the integration event stream, joined to the command by the correlation id (equal to the command id). One subscription for each write is opened before the first publish and kept for all the attempts. No event within the outcome wait is `Lost`; the queue looks for the outcome (20 ms) and, if none came, sends the command again, with no maximum, and holds every later write. A cut of the bus session, or a subscription that the bus drops because it is slow, loses the events published during the gap, and the write is then sent again. |
| Resend after a missed outcome | Jobs gives no outcome event for a repeated create that it absorbs, so a create whose first send was applied but whose outcome was missed reads as `Lost`, is sent again and keeps being sent again; the queue holds every later write. A create sent again carries the same Job id and the same command id. A finish sent again after a first send that was applied: inside the duplicate window of the command stream (set by the deployment) the broker drops the repeat and the write reads `Lost` again; after the window jobs answers `job_terminal` and the write ends with the cause `Refused`, although the first send was applied. A cancel sent again after a first send that was applied: inside the window the broker drops the repeat and the write reads `Lost` again; after it, jobs accepts a cancel of an ended Job and changes nothing. For a cancel, an outcome found before the resend ends the write. |
| Cancelled child Job | Jobs gives a call notice of the type `JOB_CANCELLED` with a message, so the end wait of a cancelled sync child Job gives `SyncEnd::Cancelled`. Jobs gives none when the same cancel removes the Caller, and then the wait stays pending until the book is dropped, with no time limit. |
| Unread items | The `Mail` of an `UnreadItem::DirectMail` has empty `to` and `cc` vectors; the inbox adapter reads the recipients with `read_mail`. A refusal of an unread read or of `read_mail` names the operation `Read`. A blank author makes `MailHandle::unread` and `read_mail` `Unreadable`. In `MailHandle::unread` and in the inbox listing of an address, one item that cannot be read makes the whole read `Unreadable`, and it stays so while the item stays unread. A reply that is malformed as a whole is `Unreadable`, and the queue sends a write again for it with no maximum. |
| Arrival consumer | Each arrival feed creates its own ephemeral consumer on the event stream. It starts at new messages, filters the version 2 item-arrived events of the wire, and asks for an acknowledgement of each event (acknowledgement wait: 10 seconds). The consumer is made before the replay read, so an item that arrives after it is in the replay or in the feed, or in both. |
| Arrival feed reads | The feed sends the acknowledgement when it reads the event. The consumer sees the events of every Run, and the feed keeps those of its own Run. Every feed also gives an `Unreadable` error event for an event of the platform that cannot be read, whatever its Run. |
| Arrival fan-out cost | Every open feed of the process receives, decodes and acknowledges (one publish) every item-arrived event of the platform, whatever its Run: the cost is the number of open feeds times the rate of events. An event for a run-level notice also makes its feed read the unread items of the whole Run once. |
| Arrival consumer lifetime | The server removes the consumer after 120 seconds in which nobody pulls it. Dropping a feed does not delete its consumer, and neither does an open that fails after the consumer was made: the consumer stays on the server until the 120 seconds have passed. The pull is driven by the feed: a feed that the importer does not poll for about two minutes (a test found the consumer gone after 200 seconds) ends with `Lost` at its next `next`, and the importer opens it again. |
| Arrival cut: redelivery | An event that the bus wrote to a connection and that the feed did not read before the cut stays unacknowledged, and the server gives it again to the pull when the connection is back, within about 10 seconds. The acknowledgement of an event read just before a cut can be lost (seen in a test), and an event that waits unread in a feed that is not polled for more than 10 seconds may be given again (not tested). So an event can come more than once, and the importer removes duplicates by item id. |
| Arrival cut: end with `Lost` | A cut of the bus session of about 30 seconds or more ends the feed with `Lost` (tested only with a scripted pull). A shorter cut can end it with `Lost` too: after a pull error other than a missed heartbeat, the feed ends with `Lost` for good when the consumer is still on the server, and with `Lost` or `InfraMissing` when it is gone. A deleted consumer ends the feed with `Lost` and a deleted stream with `InfraMissing`. The importer opens the feed again after an end, and the replay read gives the items that waited. |
| Arrival cut: tested | With a real `nats-server`: (1) an event that the bus wrote to a connection that was then cut (a proxy discards what the server sends, then closes the connection) comes after the connection returns, and the same test fails when the consumer asks for no acknowledgement; (2) in four rounds of a restart of the server in the middle of a stream of 40 events, each event whose publish the stream confirmed comes, or the feed ends with `Lost` (this test cannot force an event to be in flight, so it does not tell the two mechanisms apart); (3) after a restart the feed gives the next event, and the only event that may come before it is a repeat of the one read before the cut: this test fails if the feed ends with `Lost`, so a pull error during the restart would fail it. |
| Arrival order | The feed is opened first, then one replay read, then the live events: an item can appear in both, and the importer removes duplicates. Jobs publishes the doorbell of an item only after the item is in the unread items, so an event that the feed gives always finds its item in a read. |
| Terminated trigger | A trigger that the crate terminates (it cannot be read) or that the importer terminates is a Run that jobs fails as permanent when it has not started. If the advisory of the broker is lost, the Run stays pending; the crate does not know it. |
| Sync end | The end of a sync call is learnt from the run-level call notice, through one unread read of the Run. A completion with neither an inline Result nor a reference, and a failure with no message, are `Unreadable` and name the child Job. |
| Dispatch mode feed | The feed ends with `Lost` after an entry that is not JSON, and the undecodable entry gives `Unreadable` first. Any other entry that cannot be read gives `Unreadable` and the feed goes on. |
| Presence time to live | It is read from the information of the presence bucket. |
| Attachments of a sent mail | The inbox adapter uploads each attachment as `text/plain` with the document reference as its content. The attachment id is derived: the mail id XOR (position + 1). |
| Inbox cursor | The cursor is an item id; one snapshot read for each agent serves the batches of a listing. |
| `NodeEntry::agent_id` | It returns `Result<AgentId, PortError>`. |
| Notice texts | The runtime notices of the inbox adapter are the wording of this crate. |
| In-memory queue | The queue is in memory. A crash loses the writes still queued: a mail told as sent, a child create, the answer, the saved state, the terminal fact. The saved state of the importer can carry the agents of the book, never the queue. |
| Work from a claim | A `Work` from a claim has no trigger. The three settlement calls are `InvalidInput`. |
| Mail handle | Its send and its read marker are single calls with no retry; the adapters queue theirs. |
| Test double | Feature `test-double`. `ScriptedJobs` records the calls and answers as the test scripts; it checks no wire. It holds, besides the recorded calls, `trigger_json` (a trigger as JSON from a Run id, the three Job ids and a Passport), `work_from_trigger` (a `Work` made from that JSON over a `ScriptedJobs`), `Delivery` (`Ack`, `GiveBack` or `Terminate`, as recorded), `PresenceRecord`, and `memory_caller` with `MemoryScript`, `MemoryCall`, `Answer` and `result_ref` (a caller over memory that records its calls). The feature is off by default. |
| End to end tests | The tests of the live client need `nats-server -js` and are `#[ignore]`d: `cargo test -p br-runner-jobs -- --ignored --test-threads=1`. |

## Why

| Thing | Why it is the way it is |
|---|---|
| direct `async-nats` in `runner_transport` | the jobs runner wire is outside the `integration.*` grammar and faces the runners; its names and frames come from `contract-jobs`; one module keeps it out of the rest of the crate |
| a `Work` is the only door | every call that belongs to a Run carries the Job and the Run of its `Work`, so no call can act for another Run |
| no default for a wait or an id that the importer gives | the importer knows its runtime, its load and its jobs service; a default would hide a choice that it must make |
| a fixed look before a resend | an outcome that came between two attempts must end the write; the look is short and bounded by the attempt wait, so it never costs the resend |
| one delivery queue for each Run, first in first out | the order of the facts of a Run is their hand-over order; a write that is sent again must not pass the next one |
| the queue sends again with no maximum | a write that fails for a reason that can pass is held with its successors and shown to the observer, so the importer, who knows the cost of waiting, decides to drop the queue |
| a held read marker waits outside the order | the marker waits for the store of its entry, and a gate that takes time must not hold unrelated writes |
| the finish goes in front of its marker | a `drained()` call must not return between the marker and the finish of the call it closes |
| the caller handle holds the book weakly | dropping the book must end the waits that depend on its arrival feed |
| first outcome for a child Job id stays | the arrival feed replays on a new open, and a replayed end must change nothing |
| mail waits for the sender registration only | a recipient can be an agent of another Run, whose registration this queue cannot see |
| attachments as `text/plain` with a derived id | the port gives a document as text; the derived id makes a send that is repeated carry the same attachment id |
| a Passport is never on a GraphQL call | the gateway sets the Passport header; the crate sends the bearer token of the runner and the ids of the `Work` |
| a bad item makes `MailHandle::unread` and the inbox listing `Unreadable` | a partial list would hide an item from the agent without a sign |
| the arrival feed decodes each item alone | one bad item must end only its own wait, not the waits of the other child Jobs |
| the crate creates its trigger consumer itself | the trigger stream belongs to the runner wire, which is outside the Fabric; it is the one durable consumer of the crate, because it carries the processing of the runner type, shared by its Instances |
| one outcome subscription for each write | a command that is sent again must find an outcome that came between two attempts; a subscription opened for each attempt would miss it |
| the status facts carry a message id | a resend after a lost acknowledgement must not make a second fact on the status stream; the broker drops a repeat only inside the duplicate window of that stream |
| an ephemeral consumer for each arrival feed, from new messages | the doorbell is a wake-up and the unread items are the truth: a feed needs the events from its open on, and the replay read covers what came before; a durable consumer for each instance key replayed the whole history and was never deleted |
| the consumer is made before the replay read | an item that arrives between the two is in the replay, in the feed or in both, never in neither |
| the arrival consumer asks for an acknowledgement | without it the server counts an event as delivered when it writes it to the connection, and an event written to a connection that is then cut is gone with no sign; with it an event that the feed did not read stays unacknowledged and the server gives it again after the cut. The price is that an event can come twice, which the importer already removes for the replay |
| a long cut or a gone consumer ends the feed with `Lost` | the consumer lives on the server for 120 seconds without a pull, and an unpolled feed ends with `Lost` at about that age; when the consumer is gone the feed ends with `Lost` and the next open reads the unread items again, so no item is missed without a sign |
| `async-nats` reads the arrival consumer in `runner_transport` | the consumers of the Fabric are durable and replay the stream from the start, and its awaiter is a core subscription that loses the events of a cut; none is an ephemeral consumer that starts at new messages and gives again the events that a cut left unacknowledged, and this one module is the place where the crate names `async-nats` |
| not the ordered consumer of `async-nats` | when it re-creates itself before it read a first message it replays the whole stream, and when the stream or the consumer is gone it retries without end, so the typed ends `Lost` and `InfraMissing` would never come |
| `RefusalReason::Unknown` holds an `UnknownWord` | a public `Unknown(String)` could hold a known word, which equals the typed case but does not match its pattern |
| the started fact is a request | the Run is held for the Instance only when jobs replied accepted, and a refusal needs a typed reason, which a publish on a stream cannot give |
