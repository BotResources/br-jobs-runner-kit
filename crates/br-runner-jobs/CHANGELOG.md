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

### Behavior

- Opening an arrival feed does not wait for another feed that waits for an event.
- For each child create, finish and cancel, one outcome subscription is opened before the first publish and kept for all the attempts of the write; the queue looks for the outcome (20 ms) before it sends the command again.
- The status facts (started, plan declared, step started, completed, failed) carry a message id; a resend carries the same id.
- The replay read and the run-level read of the arrival feed decode each item alone. A notice type or an item kind that is not known is kept as `Unknown` (`CallNoticeKind::Unknown` with `CallNotice::notice_type`, `ItemKind::Unknown`, `UnreadItem::Unknown`). A bad item ends only the wait of its own child Job.
- A background create depends on the registration of the calling agent.
- Making the delivery queue checks the waits and the async runtime before it spends the single making of the `Work`.
- `Unreadable` errors carry the class and the position of a parse failure and never a value of the payload.
- The `Debug` form of a recorded `CreateChild` hides the Passport.
- The observer of the queue is not called while the agent book holds its lock.
- If the async runtime stops before the queue task has run, the writes left are reported `LeftUnsent` and `drained()` returns.
- The CI fails when `br-runner-jobs` can reach a library of the `br-llm-clients` family other than `br-llm-clients-core`.

### Limits

- Started fact: a publish on the status stream with a message id built from the Run id and the start id. A refusal of jobs for an unknown instance does not reach the crate; the queue sends again a started fact refused with `instance_unknown`.
- The wire carries no fact id for the step started, the completed and the failed facts.
- Outcome of a child create, finish and cancel: no event within the outcome wait is `Lost`; the queue looks for the outcome, then sends the command again, with no maximum, and holds every later write. A cut of the bus session, or a subscription that the bus drops because it is slow, loses the events published during the gap.
- A create whose first send was applied and whose outcome was missed is sent again and keeps being sent again, because jobs gives no event for a repeat it absorbs. A finish sent again after an applied first send ends with the cause `Refused` (`job_terminal`), although the first send was applied.
- The wire gives no notice of a cancelled or refused call and no item for a mail notice. `SyncEnd::Cancelled`, `JobCancelled`, `Refused` and `MailNotice` are never produced from the wire. The end wait of a cancelled sync child Job stays pending until the book is dropped, with no time limit.
- In `MailHandle::unread` and in the inbox listing of an address, one item that cannot be read makes the whole read `Unreadable`. A reply that is malformed as a whole is `Unreadable`, and the queue sends a write again for it with no maximum.
- The arrival consumer is durable, is never deleted, holds the instance key in its name, sees the events of every Run, and its first open replays the whole retained history; each new instance key replays it again. An event for a Run with no open feed is acknowledged and dropped.
- The dispatch mode feed ends with `Lost` after an entry that is not JSON, after an `Unreadable` event.
- The queue is in memory: a crash loses the writes still queued.
- A `Work` from a claim has no trigger: `ack`, `give_back` and `terminate` are `InvalidInput`.
- `delivery_queue` and `agent_book` can be made once for a `Work`.
- Restart data holds `restart_of_run_id` only.
- Feature `test-double`: `ScriptedJobs`, `trigger_json`, `work_from_trigger`, `Delivery`, `PresenceRecord`, `memory_caller`, `MemoryScript`, `MemoryCall`, `Answer` and `result_ref` are off by default and check no wire.
