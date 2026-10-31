# Changelog

## 0.1.0

First release. A runner type as a tool for the agentic contract.

### Added

- `RunnerTool`: built with the runner type, the tool name, the tool description, the argument schema (`Parameters`), the `ToolMode` and the `CallerHandle` of `br-runner-jobs`; optional `config`, `max_attempts`, `payload_kind`, `keep_result`, `payload_builder` and `result_processor`. `build()` gives the `Tool` or a `RunnerToolError` (`Spec` or `RunnerType`).
- `ToolMode` (`Sync`, `Background { caller_address }`), `PayloadKind` (`Inline`, `Kept`), `Payload` (`Content`, `Reference`), `AuthorFailure` (`refused`, `fault`), `CreateRemoved` and `UnknownEnd`.
- Three examples, run by the tests: `sync_tool`, `background_tool`, `builder_and_processor`.

### Behavior

- A sync call mints a UUIDv7 child Job id and a new command id for each command, opens the end wait, hands over the upload (kept payload) and the create, and waits together for the end of the create and the end of the sync call. A completed end gives the Result (inline, or read with the reference and the keep flag), hands over the finish whatever the read gave, then runs the result processor.
- A background call hands over the upload (kept payload) and the create and returns an acknowledgement that names the runner type and the child Job id; it waits for nothing after the payload builder.
- A refused create, a failed or cancelled child Job, a refused Result read, a refusal of an author function and a cancel give an error flag; every unknown or infrastructure outcome gives a fault with the error as cause, except `UnknownEnd` and `CreateRemoved`, the two causes this crate makes.
- A cancel view that triggers after the create was handed over, and a fault after the create and before the end of the sync call, hand over one cancel, except for a create that ended `Refused` or was removed. The finish and the cancel are never awaited.
- A Result with attachments gives one extra text block for each attachment; a Result with a blank message (empty or only white space) and no attachment gives one text block that says the runner returned nothing.

### Limits

- The runner type is checked at build time with the segment rule of the jobs wire.
- The crate adds no timeout, no limit and no retry.
- A sync call whose child Job is cancelled by someone else gets no end from the jobs service. The call then ends at the cancel view, or as a fault when the book of the run is dropped.
- A completed end that holds neither an inline Result nor a reference is given by the live `br-runner-jobs` as `Unreadable`, not as an end of the sync call. The call then gives a fault and one cancel.
