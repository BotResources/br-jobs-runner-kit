# Changelog

## 0.1.0

First release. The jobs side of a runner: a typed client of the jobs service, a delivery queue for each Run, and the adapters of the agent port and of the inbox port of `br-agentic-contract`.

### Added

- `JobsClient`, built from a runner type, an instance key, a reply wait and a `Connection`: `triggers(ack_wait)` and `TriggerFeed` (one `Work` for each trigger, an `Unreadable` error event for a trigger that cannot be read), `claim(claim_id, wait)`, `dispatch_modes()`, `cancel_orders()`, `publish_presence`, `delete_presence` and `presence_time_to_live`. Every wait is a required `Duration`; zero is `InvalidInput`. A missing stream or bucket is `InfraMissing`.
- `Feed<T>` with the events `ModeEvent` and `CancelEvent`: a feed ends only when it is dropped or with a typed error, and a single event that cannot be read is an `Unreadable` error event.
- `Work`, the one door to the calls that belong to a Run: the fields of the wire as received (unknown fields of the trigger kept), `ack`, `give_back` and `terminate` for a trigger, `payload`, `saved_state`, `read_result`, `cancel_own_job`, `log`, and the makers `delivery_queue`, `mail_handle`, `arrival_feed` and `agent_book`.
- `DeliveryQueue` with `QueueSettings`, `QueueObserver`, `IgnoreReports`, `QueueReport`, `QueueState`, `WriteEnd` and `Removal`: first in, first out for each Run, a write is sent again on `Unavailable`, `Lost`, `Unreadable` and a credential fetch failure, dependencies between writes, read markers held behind a mark gate, `drained()` and `state()`. Writes: the started fact (first write), the registrations of agents, mails, read markers, uploads, creates, finishes and cancels of child Jobs, the answer, the saved state, and the plan declared, step started, completed and failed facts.
- `AgentBook`, `CallerHandle`, `CreateChildInput` and `EndWait`: the agents of a Run by key, the calls of the caller side that write into the queue, and the end of a sync call by child Job id.
- `MailHandle` (send, unread items, read one mail, attachment, read marker, agent list) and `ArrivalFeed` with `Arrival`.
- `NodeEntry`, `AgentPortAdapter`, `InboxPortAdapter`, `BookLookup`, `MarkGate` and their helpers `book_lookup`, `mark_gate`, `mark_gate_stored`.
- One error type, `Error`, with `Failure`, `Cause`, `Refusal`, `Operation`, `CredentialFailure` and `CredentialKind`.
- Bus access: `src/runner_transport/` is the one module that names `async-nats`. It carries the `jobs.*` runner transport and the jobs requests on the bus connection of the `Connection`; the names, frames and ids come from `contract-jobs`. Integration traffic and the published language go through `br-util-nats-fabric`. Streams and buckets are bound and never created. The crate creates the durable consumer of the triggers of the runner type; the Fabric creates the arrival consumer.
- Feature `test-double`: `test_double::ScriptedJobs` with `Call`, `Delivery` and `PresenceRecord`, `trigger_json`, `work_from_trigger`, `memory_caller` with `MemoryScript`, `MemoryCall`, `Answer` and `result_ref`, and the recorded types `ChildCommand`, `CreateChild`, `Fact` and `LogRecord`.
- Three examples: `run_one_work` (the quickstart of the README, checked word for word by a test), `call_a_runner` and `agent_and_inbox`; the tests run all three.
- The tests of the live client run against a real `nats-server -js` and are `#[ignore]`d: `cargo test -p br-runner-jobs -- --ignored --test-threads=1`.
