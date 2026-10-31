# Changelog

## 0.1.0

Released 2026-10-31.

First release. The main loop of a runner with the jobs service: registration through the presence, parallel run slots, the Run steps, the facts of each Run and the drain.

### Added

- `RunnerBuilder::new(client, settings)` and `start(StartArgs)`: reads the time to live of the presence, publishes the presence and takes Runs in parallel slots until the shutdown, then drains and returns a `DrainReport`. A part of the loop that ends by a panic is logged and started again.
- `Settings` and `SettingsBuilder` (`runner_version`, `max_concurrent_runs`, `ack_wait`, `drain_timeout`, `cancel_grace`, `claim_wait`, `outcome_wait`, `retry_wait`) with `Error` and `ErrorCode` (`SettingMissing`, `SettingInvalid`, `JobsFailed`, `NotStarted`, `InstanceEnded`, `SignalsNotListened`, `TaskEnded`). With `with_process_signals`, `start` returns `SignalsNotListened` when the process signals cannot be listened to.
- `StartArgs` (graph, `RunStart`, `can_restart`) with `with_shutdown`, `with_process_signals`, `with_state_limit`, `with_failure_mapping` and `with_answer`; `run_start`, `RunStartInput`, `StartedRun` and `RunRefusal`.
- Push mode and claim mode, the gates (dispatch mode known and not "no entry", cancel replay complete), the claim refusals (`busy`, `instance_unknown`, `claim_spent`, any other), and the give-back of a trigger read during the drain.
- The Run steps: registration with the cancel orders already known, the delivery queue and its started fact, the trigger acknowledgement after the started fact, the saved state read of a restart, the graph through the executor, the settle of hooks and queue, and the end-of-run writes (answer of a sync call, saved state, completed or failed fact) with the failed fact `answer_refused` or `saved_state_refused` after a write that jobs refused or ended with a final error.
- `FailureMapping`, `failure_mapping`, `FailureOverride` and `ReportedFailure`: the reason codes `node_failed`, `node_refused_an_input`, `node_could_not_start`, `node_vanished`, `update_refused`, `edge_failed`, `checkpoint_not_built`, `run_failed` (the executor failed with no failure, or with a cause or an outcome this library does not know), `start_refused`, `panic`, `saved_state_invalid`, `saved_state_unavailable`, `saved_state_not_made`, `answer_not_made`, `answer_refused` and `saved_state_refused`.
- `InstanceLookup` (`book`, `run_context`, `book_lookup`), `RunContext` and `LifecycleHook` with `lifecycle_hook` and `InstanceEnded`.
- `DrainReport` and `DrainedRun`.
- Feature `test-host`: `TestHost`, `HostRun`, `HostEvent`, `HostCancel`. The host uses the clock of the runtime of the test and turns on no feature of tokio.
- Four examples: `serve_push` and `serve_claim` over `shared/serve.rs` (the quickstart of the README, checked word for word by a test), `restartable_agent` (a tool that reads the run context from the lookup, a gate for the inbox, a lifecycle hook, with stand-ins for the threads service) and `test_host`; the tests run all four.
- The companion Helm library chart `br-runner-toolkit-chart`: one required value for each setting, no default, with a schema; a test checks that its values are exactly the settings.
