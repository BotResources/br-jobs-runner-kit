# Changelog

## 0.1.0

First release.

### Added

- The configuration (`KeeperConfigInput`, `KeeperConfig`, `PoolInput`, `PoolConfig`, `FloorInput`, `FloorWindow`, `LeaseSettings`, `StoreSettings`, `ConfigError`): one JSON document, every key with its default and its rule, an unknown key is a fault.
- The pure rules (`rules`): the pool size (`desired_size`, `floor_now`, `ClaimWindow`), the damping of a scale-down (`damp`), the expiry of one pod (`judge_pod`), the failing-pool condition (`pod_failure`, `judge_pool`, `condition_write`), the expiry of a save and the tombstone (`save_expired`, `tombstone_for`), the orphan rule and the sweep plan of a turn (`TurnGrouper`, `read_manifest`, `plan_turn`), and the expiry of a record or a tombstone (`record_needs_read`, `tombstone_needs_read`, `record_is_old`, `tombstone_is_old`). They do no I/O and take the instant as an input.
- The controller (`run`, `Parts::new`): the watches of the pods of every managed namespace (found again at each sweep interval; a namespace that loses the managed label stops being served at the next one), the expiry of the pods of a pool that is not configured with the default pool values, the claim rate, the size of each pool written by server-side apply under the field manager `br-sandbox-keeper`, the events `PoolAtMax` and `WarmPodsPending`, the expiry of pods with delete preconditions (uid and resourceVersion) and a new read on a conflict, and the condition `WarmPodsFailing` on the status of the pool ReplicaSet, written only after the first sync of the namespace and only for a warm pod that has not been Ready.
- The sweep of the object store: the leader reads the manifest of every listed save on every sweep and judges its age from `made_at` only; tombstone, then manifest, then parts for an expired save; orphan attempts; old records and tombstones, judged from the end, the start or `expired_at` and never from the store time of the object. The keeper writes only under `tombstones/` (`GuardedBucket`), and the lease is checked before each key of an orphan attempt that is deleted.
- The Lease election named `br-sandbox-keeper` for 2 replicas; only the holder writes, and no write is made after the renew deadline.
- The health server (`/livez`, `/readyz`, `/metrics`) and the metrics `br_sandbox_keeper_*`.
- The sealed traits `Bucket` and `KubeApi` with `S3Bucket` and `KubeCluster`, the errors, the decision enums and the read model (`facts`, and the structs of `cluster`, `bucket` and `rules` that have public fields) marked `#[non_exhaustive]` with constructors, `StoreCredential` (the credential from `AWS_ACCESS_KEY_ID` and `AWS_SECRET_ACCESS_KEY`, never shown by `Debug`), and `KeeperError`.
- The binary `br-sandbox-keeper`: the configuration from the file named by `BR_SANDBOX_CONFIG`, JSON logs, a stop on SIGTERM or an interrupt.
- The image `ghcr.io/botresources/br-sandbox-keeper`, tagged with the version of the crate (see `images/br-sandbox-keeper/README.md`).
