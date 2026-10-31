# br-runner-jobs

The jobs side of a runner. It gives `JobsClient`, a typed client of the jobs service (triggers, claims, presence, cancel orders, dispatch mode); `Work`, one received Run with every call that belongs to it; `DeliveryQueue`, a queue of writes for each Run that sends again what jobs did not take for a reason that can pass; `AgentBook`, `CallerHandle` and `MailHandle`; and the adapters of the agent port and of the inbox port of `br-agentic-contract` (`AgentPortAdapter` and `InboxPortAdapter`, both given by `NodeEntry`). The client does one operation once. The delivery queue is the only part that sends a write again. The crate knows no graph, no agent loop, no thread and no tool: the importer decides when to call.

## Install

```toml
br-runner-jobs = { git = "https://github.com/BotResources/br-jobs-runner-kit", package = "br-runner-jobs", tag = "br-runner-jobs/v0.1.0", version = "0.1.0" }
br-runner-connection = { git = "https://github.com/BotResources/br-jobs-runner-kit", package = "br-runner-connection", tag = "br-runner-connection/v0.1.1", version = "0.1.1" }
br-agentic-contract = { git = "https://github.com/BotResources/br-llm-graph-rs", package = "br-agentic-contract", tag = "br-agentic-contract/v0.2.0", version = "0.2.0" }
br-llm-messages = { git = "https://github.com/BotResources/br-llm-messages-rs", package = "br-llm-messages", tag = "br-llm-messages/v2.1.0", version = "2.1.0" }
br-core-kernel = { git = "https://github.com/BotResources/br-rust-common", package = "br-core-kernel", tag = "br-core-kernel/v0.1.0", version = "0.1.0" }
br-core-auth = { git = "https://github.com/BotResources/br-rust-common", package = "br-core-auth", tag = "br-core-auth/v0.1.0", version = "0.1.0" }
contract-jobs = { git = "https://github.com/BotResources/svc-jobs", package = "contract-jobs", tag = "contract-jobs/v0.7.0", version = "0.7.0", features = ["integration"] }
uuid = { version = "1", features = ["v7"] }
serde_json = "1"
async-nats = "0.48"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

A crate that names the types of a sibling in its public API must reach that sibling through the same git url and the same tag string, or the types are two copies: the pins above are the ones this crate is built with. `contract-jobs` is the wire contract of the jobs service; the crate takes every subject, stream, bucket, frame and id of that wire from it.

## Quickstart

This is `examples/run_one_work.rs`. It builds the `Connection` and the `JobsClient`, opens the trigger feed and reads one `Work`, makes the delivery queue (its first write is the started fact), acknowledges the trigger, reads the payload, hands over the answer and the saved state, hands over the completed fact, and waits until the queue holds nothing.

```rust,no_run
use std::sync::Arc;
use std::time::Duration;

