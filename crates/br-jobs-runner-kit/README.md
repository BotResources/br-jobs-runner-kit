# br-jobs-runner-kit

Harness for building runners compatible with
[svc-jobs](https://github.com/BotResources/svc-jobs). A runner author
implements one trait; the kit owns the whole NATS runner transport: presence
heartbeat, trigger claim, status facts, log shipping, cancel watch, graceful
drain.

## Install

Distributed by git tag (no crates.io); the whole workspace ships as one
version and the `version` must accompany the tag:

```toml
[dependencies]
br-jobs-runner-kit = { git = "https://github.com/BotResources/br-jobs-runner-kit", package = "br-jobs-runner-kit", tag = "v0.1.1", version = "0.1.1" }
```

If your project runs `cargo-deny`, allowlist both git sources — the kit and
its transitive `contract-jobs` pin:

```toml
[sources]
allow-git = [
    "https://github.com/BotResources/br-jobs-runner-kit",
    "https://github.com/BotResources/svc-jobs",
]
```

## A complete runner

```rust,no_run
use br_jobs_runner_kit::{
    InstanceKey, RunContext, RunOutcome, Runner, RunnerConfig, RunnerType, run,
};

struct Summarizer;

#[derive(serde::Deserialize)]
struct SummarizeJob {
    url: String,
}

impl Runner for Summarizer {
    type Payload = SummarizeJob;

    async fn execute(&self, ctx: RunContext, job: Self::Payload) -> RunOutcome {
        ctx.declare_plan(vec!["fetch".into(), "summarize".into()]);
        ctx.start_step(0, "fetch");
        ctx.log(format!("fetching {}", job.url));
        if ctx.is_cancelled() {
            return RunOutcome::failed("cancelled");
        }
        ctx.start_step(1, "summarize");
        RunOutcome::Completed
    }
}

#[tokio::main]
async fn main() -> Result<(), br_jobs_runner_kit::HarnessError> {
    let config = RunnerConfig::new(
        "nats://localhost:4222",
        RunnerType::new("summarizer").expect("a valid runner type"),
        InstanceKey::new("pod-0").expect("a valid instance key"),
        env!("CARGO_PKG_VERSION"),
    );
    run(config, Summarizer).await
}
```

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
  bucket's TTL, not configured. On Kubernetes, `drain_timeout` (default 600s)
  MUST sit under the pod's `terminationGracePeriodSeconds` (Kubernetes
  default: 30s) — past the grace period the pod is SIGKILLed mid-drain while
  the claimed triggers are already acked; recovery then waits on the server's
  presence-TTL reclaim, not on redelivery.
- Re-exports: `core` (`br-jobs-runner-core` state machines) and `contract`
  (`contract-jobs`, the wire).

## Cancellation is cooperative

The kit fires the run's token the moment its cancel entry appears; only the
workload knows where stopping is safe — check `is_cancelled()` between steps
or race long awaits against `cancelled()`.

The drain-timeout abort is cooperative too: it drops each run's future at its
next await point. Work that does not yield is NOT stopped — a `spawn_blocking`
closure runs to completion on its thread, and a child process keeps running
unless spawned with `kill_on_drop` — so a runner already reported as drained
can still act until the process exits. Two consequences for runner authors:
structure long work around the cancellation token (or killable child
processes), and restore any process-global state through a `Drop` guard, never
in code after an await — the abort means that code may never run.

| Thing | Why it is the way it is |
|---|---|
| Ack = claim, never ack-after-processing | Runs are LLM-agent length (hours); JetStream `ack_wait` redelivery cannot cover them. `RunStarted` is broker-acked first, then the trigger is acked; failure recovery is the server's presence-TTL reclaim. Corollary: a broken-but-alive runner does not idle — it keeps winning triggers and failing them. The kit does not self-quarantine; watch `RunFailed` streaks server-side or in your workload. |
| Initial NATS connect retries in the background (`retry_on_initial_connect`) | Pod scheduling races CNI/NATS readiness at deploy; a bare failed dial would crash-loop a pod next to a healthy broker. Fail-loud is preserved: with the broker truly absent, the stream/bucket binds still error at boot. |
| No polling anywhere | Triggers ride a parked pull `messages()` stream pulled only when a slot is free; cancel rides a KV watch with full replay on (re)connect; the presence heartbeat put is the only timer in the process. |
| Heartbeat = presence-bucket TTL / 3, read from the bucket | The TTL is infra-owned; deriving the cadence from it makes a too-slow heartbeat impossible by construction. A bucket without `max_age` fails boot: TTL eviction is the crash signal. |
| Streams and buckets are bound, never created | Provisioning is gitops/server-side; a missing stream or bucket is a deployment bug and must fail loud at boot, not be papered over at runtime. |
| Trigger durable is `runner_{runner_type}`, shared | Instances of a type compete on one work-queue durable; JetStream distributes, `max_concurrent_runs` gates locally. |
| Status facts retry bounded (8 attempts), logs are fire-and-forget | Lifecycle facts matter but must not wedge a drain forever; after the budget the presence TTL is the server's backstop. The log stream is declared best-effort by the jobs spec. |
| A trigger already in the cancel set is acked and dropped | The server resolved that run's fate while it sat queued; starting it would only produce facts for a closed run. |
