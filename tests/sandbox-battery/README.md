# Sandbox battery: test set-up

Scripts that make the test cluster, the object store, a git server with a package registry, and their test credentials. Names are in `names.env` (one place). Needs Docker, k3d, kubectl, openssl, jq, curl.

## Run

| Command | Result |
|---|---|
| `./setup.sh` | k3d cluster `sbx-build-test` (one server, no agent; traefik, metrics-server, servicelb off; kube-router NetworkPolicy), SeaweedFS container `sbx-build-store`, bucket `sandbox-saves`, Gitea in `sbx-testsvc`, seed. Safe to run again. |
| `./setup.sh --closed` | Same on cluster `sbx-build-closed`, on a Docker network made with `--internal` (no route to the internet). A short overlap with the main cluster worked on the 16 GB Mac. Needs `kubectl` inside the node container: automatic. |
| `./verify.sh [--closed]` | Writes checks to `$EVIDENCE/checks.tsv`: store put/get, clone, packages, NetworkPolicy; closed also: no route out. |
| `./teardown.sh --closed` or `--main` | Deletes that cluster, its store container and (closed) its network. Nothing else. |

`lib.sh` gives `preflight` (stops with exit 3 when free disk of `/` is under 15 GiB), `load_now`, `record NAME VALUE UNIT` (`$EVIDENCE/measurements.tsv`) and `check NAME pass|fail DETAIL` (`$EVIDENCE/checks.tsv`). `EVIDENCE` defaults to `$HOME/sbx-build-work/battery/evidence`; export `EVIDENCE` to place the files elsewhere. Work files: `~/sbx-build-work/battery/`.

## What the set-up makes

| Thing | Where |
|---|---|
| Store, from pods | `store.sbx-teststore.svc.cluster.local:8333` (Service with a fixed Endpoint to the container), path style, region `us-east-1`, bucket `sandbox-saves`. Main cluster only, from the host: `http://127.0.0.1:28333` |
| Lifecycle on `saves/` | SeaweedFS TTL on the prefix (`SAVES_TTL`), set with `fs.configure` |
| Git server and package registry | Gitea (MIT) in `sbx-testsvc`, `https://gitea.sbx-testsvc.svc.cluster.local`, certificate from a test CA made with openssl |
| Org, repo | private org `platform`, private repo `platform/hello-rust` (Cargo project, `app.py`, `package.json`) |
| Packages in `platform` | npm `sbx-hello-npm`, pypi `sbx-hello-py`, cargo `sbx-hello-lib`, all 0.1.0 |
| Tool pods | `sbx-testtools/toolbox` (git, curl), `sbx-testtools/pytool` (python, pip); the CA is mounted at `/ca/ca.crt` |

## Credentials and files outside the repo

`~/sbx-build-work/battery/creds.env` (main) and `creds-closed.env`: mode 600, made when the set-up runs. They hold the Gitea admin and normal user, the personal access token (scopes `write:repository`, `write:package`, `read:organization`, `read:user`), the S3 access key and secret. `~/sbx-build-work/battery/ca.crt` is the test CA certificate (public). Never copy these into the repo or the evidence.

## Closed cluster

The images are in the local Docker store and are imported into the node with `docker save --platform` piped to `ctr images import`: the cluster needs no registry. `fetch_image.py` loads an image that is not local (`docker pull` hangs on the test Mac); the k3s system images (pause, coredns, local-path-provisioner) are imported too. The API port of a cluster on an internal network is not reachable from the host: the scripts run `kubectl` inside the node container (`docker exec`). k3s needs `--node-ip` (`CLOSED_NODE_IP`, with the network range in `names.env`) and `--flannel-iface`, because the network has no default route.

The battery reaches the test service with `kubectl exec` into `sbx-testtools/toolbox` (git, curl) or `sbx-testtools/pytool` (python, pip); `tbx` in `lib-cluster.sh` does this.
