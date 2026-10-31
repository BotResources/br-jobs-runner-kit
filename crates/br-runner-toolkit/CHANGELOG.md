# Changelog

## 0.1.0

Released 2026-10-31.

First release. The main loop of a runner with the jobs service: registration through the presence, parallel run slots, the Run steps, the facts of each Run and the drain.

### Added

- `RunnerBuilder::new(client, settings)` and `start(StartArgs)`: reads the time to live of the presence, then runs the presence task, the dispatch mode feed, the cancel order feed and one task for each slot, until the shutdown, then drains and returns a `DrainReport`.
- `Settings` and `SettingsBuilder` (`runner_version`, `max_concurrent_runs`, `ack_wait`, `drain_timeout`, `cancel_grace`, `claim_wait`, `outcome_wait`, `retry_wait`) with `Error` and `ErrorCode` (`SettingMissing`, `SettingInvalid`, `JobsFailed`, `NotStarted`, `InstanceEnded`).
- `StartArgs` (graph, `RunStart`, `can_restart`) with `with_shutdown`, `with_process_signals`, `with_state_limit`, `with_failure_mapping` and `with_answer`; `run_start`, `RunStartInput`, `StartedRun` and `RunRefusal`.
- Push mode and claim mode, the gates (dispatch mode known and not "no entry", cancel replay complete), the claim refusals (`busy`, `instance_unknown`, `claim_spent`, any other), and the give-back of a trigger read during the drain.
- The Run steps: registration with the cancel orders already known, the delivery queue and its started fact, the trigger acknowledgement after the started fact, the saved state read of a restart, the graph through the executor, the settle of hooks and queue, and the end-of-run writes (answer of a sync call, saved state, completed or failed fact) with the failed fact `answer_refused` or `saved_state_refused` after a refused write.
- `FailureMapping`, `failure_mapping`, `FailureOverride` and `ReportedFailure`: the reason codes `node_failed`, `node_refused_an_input`, `node_could_not_start`, `node_vanished`, `update_refused`, `edge_failed`, `checkpoint_not_built`, `sink_refused`, `run_failed`, `start_refused`, `panic`, `saved_state_invalid`, `saved_state_unavailable`, `saved_state_not_made`, `answer_not_made`, `answer_refused` and `saved_state_refused`.
- `InstanceLookup` (`book`, `run_context`, `book_lookup`), `RunContext` and `LifecycleHook` with `lifecycle_hook` and `InstanceEnded`.
- `DrainReport` and `DrainedRun`.
- Feature `test-host`: `TestHost`, `HostRun`, `HostEvent`, `HostCancel` and `advance_clock`.
- Three examples: `serve_runs` (the quickstart of the README, checked word for word by a test), `restartable_agent` and `test_host`; the tests run all three.
