# Changelog

## 0.1.0

First release.

### Added

- The configuration (`KeeperConfigInput`, `KeeperConfig`, `PoolInput`, `PoolConfig`, `FloorInput`, `FloorWindow`, `LeaseSettings`, `StoreSettings`, `ConfigError`): one JSON document, every key with its default and its rule, an unknown key is a fault.
- The pure rules (`rules`): the pool size (`desired_size`, `floor_now`, `ClaimWindow`), the damping of a scale-down (`damp`), the expiry of one pod (`judge_pod`), the failing-pool condition (`pod_failure`, `judge_pool`, `condition_write`), the expiry of a save and the tombstone (`save_expired`, `tombstone_for`), the orphan rule and the sweep plan of a turn (`TurnGrouper`, `needs_manifest`, `read_manifest`, `plan_turn`), and the expiry of a record or a tombstone (`needs_read`, `record_is_old`, `tombstone_is_old`). They do no I/O and take the instant as an input.
- The controller (`run`, `Parts`): the watches of the pods of the configured pools, the claim rate, the size of each pool written by server-side apply under the field manager `br-sandbox-keeper`, the events `PoolAtMax` and `WarmPodsPending`, the expiry of pods with delete preconditions (uid and resourceVersion) and a new read on a conflict, and the condition `WarmPodsFailing` on the status of the pool ReplicaSet.
- The sweep of the object store: tombstone, then manifest, then parts for an expired save; orphan attempts; old records and tombstones. The keeper writes only under `tombstones/` (`GuardedBucket`).
- The Lease election named `br-sandbox-keeper` for 2 replicas; only the holder writes, and no write is made after the renew deadline.
- The health server (`/livez`, `/readyz`, `/metrics`) and the metrics `br_sandbox_keeper_*`.
- The seams `Bucket`, `S3Bucket`, `KubeApi` and `KubeCluster`, the read model `facts`, `StoreCredential` (the credential from `AWS_ACCESS_KEY_ID` and `AWS_SECRET_ACCESS_KEY`, never shown by `Debug`), and `KeeperError`.
- The binary `br-sandbox-keeper`: the configuration from the file named by `BR_SANDBOX_CONFIG`, JSON logs, a stop on SIGTERM or an interrupt.
- The image `ghcr.io/botresources/br-sandbox-keeper`, tagged with the version of the crate (see `images/br-sandbox-keeper/README.md`).
