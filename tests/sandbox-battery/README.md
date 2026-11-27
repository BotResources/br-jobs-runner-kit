# Sandbox battery: test set-up

Scripts that make the test cluster, the object store, and the test servers (git over HTTPS with a package registry, an echo endpoint, a recording probe, a DNS stub) with their test credentials. Names are in `names.env` (one place). Needs Docker, k3d, kubectl, openssl, jq, curl, python3.

## The chart and its test values

`values-test.yaml` is a template (`$VAR` filled by `chart.sh render`). Pairs (client, mode): `ca` none, allow, full; `cb` allow; `canary` allow with one credential set; `platform` allow with two credential sets (git, packages); `pivot` allow and full with one credential set. Pool size is small (the Docker VM has 8 GB). The agent image is built from `agent-image/Dockerfile` (python:3.12-slim with git and cargo from the Debian repository), imported into the node and used with `image_pull_policy: IfNotPresent`. The tools image and the proxy image are pulled by digest by the node. The check image is built from `check-image/Dockerfile`: the `rancher/k3s` image of the cluster (`sh`, `kubectl`, busybox without `nc`) plus the `nc` of the Alpine busybox; `docker build -t sbx-battery-check:1 check-image`, then import it into the cluster like the agent image. The `rancher/k3s` image alone fails the check Job: it has no `nc`, so every control is `control_unavailable` and every `connect` row is `no_result`. The keeper is `ghcr.io/botresources/br-sandbox-keeper` by digest (`KEEPER_IMAGE`, `scenarios/names-chart.env`); it reads the ConfigMap `br-sandbox-keeper-config` and the store credential from the Secret `store-credentials`.

`network.internal_cidrs` replaces the chart default (which holds 172.16.0.0/12 and would deny the test servers): pod range, service range, each node address as /32 (both Docker networks), the k3d network range. The servers range is not in it. `network.public_resolvers` is our own DNS stub, not the stub of `./dns-ip.sh`, because the target names of the pivot checks need records that point at pod, Service and node addresses, and the stub of the set-up loads its records once.

## Why the servers are outside the cluster

The sandbox egress proxy cannot reach the pod network, the service network, the node addresses or any cluster DNS name, and a TLS call to an IP literal fails (no SNI). So the test servers are Docker containers on their own Docker network, reached by name, with certificates that carry the name, and names resolved by a small DNS stub.

## Run

| Command | Result |
|---|---|
| `./setup.sh` | k3d cluster `sbx-build-test` (one server, no agent; traefik, metrics-server, servicelb off; kube-router NetworkPolicy), SeaweedFS container `sbx-build-store`, bucket `sandbox-saves`, the server containers, the toolbox pods, seed. Removes the old in-cluster Gitea objects. Safe to run again. |
| `./setup.sh --closed` | Same on cluster `sbx-build-closed`; the cluster network and the servers network are both made with `--internal` (no route to the internet). |
| `./verify.sh [--closed]` | Writes checks to `$EVIDENCE/checks.tsv`: store put/get, clone, packages, DNS stub, echo, probe, address ranges, NetworkPolicy; closed also: no route out, servers still reachable. |
| `./dns-ip.sh [--closed]` | Prints the IP of the DNS stub (for `network.public_resolvers`). |
| `./chart.sh fetch` | Pulls chart `CHART_VERSION` (0.2.4) from the OCI registry into `~/sbx-battery-work/battery/chart-oci`. |
| `./chart.sh render [--closed]` | Renders `values-test.yaml` (and `values-closed.yaml` on top, closed) with the facts of the set-up (`servers.env`, node addresses, the proxy CA certificate) into `~/sbx-battery-work/battery/values-test.rendered.yaml`. Prints the `internal_cidrs` value. No secret in the output. |
| `./chart.sh install [--closed]` | Renders, runs `helm upgrade --install sbx` (no `--wait`: the chart makes the egress namespaces itself), then makes the objects the chart does not render: CA key Secret, reload key Secret, upstream CA ConfigMap and credential Secrets in each egress namespace, the store credential Secret in the system namespace (`S3_ACCESS_KEY` and `S3_SECRET_KEY` of `creds.env`). Other Secret values come from `creds.env` or are random. |
| `./extras.sh up` | Test objects of the battery: our own DNS stub (same `servers/dns.py`, fixed address 172.31.77.250, the five names, the canary name and the target names), the canary container (172.31.77.251, name `canary.test.internal`, certificate of the test CA), the recording pods and Service in `sbx-testsvc`. `down` removes them. |
| `./teardown.sh --closed` or `--main` | Deletes that cluster, its servers, its store container and its servers network (closed: also its cluster network). Nothing else. |

`lib.sh` gives `preflight` (stops with exit 3 when free disk of `/` is under `MIN_FREE_GIB`, default 8 GiB), `load_now`, `record NAME VALUE UNIT` (`$EVIDENCE/measurements.tsv`) and `check NAME pass|fail DETAIL` (`$EVIDENCE/checks.tsv`). `EVIDENCE` defaults to `$HOME/sbx-battery-work/battery/evidence`. Work files: `~/sbx-battery-work/battery/`.

## Networks and containers (main cluster; closed in brackets)

