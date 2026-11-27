# Changelog

## 0.2.0

### Added

- `resource`: the type of the custom resource `SandboxPool` (`sandbox.botresources.ai/v1alpha1`, namespaced, status subresource): `SandboxPool` (implements `kube::Resource`; the `spec` stays a raw value so a bad spec fails one resource and not a list), `SandboxPoolSpec`, `PoolSize`, `SizeBounds`, `FloorSpec`, `SandboxPoolStatus`, `SpecError`, `DefinitionError`, the constants `GROUP`, `VERSION`, `KIND`, `LIST_KIND`, `PLURAL`, `SINGULAR`, `MAX_FLOORS`, `MAX_TIME_LENGTH`, `DEFAULT_REFILL_SECONDS`, `DEFAULT_MARGIN`. `SandboxPool::read_spec` reads the spec strictly (`deny_unknown_fields` at the spec, in `size`, in `cpu`, `memory` and `disk`, and in each item of `floors`).
- `resource::definition` and `resource::definition_yaml`: the `CustomResourceDefinition` that the type gives, with `x-kubernetes-preserve-unknown-fields` on each object level of the spec and on the status, `maxItems: 16` on `floors`, `maxLength: 64` on `from` and `to`, the printer columns `Desired`, `Ready`, `Accepted` and `Age`, and the four CEL rules that a real API server of v1.33.6 ran (no rule on a request against its limit). The file `crates/br-sandbox-keeper/definition/sandboxpools.yaml` is its YAML text.
- `rights`: `RIGHTS` (the rights table of the intent as data, with `Right` and `RightScope`), `POD_TEMPLATE_CONFIGMAP`, and the closed list of network policy names (`NETWORK_POLICY_NAMES`, `NETWORK_POLICY_DEFAULT_DENY`, `NETWORK_POLICY_EGRESS_ALLOW`, `NETWORK_POLICY_EGRESS_FULL`).
- `serde-saphyr` is now a dependency (it was a dev-dependency, and is already in the lock through `kube`).

- `pool`: the pure rules of the resource: `validate` (`InvalidSpec`), `template` (`PoolTemplate`, `TemplateFault`, `RenderedTemplate`, the template hash), `fence` (`FenceFacts`, `fence_verdict`, `FenceFault`), `duplicate` (`pool_winner`, `is_duplicate`), `status` (`judge_acceptance`, `compute_status`, `decide_write`, `STATUS_WRITE_INTERVAL`), `selector`, `quantity`, `PoolSettings`, and the names (`replicaset_name`, `template_hash_key`).
- `rules::plan_rollout`, `RolloutPlan`, `RolloutState`, `Wait`, `failing_status`; the delete reason `DeleteReason::Rollout` and the metric reason `rollout`; `PodFacts` carries `template_hash` and `unschedulable`.
- Configuration keys `idle_ttl_seconds`, `idle_keep_min_state_bytes` (for the whole keeper) and `agent_container` (required, no default).

### Changed

- The key `pools` is removed from the configuration; a document that holds it is a start fault (`ConfigError::PoolsAreResources`). The pool types of the configuration (`pools`, `PoolInput`, `FloorInput`) and the errors that belonged to them are removed.
- `rules::size_pool` takes `PoolSettings`. `judge_pod` takes a flag for a pod chosen by the rollout.
- The start no longer checks that the namespace of a configured pool is managed (`KeeperError::PoolNamespaceNotManaged` is removed).
- The controller keeps a transitional `Engine.pools` list that the production runner leaves empty: this version does expiry and sweep only, and does not size a pool. The next piece drives the pools from the resources.

### Tests

- The pure rules above are covered by unit tests, one for each sentence of the intent that they touch (spec validation, template check and hash, fence verdict, duplicate rule, sizing, rollout plan, pod expiry by state, status conditions and write rate). The tests of the removed pool configuration are replaced by tests of the `pools` fault and of the new keys; the chart 0.3 tests now hold that the keeper refuses its document.
- The checked-in definition file equals the generated definition; each level of the type has the keys of its schema; the preserve mark, the bounds and the four rules are held; no other rule is present.
- The spec is read strictly at each level, with the defaults of the table, and a bad spec does not fail the object that holds it.
- The rights table equals the table of the intent row by row, and has no wildcard, no `escalate`, `bind` or `impersonate`, and no write on the objects that the keeper only reads. The rights test of the client now takes the granted set from `RIGHTS`.

### Documentation

- README: "The resource `SandboxPool`" and "Rights table", and the two new rows of the public surface. This version does not yet read a resource, a ConfigMap or a network policy, and does not call the new rights: the pure rules of `pool` are not yet wired into the controller.

## 0.1.1

### Fixed

