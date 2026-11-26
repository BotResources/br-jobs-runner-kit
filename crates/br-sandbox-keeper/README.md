# br-sandbox-keeper

The controller of the sandbox pools of `br-runner-sandbox`. It sizes each pool of warm pods from the rate of claims, deletes the pods that are old or idle or stuck or failed, writes the condition `WarmPodsFailing` on the pool ReplicaSet, and deletes the saves, records and tombstones that have expired. The package has a library and a binary. The binary loads the configuration and runs the library until it gets the stop signal.

## Install

```toml
br-sandbox-keeper = { git = "https://github.com/BotResources/br-jobs-runner-kit", package = "br-sandbox-keeper", tag = "br-sandbox-keeper/v0.1.0", version = "0.1.0" }
```

The crate takes the names of the labels, the annotations, the pool condition and the store keys from `br-runner-sandbox`, and a crate that names the types of that sibling must reach it through the same git url and the same tag string. The image `ghcr.io/botresources/br-sandbox-keeper` holds the binary and the system root certificates, and its tag is the version of the crate.

## Run

The binary reads one JSON document at the path in the variable `BR_SANDBOX_CONFIG`, and the credential of the object store from the variables `AWS_ACCESS_KEY_ID` and `AWS_SECRET_ACCESS_KEY`. The credential is not part of the document, is never logged, and is not shown by `Debug`. The keeper reads no Secret through the Kubernetes API. The logs are JSON lines on the standard output; they hold the keys of objects and the names of pods, never the content of an object. The binary ends with a failure code when the library returns an error, and with the code 0 after SIGTERM or an interrupt.

The binary runs as 2 replicas. The identity of a replica for the Lease is its `HOSTNAME` with a unique suffix. The Lease namespace is `lease_namespace`, or the namespace of the pod when the key is not set.

## Public surface

| Group | Items |
|---|---|
| Configuration | `KeeperConfigInput`, `KeeperConfig`, `PoolInput`, `PoolConfig`, `FloorInput`, `FloorWindow`, `LeaseSettings`, `StoreSettings`, `ConfigError`. `KeeperConfig::from_json` reads a document and checks every rule as a whole; a key that is not in the table below is a fault. |
| Pure rules (`rules`) | `desired_size`, `floor_now`, `damp`, `judge_pod`, `pod_failure`, `judge_pool`, `condition_write`, `save_expired`, `tombstone_for`, `needs_manifest`, `read_manifest`, `plan_turn`, `needs_read`, `record_is_old`, `tombstone_is_old`, with `ClaimWindow`, `ClaimRate`, `ScaleState`, `ScaleDownRules`, `ExpiryRules`, `SweepRules`, `TurnGrouper` and the values they take and give. They do no I/O and take the instant as an input; the controller calls them and holds no copy of a rule. |
| Seams | `bucket` (`Bucket`, `S3Bucket`, `GuardedBucket`, `BucketError`, `ObjectInfo`) and `cluster` (`KubeApi`, `KubeCluster`, `ApiError`, `PodEvent`, `DeleteOutcome`, `LeaseFacts`, `LeaseWrite`, `EventTarget`, `PoolEvent`, `FIELD_MANAGER`). `facts` holds the read model of a pod and a ReplicaSet. The traits are public so that the tests of the crate run the same flows on fakes; they are not an extension point. |
| Runner | `run(config, parts, shutdown)` with `Parts { api, bucket, clock, identity, own_namespace }`. It binds the health address, checks the API, the namespaces of the pools and the bucket, then runs the watches, the election, the sweep and the health endpoints until the token is cancelled. It returns `KeeperError` when it cannot start and never returns for a failed delete. |
| Environment | `StoreCredential`, `config_path`, `own_namespace`, `CONFIG_VARIABLE`, `ACCESS_KEY_VARIABLE`, `SECRET_KEY_VARIABLE`, `POD_NAMESPACE_FILE`, `LEASE_NAME`. |
| Errors | `KeeperError`: a configuration fault, the API unreachable at start, the store unreachable at start, the bucket absent at start, a pool namespace that is not managed, an unusable variable, a health address that cannot be bound, a task that ended. A failed action after the start is not an error: it is logged, counted in `br_sandbox_keeper_sweep_errors_total` and tried again. |

## Configuration

