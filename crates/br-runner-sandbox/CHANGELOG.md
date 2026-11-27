# Changelog

## 0.1.1

### Fixed

- A restore no longer fails on the scope root directories (`/work`, `/home/agent`) of a pod whose agent user does not own them. The save keeps the entry of each scope root directory, and `bsdtar` run as the agent user could not set the modification time of a directory that the pod gives to another owner, so the exit code was 1 and every restore ended as `Fresh(RestoreFailed)`. The restore now carries one `--exclude` for the entry of each save scope root (`^work$`, `^home/agent$`), so that entry is not applied; the content under each root is restored as before, and a save made by 0.1.0 is restored the same way.
- An exit code of `bsdtar` other than 0 is still a failed restore. The adapter reads no standard error to accept a warning.
- The tests with the real `bsdtar` print a skip line and pass when `bsdtar` is absent or the user owns the root, so the proof on a pod is the cluster battery.

### Added

- `argv::tar_restore_scoped`: the restore argument list with the scope root entries skipped. `argv::tar_restore` is unchanged and skips nothing. The restore flow uses `tar_restore_scoped` with `save_scope_paths`.

### Changed

- The owner, the mode and the modification time of a scope root directory are those of the pod, not those of the save (see "Not guaranteed" of the README).

## 0.1.0

First release.

### Added

- The configuration of the adapter (`SandboxConfigInput`, `SandboxConfig`, `StoreSettings`, `StoreCredentials`, `NetworkMode`, `NetworkRule`, `AbsolutePath`, `LabelPrefix`, `PoolId`, `DEFAULT_LABEL_PREFIX`, `ConfigError`) with every key, default and rule of the minor.
- The public keys of the pod contract: `PodKeys`; the labels `LABEL_AGENT`, `LABEL_MANAGED`, `LABEL_POOL`, `LABEL_POOL_ID`, `LABEL_ROLE`, `LABEL_THREAD`; the annotations `ANNOTATION_CLAIMED_AT`, `ANNOTATION_CLEAN`, `ANNOTATION_IDLE_SINCE`, `ANNOTATION_SAVED_AT`, `ANNOTATION_SEED_HASH`, `ANNOTATION_STATE_BYTES`, `ANNOTATION_STATE_TURN`; the values `CLEAN_FALSE`, `CLEAN_TRUE`, `POOL_CLAIMED`, `POOL_WARM`, `ROLE_SANDBOX`, `MANAGED_TRUE`; the pool condition `CONDITION_WARM_PODS_FAILING` with the reasons `REASON_IMAGE_PULL_FAILING` and `REASON_START_CRASHING`. And the keys of the object store (`manifest_key`, `part_key`, `record_key`, `records_prefix`, `saves_prefix`, `tombstone_key`, `tombstones_prefix`).
- The formats of the manifest, the record and the tombstone (`Manifest`, `ManifestPart`, `PartReport`, `Record`, `RecordState`, `Tombstone`, `TOMBSTONE_REASON`, `Sha256Hex`), and the writer and the reader of a saved part (`PartEncoder`, `PartDecoder`, `TarCounter`).
- The archive of a seed set and the text of its hash (`seed_archive`, `seed_hash_text`, `seed_mode`, `check_seed_path`).
- The image errors (`ImageError`) as typed causes of a fault of the port, and the error type of the crate (`Error`), with the errors of its parts (`ExecError`, `FormatError`, `RecordError`, `SeedError`, `TarError`, `WebAccessMismatch`, `InvalidClientId`, `InvalidAttemptId`, `InvalidTimestamp`, `InvalidDigest`).
- The pure decisions of open, save and release (`check_web_access`, `judge_live_pod`, `live_expired`, `plan_start`, `recheck_after_restore`, `restore_failed`, `release_plan`, `cold_pod_manifest`, `cold_pod_marks`) and the patches of a claim, a save, a release and a seed (`warm_claim_patch`, `live_claim_patch`, `saved_patch`, `idle_patch`, `seed_hash_patch`), with the value types that they take and give: `LivePolicy`, `LiveVerdict`, `PodMarks`, `ColdPodMarks`, `StartPlan`, `StoreView`, `RestoreRecheck`, `ReleasePlan`.
- The object store boundary (`Store`, `PartUpload`, `PartStream`, `StoreFuture`, `PutOutcome`, `StoreError`) and its S3-compatible client (`S3Store`).
- The cluster boundary (`Cluster`, `ClusterFuture`, `ClusterError`, `PodView`, `ReplicaSetView`, `ConditionView`, `PodEvent`, `LabelSelector`, `ExecRequest`, `ExecIo`, `ExecOut`, `ExecStdin`).
- The context of the flows (`Env`), the clock (`Clock`, `SystemClock`), `Timestamp`, and the identifiers `ClientId`, `AttemptId` and `SaveKey`. `Env`, `Cluster`, `Store`, `PartUpload` and `Clock` are public so that the tests of the crate run the same flows on fakes; they are not an extension point.
- The exec layer of `Env` (`run`, `read`, `write`, `list`, `kill`), the bsdtar argument lists (`argv`), `parse_find_output`.
- The save flow, the restore flow, the seed write and the read of the store view of a key (`Env::save`, `restore`, `write_seed`, `load_manifest`, `read_store_view`, `SaveOutcome`, `RestoreVerdict`).
- The adapter `Sandbox`, built by `Sandbox::connect` (kube client, S3 client and the watch of the warm pods) or by `Sandbox::new` from an `Env`: the implementation of the sandbox port, with the open flow (instance first, mode check, live reuse by the time of the save, warm claim with the version check, the failing-pool check before the cold path, the cold pod from the pool template, the open timeout, restore and seed at the same time), release (idle-since at the release, a pod already gone is a success), `erase` of a thread, and the cleanup of a dropped `open`.
- `Sandbox::connect` and `S3Store::new` install the `ring` default crypto provider of rustls when the process has none. A provider that the caller set is kept.
- An `http` store endpoint is accepted; the S3 credentials then travel on that transport with no TLS.
- Live reuse of a kept pod needs the pod to be Ready. `kill` with a `ProcessId` that is not a positive decimal number is a port refusal. The `bsdtar` calls carry `--no-xattrs --no-acls --no-fflags`, so extended attributes, ACLs and file flags are not saved, restored or seeded.
- A `tracing` warning with the reason and the message (`ImageFailing`) or the wait (`ImageUnavailable`); the port error holds the typed cause only.
