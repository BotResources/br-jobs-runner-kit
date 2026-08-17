# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
The workspace releases as one unified version; this is its single changelog.
A release is a PR that bumps `[workspace.package] version` and adds the
matching `## [X.Y.Z] — YYYY-MM-DD` heading; merging it auto-tags `v{X.Y.Z}`.

## Unreleased

## [0.1.0] — 2026-08-17

### Added

- CI/CD: quality gates (fmt/clippy/tests+doctests, cargo doc, cargo-deny, cargo-machete, cargo-semver-checks against the previous unified tag, changelog + README-pin gates, shellcheck, trufflehog) and `release-tags.yml` auto-tagging `v{version}` with a GitHub Release when the bumped version has a dated changelog heading. Branch protection is declared in `scripts/setup-branch-protection.sh`.

- `br-jobs-runner-core`: pure runner-side state machines — `PresenceSession` (heartbeat scheduling, READY/DRAINING/delete), `SlotPool` (capacity-bounded run claims, drain refusal), `CancelSet` (cancel-bucket fold with full-replay swap), `Backoff` (capped exponential reconnect delays).
- `br-jobs-runner-kit`: the runner harness over `contract-jobs/v0.1.0` — `Runner` trait (typed payload from the trigger's `config`), `RunContext` (plan/step/log reporting stamped on the wire shapes, cancellation, `stub()` for infra-free runner tests), `RunOutcome` with the contract's structured `FailureReport` + `retry_after`, contract-validated `RunnerConfig`/`RunnerType`/`InstanceKey`, and the full NATS transport: `run`/`run_until` loop with ack-as-claim trigger consumption on a shared `runner_{type}` durable, slot-gated parallelism, presence heartbeat derived from the bucket TTL (READY/DRAINING/delete), cancel-bucket watch with full replay on reconnect, best-effort logs, bounded status-fact retries, graceful drain with timeout. Bind-only fail-loud infra; e2e-proven against a real NATS JetStream (7 scenarios).
