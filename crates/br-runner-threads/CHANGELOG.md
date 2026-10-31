# Changelog

## 0.2.0

Rebuilds the crate on the `Connection` of `br-runner-connection` and adds the adapters of the Threads ports of `br-agentic-contract` 0.2.0. This is a breaking release: the 0.1 surface (`ThreadsClient::new(actor, bus, graphql)`, `transport::*`, the sink factory) is replaced.

### Added

- `ThreadClient`, built from a `Connection` with a check wait: `create_thread`, `write_message`, `open_stream`, `close_message`, `read_context`, `set_title`, `fail_title_job`, `request_upload`. Every wait is a required `Duration`; zero is `InvalidInput`.
- `StreamHandle`, `StreamWriter`, `ResponseSink` and `ResponseControl` (`complete`, `discard`, `failure`): chunks numbered from 0 with no gap, `StreamBroken` on a chunk the bus did not take, `StreamClosed` after a closing. A keepalive is numbered like any other chunk and is counted in the seal.
- The closing table of the client (`Closing`, `ClosureKind`): Completed sends a complete, BudgetExhausted, Failed and Interrupted send a fail, Cancelled sends a cancel; the seal is sent as the README states. The three commands carry no closure reason and no detail.
- `ThreadPortAdapter` and `ThreadAccessAdapter` with the thread queue: first in, first out for each thread, one delivery task for each thread, sending again on `Unavailable`, `Lost`, `Unreadable` (commands) and `StreamBroken` (chunks), reports through `QueueObserver`, items kept through `QueueStore` (`MemoryQueueStore` provided), `queue_state()`, `entry_stored()` and `instance_ended()`. The adapter sends no keepalive and no parent message id.
- `WriteContext` and `WriteContextFn`, `ThreadAccessSettings`.
- `OpenStream::impersonator_id`, sent in the opening message as `WholeMessage::impersonator_id` is sent in a whole message.
- Feature `test-double`: `test_double::MemoryThreads`, `test_double::refusal(code, message, command_id)` and `test_double::failure(cause, text)`.
- Six examples; `runner_wiring` is the quickstart of the README. The real-bus tests are `#[ignore]` and need a `nats-server` binary.

### Changed

- One error type owned by the crate: `InvalidInput`, `Credential`, `Unauthenticated`, `Unavailable`, `Lost`, `Refused`, `Unreadable`, `StreamBroken`, `StreamClosed`, `InfraMissing`, `Store`. A `Store` error from a `QueueStore` keeps its text as it is, without a second `store:` prefix.
- A `Failed` closing with no detail is `InvalidInput("a Failed closing needs a detail")`.
- The chunk path gives `StreamBroken` or `InvalidInput` only, never `Unavailable`.
- The client depends on `br-runner-connection` 0.1.1 and `br-agentic-contract` 0.2.0, and no longer on `br-llm-clients-common-auth` or `br-llm-clients-common-http`.
- `complete_response`, `open_response`, `open_result`, `close_turn`, `add_entry`, `complete_result`, `add_input_turn` and `discard_response` of one thread run one at a time: no other of these operations goes between the deltas of a response that `complete_response` queues.

### Migrating from 0.1

- `ThreadsClient` is `ThreadClient`. `ThreadsClient::new(actor, bus, graphql)` is `ThreadClient::new(&Connection, check_wait)`; the `transport` module (`BusTransport`, `GraphqlTransport`, `NatsBus`, `HttpGraphql`, `MemoryBus`, `MemoryGraphql`) is gone. Use `test_double::MemoryThreads` in tests.
- `Error::AnswerLost(String)` is `Error::Lost(Failure)`; `Error::Auth(String)` is `Error::Credential(CredentialFailure)`; `Error::Infra(String)` is `Error::InfraMissing(String)`; `Error::Unavailable(String)` is `Error::Unavailable(Failure)`; `Error::Store(String)` is new. `Failure` holds a `Cause` (`WaitFired`, `Transport`, `NoListener`) and a text.
- `Refusal` gains `reason` and `message`, and is `#[non_exhaustive]`: build one in tests with `test_double::refusal`, and match its fields.
- `SinkFactory` and `StreamHandle::sink_factory` are gone. `StreamHandle::response(entry)` and `StreamWriter::response(entry)` give a `ResponseSink` and a `ResponseControl`; the sink is a `StreamSink` of `br-agentic-contract` and its `send` gives nothing back, so read a failure with `ResponseControl::failure`.
- The ids of an operation are `Uuid` fields named `thread_id` (`thread: ThreadRef` before) on `NewThread`, `WholeMessage`, `OpenStream`, `Closing`, `ContextQuery`, `ThreadContext`, `SetTitle`, `FailTitleJob` and `UploadRequest`; `ThreadOrigin::parent_thread` is `parent_thread_id`.
- `wait` is a `Duration` (`Option<Duration>` before) on `NewThread`, `WholeMessage`, `OpenStream` and `Closing`; zero is `InvalidInput`.
- `Confirmed` is exported by the crate root.

### Bus access

- Commands and outcomes go through `br-util-nats-fabric` with typed coordinates; chunks go on the streaming stream of `contract-threads`. `async-nats` is named in one module only, `src/chunk_lane/`. The crate writes no subject, stream or bucket name and never creates infra. At building, a missing stream is `InfraMissing`.

## 0.1.0

- First release: the typed client of Threads for runner code. Creating threads, whole messages, streams with handle, writers and sink factory, closings, context reads, title and upload operations, bus, GraphQL and in-memory transports, one error type.
