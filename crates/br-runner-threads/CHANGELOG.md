# Changelog

## 0.5.1

A runner can say which message its reply answers. The public API only grows, and nothing changes for a runner that does not call the new operation.

### Added

- `ThreadAccessAdapter::reply_to(thread, reply, parent)`, beyond the port: names the parent of one message that the adapter has not queued yet. When the adapter queues the message whose id is `reply` on that thread (the opening of that AI turn or that input turn), it takes `parent` as its `parent_message_id` in place of the tip, then becomes the tip. No other message takes it: any other message follows the tip and leaves the pair in place. A second call with the same `reply` replaces the parent, and a `read` of the thread keeps the pair. It is `InvalidInput` when the adapter already queued the opening of the turn `reply`, or when `reply` and `parent` are the same id. The adapter does not check `parent` against Threads: Threads refuses a message whose parent it does not hold, and the messages chained after it, until the next `read`. The pair is memory only, like the tip; a pair whose `reply` is never queued changes no message. Before, the parent was always the tip, so a runner that wrote a reply again could only chain it after the old reply; now it can chain it to the message that the reply answers, as a sibling of the old one.

## 0.5.0

Annotations through the adapter: a runner whose reply goes through the thread-access adapter can name an artifact it uploaded. The public API only grows.

### Added

- `ThreadAccessAdapter::add_annotation(thread, turn_id, after, annotation)`, beyond the port: queues an annotation chunk of an open AI turn, named by its id only, after an entry that the adapter queued into that turn (an entry, a tool result, or a model response through its deltas), numbered and sealed with the turn's other chunks. It is `InvalidInput` when the adapter holds no state of the turn, when the closing of the turn was accepted, when the entry is not one it queued into that turn, or when the annotation id is not a UUIDv7. An `AnnotationBody::Artifact` names an artifact so that Threads attaches it at the closing of the message.

## 0.4.0

Decides the parent of a message: the adapter chains each message it writes to the thread, so Threads reads the whole conversation, not the latest message alone. The public API does not change.

### Changed

- Each message that the thread-access adapter queues on a thread (the whole message of an input turn, the opening of an AI turn) takes the tip of that thread as its `parent_message_id`, then becomes the tip. The tip is the last turn of the last `read` of the thread, then each message queued since; with no tip the message starts a tree. The parent is fixed when the item is accepted and travels in the item. Before, every message was sent with no parent: Threads stored each one as a root, and a thread read back (by a runner at its start, or by people) held only its latest message.

## 0.3.0

Follows contract-threads 0.7.0: Threads no longer orders a title Job, so a runner sets the title of a thread with no Job id. This is a breaking release.

### Changed

- `SetTitle` has no `title_job_id` any more: `ThreadClient::set_title` sends `threadsSetTitle(threadId, title)`.
- `contract-threads` 0.7.0 (was 0.6.0).

### Removed

- `ThreadClient::fail_title_job` and `FailTitleJob`: the mutation `threadsFailTitleJob` is gone from Threads 0.7.

## 0.2.0

Rebuilds the crate on the `Connection` of `br-runner-connection` and adds the adapters of the Threads ports of `br-agentic-contract`. This is a breaking release: the 0.1 surface (`ThreadsClient::new(actor, bus, graphql)`, `transport::*`, the sink factory) is replaced.

### Added

- `ThreadClient`, built from a `Connection` with a check wait: `create_thread`, `write_message`, `open_stream`, `close_message`, `read_context`, `set_title`, `fail_title_job`, `request_upload`. Every wait is a required `Duration`; zero is `InvalidInput`.
- `StreamHandle`, `StreamWriter`, `ResponseSink` and `ResponseControl` (`complete`, `discard`, `failure`): chunks numbered from 0 with no gap, `StreamBroken` on a chunk the bus did not take, `StreamClosed` after a closing. A keepalive is numbered like any other chunk and is counted in the seal.
- The closing table of the client (`Closing`, `ClosureKind`): Completed sends `message.complete` (version 2) and BudgetExhausted, Failed, Interrupted and Cancelled send `message.fail` (version 2); each names its closure with the reason and the detail. The seal of the chunks is sent with a `Completed` closing, and with the other closings only when the handle is healthy and holds at least one chunk that is not a keepalive. A keepalive is in the seal and is not a chunk for these rules, so a `Completed` closing of a handle that holds nothing but keepalives is `InvalidInput`. Through the adapter, a turn is healthy until a chunk of its message ends refused or dropped; a closing accepted after that carries no seal, except a `Completed` closing.
- `ThreadPortAdapter` and `ThreadAccessAdapter` with the thread queue: first in, first out for each thread, one delivery task for each thread, sending again on `Unavailable`, `Lost`, `Unreadable` (commands) and `StreamBroken` (chunks), reports through `QueueObserver`, items kept through `QueueStore` (`MemoryQueueStore` provided), `queue_state()`, `entry_stored()` and `instance_ended()`. The adapter sends no parent message id. On one thread, `complete_response`, `open_response`, `open_result`, `close_turn`, `add_entry`, `complete_result`, `add_input_turn` and `discard_response` run one at a time, and so does each delta of a response sink: no other operation goes between the deltas that `complete_response` queues. A port call that is dropped after it began to keep its items ends as if it had been awaited.
- Keepalives of the adapter: for each open AI turn of the adapter, a keepalive chunk is queued when the keepalive interval has passed since the last item of the turn was accepted and no item of the thread waits. They stop at an accepted closing, at `instance_ended` for the agent, at an opening refused for good and when the adapter is dropped. A keepalive that the store cannot keep is reported as `ItemRefused` with item kind `Chunk` and the `Store` error.
- `ThreadAccessSettings::keepalive_interval`, a required building value with no default; zero is `InvalidInput`.
- `WriteContext` and `WriteContextFn`, `ThreadAccessSettings`.
- `OpenStream::impersonator_id`, sent in the opening message as `WholeMessage::impersonator_id` is sent in a whole message.
- Feature `test-double`: `test_double::MemoryThreads`, `test_double::refusal(code, message, command_id)` and `test_double::failure(cause, text)`.
- `test_double` also exports `Operation`, `Command`, `CommandBody` and `Outcome`, and `MemoryThreads` has `operations`, `commands`, `chunks`, `answer_commands`, `answer_chunks`, `answer_graphql`, `pause` and `resume`; `pause` and `resume` hold and release the answers.
- Six examples; `runner_wiring` is the quickstart of the README.

