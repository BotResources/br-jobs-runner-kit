# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
This file is the changelog of the frozen crates `br-jobs-runner-core` and
`br-jobs-runner-kit`, released up to the unified tag `v0.1.2`. Every registered
library keeps its own `crates/<crate>/CHANGELOG.md` with plain `## X.Y.Z`
headings and is released under the tag `{library}/v{version}`.

## Unreleased

### Fixed

- `images-check-gates.sh` matches the status functions `always`, `cancelled`, `failure` and `success` in any letter case, as GitHub does: `Always()` or `FAILURE()` in the condition of the job `manifest list` is now refused. `test-images-gates.sh` has 16 more tests (`Always`, `FAILURE`, `Cancelled`, `SUCCESS`, each in the plain, quoted, folded and literal forms of the condition).

### Changed

- `br-runner-jobs` 0.5.0 and `br-runner-threads` 0.8.0: depend on `br-agentic-harness-contract` 0.2.0. `br-runner-wiring-example` takes both crates from the workspace paths and moves to `br-agentic-harness-contract` 0.2.0 and `br-agentic-harness` 0.2.0. `br-runner-toolkit` keeps its dev-dependency on the tag `br-runner-jobs/v0.4.0`, so `Cargo.lock` holds a second copy of `br-agentic-harness-contract` (0.1.0) for that dev-only edge; `br-agentic-contract` has one copy (0.3.0). No source change of either crate; a consumer moves to the contract 0.2.0 together with the crates. `Cargo.lock` records the new versions. See `crates/br-runner-jobs/CHANGELOG.md` and `crates/br-runner-threads/CHANGELOG.md`.

- `images/br-sandbox-tools/README.md`: the licence table no longer says that no copyleft code is present. It lists the GCC runtime code that `gcc` 15.2.0 links into the static programs (start-up objects on both architectures; 16 `libgcc.a` members in `bsdtar` on `linux/arm64`, none on `linux/amd64`) under GPL-3.0-or-later WITH GCC-exception-3.1, with the measure. The `licenses` label of the image lists the GCC runtime exception.

- `br-runner-as-tool` 0.3.2: the error flag of a failed SYNC child ends with `Cause: <reason code>: <diagnostic text>` (see `crates/br-runner-as-tool/CHANGELOG.md`). `Cargo.lock` records its new version.

- `br-agentic-contract` is pinned in each crate that uses it (`br-runner-as-tool`, `br-runner-jobs`, `br-runner-threads`, `br-runner-toolkit`), no longer in the workspace dependencies. `br-runner-as-tool` is on `br-agentic-contract/v0.3.0` since its 0.3.0, `br-runner-threads` since its 0.6.1, `br-runner-jobs` since its 0.3.0, and `br-runner-toolkit` since its 0.3.0.

### Added