- A pod list that fails, a pod watch that cannot be opened or reopened, a watch that breaks and an error event of the watch were retried with no log line and no count. Each now gives one log line at the error level (JSON, with the namespace and the reason) and one count in `br_sandbox_keeper_sweep_errors_total`, and is tried again after one second as before. `/readyz` does not change.
- A tombstone that cannot be serialized gave an empty list of steps: the expired save stayed whole with no trace. It now gives one step, `StoreStep::TombstoneNotSerialized`, that the sweep logs with the key of the tombstone and counts in `br_sandbox_keeper_sweep_errors_total`; the save is left whole and the next sweep tries again.
- A Lease read that fails and a Lease create or update that fails were logged and tried again, and were not counted. Each is now also counted in `br_sandbox_keeper_sweep_errors_total`. A conflict on a Lease write is not a failure and is still not counted. The help text of the counter now reads "Actions that failed after the start."
- The test `sweep::tests::aged::an_unreadable_record_or_tombstone_is_kept_logged_and_counted` failed on some runs: the helper that captures log lines installed a subscriber for one thread, and a log callsite that another thread had reached first could be disabled for it. The helper now installs one global subscriber once and routes each line to the capture of the current thread.

### Added

- `PodEvent::WatchFailed { reason }`: the event that a failed pod list or pod watch gives. `PodEvent` was already `#[non_exhaustive]`.
- `StoreStep::TombstoneNotSerialized { key }`. `StoreStep` was already `#[non_exhaustive]`.

### Tests

- A failed list, a watch that cannot be opened, an error event and a line that cannot be read each give a `WatchFailed` event; the controller logs it at the error level and counts it; a watch that ends with no error gives none.
- The failure of a tombstone serialization is injected into the plan of an expired save; the sweep step is logged and counted and writes nothing.
- A run whose store fails after the start, and every call of the store client with a refused credential, show no credential value in the logs, the `/metrics` text, the error texts and the writes to the cluster.
- The binary, started with a configuration, a kubeconfig and a store that a local test server answers, ends with code 0 after `SIGTERM` (`tests/sigterm.rs`, on Unix).
- The store client of the sweeper in `run` is the guarded client: a write outside `tombstones/` and any key outside the three prefixes are refused (the wiring is one function, `sweeper_of`).
- The parts left by a crash between the delete of the manifest and the delete of a part are removed at the next sweep when they are older than `attempt_ttl_seconds`, and stay until then when they are younger.
- A Lease read that fails, a Lease create that fails and a Lease update that fails are each logged at the warn level and counted once, and the next round is after the retry period; a read and a write that succeed and a conflict count nothing.
- A configuration test uses a host name under `.test`.

### Documentation

- README: the "Pod watch" bullet and the new `PodEvent` and `StoreStep` variants. "Not guaranteed" states the limit of the repair of a part after a crash, the `force` of the server-side apply, and that no test covers the outbound connections of the keeper. "Rights" states that no test of the crate checks the TLS verification of `kube` in a handshake.

## 0.1.0

First release.

### Added

