# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
This file is the changelog of the frozen crates `br-jobs-runner-core` and
`br-jobs-runner-kit`, released up to the unified tag `v0.1.2`. Every registered
library keeps its own `crates/<crate>/CHANGELOG.md` with plain `## X.Y.Z`
headings and is released under the tag `{library}/v{version}`.

## Unreleased

### Added

- `br-runner-threads` 0.1.0 (new library): the typed client of the Threads service for runner code.
- `br-runner-connection` 0.1.0 (new library): the one connection of a runner to the platform, a bearer-authenticated GraphQL call and one request-reply on the bus.

### Fixed

- `br-runner-connection` 0.1.1: the code follows the corrected intent of 0.1 (readable `UNAUTHENTICATED`, input checks, subject and size checks of the bus, the text of the service carried as given and no longer redacted, `Cause::NoListener`, the four examples). The extension of the runner-transport exception to the bus access of this crate is pending ratification. A dependency gate (`check-no-llm-clients.sh`) forbids any `br-llm-clients*` library in its graph. See `crates/br-runner-connection/CHANGELOG.md`.

### Security

- `Cargo.lock` only: `rustls` 0.23.43 → 0.23.45 (RUSTSEC-2026-0285, TLS 1.3
  handshake messages accepted across encryption level boundaries),
  `rustls-webpki` 0.103.14 → 0.103.15, and `chacha20` 0.10.1 (yanked) →
  0.10.2, all reached through `async-nats`. No manifest range and no source
  change, so no release: a consumer resolves its own lockfile and takes the
  fixed versions with `cargo update -p rustls -p chacha20`. This makes the
  `cargo-deny` advisories gate green again on `main`.
- `Cargo.lock` only: `rustls` 0.23.45 → 0.23.46 (RUSTSEC-2026-0346,
  RUSTSEC-2026-0347, RUSTSEC-2026-0348, RUSTSEC-2026-0349), `rustls-webpki`
  0.103.15 → 0.103.16 (RUSTSEC-2026-0382), `webpki-roots` 1.0.9 → 1.0.11
  (RUSTSEC-2026-0381) and `tokio` 1.53.1 → 1.54.1 (RUSTSEC-2026-0386), all
  reached through `async-nats`. No manifest range and no source change, so no
  release.

### Changed

- Release pipeline: the unified `v{version}` auto-tag is replaced by the Libraries-registry per-crate release (`{library}/v{version}`), gated by the registry and recorded in it. `br-jobs-runner-core` and `br-jobs-runner-kit` are not registered libraries: they are frozen at 0.1.2 and the pipeline refuses any bump of them. Their `version` is now a literal in each crate manifest (no `[workspace.package] version`). No source change, no release.

## [0.1.2] — 2026-08-18

### Fixed

- `RunContext::log` and `log_with` now publish the `INFO`, `WARNING`, and
  `ERROR` values required by `svc-jobs`. The convenience API accepts these
  levels case-insensitively and treats `WARN` as `WARNING`; unsupported levels
  are rejected locally with an operational warning instead of being published
  and permanently discarded by Jobs.

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
