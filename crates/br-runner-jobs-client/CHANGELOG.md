# Changelog

## 0.1.0

New library: the typed client of the jobs service for a runner, and the machinery that a runner needs around it. It is the client part of `br-runner-jobs` 0.2.0, copied as it is, with its unit tests, its test double and its examples `run_one_work` and `call_a_runner`. `br-runner-jobs` is unchanged by this release.

### Added

- The client, the Work, the delivery queue, the agent book, the caller handle, the mail handle, the end wait, the feeds, the bus transport, the errors (`Error`, `RefusalReason`) and `RunnerType`, with the behaviour of `br-runner-jobs` 0.2.0.
- The seams `BookLookup` (with `book_lookup`) and `QueueObserver`, both with no default.
- Public surface for adapter libraries, with the same shape as in `br-runner-jobs` 0.2.0 and nothing added: `Backend` and the wire items of its calls (`Fact`, `LogRecord`, `CreateChild`, `ChildCommand`, `RawFeed`, `TriggerSource`, `TriggerDelivery`, `IncomingTrigger`, `ChildSession`, `BoxFuture`); `WorkCore` and `WorkIds`; `Write` and `QueueWriter`; `BookedAgentRef`, `RegistrationEnd`, `EntryKind`, `MailEntry`, `Recipients`, `ItemWatch` and `Wake`; and the methods of `AgentBook` that the adapters call (`agent_for`, `holds_inbox`, `key_of`, `registration_end`, `registration_queued`, `arrival_state`, `queue_writer`, `core`, `record_listed`, `listed`, `take_mail`, `mark`, `keep_snapshot`, `snapshot`, `drop_snapshot`, `keep_recipients`, `recipients`, `marked_items`, `subscribe_items`).
- Feature `test-double`: the scripted jobs and the in-memory caller side.

### Not in this library

- The adapters of the agent port and of the incoming port, the node entry and the mark gate stay in `br-runner-jobs`.
