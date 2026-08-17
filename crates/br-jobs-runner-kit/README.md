# br-jobs-runner-kit

Harness for building runners compatible with
[svc-jobs](https://github.com/BotResources/svc-jobs). A runner author
implements one trait; the kit owns the whole NATS runner transport: presence
heartbeat, trigger claim, status facts, log shipping, cancel watch, graceful
drain.

## Surface

- `run(config, runner)` — connects, binds the declared streams/buckets
  (fail-loud, never provisions), announces presence, and processes triggers
  until SIGTERM/ctrl-c, then drains: stops claiming, finishes in-flight runs
  (bounded by `drain_timeout`), deletes the presence entry.
  `run_until(config, runner, shutdown)` takes a custom shutdown future.
- `Runner` — the one trait to implement: `execute(ctx, payload) -> RunOutcome`
  with a typed `Payload: DeserializeOwned`, decoded from the trigger's
  `config`; an undecodable payload fails the run with reason code
  `payload_invalid`.
- `RunContext` — handed to each run: `declare_plan(steps)`,
  `start_step(index, label)`, `log(message)` / `log_with(level, step, message)`,
  `cancelled().await` / `is_cancelled()`, `run_id()` / `job_id()` / `attempt()`.
  `RunContext::stub(run_id)` builds a test context exposing the emitted
  `RunFact`s and the cancel token, so runner authors unit-test with no infra.
- `RunOutcome` — `Completed`, or `Failed { report, retry_after }` with the
  contract's structured `FailureReport`; `RunOutcome::failed(code)` /
  `failed_retry_after(code, delay)` for the common cases. The `retry_after`
  hint may lengthen the server's backoff, never shorten it.
- `RunnerConfig` — `nats_url`, contract-validated `RunnerType`/`InstanceKey`,
  `runner_version`, `max_concurrent_runs` (default 1), `ack_wait`,
  `drain_timeout`. The heartbeat interval is derived from the presence
  bucket's TTL, not configured.
- Re-exports: `core` (`br-jobs-runner-core` state machines) and `contract`
  (`contract-jobs`, the wire).

## Cancellation is cooperative

The kit fires the run's token the moment its cancel entry appears; only the
workload knows where stopping is safe — check `is_cancelled()` between steps
or race long awaits against `cancelled()`.

| Thing | Why it is the way it is |
|---|---|
| Ack = claim, never ack-after-processing | Runs are LLM-agent length (hours); JetStream `ack_wait` redelivery cannot cover them. `RunStarted` is broker-acked first, then the trigger is acked; failure recovery is the server's presence-TTL reclaim. |
| No polling anywhere | Triggers ride a parked pull `messages()` stream pulled only when a slot is free; cancel rides a KV watch with full replay on (re)connect; the presence heartbeat put is the only timer in the process. |
| Heartbeat = presence-bucket TTL / 3, read from the bucket | The TTL is infra-owned; deriving the cadence from it makes a too-slow heartbeat impossible by construction. A bucket without `max_age` fails boot: TTL eviction is the crash signal. |
| Streams and buckets are bound, never created | Provisioning is gitops/server-side; a missing stream or bucket is a deployment bug and must fail loud at boot, not be papered over at runtime. |
| Trigger durable is `runner_{runner_type}`, shared | Instances of a type compete on one work-queue durable; JetStream distributes, `max_concurrent_runs` gates locally. |
| Status facts retry bounded (8 attempts), logs are fire-and-forget | Lifecycle facts matter but must not wedge a drain forever; after the budget the presence TTL is the server's backstop. The log stream is declared best-effort by the jobs spec. |
| A trigger already in the cancel set is acked and dropped | The server resolved that run's fate while it sat queued; starting it would only produce facts for a closed run. |
