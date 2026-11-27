# br-runner-sandbox

The adapter library of the sandbox port of `br-agentic-harness-contract`: a pod of a Kubernetes cluster, claimed from a pool of warm pods, with a save and a restore of the work folder between the turns of one agent. `Sandbox` implements the eight operations of the port (open, run, read, write, list, kill, save, release) for one namespace and one pool, and has one extra function, `erase`, for the saves of a thread.

## Install

```toml
br-runner-sandbox = { git = "https://github.com/BotResources/br-jobs-runner-kit", package = "br-runner-sandbox", tag = "br-runner-sandbox/v0.1.2", version = "0.1.2" }
br-agentic-harness-contract = { git = "https://github.com/BotResources/br-llm-graph-rs", package = "br-agentic-harness-contract", tag = "br-agentic-harness-contract/v0.2.0", version = "0.2.0" }
```

A crate that names the types of a sibling in its public API must reach that sibling through the same git url and the same tag string, or the types are two copies: the pin of `br-agentic-harness-contract` above is the one this crate is built with. The crate does not compile against a version of the contract below 0.2.0.

## Public surface

Every item below is public, and no other item is. The cluster, store and clock traits (`Cluster`, `Store`, `PartUpload`, `Clock`) and `Env` are public so that the tests of the crate run the same flows on fakes. They are not an extension point and no other shape is promised: a caller builds the adapter with `Sandbox::connect`.