### Changed

- One error type owned by the crate: `InvalidInput`, `Credential`, `Unauthenticated`, `Unavailable`, `Lost`, `Refused`, `Unreadable`, `StreamBroken`, `StreamClosed`, `InfraMissing`, `Store`. A `Store` error that a `QueueStore` returns reaches the caller with its text unchanged.
- A `Failed` closing with no detail is `InvalidInput("a Failed closing needs a detail")`.
- The chunk path of the client gives `StreamBroken`, `StreamClosed` or `InvalidInput`, never `Unavailable`; through the adapter a chunk write can also give `Store`.
- `queue_state` clears `last_failure` of a thread when an item of that thread ends, delivered, refused or dropped.
- `instance_ended` drops the states of the agent between the operations of each thread.
- The client depends on `br-runner-connection` and `br-agentic-contract`, and no longer on `br-llm-clients-common-auth` or `br-llm-clients-common-http`.

### Migrating from 0.1

- `ThreadsClient` is `ThreadClient`. `ThreadsClient::new(actor, bus, graphql)` is `ThreadClient::new(&Connection, check_wait)`; the `transport` module (`BusTransport`, `GraphqlTransport`, `NatsBus`, `HttpGraphql`, `MemoryBus`, `MemoryGraphql`) is gone. Use `test_double::MemoryThreads` in tests.
- `Error::AnswerLost(String)` is `Error::Lost(Failure)`; `Error::Auth(String)` is `Error::Credential(CredentialFailure)`; `Error::Infra(String)` is `Error::InfraMissing(String)`; `Error::Unavailable(String)` is `Error::Unavailable(Failure)`; `Error::Store(String)` is new. `Failure` holds a `Cause` (`WaitFired`, `Transport`, `NoListener`) and a text.
- `Refusal` gains `reason` and `message`, and is `#[non_exhaustive]`: build one in tests with `test_double::refusal`, and match its fields.
- `SinkFactory` and `StreamHandle::sink_factory` are gone. `StreamHandle::response(entry)` and `StreamWriter::response(entry)` give a `ResponseSink` and a `ResponseControl`; the sink is a `StreamSink` of `br-agentic-contract` and its `send` gives nothing back, so read a failure with `ResponseControl::failure`.
- The ids of an operation are `Uuid` fields named `thread_id` (`thread: ThreadRef` before) on `NewThread`, `WholeMessage`, `OpenStream`, `Closing`, `ContextQuery`, `ThreadContext`, `SetTitle`, `FailTitleJob` and `UploadRequest`; `ThreadOrigin::parent_thread` is `parent_thread_id`.
- `wait` is a `Duration` (`Option<Duration>` before) on `NewThread`, `WholeMessage`, `OpenStream` and `Closing`; zero is `InvalidInput`.
- `Confirmed` is exported by the crate root.
- `BudgetExhausted` was sent as `message.complete` (version 2) with the seal and needed a handle. It is now sent as `message.fail` (version 2) that names the closure, needs no handle, and carries the seal only when the handle is healthy and holds a chunk that is not a keepalive.
- `Failed`, `Interrupted` and `Cancelled` were sent as `message.fail` (version 2) with no seal. They now carry the seal on the same condition as `BudgetExhausted`.
- A `Completed` closing of a handle that holds no chunk was sent as a failure with no seal. It is now `InvalidInput`, and so is one of a handle that holds nothing but keepalives.

### Bus access

- Commands and outcomes go through `br-util-nats-fabric` with typed coordinates; chunks go on the streaming stream of `contract-threads`. `async-nats` is named in one module only, `src/chunk_lane/`. The crate writes no subject, stream or bucket name and never creates infra. At building, a missing stream is `InfraMissing`.

## 0.1.0

- First release: the typed client of Threads for runner code. Creating threads, whole messages, streams with handle, writers and sink factory, closings, context reads, title and upload operations, bus, GraphQL and in-memory transports, one error type.
