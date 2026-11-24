# Changelog

## 0.3.1

Released 2026-11-24.

Re-pin on `br-runner-jobs-client` 0.1.1 (was 0.1.0) and on `br-runner-jobs` 0.4.0 (was 0.3.0, dev-dependency). No change of the public surface or of the behavior.

## 0.3.0

Opens the line 0.3. The toolkit takes the jobs side from `br-runner-jobs-client` 0.1.0 and the contract `br-agentic-contract` 0.3.0.

### Changed

- The crate depends on `br-runner-jobs-client` 0.1.0 and on `br-agentic-contract` 0.3.0 (per-crate pins). It has no adapter library in its normal dependencies: `br-runner-jobs` 0.3.0 is a `dev-dependency`, used only by the tests and by the example `restartable_agent` (node entries and the mark gate).
- The tests and the examples make their jobs client with the test double of `br-runner-jobs-client`, or with `br-runner-connection` against a locally started bus.
- The README tells the new dependencies and the pins of the install block.

### Breaking

- The public builders and types name the types of `br-runner-jobs-client` and no longer those of `br-runner-jobs`: `RunnerBuilder::new` and `TestHost::new` take its `JobsClient`; `with_queue_observer` takes its `QueueObserver`; `RunStartInput` holds its `Work`; `InstanceEnded` lists its `BookedAgent`; `RunContext` gives its `AgentBook`, caller handle and mail handle; `InstanceLookup::book_lookup` gives its `BookLookup`; `AnswerFn` and the failure types use its `Content` and `FailureKind`. A runner author that named these types imports them from `br-runner-jobs-client`. The behaviour of the loop, the runs, the cancel, the drain and the saved state is that of 0.2.
- `br-agentic-contract` 0.3.0 replaces 0.2.0 in the public API (`PortError`, `PortFuture`): a consumer moves to the same tag.
- The tests that checked the install block against `br-runner-jobs` check it against `br-runner-jobs-client`; no test was deleted.

## 0.2.0

Opens the line 0.2.

### Changed

- A Run that a cancel order ended hands over the failed fact `run_cancelled` in place of no terminal fact: when the executor returns `RunCancelled`, and when the order is seen before the executor starts, in push and in claim mode. The fact is permanent, goes through the `FailureMapping` like any failure, with an empty text and so an empty diagnostic unless the mapping gives one, and is never the completed fact. jobs keeps a run it asked to stop among its current runs, against the capacity of the runner type, until the runner hands over a terminal fact: with no terminal fact the slot stayed taken until the pod died.
- When the executor has returned, the settle gives its observer one turn of the scheduler before it logs "the run waits for the executor to report its last observations". The executor tells its outcome through a channel and returns before the observer has read it, so nearly every Run logged that warning; it is now logged only when the observations are really late.

### Breaking

- No public signature changes, but a `FailureMapping` is now called for a cancelled Run, with the reason code `run_cancelled`, and `HostCancel` ends the Run of a `TestHost` with that failed fact. A test or a mapping that expected no terminal fact after a cancel must change.

## 0.1.1

Uses `br-runner-jobs` 0.2.0.

### Changed

- A claim refusal is matched by its typed reason.
- A Run whose started fact jobs refuses is not executed: the trigger of a `PUSH` Run is acknowledged and dropped, and a `CLAIM` slot claims again with a new claim id.
- The arrival feed needs the `br-runner-jobs` 0.2.0 doorbell (version 2 of the item-arrived events).
- The arrival feed of a Run is opened with an acknowledgement wait of 10 s and an inactivity limit of 120 s.

### Breaking

- `RunnerBuilder::new` and `TestHost::new` take the `JobsClient` of `br-runner-jobs` 0.2.0.

## 0.1.0

First release. The main loop of a runner with the jobs service: registration through the presence, parallel run slots, the Run steps, the facts of each Run and the drain.

### Added