| Group | Items |
|---|---|
| Configuration | `SandboxConfigInput`, `SandboxConfig`, `StoreSettings`, `StoreCredentials`, `NetworkMode`, `NetworkRule`, `AbsolutePath`, `LabelPrefix`, `PoolId`, `DEFAULT_LABEL_PREFIX`, `ConfigError`. The input is filled by the importer (it deserializes from the keys below; a key that is not in the table is not read). `SandboxConfig::try_from` checks every rule and gives a `ConfigError`. The adapter reads no environment variable and no file. `Debug` and `Display` of the configuration show no part of `store_credentials`. |
| Keys and pod contract | `PodKeys`, `LABEL_AGENT`, `LABEL_MANAGED`, `LABEL_POOL`, `LABEL_POOL_ID`, `LABEL_ROLE`, `LABEL_THREAD`, `ANNOTATION_CLAIMED_AT`, `ANNOTATION_CLEAN`, `ANNOTATION_IDLE_SINCE`, `ANNOTATION_SAVED_AT`, `ANNOTATION_SEED_HASH`, `ANNOTATION_STATE_BYTES`, `ANNOTATION_STATE_TURN`, `CLEAN_FALSE`, `CLEAN_TRUE`, `POOL_CLAIMED`, `POOL_WARM`, `ROLE_SANDBOX`, `MANAGED_TRUE`, `CONDITION_WARM_PODS_FAILING`, `REASON_IMAGE_PULL_FAILING`, `REASON_START_CRASHING`, `manifest_key`, `part_key`, `record_key`, `records_prefix`, `saves_prefix`, `tombstone_key`, `tombstones_prefix`. The exact text of the labels, annotations and the pool condition, with the default prefix `br.sandbox`, and the keys of the object store. `PodKeys` rewrites the prefix of the same items for another `label_prefix`, so no key has a second copy. Each store key holds the client, the thread, the agent and the turn. |
| Sandbox | `Sandbox`. The adapter. `Sandbox::connect(config)` builds the kube client and the S3 client and starts the watch of the warm pods; `Sandbox::new(env)` takes an `Env` built by the caller. Both must run inside a Tokio runtime, because the watch of the warm pods is a task of that runtime; dropping the last clone of the adapter stops it. It implements `SandboxPort`. `open_box`, `save_box` and `release_box` are the same operations with the error type of the crate. `erase(thread)` deletes the pods of the thread in the namespace, then every object under the three prefixes of the thread for the client of the adapter (manifests first, then tombstones, records and parts); a failure is a port fault. |
| Cluster seam | `Cluster`, `ClusterFuture`, `ClusterError`, `PodView`, `ReplicaSetView`, `ConditionView`, `PodEvent`, `LabelSelector`, `ExecRequest`, `ExecIo`, `ExecOut`, `ExecStdin`. The boundary of the kube API (list, get, patch, create and delete of pods, the pool ReplicaSet and its condition, the pod watch, the pod exec with stdin, stdout, stderr and the exit code). `Sandbox::connect` uses the implementation on `kube`, which is not public. |
| Store seam | `Store`, `PartUpload`, `PartStream`, `StoreFuture`, `PutOutcome`, `StoreError`, `S3Store`. The object store boundary (get, streamed get, size, put, conditional create, streamed upload, list, delete) and its S3-compatible client with the system TLS roots. A conditional create that finds the object gives `PutOutcome::AlreadyExists`. |
| Context, clock and identifiers | `Env`, `Clock`, `SystemClock`, `Timestamp`, `ClientId`, `AttemptId`, `SaveKey`. `Env` holds the configuration, a `Cluster`, a `Store` and a clock; `Timestamp` keeps milliseconds. `Env::new`, `pod_keys`, `agent_request`, `seed_request` build the context and the exec requests. `Env::run`, `read`, `write`, `list`, `kill` are the exec layer: `sh -c` in `/work`, `cat`, `find`, a write through bsdtar extract with the stdin closed at the end, and a kill by process id. `Env::save`, `restore`, `write_seed`, `load_manifest`, `read_store_view` are the save flow (a streamed part, then the record and the manifest as the commit mark, then the pod annotations), the restore flow with the manifest recheck, the seed write through the seed container, and the read of the store view of a key. |
| Formats | `Manifest`, `ManifestPart`, `PartReport`, `Record`, `RecordState`, `Tombstone`, `TOMBSTONE_REASON`, `Sha256Hex`, `PartEncoder`, `PartDecoder`, `TarCounter`, `SaveOutcome`, `RestoreVerdict`. The JSON formats of the saved objects, checked when they are read; the writer and the reader of a part (one zstd stream of one tar, with the size and the SHA-256 of the raw and of the stored bytes, and the count of the tar entries); the result of a save and of a restore. |
| Errors | `Error`, `ImageError`, `ExecError`, `FormatError`, `RecordError`, `SeedError`, `TarError`, `WebAccessMismatch`, `InvalidClientId`, `InvalidAttemptId`, `InvalidTimestamp`, `InvalidDigest`, with `ConfigError` and `StoreError` and `ClusterError` from the groups above. `Error` is the error type of the crate. The image errors reach the port as the cause `OpenFault` of a fault, which `OpenFault::of` reads. |
| Helpers | `argv` (the module with `tar_create`, `tar_restore`, `tar_restore_scoped`, `tar_write`, `tar_seed`, `run_foreground`, `run_background`, `read`, `list`, `kill`: the argument lists of the execs), `parse_find_output`, `seed_archive`, `seed_hash_text`, `seed_mode`, `check_seed_path`, and the decisions of open, save and release as pure functions of values: `check_web_access`, `judge_live_pod`, `live_expired`, `plan_start`, `recheck_after_restore`, `restore_failed`, `release_plan`, `cold_pod_manifest`, `cold_pod_marks`, `warm_claim_patch`, `live_claim_patch`, `saved_patch`, `idle_patch`, `seed_hash_patch`, with the values that they take and give: `LivePolicy`, `LiveVerdict`, `PodMarks`, `ColdPodMarks`, `StartPlan`, `StoreView`, `RestoreRecheck`, `ReleasePlan`. The archive of a seed set has the owner 0:0 and the write bits of the mode removed. |

## Behaviour of the adapter

