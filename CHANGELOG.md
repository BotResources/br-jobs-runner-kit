# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
The workspace releases as one unified version; this is its single changelog.
A release is a PR that bumps `[workspace.package] version` and adds the
matching `## [X.Y.Z] — YYYY-MM-DD` heading; merging it auto-tags `v{X.Y.Z}`.

## Unreleased

## [0.1.1] — 2026-08-18

First-consumer feedback release (ru-scaffold, the first runner on the kit).

### Fixed

- The initial NATS connect now retries in the background (`retry_on_initial_connect`) instead of failing the boot on the first refused dial — a pod scheduled while CNI/NATS readiness is still settling no longer crash-loops. Fail-loud is preserved: with the broker truly absent, the stream/bucket binds still error at boot.

### Added

- README (`br-jobs-runner-kit`): the deployment and abort semantics the first consumer had to discover by reading the source — `drain_timeout` must sit under the pod's `terminationGracePeriodSeconds` (Kubernetes default 30s vs the kit's 600s default); the drain-timeout abort is cooperative and cannot stop `spawn_blocking` closures or non-`kill_on_drop` child processes; process-global state needs a `Drop` guard; ack-as-claim means a broken-but-alive runner burns triggers rather than idling.

## [0.1.0] — 2026-08-17

### Added

- CI/CD: quality gates (fmt/clippy/tests+doctests, cargo doc, cargo-deny, cargo-machete, cargo-semver-checks against the previous unified tag, changelog + README-pin gates, shellcheck, trufflehog) and `release-tags.yml` auto-tagging `v{version}` with a GitHub Release when the bumped version has a dated changelog heading. Branch protection is declared in `scripts/setup-branch-protection.sh`.

- `br-jobs-runner-core`: pure runner-side state machines — `PresenceSession` (heartbeat scheduling, READY/DRAINING/delete), `SlotPool` (capacity-bounded run claims, drain refusal), `CancelSet` (cancel-bucket fold with full-replay swap), `Backoff` (capped exponential reconnect delays).
- `br-jobs-runner-kit`: the runner harness over `contract-jobs/v0.1.0` — `Runner` trait (typed payload from the trigger's `config`), `RunContext` (plan/step/log reporting stamped on the wire shapes, cancellation, `stub()` for infra-free runner tests), `RunOutcome` with the contract's structured `FailureReport` + `retry_after`, contract-validated `RunnerConfig`/`RunnerType`/`InstanceKey`, and the full NATS transport: `run`/`run_until` loop with ack-as-claim trigger consumption on a shared `runner_{type}` durable, slot-gated parallelism, presence heartbeat derived from the bucket TTL (READY/DRAINING/delete), cancel-bucket watch with full replay on reconnect, best-effort logs, bounded status-fact retries, graceful drain with timeout. Bind-only fail-loud infra; e2e-proven against a real NATS JetStream (7 scenarios).