| Thing | Where |
|---|---|
| Cluster network | `k3d-sbx-build-test` (`sbx-build-closed-net`, 10.88.0.0/24): the node, the store, the probe |
| Servers network | `sbx-build-servers`, 172.31.77.0/24 (`sbx-build-closed-servers`, 10.89.0.0/24, internal). Not the cluster network, not the pod range 10.42.0.0/16, not the service range 10.43.0.0/16 |
| Node | every node is attached to both networks (`docker network connect`), so pods reach the servers through the node (source address seen by the servers: the node address on the servers network) |
| `sbx-build-gitea` | Gitea (MIT) rootless, HTTPS on 443, names `git.test.internal` and `registry.test.internal` (one container, one certificate). Gitea writes package file links with the host of its root URL (the git name): clients use the git name. Data and config on two `tmpfs` mounts with `exec` (a default `tmpfs` is `noexec` and the Gitea git hooks then fail; RAM, 512 MB cap, lost when the container stops: run `./setup.sh` again; passwords stay, token is made again). No volume, no host mount |
| `sbx-build-echo` | HTTPS 443, `echo.test.internal`. Answers JSON with method, path, headers, body. Log lines: `CONN` for every TCP connection (before TLS), `SNI`, `REQ`, `TLSFAIL` |
| `sbx-build-internal-probe` | Plain TCP/HTTP on ports 80, 443, 8080, `probe.test.internal`. On the cluster network, so inside the node range of the proxy deny list. Log lines: `CONN` (with `port=`), `DATA` (first request line). Answers 200 `probe` to an HTTP request |
| `sbx-build-dns` | DNS stub (Python, stdlib, UDP and TCP 53). A records from a file for the five names; any other name gets NXDOMAIN; nothing is forwarded. Logs every query |
| `sbx-build-store` | SeaweedFS (binds `0.0.0.0`: it must answer on both networks); attached to the cluster network and to the servers network. `store.test.internal:8333` (plain HTTP) points to its servers-network address |

All server containers: no `--privileged`, no host mount, `--cap-drop ALL`, `no-new-privileges`, memory cap, files copied in with `docker cp`. The certificate (SAN: the five names) is signed by the test CA in `~/sbx-battery-work/battery/pki`. Count connections with `conn_count CONTAINER` (`servers.sh`): lines with ` CONN ` in `docker logs`.

Closed mode uses container names with the prefix `sbx-build-closed-`.

## Files outside the repo

| File | Content |
|---|---|
| `~/sbx-battery-work/battery/creds.env` (`creds-closed.env`) | mode 600. Gitea admin and normal user, personal access token (scopes `write:repository`, `write:package`, `read:organization`, `read:user`), S3 key and secret, `GITEA_URL`, `REGISTRY_URL`, `CA_FILE`. Never copy into the repo or the evidence |
| `~/sbx-battery-work/battery/servers.env` (`servers-closed.env`) | no secrets: `SERVERS_NETWORK`, `SERVERS_CIDR`, `CLUSTER_NETWORK_CIDR`, `NODE_CIDRS` (every node address on every network as /32, plus the cluster network range; comma list), `NODE_ADDRESSES`, `POD_CIDR`, `SERVICE_CIDR`, the names and IPs (`GIT_`, `REGISTRY_`, `ECHO_`, `PROBE_`, `STORE_`, `DNS_`) and the container names |
| `~/sbx-battery-work/battery/ca.crt` | test CA certificate (public) |

The node address on a network can change when the node container restarts: run `./setup.sh` again and read `servers.env` again.

## How pods use the names

Pods of the cluster do not resolve the test names (cluster DNS forwards to the Docker resolver, which does not know them). The tool pods `sbx-testtools/toolbox` (git, curl) and `sbx-testtools/pytool` (python, pip) carry `hostAliases` with the container IPs, and the test CA at `/ca/ca.crt`. `tbx` in `lib-cluster.sh` runs a command in the toolbox. The DNS stub is tested directly from `pytool` (`servers/dnsq.py`). Sandboxes under test use the stub through `network.public_resolvers` (value from `./dns-ip.sh`).

## What the set-up makes in the cluster

Namespaces `sbx-testsvc` (empty: kept for scenario control pods), `sbx-teststore` (Service `store` with a fixed Endpoint to the store container, `store.sbx-teststore.svc.cluster.local:8333`, path style, region `us-east-1`, bucket `sandbox-saves`), `sbx-testtools` (the two pods). Org `platform` (private), repo `platform/hello-rust`, packages `sbx-hello-npm`, `sbx-hello-py`, `sbx-hello-lib` (0.1.0). Lifecycle on `saves/`: SeaweedFS TTL (`SAVES_TTL`).

## Not done

- No network capture is made in the sandbox. The canary scan covers the environment, process memory and files; "zero hits in a network capture" is not proved.
- The full flow on the closed cluster is a hand-made `kubectl` flow (`scenarios/s12-closed-cluster.sh`); the adapter is not run on the closed cluster.
- `SEAWEED_IMAGE` is pinned by digest. The fetch fallback of `fetch_image.py` for a digest reference is not tried; the image is expected in the local Docker store.

## Closed cluster

The images are in the local Docker store and are imported into the node with `docker save --platform` piped to `ctr images import`: the cluster needs no registry. `fetch_image.py` loads an image that is not local (`docker pull` hangs on the test Mac). The API port of a cluster on an internal network is not reachable from the host: the scripts run `kubectl` inside the node container. k3s needs `--node-ip` (`CLOSED_NODE_IP`) and `--flannel-iface`, because the network has no default route.
