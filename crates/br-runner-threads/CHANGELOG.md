# Changelog

## 0.2.0

Rebuilds the crate on the `Connection` of `br-runner-connection` and adds the adapters of the Threads ports of `br-agentic-contract` 0.2.0. This is a breaking release: the 0.1 surface (`ThreadsClient::new(actor, bus, graphql)`, `transport::*`, the sink factory) is replaced.

### Added

- `ThreadClient`, built from a `Connection` with a check wait: `create_thread`, `write_message`, `open_stream`, `close_message`, `read_context`, `set_title`, `fail_title_job`, `request_upload`. Every wait is a required `Duration`; zero is `InvalidInput`.
- `StreamHandle`, `StreamWriter`, `ResponseSink` and `ResponseControl` (`complete`, `discard`, `failure`): chunks numbered from 0 with no gap, `StreamBroken` on a chunk the bus did not take, `StreamClosed` after a closing. A keepalive is numbered like any other chunk and is counted in the seal.
- The closing table of the client (`Closing`, `ClosureKind`): Completed sends `message.complete` (version 2) and BudgetExhausted, Failed, Interrupted and Cancelled send `message.fail` (version 2); each names its closure with the reason and the detail. The seal of the chunks is sent with a `Completed` closing, and with the other closings only when the handle is healthy and holds at least one chunk that is not a keepalive. A keepalive is in the seal and is not a chunk for these rules, so a `Completed` closing of a handle that holds nothing but keepalives is `InvalidInput`.
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
- The client depends on `br-runner-connection` 0.1.1 and `br-agentic-contract` 0.2.0, and no longer on `br-llm-clients-common-auth` or `br-llm-clients-common-http`.

### Migrating from 0.1

- `ThreadsClient` is `ThreadClient`. `ThreadsClient::new(actor, bus, graphql)` is `ThreadClient::new(&Connection, check_wait)`; the `transport` module (`BusTransport`, `GraphqlTransport`, `NatsBus`, `HttpGraphql`, `MemoryBus`, `MemoryGraphql`) is gone. Use `test_double::MemoryThreads` in tests.
- `Error::AnswerLost(String)` is `Error::Lost(Failure)`; `Error::Auth(String)` is `Error::Credential(CredentialFailure)`; `Error::Infra(String)` is `Error::InfraMissing(String)`; `Error::Unavailable(String)` is `Error::Unavailable(Failure)`; `Error::Store(String)` is new. `Failure` holds a `Cause` (`WaitFired`, `Transport`, `NoListener`) and a text.
- `Refusal` gains `reason` and `message`, and is `#[non_exhaustive]`: build one in tests with `test_double::refusal`, and match its fields.
- `SinkFactory` and `StreamHandle::sink_factory` are gone. `StreamHandle::response(entry)` and `StreamWriter::response(entry)` give a `ResponseSink` and a `ResponseControl`; the sink is a `StreamSink` of `br-agentic-contract` and its `send` gives nothing back, so read a failure with `ResponseControl::failure`.
- The ids of an operation are `Uuid` fields named `thread_id` (`thread: ThreadRef` before) on `NewThread`, `WholeMessage`, `OpenStream`, `Closing`, `ContextQuery`, `ThreadContext`, `SetTitle`, `FailTitleJob` and `UploadRequest`; `ThreadOrigin::parent_thread` is `parent_thread_id`.
- `wait` is a `Duration` (`Option<Duration>` before) on `NewThread`, `WholeMessage`, `OpenStream` and `Closing`; zero is `InvalidInput`.
- `Confirmed` is exported by the crate root.
- A closing that is not `Completed` now carries the seal when the handle is healthy and holds a chunk that is not a keepalive; 0.1 sent none. A `Completed` closing of a handle that holds nothing but keepalives is now `InvalidInput`.

### Bus access

- Commands and outcomes go through `br-util-nats-fabric` with typed coordinates; chunks go on the streaming stream of `contract-threads`. `async-nats` is named in one module only, `src/chunk_lane/`. The crate writes no subject, stream or bucket name and never creates infra. At building, a missing stream is `InfraMissing`.

## 0.1.0

- First release: the typed client of Threads for runner code. Creating threads, whole messages, streams with handle, writers and sink factory, closings, context reads, title and upload operations, bus, GraphQL and in-memory transports, one error type.
