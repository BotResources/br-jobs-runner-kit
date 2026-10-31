# Changelog

## 0.2.0

Rebuilds the crate on the `Connection` of `br-runner-connection` and adds the adapters of the Threads ports of `br-agentic-contract` 0.2.0. This is a breaking release: the 0.1 surface (`ThreadsClient::new(actor, bus, graphql)`, `transport::*`, the sink factory) is replaced.

### Added

- `ThreadClient`, built from a `Connection` with a check wait: `create_thread`, `write_message`, `open_stream`, `close_message`, `read_context`, `set_title`, `fail_title_job`, `request_upload`. Every wait is a required `Duration`; zero is `InvalidInput`.
- `StreamHandle`, `StreamWriter`, `ResponseSink` and `ResponseControl`: chunks numbered from 0 with no gap, `StreamBroken` on a chunk the bus did not take, `StreamClosed` after a closing.
- The closing table of the client (`Closing`, `ClosureKind`): Completed sends a complete, BudgetExhausted, Failed and Interrupted send a fail, Cancelled sends a cancel; the seal is sent as the README states.
- `ThreadPortAdapter` and `ThreadAccessAdapter` with the thread queue: first in, first out for each thread, one delivery task for each thread, sending again on `Unavailable`, `Lost`, `Unreadable` (commands) and `StreamBroken` (chunks), reports through `QueueObserver`, items kept through `QueueStore` (`MemoryQueueStore` provided), `queue_state()`, `entry_stored()` and `instance_ended()`.
- `WriteContext` and `WriteContextFn`, `ThreadAccessSettings`.
- `Refusal::new`.
- Feature `test-double`: `test_double::MemoryThreads`.
- Six examples (`runner_wiring` is the quickstart of the README, checked word for word by a test) and real-bus tests against a real `nats-server`.

### Changed

- One error type owned by the crate: `InvalidInput`, `Credential`, `Unauthenticated`, `Unavailable`, `Lost`, `Refused`, `Unreadable`, `StreamBroken`, `StreamClosed`, `InfraMissing`, `Store`.
- The client depends on `br-runner-connection` 0.1.1 and `br-agentic-contract` 0.2.0, and no longer on `br-llm-clients-common-auth` or `br-llm-clients-common-http`.

### Bus access

- Commands and outcomes go through `br-util-nats-fabric` with typed coordinates; chunks go on the streaming stream of `contract-threads`. `async-nats` is named in one module only, `src/chunk_lane/`. The crate writes no subject, stream or bucket name and never creates infra. At building, a missing stream is `InfraMissing`.

### Not done in this version

- The commands carry no named closure and no detail.
- The chunk path never gives `Unavailable`.

## 0.1.0

- First release: the typed client of Threads for runner code. Creating threads, whole messages, streams with handle, writers and sink factory, closings, context reads, title and upload operations, bus, GraphQL and in-memory transports, one error type.
