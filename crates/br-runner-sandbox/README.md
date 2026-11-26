# br-runner-sandbox

The adapter library of the sandbox port of `br-agentic-harness-contract`: a pod of a Kubernetes cluster, claimed from a pool of warm pods, with a save and a restore of the work folder between the turns of one agent. This version holds the parts of the adapter that need no cluster: the configuration, the keys of the pod contract and of the object store, the formats of the saved objects, the reader and the writer of a saved part, the archive of the seed, the typed image errors and the pure decisions of open, save and release.

## Install

```toml
br-runner-sandbox = { git = "https://github.com/BotResources/br-jobs-runner-kit", package = "br-runner-sandbox", tag = "br-runner-sandbox/v0.1.0", version = "0.1.0" }
br-agentic-harness-contract = { git = "https://github.com/BotResources/br-llm-graph-rs", package = "br-agentic-harness-contract", tag = "br-agentic-harness-contract/v0.2.0", version = "0.2.0" }
```

A crate that names the types of a sibling in its public API must reach that sibling through the same git url and the same tag string, or the types are two copies: the pin of `br-agentic-harness-contract` above is the one this crate is built with. The crate does not compile against a version of the contract below 0.2.0.

## What the crate gives

| Item | What it is |
|---|---|
| `SandboxConfigInput`, `SandboxConfig` | The input is filled by the importer (it deserializes from the keys below; a key that is not in the table is not read). `SandboxConfig::try_from` checks every rule and gives a `ConfigError`. The adapter reads no environment variable and no file. `Debug` and `Display` of the configuration show no part of `store_credentials`. |
| `PodKeys`, the `LABEL_*`, `ANNOTATION_*`, `POOL_*`, `CLEAN_*`, `ROLE_SANDBOX`, `MANAGED_TRUE`, `CONDITION_WARM_PODS_FAILING` and `REASON_*` items | The exact text of the labels, annotations and the pool condition, with the default prefix `br.sandbox`. `PodKeys` rewrites the prefix of the same items for another `label_prefix`, so no key has a second copy. |
| `part_key`, `manifest_key`, `record_key`, `tombstone_key`, `saves_prefix`, `records_prefix`, `tombstones_prefix` | The keys of the object store. Each one holds the client, the thread, the agent and the turn. |
| `Manifest`, `Record`, `Tombstone`, `PartReport` | The JSON formats of the saved objects, checked when they are read. |
| `PartEncoder`, `PartDecoder`, `TarCounter` | The writer and the reader of a part: one zstd stream of one tar, with the size and the SHA-256 of the raw and of the stored bytes, and the count of the tar entries. |
| `seed_archive`, `seed_hash_text`, `seed_mode`, `check_seed_path` | The archive of a seed set (owner 0:0, the write bits of the mode removed) and the text of the hash of the seed set. |
| `ImageError`, `Error` | The image errors with their details, and the error type of the crate. The image errors reach the port as the cause `OpenFault` of a fault, which `OpenFault::of` reads. |
| `check_web_access`, `judge_live_pod`, `plan_start`, `recheck_after_restore`, `release_plan` and the patch functions | The decisions of open, save and release that are pure functions of values. |

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
| `ImageError` keeps the details, the port gets `OpenFault` | The port reads the cause as `OpenFault`, a value with no field. The reason, the message and the wait stay on `ImageError` for the logs of the caller. |
| `AbsolutePath` drops empty and `.` parts | `/work/` and `/work` are one path, so a scope never holds the same folder twice. |