| Key | Meaning | Default |
|---|---|---|
| `label_prefix` | prefix of the labels and annotations (the value of the adapter) | `br.sandbox` |
| `managed_namespace_selector` | label selector of the namespaces that the keeper serves | `{label_prefix}/managed=true` |
| `store_bucket`, `store_endpoint`, `store_region`, `store_path_style` | the object store (S3 API) | required; `store_path_style` false |
| `listen_address` | address of `/livez`, `/readyz` and `/metrics` | required |
| `save_ttl_seconds` | age after which a save is expired | required |
| `skew_margin_seconds` | extra wait before the keeper deletes an expired save | 60 |
| `sweep_interval_seconds`, `sweep_page_size` | the pass over the store | 60, 1000 |
| `attempt_ttl_seconds` | age of an orphan attempt | 3600 |
| `tombstone_ttl_seconds` | age of a record or tombstone | 31 536 000 |
| `stuck_ttl_seconds` | cap on the age of a dirty claimed pod | 7200 |
| `failed_pod_ttl_seconds` | age of a Failed or Evicted pod | 300 |
| `warm_max_age_seconds` | maximum age of a warm pod | 1800 |
| `pending_alarm_seconds` | time that a warm pod may stay Pending before the event | 60 |
| `rate_window_seconds`, `rate_bucket_seconds` | the window of the claim rate and the width of its buckets | 300, 10 |
| `scale_down_cooldown_seconds`, `scale_down_step`, `scale_down_step_interval_seconds` | damping of a scale-down | 120, 5, 10 |
| `lease_namespace`, `lease_duration_seconds`, `renew_deadline_seconds`, `retry_period_seconds` | leader election | namespace of the pod, 15, 10, 2 |
| `timezone` | time zone name of the floors of the day | `UTC` |
| per pool: `id`, `namespace`, `replicaset`, `min_replicas`, `max_replicas` | the pool | required |
| per pool: `refill_seconds`, `margin` | the sizing rule | 3, 2 |
| per pool: `floors` | windows `{from, to, floor}` of the day; `to` may be `24:00` | none |
| per pool: `idle_ttl_seconds` | how long a saved pod stays bound when its state is not smaller than the threshold | 120 |
| per pool: `idle_keep_min_state_bytes` | the threshold of the state size | 10 000 000 |

A document is a fault when `save_ttl_seconds` is 0 or below; a duration that drives a timer is 0 (except `idle_ttl_seconds` and `skew_margin_seconds`); `min_replicas` is above `max_replicas`; `rate_bucket_seconds` is above `rate_window_seconds`; `scale_down_step` is below 1; the Lease durations are not in the order `lease_duration_seconds` above `renew_deadline_seconds` above `retry_period_seconds`; the time zone is unknown; a window ends no later than it begins; two pools have the same id or the same ReplicaSet in one namespace; a required key is missing; or a key is unknown.

## Labels and annotations

The keeper reads these names through the label prefix and takes them from `br-runner-sandbox`. It never writes a label or an annotation, never claims a pod, and never patches a pod.

| Role | Name | Kind | Use |
|---|---|---|---|
| pool state | `{label_prefix}/pool` = `warm` or `claimed` | pod label | which pods are warm or claimed; the claim rate |
| pool id | `{label_prefix}/pool-id` | pod label | the pool of a pod |
| thread, agent | `{label_prefix}/thread`, `{label_prefix}/agent` | pod labels | log fields only |
| clean or dirty | `{label_prefix}/clean` | pod annotation | idle or stuck |
| claimed-at | `{label_prefix}/claimed-at` | pod annotation | the clock of `stuck_ttl_seconds` |
| idle-since | `{label_prefix}/idle-since` | pod annotation | the clock of the idle deadline |
| state-bytes | `{label_prefix}/state-bytes` | pod annotation | the choice of `idle_ttl_seconds` or 0 |
| managed namespace | `{label_prefix}/managed` = `true` | namespace label | which namespaces the keeper serves |

A claimed pod whose annotation that its rule needs is missing or unreadable is never deleted by that rule. It is logged and counted in `br_sandbox_keeper_unreadable_pods`.

## Behaviour