| Call | What the adapter does |
|---|---|
| `open` | It takes the instance first (a second open of an instance that has an open box is a refusal). Then, in this order: the mode check against `network_mode` (a refusal, before any call to the cluster or the store); with no parent turn the start is `Fresh(NoSaveEver)` and the store is not read; with a parent, the live reuse of a kept pod of the thread and the agent (one list by labels, one patch that carries the version read; an expired save of a pod deletes the pod and gives `Fresh(SaveExpired)`), then the three reads of the store; the claim of a Ready warm pod (one patch with the version, the next pod on a conflict or a missing pod, at most `claim_max_attempts` tries); when no Ready warm pod exists and the condition `WarmPodsFailing` of the pool ReplicaSet is `True`, `ImageFailing` at once with no cold pod; the cold pod from the template of the pool ReplicaSet (`pool=claimed`, thread and agent labels, `clean=false`, `claimed-at`, no owner reference, the `role` label and the spec untouched), waited for at most `open_timeout_seconds`; the restore and the seed write at the same time; the result. A restore that does not end well deletes its pod, and a new pod is claimed with the seed only. A seed write that fails deletes the pod and is a port fault. A pool ReplicaSet that does not exist is a port fault. |
| a dropped `open` | The instance is removed at once, and the pod that the call claimed or made is deleted by a task of the runtime, from the moment the claim patch or the create call has returned. A drop while that patch or create call is in flight can leave the pod that it claimed or made with `clean=false`, held by no box of this adapter, because the adapter has no name to hold before the call returns (a cold pod is named by the cluster, and a pod held before a conflicting claim would delete the pod of another open). A delete that is lost leaves the same pod. In both cases the component that manages the pool removes a pod with `clean=false`. |
| `save` | The save flow of `Env::save`. The box is marked saved in memory only when the pod annotation patch succeeded. |
| `release` | A saved box with `bytes_raw` at or above `idle_keep_min_state_bytes` keeps its pod and gets `idle-since` set at the release; any other box has its pod deleted. A pod that is already gone is a success. The box is marked released in every case, also when the delete fails (a port fault). Releasing twice is accepted. |
| `run`, `read`, `write`, `list`, `kill`, `save` on a box that was never opened or is released | A port fault, with no exec. |

The harness drives the order of open, save and release through the `RunLifecycle` trait, which belongs to the harness: this crate holds no implementation of it and does not depend on the harness.

## Details of the calls

- Live reuse of a kept pod also needs the pod to be Ready and not in deletion, in addition to the conditions on its annotations.
- `kill` takes a `ProcessId` that is a positive decimal number (no sign, no leading zero, within the range of `u32`). Any other text is a port refusal, with no exec. A process that has already ended is not an error.
- Every `bsdtar` call carries `--no-xattrs --no-acls --no-fflags`, and the save adds `--options hdrcharset=BINARY`. Extended attributes, ACLs and file flags are not saved, not restored and not written by a seed or by `write`.
- When the status of an exec of `run`, `read`, `write`, `list` or `kill` is 128 or more, the adapter reads the pod once. A pod that is not found, that is being deleted, or whose phase is not exactly `Running` (no phase, `Pending`, `Unknown`, `Failed`, `Succeeded`) is a port fault (`ClusterError::Gone`), and a read of the pod that fails is a port fault (`ClusterError::Call`); in a `Running` pod that is not being deleted the status is kept (a command that ends with 137 is an exit code). A status below 128 reads no pod. `save` needs no such read: any status other than 0 of its `bsdtar` is a port fault.
- A store endpoint with the scheme `http` is accepted. The S3 credentials (the access key id, and the signature made with the secret key) then travel on that transport with no TLS. Whether that is allowed is the choice of the caller and of its network.

## Not guaranteed

- The owner, the mode and the modification time of a scope root directory (`/work`, `/home/agent` by default) are those of the pod, not those of the save: the restore does not apply the entry of a scope root directory, because the agent user does not own the mount roots of the pod and cannot set their time. Everything under a scope root is restored with its content, mode, numeric owner and modification time. A save made by 0.1.0 holds those entries and is restored the same way. A scope root that is a file, not a directory, is not restored; the save scope is a list of directories.
- The adapter does not authenticate its caller and does not check the access to a thread. The caller gives the thread and the agent from its own context, and a caller that gives another thread gets the saves of that thread. The caller calls `erase` only for a thread that it may erase.
- The store must honour a conditional create (`If-None-Match`). `S3Store` writes the manifest with one. A store that answers that the conditional create is not implemented gives `StoreError::ConditionalCreateUnsupported`. A store that accepts the call and ignores the condition is not detected and is not supported: a second writer of one key may then replace the manifest.
- The caller opens one turn of an agent at a time. Two turns of one agent at the same time are not guaranteed to be safe: each gets its own pod, and a save of a turn that already has a manifest is the refusal "already saved".
- A `Continued` box may still hold the processes that an earlier turn left. A `Restored` box holds none.
- No limit is set on the size or on the number of files of a save or of a seed. A save is one part. A save above the disk of the pod is not handled. A restore is bounded by `restore_timeout_seconds`: one that exceeds it ends as `Fresh(RestoreFailed)`. Its speed is not guaranteed (no figure such as 100 MB in 500 ms is promised).
- A box does not survive a restart of the adapter: the adapter keeps its boxes in memory, so an operation on a `BoxId` of an earlier run is a port fault. The pod keeps its annotations, and the next open starts from them or from the last save by the rules of `open`.
- The check of the pod after an exec status of 128 or more reads the pod once, after the status, at one instant. A pod that the kubelet is evicting can still show the phase `Running` with no deletion mark at that instant, so an exit code can still be returned in that window.
- The virtual-host style of the store (`store_path_style` false, the default) has a known defect: the object URL is built with a double slash before the key, which a strict store refuses and a store that keeps the empty segment reads as another key; use the path style (`store_path_style` true).

