# Changelog

## 0.1.0

First release.

### Added

- The configuration of the adapter (`SandboxConfigInput`, `SandboxConfig`, `ConfigError`) with every key, default and rule of the minor.
- The public keys of the pod contract (`PodKeys` and the label, annotation, pool and condition items) and the keys of the object store.
- The formats of the manifest, the record and the tombstone, and the writer and the reader of a saved part (`PartEncoder`, `PartDecoder`, `TarCounter`).
- The archive of a seed set and the text of its hash.
- The image errors (`ImageError`) as typed causes of a fault of the port, and the error type of the crate (`Error`).
- The pure decisions of open, save and release, and the patches of a claim, a save, a release and a seed.
- The object store boundary (`Store`, `PartUpload`, `PutOutcome`, `StoreError`) and its S3-compatible client (`S3Store`).
- The cluster boundary (`Cluster` and its views, `ClusterError`) and its implementation on `kube` (`KubeCluster`).
- The exec layer of `Env` (`run`, `read`, `write`, `list`, `kill`), the bsdtar argument lists and `ExecError`.
- The save flow, the restore flow, the seed write and the read of the store view of a key (`Env::save`, `restore`, `write_seed`, `load_manifest`, `read_store_view`).
- The adapter `Sandbox`: the implementation of the sandbox port, with the open flow (instance first, mode check, live reuse by the time of the save, warm claim with the version check, the failing-pool check before the cold path, the cold pod from the pool template, the open timeout, restore and seed at the same time), the watch of the warm pods, release (idle-since at the release, a pod already gone is a success), `erase` of a thread, the cleanup of a dropped `open`, and `cold_pod_manifest`.
- A `tracing` warning with the reason and the message (`ImageFailing`) or the wait (`ImageUnavailable`); the port error holds the typed cause only.