- `RunnerBuilder::new(client, settings)` and `start(StartArgs)`: reads the time to live of the presence, publishes the presence and takes Runs in parallel slots until the shutdown, then drains and returns a `DrainReport`. A part of the loop that ends by a panic is logged and started again.
- `Settings` and `SettingsBuilder` (`runner_version`, `max_concurrent_runs`, `ack_wait`, `drain_timeout`, `cancel_grace`, `claim_wait`, `outcome_wait`, `retry_wait`) with `Error` and `ErrorCode` (`SettingMissing`, `SettingInvalid`, `JobsFailed`, `NotStarted`, `InstanceEnded`, `SignalsNotListened`, `TaskEnded`). With `with_process_signals`, `start` returns `SignalsNotListened` when the process signals cannot be listened to.
- `StartArgs` (graph, `RunStart`, `can_restart`) with `with_shutdown`, `with_process_signals`, `with_state_limit`, `with_failure_mapping` and `with_answer` (which takes any function from the state to the answer; the boxed form of that function is exported as `AnswerFn`, and no public signature uses it); `run_start`, `RunStartInput`, `StartedRun` and `RunRefusal`. With neither a shutdown token nor `with_process_signals`, `start` never returns.
- Push mode and claim mode, the gates (dispatch mode known and not "no entry", cancel replay complete), the claim refusals (`busy`, `instance_unknown`, `claim_spent`, any other), and the give-back of a trigger read during the drain.
- The Run steps: registration with the cancel orders already known, the delivery queue and its started fact, the trigger acknowledgement after the started fact, the saved state read of a restart, the graph through the executor, the settle (the executor has reported all its instances, every hook has returned and the queue holds no write handed over before the executor returned), and the end-of-run writes (answer of a sync call, saved state, completed or failed fact) with the failed fact `answer_refused` or `saved_state_refused` after a write that jobs refused or ended with a final error.
- `FailureMapping`, `failure_mapping`, `FailureOverride` and `ReportedFailure`: the reason codes `node_failed`, `node_refused_an_input`, `node_could_not_start`, `node_vanished`, `update_refused`, `edge_failed`, `checkpoint_not_built`, `run_failed` (the executor failed with no failure, or with a cause or an outcome this library does not know), `sink_refused` (mapped from the executor, never sent by this library: it gives the executor no checkpoint sink), `start_refused`, `panic`, `saved_state_invalid`, `saved_state_unavailable`, `saved_state_not_made`, `answer_not_made`, `answer_refused` and `saved_state_refused`.
- `InstanceLookup` (`book`, `run_context`, `book_lookup`), `RunContext` and `LifecycleHook` with `lifecycle_hook` and `InstanceEnded`. `RunnerBuilder::with_lifecycle_hook` and `RunnerBuilder::with_queue_observer` (the same two on `TestHost`): the observer gets every `QueueReport`; the library logs a failed attempt as a warning and a removed write as an error, with the run id, the job id and the name of the write, and logs no delivery. A panic of the observer or of a hook is caught and logged. `RunStartInput`, `InstanceEnded` and `ReportedFailure` are `#[non_exhaustive]`: the library builds them, the author reads them.
- A panic of the Run body (the call of the run start function before it returns its future, or the executor) fires the cancel token of the Run, the one the executor holds, and ends the Run with the reason code `panic`. A panic inside the future that the run start function returns is caught earlier: it ends the Run with the reason code `panic` and does not fire the token.
- `DrainReport` and `DrainedRun` (`job_id`, `run_id`, `aborted`, `executor_running`, `instances_without_return`, `unsent`). The drain waits for a claim in flight; a Run that jobs gives to a claim answered after the drain began is not started and is logged as an error. At the drain timeout the drain fires the cancel token of each aborted Run. A second signal during the drain has no effect.
- A lifecycle hook is called for an instance the executor reports late, while the Run is still live: the lookup holds the instance, the hook gets the agents of the live book, and the settle waits for it. After the drain aborted a Run, a late instance is not held by the lookup and its hook is called without being awaited. After the executor panicked, a late instance is held by the lookup until the Run is dropped, the lookup answers `InstanceEnded` for it, and its hook is called without being awaited.
- Feature `test-host`: `TestHost`, `HostRun`, `HostEvent`, `HostCancel`. The host uses the clock of the runtime of the test and turns on no feature of tokio. It replaces two settings: one slot, and a drain with no time limit. It ignores the shutdown token and the process signals of the start arguments: the host ends after its Run. Its `events` hold the ends of the writes of the queue (delivered or removed).
- Four examples: `serve_push` and `serve_claim` over `shared/serve.rs` (the quickstart of the README, checked word for word by a test), `restartable_agent` (a tool that reads the run context from the lookup, a gate for the inbox, a lifecycle hook, with stand-ins for the threads service) and `test_host` (a model of a runner test with `TestHost`, with its own `main` over the in-process double); the tests run all four.
- The companion Helm library chart `br-runner-toolkit-chart`: one required value for each setting, no default, with a schema; a test checks that its values are exactly the settings.