- `br-runner-sandbox` 0.1.0: the adapter of the sandbox port of `br-agentic-harness-contract` 0.2.0: the `Sandbox` type with the open, save and release flows, the warm pod watch, the cold pods and the erase of a thread (see `crates/br-runner-sandbox/CHANGELOG.md`). `Cargo.lock` records the new crate and its `tracing` dependency.
- Image `br-sandbox-python` (`images/br-sandbox-python/`): test and reference image of the sandbox, for `linux/amd64` and `linux/arm64`: Python 3.12, pandas, openpyxl, python-docx, python-pptx and headless LibreOffice. The base is pinned by index digest and the Python packages by version (no hashes); the Debian packages are not pinned. It works as any numeric non-root uid with no passwd entry and needs only `/bin/sh`, `cat` and `find`; it holds no sandbox tool and no agent code. Its `test.sh` starts the image with all capabilities dropped and no network, checks the installed packages against `requirements.txt`, converts a docx, an xlsx and a pptx to pdf (as uid 12345, as uid 54321 with gid 0, on a read-only root filesystem with tmpfs, and with an empty volume on `/home/agent`), and fails when the list of checks that ran differs from its fixed list.
- Image `br-sandbox-tools` (`images/br-sandbox-tools/`): static `bsdtar` (libarchive 3.8.11) and `pause` (our own reaper) for `linux/amd64` and `linux/arm64`, one manifest list, `FROM scratch`, under 5 MB per architecture. The README of the image holds the contract (paths, argv, exit rule, init command); its tests (`.github/scripts/test-tools-image.sh`) assert each documented behaviour, read the save, restore and init command lines from that README, and fail when a listed test has no result. The contract now states, with a test each: root without `CAP_SYS_ADMIN` cannot remount the read-only mount, what a save does with a socket, a fifo, a device node and a sparse file, `pause` printing nothing and ignoring its arguments and stdin, and one file read by `create` and written by `extract`.
- Workflows `.github/workflows/images.yml` and `.github/workflows/images-build.yml`, and `images/README.md`. `images-build.yml` (trigger `workflow_call` only; inputs `image`, `images_dir`, `push`, `tag`, `ref`; output `digests`, `{}` when nothing was pushed) builds every `images/<name>/Dockerfile` (repo root as context) for both architectures and tests each one. With `push` false it tests the loaded image and pushes nothing. With `push` true each platform image is pushed by digest with no tag, pulled back by that digest and tested; the tag `ghcr.io/botresources/<name>:<tag>` and the manifest list are created from those two digests only when every build job passed. If a test fails, the untagged platform digests stay in the registry; the tag and the list do not exist. `images.yml` (pull request, push to `main`, manual dispatch) decides `push` and `tag` from the event (`images-plan.sh`) and calls `images-build.yml`; a caller grants `packages: write`. `images/README.md` states the inputs, the policy, the `licenses` label requirement and the `images/<name>/test.sh` convention. A pull request whose head repository is empty or unknown does not push. Every pull request also runs `images-build.yml` with `push` false on the fixture folder `.github/fixtures/images/` (the image `no-tests`, no `test.sh`), which proves the no-push path and the path of an image with no tests; the fixture is never pushed. `images-check-gates.sh` checks that the job `manifest list` needs the build jobs with no status function in its condition. The tag `pr-<number>-<7>` of a pull request is never removed by the workflow.
- `br-runner-wiring-example` 0.0.0 (not published, no registry entry): an end-to-end example and tests that wire the toolkit, the jobs adapters (the harness mail port and the mark gate), the Threads adapter (`entry_stored`, the harness history) and `br-agentic-harness` over the in-process doubles. See `crates/br-runner-wiring-example/README.md`.
- `br-runner-toolkit` 0.3.1: re-pinned on `br-runner-jobs-client` 0.1.1 and `br-runner-jobs` 0.4.0 (dev-dependency); no change of the public surface. See `crates/br-runner-toolkit/CHANGELOG.md`.
- `br-runner-as-tool` 0.3.1: re-pinned on `br-runner-jobs-client` 0.1.1; no change of the public surface. See `crates/br-runner-as-tool/CHANGELOG.md`.
- `br-runner-jobs-client` 0.1.1: `NewMailInput::new` and the jobs seq of an unread item (`MailHandle::unread_sequenced`, `SequencedItem`); additive. See `crates/br-runner-jobs-client/CHANGELOG.md`.
- `br-runner-as-node` 0.3.1: re-pinned on `br-runner-jobs-client` 0.1.1; no change of the public surface. See `crates/br-runner-as-node/CHANGELOG.md`.
- `br-runner-toolkit` 0.3.0 (breaking): depends on `br-runner-jobs-client` 0.1.0 and `br-agentic-contract` 0.3.0 in place of `br-runner-jobs` 0.2.0, which stays only as a `dev-dependency`. The public builders and types name the types of the jobs client. See `crates/br-runner-toolkit/CHANGELOG.md`.
- `br-runner-jobs` 0.4.0 (breaking): adds the adapter of the harness mail port of `br-agentic-harness-contract` 0.1.0 and the fixes of the audit of 0.3.0, with a CI step that keeps the crate off the bus. See `crates/br-runner-jobs/CHANGELOG.md`.
- `br-runner-as-tool` 0.3.0 (breaking): depends on `br-runner-jobs-client` 0.1.0 and `br-agentic-contract` 0.3.0 in place of `br-runner-jobs`. See `crates/br-runner-as-tool/CHANGELOG.md`.
- `br-runner-as-node` 0.3.0 (breaking): imports `br-runner-jobs-client` 0.1.0 in place of `br-runner-jobs`; the re-export of RunnerType comes from the jobs client. See `crates/br-runner-as-node/CHANGELOG.md`.
- `br-runner-threads` 0.7.0: `ThreadAccessAdapter` implements the history port `HarnessHistory` of `br-agentic-harness-contract` 0.1.0 (additive, no break); `stored` gives `Stored` only when Threads confirmed the entry, and `Error::UnknownEntry` is added. Depends on `br-agentic-harness-contract` 0.1.0. See `crates/br-runner-threads/CHANGELOG.md`.
- `br-runner-jobs` 0.3.0 (breaking): the adapters of the incoming port (`IncomingPortAdapter`), on `br-agentic-contract` 0.3.0 and `br-runner-jobs-client` 0.1.0. See `crates/br-runner-jobs/CHANGELOG.md`.
- `br-runner-threads` 0.6.1: re-pinned on `br-agentic-contract` 0.3.0; no change of the public surface. See `crates/br-runner-threads/CHANGELOG.md`.
- `br-runner-jobs-client` 0.1.0 (new library): the typed client of the jobs service for a runner and the machinery around it, copied from the client part of `br-runner-jobs` 0.2.0. See `crates/br-runner-jobs-client/CHANGELOG.md`.
- `br-runner-threads` 0.1.0 (new library): the typed client of the Threads service for runner code.
- `br-runner-connection` 0.1.0 (new library): the one connection of a runner to the platform, a bearer-authenticated GraphQL call and one request-reply on the bus.
- `br-runner-threads` 0.2.0: rebuilt on the Connection, with the thread-port and thread-access adapters and the thread queue. See `crates/br-runner-threads/CHANGELOG.md`.
- `br-runner-jobs` 0.1.0 (new library): the jobs side of a runner, with a typed client of the jobs service, a delivery queue for each Run, and the adapters of the agent port and the inbox port. See `crates/br-runner-jobs/CHANGELOG.md`.
- `br-runner-jobs` 0.2.0: the started fact is a request with one typed reply, the arrival feed reads the version 2 doorbell through an ephemeral consumer whose acknowledgement wait and inactivity limit the importer gives, and the unread read gives mail, cancel and refusal notices. A typed runner type, a typed refusal reason and public makers of failures are added. See `crates/br-runner-jobs/CHANGELOG.md`.
- `br-runner-as-tool` 0.1.0 (new library): a runner type as a tool, the `Tool` of the agentic contract that makes another runner do a piece of work in sync or background mode. See `crates/br-runner-as-tool/CHANGELOG.md`.
- `br-runner-toolkit` 0.1.0 (new library): the main loop of a runner with the jobs service, with the registration through the presence, parallel run slots, the delivery of the facts of each run, the cancel orders, the drain and a test host. See `crates/br-runner-toolkit/CHANGELOG.md`.
- `br-runner-toolkit` 0.1.1: uses `br-runner-jobs` 0.2.0, matches a claim refusal by its typed reason, does not execute a Run whose started fact jobs refuses, and opens the arrival feed with an acknowledgement wait of 10 s and an inactivity limit of 120 s. See `crates/br-runner-toolkit/CHANGELOG.md`.
- `br-runner-as-tool` 0.1.1: uses `br-runner-jobs` 0.2.0, makes the runner type as its `RunnerType`, and a sync call whose child Job is cancelled by someone else ends with an error flag. See `crates/br-runner-as-tool/CHANGELOG.md`.
- `br-runner-threads` 0.3.0: follows contract-threads 0.7.0; `set_title` takes no title Job id and `fail_title_job` is removed. See `crates/br-runner-threads/CHANGELOG.md`.
- `br-runner-as-node` 0.1.0 (new library): `RunnerAsNode`, a node of a graph that makes another runner do one step by a sync child Job of the jobs service. See `crates/br-runner-as-node/CHANGELOG.md`.
- `br-runner-as-node` 0.2.0: builds on `br-runner-jobs` 0.2.0 (breaking: `RunnerType` replaces the re-export of `SubjectSegment`), and a child Job cancelled by another party now arrives as `Outcome::Cancelled`. See `crates/br-runner-as-node/CHANGELOG.md`.
- `br-runner-threads` 0.4.0: the thread-access adapter chains each message it writes to the tip of its thread, so Threads reads the whole conversation. See `crates/br-runner-threads/CHANGELOG.md`.
- `br-runner-threads` 0.5.0: `ThreadAccessAdapter::add_annotation(thread, turn_id, after, annotation)` queues an annotation chunk of an open AI turn, numbered and sealed with the turn's chunks, so a runner can name an artifact it uploaded in its reply. See `crates/br-runner-threads/CHANGELOG.md`.
- `br-runner-threads` 0.5.1: `ThreadAccessAdapter::reply_to(thread, reply, parent)` names the parent of one message not queued yet, by the id of that message, so a runner that writes a reply again chains it to the message it answers and not to the tip. See `crates/br-runner-threads/CHANGELOG.md`.
- `br-runner-threads` 0.5.2: `ThreadClient::read_context_of_branch(query, branch_tip)` reads the context of the branch that ends at one message, so a runner can answer a message that is not on the active branch. See `crates/br-runner-threads/CHANGELOG.md`.
- `br-runner-as-tool` 0.2.0: the runner type is the `RunnerType` of `br-runner-jobs` that the caller gives, the build fails only with the `ToolSpec` error, and the acknowledgement of a background call tells the agent about the notice "refused" and the answer by mail. Breaking: `RunnerTool::new` takes a `RunnerType`, and `RunnerToolError::RunnerType` is removed. See `crates/br-runner-as-tool/CHANGELOG.md`.
- `br-runner-toolkit` 0.2.0: a Run that a cancel order ended, before or after its graph started, hands over the failed fact `run_cancelled` through the failure mapping, never the completed fact, so jobs frees the capacity of a run it asked to stop; the settle logs the wait for the last observations of the executor only when they are really late. See `crates/br-runner-toolkit/CHANGELOG.md`.
- `br-runner-threads` 0.6.0: follows Threads 0.12 (contract-threads 0.10.0), where a message names no other message. Breaking: no parent message id on `WholeMessage` and `OpenStream`, `ThreadAccessAdapter::reply_to` is removed, and `ThreadClient::read_context_up_to(query, up_to)` replaces `read_context_of_branch`. See `crates/br-runner-threads/CHANGELOG.md`.

### Fixed

- `br-runner-connection` 0.1.1: input checks and error reading fixed (readable `UNAUTHENTICATED`, input checks, subject and size checks of the bus, the text of the service carried as given and no longer redacted, `Cause::NoListener`, the four examples). A dependency gate (`check-no-llm-clients.sh`) forbids any `br-llm-clients*` library in its graph. See `crates/br-runner-connection/CHANGELOG.md`.

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
