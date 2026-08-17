# br-jobs-runner-kit

Harness for building runners compatible with
[svc-jobs](https://github.com/BotResources/svc-jobs). A runner author
implements one trait; the kit owns the NATS lifecycle (presence heartbeat,
trigger claim, status facts, cancel watch).

**Status: API surface only.** The NATS transport lands once
`contract-jobs/v0.1.0` is tagged in the svc-jobs repo.

## Surface

- `Runner` — the one trait to implement: `execute(ctx, payload) -> RunOutcome`,
  with a typed `Payload: DeserializeOwned`.
- `RunContext` — handed to each run: `declare_plan`, `start_step`, `log`,
  `cancelled().await` / `is_cancelled()`, `run_id()`. `RunContext::stub()`
  builds a test context exposing the emitted `RunFact`s and the cancel token,
  so runner authors can unit-test without any infra.
- `RunOutcome` / `FailureReport` — completed, or failed with a reason and an
  optional `retry_after` hint (the server may lengthen its backoff from it,
  never shorten).
- `RunnerConfig`, `RunnerType`, `InstanceKey` — validated configuration;
  defaults: one run slot, 30s presence TTL, 10s heartbeat.
- `core` — re-export of `br-jobs-runner-core`, the pure state machines the
  transport will drive.

| Thing | Why it is the way it is |
|---|---|
| `RunFact` is an internal enum, not the wire | Placeholder until the transport maps it onto `contract-jobs` types; it is replaced at wiring, never kept alongside the contract. |
| `RunnerType`/`InstanceKey` charset (`a-z0-9-_`, ≤64) | Stand-in precondition until `contract-jobs`'s `SubjectSegment` becomes the validator at wiring; conservative enough to never emit a subject-breaking key. |
| Fact emission is best-effort (`send` errors ignored) | A dropped receiver only happens when the harness side is gone and the run is being torn down; logs/status are declared best-effort by the jobs spec. |
| Ack = claim, never ack-after-processing | Runs are LLM-agent length (hours); JetStream `ack_wait` redelivery cannot cover them. Failure recovery is the presence-TTL reclaim, server-side. |