## What the adapter expects from the chart

- The pool is the ReplicaSet `warm-{pool_id}` of the namespace of the adapter. Its pod template carries the labels `role=sandbox`, `pool=warm` and `pool-id={pool_id}` (with the prefix of `label_prefix`).
- A pod has two containers: the agent container (`agent_container`, `sandbox` by default), which runs as a non-root user, and the seed container (`seed_container`, `seed` by default), which runs as root in the user namespace of the pod with `pause` as its process.
- A volume of kind `emptyDir` is mounted at `seed_root` (`/skills` by default): read-write in the seed container, read-only in the agent container.
- The static `bsdtar` is at `seed_tools_path` (`/sbx/bin/bsdtar` by default) in the seed container and at `tools_path` (`/opt/sbx/bin/bsdtar` by default) in the agent container.
- `/work` and `/home/agent` are writable by the agent user. The image of the agent container has `/bin/sh`, `cat` and `find`.
- The rights of the adapter in its namespace: on `pods` get, list, watch, patch, create and delete; on `pods/exec` create; on `replicasets` get, list and watch. The adapter needs no right on secrets and no right in another namespace.

## What the adapter expects from the component that manages the pool

The adapter does not write any of the following. Another component does, and the adapter only reads the result.

- The component writes the condition `WarmPodsFailing` on the status of the pool ReplicaSet: status `True`, with the reason `ImagePullFailing` or `StartCrashing`, when a warm pod of the pool fails to start, and `False` when none fails. This crate never writes that condition. Without it the adapter never gives `ImageFailing`: when no pod can be used, `open` ends with `ImageUnavailable`, and with `cold_create` on it first waits `open_timeout_seconds` for the cold pod.
- The component removes the pods that stay with `clean=false` and the kept pods that are past their idle time, expires the saves (it writes the tombstone, then deletes the manifest, then the part), and sweeps the parts of uploads that never got a manifest. The adapter writes no tombstone and deletes no save by age (`erase` aside).

## What the adapter expects from the bucket

- The bucket named by `store_bucket` exists. The adapter never creates it.
- The credentials in `store_credentials` allow, on that bucket, get, put (including the conditional create of the manifest), delete, list, and multipart upload (the part of a save is a streamed multipart upload).

## Image errors

`ImageFailing` and `ImageUnavailable` leave `open` as the cause `OpenFault` of a port fault, which `OpenFault::of` reads. That cause is a value with no field and the port reads it by its exact type, so the reason and the message of the condition (`ImageFailing`) and the wait that ended (`ImageUnavailable`) cannot travel in the port error. The adapter writes them in a `tracing` warning (`reason` and `message`, or `waited_ms`) when it makes the error, and a caller that holds the adapter reads them as `Error::Image(ImageError)` from `open_box`.

## Configuration keys

| Key | Default |
|---|---|
| `namespace`, `client_id`, `pool_id`, `network_mode`, `save_ttl_seconds`, `store_endpoint`, `store_bucket`, `store_region`, `store_credentials` | none, required |
| `kube_context`, `exec_timeout_seconds`, `save_timeout_seconds` | absent |
| `label_prefix` | `br.sandbox` |
| `allowlist` | empty (not empty for `allow`, empty for `none` and `full`) |
| `save_scope_paths` | `["/work", "/home/agent"]` |
| `save_zstd_level` | 3 |
| `restore_timeout_seconds`, `open_timeout_seconds` | 30 |
| `idle_ttl_seconds` | 120 |
| `idle_keep_min_state_bytes` | 10000000 |
| `claim_max_attempts` | 20 |
| `cold_create` | true |
| `agent_container`, `seed_container` | `sandbox`, `seed` |
| `tools_path`, `seed_tools_path`, `seed_root` | `/opt/sbx/bin/bsdtar`, `/sbx/bin/bsdtar`, `/skills` |
| `store_path_style` | false |

