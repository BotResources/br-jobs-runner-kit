# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
The workspace releases as one unified version; this is its single changelog.

## Unreleased

### Added

- `br-jobs-runner-core`: pure runner-side state machines — `PresenceSession` (heartbeat scheduling, READY/DRAINING/delete), `SlotPool` (capacity-bounded run claims, drain refusal), `CancelSet` (cancel-bucket fold with full-replay swap), `Backoff` (capped exponential reconnect delays).
- `br-jobs-runner-kit`: the runner-facing API surface — `Runner` trait, `RunContext` (plan/step/log reporting, cancellation, `stub()` for infra-free runner tests), `RunOutcome`/`FailureReport` with `retry_after`, validated `RunnerConfig`/`RunnerType`/`InstanceKey`. NATS transport intentionally absent until `contract-jobs/v0.1.0` is tagged.
