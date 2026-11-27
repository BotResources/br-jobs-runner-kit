# Changelog

## 0.1.3

### Fixed

- The save now reads the exec output of its `bsdtar` at full speed, which avoids the condition that triggers the cut (a slow reader, see Known issues). Before, one read of at most 64 KiB was followed by the hashes, the tar count, the compression and an awaited upload write before the next read. Now one future reads the output and sends each chunk to a queue in memory, and a second future receives the chunks, hashes, counts, compresses and uploads them; the status of the exec and the standard error are read at the same time, as before. A failure of the second future (the store, the compression, the tar count) ends the save as before and stops the read; a closed queue stops the read with an error. The save keeps the sentences of the intent: zstd in the runner, one part, the two SHA-256 and the counts, the record states, `save_timeout_seconds`. The check of the count of the tar entries stays: a cut that still occurs fails closed (`Error::Format` with the record `failed` and the error `exec`) and no manifest is written. The adapter does not retry a cut stream.
- The breach about the cut of a large save is open until the rows of the cluster battery pass; this version does not claim that the cut is fixed.
- `read` and `run` were not changed.

### Known issues

- The virtual-host style of the store (`store_path_style` false, the default) builds the object URL with a double slash before the key. A test store refused the request with a 405; the effect on a store that keeps the empty segment was not tested. Use the path style (`store_path_style` true).
- A background `run` returns the id of the shell that runs the command text, not always the id of the command itself: a `kill` with that id may leave the command running, and the processes that the killed process started are not killed by `kill`.
- A save of a large tree can fail on a node under heavy load; the failure is a port fault and no manifest is written, so no save exists (an uploaded part may remain in the store until the sweep).
- The output of the save that is read and not yet compressed and uploaded is held in memory, and nothing is written to disk by the adapter. Worst case: one save can hold the raw size of the save scope (the whole tar of the work folder) in the memory of the runner, for each save that runs at the same time, because the reader fills the memory with no bound when the upload is slower than the read. The encoder uses memory too. This version sets no limit on the size of a save and keeps no spool on disk.
- The end of the exec output is lost on the server side of the exec stream: it was seen with a second client, `kubectl` 1.34 on a test cluster of one node, so it is not in the adapter and not in the Rust library. The pace of the reader triggers it, and the pace that triggers it is not a fixed number. A fast reader got all the bytes in 10 of 10 runs; a slow reader (1 KiB reads, a pause of 50 ms for each 256 KiB) got fewer bytes in 18 of 20 runs, and in most of the short runs the command ended with exit code 0 and nothing on stderr; a pause of 5 ms for each 256 KiB lost nothing in 10 runs. With the library that the adapter uses, a paced reader that hashes, compresses and counts lost bytes in 36 of 40 runs and a fast reader in 0 of 40. In all these runs the loss came before the status of the exec. Not shown: which part of the server loses the bytes, and other clusters. The adapter cannot give the lost bytes back: the save reads at full speed, which avoids the condition, and a cut that still occurs fails closed (the tar count check).
- `read` and `run` read the exec output with a fast reader and have no check of the count of bytes. A slow reader of `read` or `run` could lose output with no error: a known open point, not changed in 0.1.3.

## 0.1.2

### Fixed

- A pod that is gone, not `Running` or being deleted while an exec runs is now a port fault (`Error::Cluster(ClusterError::Gone)`), not an exit code, except in one window: the pod is read once at one instant, and a pod that the kubelet is evicting can still show `Running` with no deletion mark, so an exit code can still come back then. The API ends the exec stream of a killed process with a status of 137, and a foreground `run` gave `Ok(Finished)` with exit code 137 and empty streams. When the status of an exec of `run`, `read`, `write`, `list` or `kill` is 128 or more, the adapter reads the pod once; a pod that is not found, that has a deletion timestamp, or whose phase is not exactly `Running` (no phase, `Pending`, `Unknown`, `Failed`, `Succeeded`), is the fault `Gone`. A pod that is `Running` and not being deleted keeps the status as it was: a command that really ends with 137 (`kill -9`, the out-of-memory killer) is still `Ok` with exit code 137 for a foreground `run`, a `read` of a file that cannot be read is still a refusal, and a background `run`, a `write`, a `list` or a `kill` keeps its exec failure.
- When the read of the pod fails, the operation is a port fault (`ClusterError::Call`), not an exit code. A status below 128 reads no pod.
- `save` needed no change: any status other than 0 of the `bsdtar` of a save is already a port fault.

### Known issues

- The virtual-host style of the store (`store_path_style` false, the default) builds the object URL with a double slash before the key. A test store refused the request with a 405; the effect on a store that keeps the empty segment was not tested. Use the path style (`store_path_style` true).
- A background `run` returns the id of the shell that runs the command text, not always the id of the command itself: a `kill` with that id may leave the command running, and the processes that the killed process started are not killed by `kill`.
- A save of a large tree can fail on a node under heavy load; the failure is a port fault and no manifest is written, so no save exists (an uploaded part may remain in the store until the sweep).

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