## Store layout

| Object | Key |
|---|---|
| Part | `saves/{client}/{thread}/{agent}/{turn}/{attempt}/part-0.tar.zst` |
| Manifest | `saves/{client}/{thread}/{agent}/{turn}/manifest.json` |
| Record | `records/{client}/{thread}/{agent}/{turn}.json` |
| Tombstone | `tombstones/{client}/{thread}/{agent}/{turn}.json` |

## Why

| Thing | Why it is the way it is |
|---|---|
| Times keep milliseconds | The time `made_at` of a manifest, the annotation `saved-at` of a pod and the time of a receipt are one value, so that a save has one expiry whichever way it is read. |
| A manifest is parsed against the key that was asked | A manifest names the key of its own part. A manifest that names another key, or a part key that is not the one built from its own fields, is refused when read. |
| The decoder stops at the sizes of the manifest | A part that is larger than its manifest says is a fault, not a stream to follow to its end. |
| `ImageError` keeps the details, the port gets `OpenFault` | The port reads the cause as `OpenFault`, a value with no field, by its exact type: a richer cause would make `OpenFault::of` answer nothing. The details go to the log and to `open_box`. |
| The crate installs the `ring` crypto provider of rustls when none is set | The kube client (kube-client 5.2) builds its rustls configuration with no provider parameter, and the S3 client of `reqwest` panics with no provider; neither takes an explicit one without rebuilding its client stack, so `Sandbox::connect` (when it builds the kube client) and `S3Store::new` install the provider when the process has none. A provider set by the caller is kept. |
| The warm pods come from a watch, the first open waits for its first snapshot | An open must not list the pods of the pool each time, and an early open must not read an empty pool as a failing one. |
| The cold pod copies only the labels, the annotations and the spec of the template | The name, the owner reference and the other metadata of the template would make the pod a member of the ReplicaSet. |
| The S3 delete is one request per key | Some S3-compatible stores have no multi-object delete. A delete of a key that is not there is a success. |
| The part is checked by its stored size after the upload | The store has no checksum the adapter can trust on every compatible store, so the size is read back and compared with the size counted while streaming. |
| The manifest is written last, with a conditional create | The manifest is the commit mark. A conflict is `AlreadySaved` and the part stays for the component that manages the pool. |
| The pod is read only when the exec status is 128 or more | A deleted pod ends the exec stream with the status of the killed process, 137 or 143, which is also what a live `kill -9` gives. A status below 128 cannot come from a signal, so those calls read nothing; the one read that decides is made after the status and before the status is used. |
| The stderr of an exec is never put in an error or a log | A command can print a secret. The foreground `run` returns the stderr to the harness as part of the result of the command. The other calls (`read`, `write`, `list`, `kill`) hold it in memory in the output of the exec and drop it with the output, and the `bsdtar` calls of save and restore drain it and drop it so that the process does not block. No error and no log holds it. |
| `reqwest` is a dependency with no code of its own | It carries the `rustls-no-provider` feature of the TLS that the S3 client uses. |
| The restore skips the entry of each scope root with an anchored `--exclude` | The mount roots of the pod belong to another owner than the agent user, and `bsdtar` run by the agent user exits with 1 when it sets the time of a directory that it does not own. The exit code is never read as a success: the entry is not applied, so the call ends with 0. The pattern is anchored at both ends and the pattern characters of a path are escaped, so only the entry of the root is skipped and the content under it is restored. |
| The tests with the real `bsdtar` run only where they can | They need `bsdtar` on the path, and the first two also need a folder (`/private/tmp` or `/tmp`) that the user running the tests does not own; the third needs only `bsdtar`. Where one is missing, as in a container run as root or a CI image with no `bsdtar`, the test returns after one line on stderr that names the test and the reason; cargo shows that line only with `--nocapture` or when the test fails, so a green run does not prove that the test ran. The proof on a pod is two rows of the cluster battery: "the argument lists of bsdtar" and "save then restore gives back files, modes, owners, dirs and times". |
| `AbsolutePath` drops empty and `.` parts | `/work/` and `/work` are one path, so a scope never holds the same folder twice. |
| A released box keeps its record in memory | A second release of a box is accepted and the release of a box that was never opened is a fault, so the record of a released box is the only memory that tells the two apart. It holds a few values per box and lives as long as the adapter, which a runner makes one per process. |