use br_core_kernel::ServiceAccountId;
use br_runner_connection::{Connection, FixedToken};
use br_runner_jobs::{
    Content, DeliveryQueue, Error, IgnoreReports, JobsClient, QueueSettings, Removal, Work,
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
    runner_type: &str,
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

pub async fn run_one_work(work: Work) -> Result<String, Box<dyn std::error::Error>> {
    let queue = start_queue(&work).await?;
    work.ack().await?;
    let payload = work.payload().await?;
    let answer = Content {
        message: format!("received: {}", payload.message),
        attachments: Vec::new(),
    };
    let answered = queue.answer(answer);
    let saved = queue.saved_state(json!({ "steps_done": 1 }));
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
    let client = build_client(runner_type, instance_key, connection).await?;
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
| `JobsClient::new(runner_type, instance_key, reply_wait, connection)` | Builds the client of one Instance of one runner type on a `Connection`. Checks the streams and buckets that the wire needs; sends nothing else. |
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
| `Error`, `Failure`, `Cause`, `Refusal`, `Operation`, `CredentialFailure`, `CredentialKind` | The single error type and its parts. All are `#[non_exhaustive]`: match with a final `_` arm. |
| `test_double::*` (feature `test-double`) | `ScriptedJobs` (a `JobsClient` over memory that records the calls as `Call` and answers as the test scripts), `trigger_json`, `work_from_trigger`, `Delivery`, `PresenceRecord`, `memory_caller`, `MemoryScript`, `MemoryCall`, `Answer`, `result_ref`, and the types `ChildCommand`, `CreateChild`, `Fact`, `LogRecord` that the recorded calls hold. |

## Bus access

The module `src/runner_transport/` is the one module that names `async-nats`. It carries the `jobs.*` runner transport (the trigger stream, the status and log publishes, the claim request-reply, the `JOBS_CANCEL` and `JOBS_PRESENCE` buckets) and the jobs requests (the registration of agents, the agent list and the read marker, as request-reply) on the bus connection that the `Connection` gives back. The started fact and the facts of a Run are publishes on the status stream, acknowledged by the broker. The subjects, the stream and bucket names, the frames and the ids come from `contract-jobs`: this crate makes none. `integration.*` traffic and the published language go only through `br-util-nats-fabric`, which `runner_transport` builds on the bus connection of the `Connection`: the child Job commands (create, finish, cancel) with their outcome events, the arrival events and the dispatch mode. Streams and buckets are bound, not created, and a missing one is `InfraMissing`. Durable consumers are created by the Fabric (the arrival consumer, see Limits); the one durable consumer that `runner_transport` creates itself is the one of the triggers of the runner type, named `runner_jobs_trigger_` and the runner type. This crate creates no stream, no bucket and no other durable consumer; the watches and awaiters that the feeds and the outcome waits open are ephemeral.

## Rules the code keeps

**Making the client and the feeds**

- A runner type or an instance key that the wire does not accept (empty, a dot, a space, `*`, `>`, or too long) is `InvalidInput`. So is a zero reply wait, a zero acknowledgement wait and a zero claim wait. The crate reads no environment variable and no file, and has no default for a wait.
- Making the client checks the three streams of the runner wire, the two buckets, the published-language bucket, and the event stream and the command stream of the integration bus. A missing stream or bucket, or a presence bucket with no time to live, is `InfraMissing`. A check that gets no answer within the reply wait, or a bus connection that is not connected, is `Unavailable`.
- Every feed ends only when the importer drops it or with an error. A single event that cannot be read is an `Unreadable` error event and the feed goes on. Any other error ends the feed, and `Feed::next` gives that error again on every later call. A feed whose consumer or watch fails is `Lost`, or `InfraMissing` when its stream or bucket is gone. The crate never opens a feed again; a cut of the bus session is repaired by the transport, and the crate repeats no call for it.
- The dispatch mode feed gives the mode at open, then each change; it does not repeat a value it just gave. A runner type with no entry, also after the entry is deleted, gives `ModeEvent::NoEntry`. An entry that cannot be read is an `Unreadable` event, never a default mode.
- The cancel feed gives the entries that exist, then `ReplayComplete` once for each open (an empty bucket gives it at once), then `OrderMade` and `OrderDeleted`. A key that is not a Run id is ignored. A deletion is reported only after the replay is complete. An order that cannot be read, or that names another Run than its key, is an `Unreadable` event. The crate does not act on an order.
- The trigger feed uses one durable consumer for the runner type, shared by all its Instances: explicit acknowledgement, the acknowledgement wait given at the open, every trigger of the stream from the start, only the triggers of the runner type, no limit on deliveries and no limit on triggers in flight. A consumer that exists already with another configuration is made equal to this one, so the acknowledgement wait of the last Instance that opened the feed applies. The consumer pulls in batches of one message.
- `ack`, `give_back` and `terminate` return when the broker has confirmed the settlement. `give_back` and `terminate` take the `Work`. A `Work` that comes from a claim has no trigger: these three calls are `InvalidInput`.
- A trigger that is not JSON, that is not an object, that lacks a field of the wire, that has no Passport or a Passport that cannot be read, or a background call with no caller address, is terminated by the crate and given as an `Unreadable` error event, never as a `Work`. The error event holds the Job id and the Run id when they can be read. A Passport with a field that the `Passport` type of `br-core-auth` does not know cannot be read. The first error of the trigger consumer ends the feed: `TriggerFeed::next` gives it again afterwards.
- A claim is one request. The caller gives the claim id (not nil) and the wait. The crate waits for the claim wait plus the reply wait of the bus; no reply by then is `Lost`. The reply is a `Work`, nothing (`Ok(None)`), or `Refused` for the claim with the reason as the wire names it and an empty message; a busy refusal carries the delay in `retry_after_ms`. A reply that is not a JSON object or that cannot be read is `Unreadable`. A new claim is the importer's choice.
- A `Work` keeps the fields of the trigger, or of the `run` object of a claim reply, that the crate does not know (`unknown_fields`). Unknown fields at the top of a claim reply are not kept. The wire version of a trigger or of a claim reply is not checked.

**Work**

- A `Work` comes only from a trigger or from a claim reply that the client received. No public constructor makes one from raw values, except the test double. Every call of a `Work` carries the Job and the Run of that `Work`; the caller cannot give others.
- The Passport is the whole Passport as `br-core-auth` defines it. `payload`, `saved_state` and `read_result` are GraphQL reads; the crate does not check the form of the payload, the restart flag or the call mode before the call and gives the reply of jobs as it is. `read_result` takes the reference that the end of a sync call gave and a keep flag that the importer sets. A read is not sent again.
- `log` publishes a line on the bus without waiting for the broker. A failure to send is returned and the line is not published again. The level is typed. The step index and the id of the line are optional and come from the importer.
- `cancel_own_job` sends the cancel command of the Job of the `Work` with a command id that the importer gives, and returns when jobs accepts it or refuses it with its reason. It does not go through the delivery queue.
- `delivery_queue` is made once for a `Work`, and `agent_book` once: a second making is `InvalidInput`. Making the queue needs an async runtime, and hands over the started fact as its first write.

**Delivery queue of a Run**

- The queue holds the writes of one Run in hand-over order: the started fact, the registrations of agents, the mails and the released read markers of the adapters, the uploads, creates, finishes and cancels of child Jobs, the answer, the saved state, and the plan declared, step started, completed and failed facts. A task of the queue, on the async runtime of the importer, sends them. The crate sets no cap, no deadline, no budget and no maximum size.
- A hand-over returns at once and gives the `WriteEnd` of that write. A write ends in one of two ways: delivered, or removed with a cause: `Refused` (with the refusal of jobs), `Failed` (with a final error), `DependsOnRemoved` or `LeftUnsent`. A hand-over is an error only for what the crate checks before it (a plan with no step); after the hand-over, nothing becomes an error of the call. Making the queue refuses a zero wait.
- The queue is first in first out: a write is sent only when every earlier write has ended. Each attempt is bounded by the outcome wait; an attempt with no outcome by then is `Lost`.
- `Unavailable`, `Lost`, `Unreadable` and a `Credential` error of the kind "fetch failed" are sent again, identical, after the wait between attempts, with no maximum. A refusal `instance_unknown` of the started fact is also sent again. Every other failure is final: `Unauthenticated`, `Credential` of the kind "no usable credential", `InvalidInput`, `InfraMissing` (for example a missing command stream), and every other refusal. A final refusal of jobs removes the write with the cause `Refused`; any other final error removes it with the cause `Failed`.
- A write that keeps failing with an error that is sent again holds every later write, with no maximum. The observer and `state()` show it; what to do is the importer's choice. The crate declares no stall, writes no log and takes no action.
- Dependencies between writes: every write but the started fact depends on the started fact; a mail depends on the registration of its sender, and a read marker on the registration of its agent (the registrations of the recipients are not waited for); a create depends on the upload for the same Job id when one was handed over; a finish or a cancel depends on the create of that Job id when one is in this queue; the completed fact depends on the answer and on the saved state. A write whose dependency ended removed is removed with the cause `DependsOnRemoved`, at once when it is handed over after that end. A create whose upload was refused is removed with the cause `Refused` and the refusal of the upload. Every other write is independent.
- A read marker from the inbox adapter waits outside the order of the queue until its mark gate ends. `Stored`: the marker joins the end of the queue, and for the notice of a completed call the finish of that child Job (command id is the item id of the notice) joins just before it. `Dropped`: the marker is removed with the cause `DependsOnRemoved`. A marker whose dependency was removed already is removed when its gate ends, not before. A held marker is listed by `state()`, and counts for `drained()`.
- `drained()` returns when every write handed over before the call has ended, held read markers included, and gives the state of the queue; it sets no timeout and loses nothing if abandoned. `state()` gives, at once, the writes waiting (named by kind and ids), the instant the oldest was handed over, and the last failed attempt with its error; that failure is cleared when its write ends. A write is listed until the report of its end has returned.
- The observer receives every failed attempt that will be sent again (`Retrying`), every removal and every delivery, naming the write by its kind and ids only. The report of a refused mail carries the text of jobs. `report` is a plain synchronous call. The queue task makes it; a write that is removed at once (handed over after the drop, or after the end of its dependency) is reported by the hand-over call itself, on the thread of the caller; the writes left at a drop are reported by the queue task.
- Dropping the `DeliveryQueue` stops delivery. The writes left, held read markers included, are reported as `LeftUnsent` when the queue task next runs; a write handed over after the drop ends at once with `LeftUnsent`. The queue lives in memory and the crate writes no file for it.
- "Delivered" means that jobs accepted the write. For the plan declared, step started, completed and failed facts, and for the started fact, it means that the broker acknowledged the publish.
- The child Job commands (create, finish, cancel) are sent as integration commands carrying the command id. The producer of a create is the runner type. A create carries the Passport of its `Work`, the Job of the `Work` as parent and its Initial Job. Finish and cancel carry the Run and the instance key of the `Work`.

**Agent book, caller handle and end wait**

- The agent book holds the agents of one Run by key. `agent_book` takes the agents of the previous Run that the importer restored (key, agent id, whether it holds an inbox) and one flag "can restart" for the whole book. A restored agent id that is not a UUIDv7, and a key or an id given twice, is `InvalidInput`, and the book is not spent. Making the book waits for nothing: the registrations go into the queue right after the started fact.
- The first call for a key, by `AgentPortAdapter` or by `NodeEntry::agent_id`, mints a UUIDv7, makes the agent (with an inbox for the label that holds the inbox of the entry, without one for any other label) and puts its registration in the queue. Calls for the same key give the same id, also at the same time.
- `CallerHandle` writes into the queue of the Run. `upload_payload`, `create_child`, `finish` and `cancel` return at once with the end of the write; the importer hands the upload over before its create and the cancel after its create. `create_child` takes the Job id, the command id, the runner type, the call mode (sync, or background with the address of the calling agent), the configuration, the form of the payload and an optional maximum attempt count. The parent Job, the Initial Job and the Passport are those of the `Work` and cannot be set. The crate checks no size and gives the refusal of jobs with its reason.
- In sync mode the end of the call arrives on the arrival feed: completed (the Result inline when jobs gives it inline, else a reference), or failed (the report and the message). The importer gives each event to `AgentBook::arrival`, which routes each end to the `EndWait` of its child Job id.
- An `EndWait` is local: it sends nothing, cannot be refused and can be opened before the create. It gives, in this order: `BookDropped` when the book is dropped; the outcome the book holds for the child Job id; the `InfraMissing` that the book keeps; the `InfraMissing` of an end of the feed that came after the wait was opened; else it stays pending. The book keeps one outcome for each child Job id, an end or `Unreadable`: the first one stays. An `Unreadable` event that names a child Job id ends the wait of that id; one that names none ends no wait.
- `arrival_ended` ignores an `Unreadable` error. A feed that ends with `InfraMissing`, given to it, ends every open wait with `InfraMissing`; the book keeps it until `arrival_open_again` is called. A feed that ends with `Lost` ends no wait: the waits stay pending until the importer opens the feed again. When the book is dropped every open wait gives `BookDropped`, and so does a wait opened later: the caller handle holds the book weakly, and its hand-overs still go to the queue.

**Mail handle and arrival feed**

- `MailHandle` calls are single calls made at once: send, read the unread items of an address, read one mail, fetch an attachment, set the read marker, list the agents of the Tree. A mail with attachments uploads each attachment, then sends the mail. The unread items come in the order jobs gives, with no order added. A refusal carries the reason and the text of jobs; the lines of a `badAddresses` refusal are appended to the message.
- An unread read that holds one item that cannot be read is `Unreadable` as a whole.
- The arrival feed is opened first, then one `jobsUnread` replay read of the whole Run is made, then the live events follow. The replay gives one `Arrival::Item` for each unread item, and an end for each run-level call notice. The importer removes duplicates by item id or by child Job id. An unreadable replay read gives one `Arrival::Unreadable` and the feed goes on; any other error of that read fails the open. An event for a run-level call notice (an item with no address) makes the feed read the unread items of the whole Run once, to learn the end of the sync call from the notice. If that read fails with an error other than `Unreadable`, the feed ends with that error after the item event was given, so a transient `Lost` or `Unavailable` on that read ends the feed and the importer opens it again.
- The arrival feed gives the address, the item id and the kind of item; it carries no content. An event of the bus that cannot be read is given to every open arrival feed of the client as an `Unreadable` event.

**Adapters**

- `NodeEntry` is made for one node, from the async lookup of the `AgentBook` of an instance. An entry with an inbox takes the label that holds the inbox and a mark gate. Calls that need the book await the lookup; an error of the lookup is an error of the port. Every error of this crate reaches the port as `PortError::Fault` with that error as cause; a refusal that the port states is `PortError::Refused`.
- `AgentPortAdapter` gives the agent id of a key at once. It gives no author name.
- `InboxPortAdapter` serves only an agent that holds the inbox of its node entry; any other call is a fault. Listing gives every unread item of the address of the agent, with no filter by thread, in the batches and with the cursor that the port states. The cursor is an item id. A listing that needs several batches keeps one snapshot read for the agent and cuts the next batches from it; a cursor that the snapshot does not hold causes a new read. The listing and the waits leave out every item that this agent marked, for the life of the book, whatever the end of its marker. A new agent whose registration is still queued has an empty inbox and the listing calls jobs for nothing; an agent restored by the book is read at once.
- A direct mail becomes a mail to the agent with the `to` and `cc` recipients that `read_mail` gives (one `read_mail` for each direct mail, kept in the book); it needs the agent in `to`, a source conversation and an author name that is not blank. A cc notice becomes a cc mail with the mail id and the subject. A call notice, and a mail notice, become a runtime notice with the item id as its entry id. An item that cannot be made into what the port states is a fault of the whole listing.
- `wait` registers for the events of the address and for the end of the registration of its agent, then reads the unread items; it returns when a read holds an item that this agent has not marked, and reads again at each event of that address. It ends with a fault when the importer gave the end of the arrival feed to the book, when the book is dropped, when the book keeps the end of the feed with `InfraMissing` and the first read holds no item, and when a read gets no answer from jobs. The crate reads nothing again.
- `mark_read` returns at once for every kind of item. Its key is the agent id and the entry id: the mail id for a mail, the item id for a notice. A pair that the adapter did not list, or that is marked already, is not an error and changes nothing. The marker is held until the mark gate of the node entry ends. For a call notice of a completed call, the finish of that child Job goes into the queue before the marker.
- `send` first asks the mail for its own check (`NewMail::refusal` of `br-agentic-contract`: a recipient in `to`, no recipient listed twice in `to` and `cc` together); a mail that fails it is `PortError::Refused`. A mail whose id the book took already returns success and changes nothing. Any other mail goes into the queue with the agent id as sender and returns success: the adapter has taken the mail, jobs has not accepted it yet. If jobs refuses it, the observer gets the report with the text of jobs.
- `documents` reads the mail at the address of the agent and fetches its attachments. A mail of which the agent is not a recipient in `to`, and a refusal of jobs with the reason `mail_not_yours`, are a refused request of the port with the reason; every other error, and a read that gets no answer, is a fault.
- The text of a runtime notice for a call is a fixed text of this crate, in English, that names the child Job (accepted, queue position, started, completed with the Result or its reference, failed with the reason code, cancelled, refused).

**Errors**

- `Refused` names the operation and the reason as the wire names it, and carries the text of jobs; a reason that the crate does not know is kept as it is. `Unauthenticated`: the token was refused. `Credential`: the credential gave no token (`NoUsable`, `FetchFailed`) and nothing was sent. `Unavailable`: the service received nothing, with a cause (`WaitFired`, `Transport`, `NoListener`). `Lost`: the call was sent and no usable answer came, so it may have been applied. `InfraMissing`: a stream or bucket is absent. `Unreadable`: a reply or event of jobs that cannot be read; the crate fails closed and never falls back to a default. `BookDropped`: an end wait ended because the book was dropped. `InvalidInput`: a bad value, or a second making.
- An error of `br-runner-connection` that this crate does not know is read as `Lost`.

**Secrets and text forms**

- The GraphQL calls carry the bearer token of the runner and the ids of the `Work`: the Job, the Run and the instance key. They never carry the Passport.
- `Work` gives the Passport and the three Job ids (its Job, its parent Job and its Initial Job) through its accessors only. Its `Debug` and `Display` forms show none of them; `Debug` shows the attempt, the call mode and the restart data. Outside the `test-double` feature, no other object (client, feed, handle, book, queue, adapter, error, report, state) shows the token, the Passport or those ids in its `Debug` or `Display` form; tests search for them (the text forms of the `Work` and the error texts, the book, and the reports of the queue). Two texts are carried as they were given and are not scrubbed: the text of a `Credential` error, as the credential gives it, and the message of a `Refused` error, as the service gave it. The `Unreadable` error event is not a `Work`: its `Debug` form shows the Job id and the Run id of the trigger that could not be read. `JobsClient` and `TriggerFeed` have no `Display` form.
- The crate mints no id but the agent id of the first call for a key. Every other id that it sends comes from the importer, or is derived (see Limits).

**Dependency graph**

- The dependency graph of the crate holds `br-llm-clients-core`, through `br-agentic-contract`, and no other library of the `br-llm-clients` family.

## Limits of this version

| What | State |
|---|---|
| Started fact | It is a publish on the status stream of the runner type: the Run id and the instance key go on the wire. Delivered means that the broker acknowledged the publish. The start id is not sent, so a started fact that jobs refuses for an unknown instance cannot be read from the publish. The queue still keeps the start id in the name of the write. The queue sends again a started fact that is refused with `instance_unknown`; the live transport gives no such refusal. |
| Facts without an id | The wire carries no fact id for the step started, the completed and the failed facts. The plan declared fact carries the optional declaration id the importer gives. |
| Outcome of a child create, finish and cancel | It is read from an event of the integration event stream, joined to the command by the correlation id (equal to the command id). The subscription is opened before the command is published. No event within the outcome wait is `Lost`, and the queue sends the command again. |
| Repeated create that jobs absorbs | Jobs gives no outcome event for a repeated create that it absorbs, so the repeat reads as `Lost` and the queue sends it again; while no event comes, the queue holds every later write. |
| Cancelled child Job | The unread read and the arrival feed cannot read a notice of a cancelled or refused call, and the wire has no item for a mail notice. `SyncEnd::Cancelled` exists, and nothing reads it from the wire. A cancelled child Job gives no end to the end wait. An unread read that holds such an item is `Unreadable` as a whole, and it stays so while the item stays unread: every inbox listing and wait for that address is a fault, the replay read of the arrival feed gives one `Arrival::Unreadable`, and each later run-level call notice makes the feed read again, gets `Unreadable` again, and gives an `Arrival::Unreadable` that names no child Job. So the end waits of the other child Jobs of the Run stay pending too. |
| Unread items | The `Mail` of an `UnreadItem::DirectMail` has empty `to` and `cc` vectors; the inbox adapter reads the recipients with `read_mail`. A refusal of an unread read or of `read_mail` names the operation `Read`. A blank author makes `MailHandle::unread` and `read_mail` `Unreadable`. |
| Arrival consumer | The arrival feed opens one durable consumer on the event stream, created by the Fabric, named `runner_jobs_arrivals_`, the length of the runner type, `_`, the runner type, `_` and the instance key. It filters the item-arrived events of the wire, and its first open replays the whole retained history. All the feeds of the client share it. An event for a Run with no open feed when it is read is acknowledged and dropped. A feed whose consumer failed is replaced at the next open. |
| Arrival order | The feed is opened first, then one replay read, then the live events: an item can appear in both, and the importer removes duplicates. |
| Sync end | The end of a sync call is learnt from the run-level call notice, through one unread read of the Run. A completion with neither an inline Result nor a reference, and a failure with no message, are `Unreadable` and name the child Job. |
| Dispatch mode feed | The feed ends with `Lost` after an entry that is not JSON, and the undecodable entry gives `Unreadable` first. |
| Presence time to live | It is read from the information of the presence bucket. |
| Attachments of a sent mail | The inbox adapter uploads each attachment as `text/plain` with the document reference as its content. The attachment id is derived: the mail id XOR (position + 1). |
| Inbox cursor | The cursor is an item id; one snapshot read for each agent serves the batches of a listing. |
| `NodeEntry::agent_id` | It returns `Result<AgentId, PortError>`. |
| Notice texts | The runtime notices of the inbox adapter are the wording of this crate. |
| In-memory queue | The queue is in memory. A crash loses the writes still queued: a mail told as sent, a child create, the answer, the saved state, the terminal fact. The saved state of the importer can carry the agents of the book, never the queue. |
| Work from a claim | A `Work` from a claim has no trigger. The three settlement calls are `InvalidInput`. |
| Mail handle | Its send and its read marker are single calls with no retry; the adapters queue theirs. |
| Test double | `ScriptedJobs` records calls and answers as scripted; it checks no wire. |
| End to end tests | The tests of the live client need `nats-server -js` and are `#[ignore]`d: `cargo test -p br-runner-jobs -- --ignored --test-threads=1`. |

## Why

| Thing | Why it is the way it is |
|---|---|
| direct `async-nats` in `runner_transport` | the jobs runner wire is outside the `integration.*` grammar and faces the runners; its names and frames come from `contract-jobs`; one module keeps it out of the rest of the crate |
| a `Work` is the only door | every call that belongs to a Run carries the Job and the Run of its `Work`, so no call can act for another Run |
| no default for any wait or id | the importer knows its runtime, its load and its jobs service; a default would hide a choice that it must make |
| one delivery queue for each Run, first in first out | the order of the facts of a Run is their hand-over order; a write that is sent again must not pass the next one |
| the queue sends again with no maximum | a write that fails for a reason that can pass is held with its successors and shown to the observer, so the importer, who knows the cost of waiting, decides to drop the queue |
| a held read marker waits outside the order | the marker waits for the store of its entry, and a gate that takes time must not hold unrelated writes |
| the finish goes in front of its marker | a `drained()` call must not return between the marker and the finish of the call it closes |
| the caller handle holds the book weakly | dropping the book must end the waits that depend on its arrival feed |
| first outcome for a child Job id stays | the arrival feed replays on a new open, and a replayed end must change nothing |
| mail waits for the sender registration only | a recipient can be an agent of another Run, whose registration this queue cannot see |
| attachments as `text/plain` with a derived id | the port gives a document as text; the derived id makes a send that is repeated carry the same attachment id |
| a Passport is never on a GraphQL call | the gateway sets the Passport header; the crate sends the bearer token of the runner and the ids of the `Work` |
| a bad item makes the whole unread read `Unreadable` | a partial list would hide an item from the agent without a sign |