- **Pool size.** `desired = clamp( max( ceil(peak_claim_rate × refill_seconds) + margin, floor_now ), min_replicas, max_replicas )`, in integer arithmetic. The peak claim rate is the highest bucket of `rate_bucket_seconds` in the last `rate_window_seconds`. A claim is a pod that changes from `warm` to `claimed`, or a pod that appears already `claimed` with no owner and is not older than the window. Each replica counts the claims from its own watch. A scale-up is applied at once. A scale-down is applied only after `desired` has stayed below the replicas for `scale_down_cooldown_seconds`, then by at most `scale_down_step` each `scale_down_step_interval_seconds`. The keeper writes `spec.replicas` by server-side apply under the field manager `br-sandbox-keeper`, only when the value differs, and raises `PoolAtMax` once for each transition into a size above `max_replicas`. The chart must not render `spec.replicas`.
- **Pod expiry.** One rule for each state, as a pure function of the pod: a warm pod at `creationTimestamp + warm_max_age_seconds`; a clean claimed pod at `idle-since + ttl` (`idle_ttl_seconds` when `state-bytes` is not below the threshold, 0 below it); a clean claimed pod with no `idle-since`, and a dirty claimed pod, at `claimed-at + stuck_ttl_seconds`; a Failed pod `failed_pod_ttl_seconds` after the keeper first saw it so. A warm pod that is Pending and not failing raises `WarmPodsPending` once for each pod after `pending_alarm_seconds`. A delete carries the uid and the resourceVersion of the last read; on a conflict the pod is read again and judged again, and a pod that is gone is not an error.
- **Failing pool.** The condition `WarmPodsFailing` on the status of the pool ReplicaSet is `True` when a warm pod that is not Ready has a container with a pull failure (`ImagePullFailing`) or a crash at start (`StartCrashing`), and `False` otherwise; it is written `False` at the first judgement. `lastTransitionTime` changes only when the status changes.
- **Saves.** Every `sweep_interval_seconds` the leader lists `saves/` in pages. A save is expired when `now - made_at` is above `save_ttl_seconds + skew_margin_seconds`. For one expired save the order is: write the tombstone (`expired_at = made_at + save_ttl_seconds`, a pure function of the manifest), delete the manifest, delete the parts of every attempt, leave the record. An attempt that no manifest names is an orphan once its newest object is older than `attempt_ttl_seconds`. A record or tombstone is deleted when `tombstone_ttl_seconds` has passed since its end, its start or its `expired_at`. A manifest that cannot be read is never deleted for age. The keeper writes only under `tombstones/`, and `GuardedBucket` refuses any other write or any key outside `saves/`, `records/` and `tombstones/`.
- **Leader election.** A Lease named `br-sandbox-keeper`. Both replicas keep their watches; only the holder writes (replicas, condition, events, pod deletes, tombstones, object deletes). A write is refused once `renew_deadline_seconds` has passed without a renewal.
- **Endpoints.** `/livez` answers 200 while the process runs. `/readyz` answers 503 until every watched namespace has done its first full sync, and while the store check of the start has not passed. `/metrics` answers in the Prometheus text format: `br_sandbox_keeper_pool_desired`, `_pool_warm_ready`, `_claims_per_second`, `_idle_pods` (label `pool`), `_pods_deleted_total` (label `reason`: `warm_max_age`, `idle`, `stuck`, `failed`), `_saves_deleted_total`, `_tombstones_written_total`, `_orphans_deleted_total`, `_sweep_errors_total`, `_unreadable_pods`, `_is_leader`. The events are `PoolAtMax` and `WarmPodsPending`, on the ReplicaSet of the pool.

## Rights

In each managed namespace: `pods` get, list, watch, delete; `replicasets` get, list, watch, patch; `replicasets/status` patch; `events` create. In the Lease namespace: `leases` get, create, update. Cluster-wide: `namespaces` get, list, watch. No `pods/exec`, no `pods/patch`, no `secrets`. In the store: read and delete on `saves/`, `records/` and `tombstones/`, write on `tombstones/` only. The keeper calls only the API server and the store. TLS verifies with the system root certificates, and the dependency tree has no `webpki-roots`.

## Not guaranteed

- A pool that is not in the configuration is not served: its pods are not watched for sizing or expiry.
- The keeper watches the namespaces of the configured pools, not every managed namespace.
- With no keeper running no save is deleted and no pool is resized.
- The `last_modified` time of an object gates the reads: a manifest, record or tombstone younger than its limit by the store's own time is not read.
- The pool size follows the claim rate with the lag of one bucket (`rate_bucket_seconds`) for a change that is not caused by a pod event, because the controller evaluates the pools at each bucket.

## Why

| Thing | Why it is the way it is |
|---|---|
| Each replica counts claims from its own watch | A change of leader loses no rate; a cold start gives the floor |
| The tombstone has no skew margin | The adapter's read-side check fires first; the tombstone is the same bytes whenever it is written, so no conditional write is needed |
| The tombstone is written before the manifest is deleted | The adapter never meets "record done, no manifest, no tombstone" for a save that the keeper expired |
| The manifest is deleted before the parts | No half of a save is ever visible |
| `GuardedBucket` wraps the store client | The rule "writes only under `tombstones/`" is enforced by one type, not by the care of each call |
| A failed action counts in `sweep_errors_total` | The metrics list has no other error counter; the action is tried again at the next event or pass |
| `/readyz` reflects only the start checks and the first sync | A lost API or store at run time is logged and counted; the replica stays up and tries again |
| Pools are evaluated at each rate bucket as well as on events | The floor of the day and the sliding window change `desired` with no pod event |