- The configuration (`KeeperConfigInput`, `KeeperConfig`, `PoolInput`, `PoolConfig`, `FloorInput`, `FloorWindow`, `LeaseSettings`, `StoreSettings`, `ConfigError`, with `KeeperConfig::from_json`, `KeeperConfig::pool` and `KeeperConfig::pod_keys`): one JSON document, every key with its default and its rule, an unknown key is a fault. `KeeperConfigInput`, `PoolInput` and `FloorInput` are `#[non_exhaustive]`: an importer starts from `KeeperConfigInput::default()`, `PoolInput::default()` or `FloorInput::default()` and calls `KeeperConfig::try_from` (the `TryFrom<KeeperConfigInput>` impl); the three input types also implement `Deserialize`.
- The pure rules (`rules`): the pool size (`desired_size`, `floor_now`, `ClaimWindow`), the damping of a scale-down (`damp`), the expiry of one pod (`judge_pod`), the failing-pool condition (`pod_failure`, `judge_pool`, `condition_write`), the expiry of a save and the tombstone (`save_expired`, `tombstone_for`), the orphan rule and the sweep plan of a turn (`TurnGrouper`, `read_manifest`, `plan_turn`), and the expiry of a record or a tombstone (`record_is_old`, `tombstone_is_old`). They do no I/O and take the instant as an input. The result types are `Damped`, `Desired`, `Judgment`, `Action`, `DeleteReason`, `PodFailure`, `FailureKind`, `PoolJudgement`, `ConditionWrite`, `ClaimRate`, `ManifestRead`, `StoreStep`, `TurnListing` and `AttemptListing`; the state and input types are `ClaimWindow`, `ScaleState`, `ScaleDownRules`, `ExpiryRules`, `SweepRules` and `TurnGrouper`. `ScaleState` and `TurnGrouper` are built only with their public `Default` (`ScaleState::default()`, `TurnGrouper::default()`); `SweepRules` is also built with `From<&KeeperConfig> for SweepRules`. The methods are `ClaimWindow::new`, `ClaimWindow::record`, `ClaimWindow::peak`, `ClaimRate::per_second`, `Desired::above_max`, `DeleteReason::label`, `TurnGrouper::push`, `TurnGrouper::finish`, and the constructors `ExpiryRules::new`, `ScaleDownRules::new` and `SweepRules::new`.
- The three parts of the library (the configuration, the pure rules and the runner) are the intended entry points; the other public items below are the surface that they are built from. The standard derives (`Debug`, and `Clone`, `Copy`, `PartialEq`, `Eq`, `Hash` on the types that have them) are not named type by type.
- The controller (`run`, `Parts::new`): the watches of the pods of every managed namespace (found again at each sweep interval; a namespace that loses the managed label stops being served at the next one), the expiry of the pods of a pool that is not configured with the default pool values, the claim rate, the size of each pool written by server-side apply under the field manager `br-sandbox-keeper`, the events `PoolAtMax` and `WarmPodsPending`, the expiry of pods with delete preconditions (uid and resourceVersion) and a new read on a conflict, and the condition `WarmPodsFailing` on the status of the pool ReplicaSet, written only after the first sync of the namespace and only for a warm pod that has not been Ready.
- The sweep of the object store: the leader reads the manifest of every listed save, and every listed record and tombstone, on every sweep and judges its age from its content only; an object that cannot be read or holds no readable time is kept, logged and counted in `br_sandbox_keeper_sweep_errors_total`; tombstone, then manifest, then parts for an expired save; orphan attempts; old records and tombstones, judged from the end, the start or `expired_at` and never from the store time of the object. The keeper writes only under `tombstones/` (`GuardedBucket`), and the lease is checked before each key of an orphan attempt that is deleted.
- The Lease election named `br-sandbox-keeper` for 2 replicas; only the holder writes, and no write is made after the renew deadline.
- The health server (`/livez`, `/readyz`, `/metrics`) and the metrics `br_sandbox_keeper_*`: `pool_desired`, `pool_warm_ready` and `claims_per_second` with the label `pool`, `pods_deleted_total` with the label `reason`, and the others with no label (`idle_pods` counts every claimed clean pod of a managed namespace).
- The condition `WarmPodsFailing` is `False` with the reason `NoWarmPodFailing` and an empty message when no warm pod fails.
- The sealed traits `Bucket` (`get`, `put`, `delete`, `list_page`, `check`) and `KubeApi` (`get_pod`, `delete_pod`, `watch_pods`, `managed_namespaces`, `get_replica_set`, `apply_replicas`, `apply_condition`, `create_event`, `get_lease`, `create_lease`, `update_lease`) with `S3Bucket::new`, `GuardedBucket::new`, `KubeCluster::new` and `KubeCluster::connect`, the aliases `BucketFuture` and `ApiFuture`, the store prefixes `SAVES_PREFIX`, `RECORDS_PREFIX` and `TOMBSTONES_PREFIX`, the event reasons `EVENT_POOL_AT_MAX` and `EVENT_WARM_PODS_PENDING`, `FIELD_MANAGER`, `PoolEvent::reason`, `Mark::is_absent`, the errors `ApiError` and `BucketError`, the types of `cluster` (`PodEvent`, `DeleteOutcome`, `LeaseFacts`, `LeaseWrite`, `EventTarget`, `PoolEvent`) and of `bucket` (`ObjectInfo`), the read model of `facts` (`PodFacts` with `version`, `PodVersion`, `ContainerFacts` with `waiting` and `terminated`, `Terminated`, `ConditionFacts`, `ReplicaSetFacts`, `Phase`, `PoolState`, `Mark`), the decision enums, and every struct of `cluster`, `bucket` and `rules` that has public fields marked `#[non_exhaustive]`, built with the constructors `PodFacts::new`, `PodVersion::new`, `Terminated::new`, `ConditionFacts::new`, `ReplicaSetFacts::new`, `ObjectInfo::new`, `LeaseWrite::new`, `EventTarget::new`, `ContainerFacts::waiting`, `ContainerFacts::terminated` (and `ContainerFacts::default()`), `StoreCredential` (`from_lookup`, `access_key_id`, `secret_access_key`; the credential from `AWS_ACCESS_KEY_ID` and `AWS_SECRET_ACCESS_KEY`, never shown by `Debug`), and `KeeperError`; `ConfigError`, `ApiError`, `BucketError` and `KeeperError` implement `Display` and `std::error::Error`, and `From<ConfigError> for KeeperError` is implemented.
- The items that the binary reads from its environment: `config_path`, `own_namespace`, `CONFIG_VARIABLE` (`BR_SANDBOX_CONFIG`), `ACCESS_KEY_VARIABLE` (`AWS_ACCESS_KEY_ID`), `SECRET_KEY_VARIABLE` (`AWS_SECRET_ACCESS_KEY`), `POD_NAMESPACE_FILE` (the service account file that holds the namespace of the pod) and `LEASE_NAME` (`br-sandbox-keeper`).
- The binary `br-sandbox-keeper`: the configuration from the file named by `BR_SANDBOX_CONFIG`, JSON logs, a stop on SIGTERM or an interrupt.
- The image `ghcr.io/botresources/br-sandbox-keeper`, tagged with the version of the crate (see `images/br-sandbox-keeper/README.md`).
