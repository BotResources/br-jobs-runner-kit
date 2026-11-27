# Sandbox battery: scenarios

One script for each check that runs with `kubectl` and `helm`. The checks that go through the Rust adapter are described in `rust-checks.md` and implemented in `../rust` (its README lists which test covers which check). The scripts follow the real names of chart `br-sandbox-chart` 0.2.4 (its README is the contract; `../chart.sh fetch` pulls it, `../chart.sh install` installs it with `../values-test.yaml`). Every name that comes from the chart is in `names-chart.env`. Run all: `./run-all.sh`. Run some: `./run-all.sh s05-admission s07-egress-namespace`.

| Script | What it proves |
|---|---|
| `s01-docker-parity.sh` | Script of five commands against plain Docker (`docker-parity-*`), SIGTERM, no orphan, `pause` as pid 1 |
| `s02-two-agents.sh` | Two agents of one thread (`two-agents-*`) |
| `s03-two-clients.sh` | Two clients, two namespaces, cross connection fails (`two-clients-*`) |
| `s04-network-modes.sh` | Non-root pod in each of the three modes reaches no node, Service or pod address and resolves no cluster name; only the tunnel port (pod address and ClusterIP of `HTTPS_PROXY`) answers; the other proxy ports are closed; mode `allow` lets the listed site through and answers 403 or 502 for the rest; the chart check Job (new `network_check.run_id`, run after the claimed pods are deleted so the quota has room for the probe pods; it must succeed and report at least one probe) has no probe other than the expected ones (`network-modes-*`) |
| `s05-admission.sh` | Admission policy with the identity of the runner: legitimate pod accepted; refused: uid 0, no `runAsNonRoot`, seed name with another image, root container under another name, init container shaped like the seed, shared process namespace, ephemeral container shaped like the seed (the runner has no right on that subresource: RBAC refuses, an admin dry run shows the policy refusal), no role, `role=proxy`, Secret volumes and references; relabel and removal of the role; the check ServiceAccount may make a uid 0 pod and not a shared-process one (`admission-*`) |
| `s06-seed-read-only.sh` | Seed read-only for the non-root agent (`seed-*`) |
| `s07-egress-namespace.sh` | Egress namespace: labels, no Role, runner, keeper and sandbox identities refused (can-i and real attempts, on a stand-in pod and on the real proxy pod and its CA key Secret), only the tunnel port reachable from the sandbox of the same client and mode (pod address and ClusterIP), nothing from other pairs or from outside (`egress-*`) |
| `s08-internal-targets.sh` | Root pods (made by the identity of the check ServiceAccount) in `ca` none, allow, full and `pivot` full: direct TCP to the recording probe, a pod, a Service, both addresses of the node (also ports 10250 and 6443) is refused; through the proxy no internal target answers 2xx (names by TLS, IP literals, absolute-form request); node second address at ports 10250 and 6443 refused; the three recorders count zero new connections; positive controls for the recorders and for the proxy path (`internal-*`) |
| `s10-proxy-canary.sh` | Canary through the proxy with TLS (`canary-*`); the canary service is our own container on the servers network; the proxy has no response scrub, so the echo case is recorded as `fail` named `canary-echo-not-met` with the hit count. The scan (`canary-scan.py`) covers the environment, process memory and files only. A network capture in the sandbox is not done: no script or test captures traffic, so "zero hits in a network capture" is not proved |
| `s11-platform.sh` | Platform flow in mode `allow` with two credential sets: clone a private repository over HTTPS, download packages, build, push a branch; then the resolver of the proxy pods logged only test names (`platform-*`) |
| `s12-closed-cluster.sh` | Render with mirror and pull secrets (the pod templates are read with `kubectl create --dry-run=client`, which renders only the given file), vendor host search (a vendor host counts only in the host position of an image reference, that is at the start of a token and before the first `/`, and in the host of a URL; a vendor host after the mirror host is the expected mirror path and does not count; three checks prove the search on sample lines), then a hand-made flow on the closed cluster (`closed-cluster-*`): `kubectl exec`, `bsdtar` and an upload through the toolbox pod. It does not go through the adapter; the flow through the adapter on the closed cluster is not done |
| `s13-pivot-barriers.sh` | The three barriers against an internal target, in all eight combinations: allowlist, proxy deny list (`upstream_deny_cidrs`), `egress-proxy` policy. Targets are test names that the resolver points at a pod, a Service and the node addresses (names are on the allowlist), and the same addresses as IP literals (not on the allowlist). Zero new connections at the recorders where at least one barrier acts; the combination with no barrier is the positive control (every recorder sees a connection) (`pivot-*`) |
| `s14-fresh-pod-race.sh` | A probe pod of the chart check Job (mode `none`) runs the probe script of the chart at once after its start and 30 s after it. The script tries each probe that must be refused again until it is refused, up to `network_check.enforcement_wait_seconds`, and reports the seconds from the start of the probe script to the first refusal of each probe. Both runs must show every probe refused after the retry and none reached at the limit. The reported seconds are not the window between pod start and enforcement of the network policy controller (that window is measured by `m-window.sh`) |
| `s15-keeper-ready.sh` | The keeper pod of the chart starts from the chart document with the real image and becomes Ready: image reference by digest with no command override, `imageID` equals the manifest list digest or a platform digest, Ready within 120 s, ConfigMap `br-sandbox-keeper-config` holds one JSON object with keeper keys only and no credential, the store credential comes from the Secret, `/readyz` answers 200 (asked from the node: the policy `default-deny` of the system namespace refuses the ask from a pod), no error line in the log, restart count 0 after 60 s; records the first 30 log lines with the credentials removed (`keeper-ready-*`) |
| `m-window.sh` | Window between pod start and network policy enforcement for pods of pool `ca-allow`: the replacement of a deleted warm pod, and a pod shaped like the warm pod made by hand with the runner identity (its agent container runs the probe loop as its command). Per sample, in ms from the pod creation request: first Ready, first command that runs in the agent container, first refused connection to the recording probe (in-pod loop, 10 ms pause and 40 ms timeout), the moment the node firewall holds the jump for the pod (polled from the host), and whether the first command still reached the probe. All times come from the clock of the host: output of `kubectl` and `docker exec` is stamped when it arrives; the in-pod loop prints only its own elapsed time, and the start of the loop is the earliest `stamp - elapsed` over its lines, so the pod clock is never compared with the host clock. Needs `sbx-testsvc` servers and the recording probe; reads `WINDOW_*` and `REPS` from the environment; stops when the free disk is under `WINDOW_MIN_FREE_MB` |
| `m-*.sh` | Measurements: restore stalls, NetworkPolicy delay (label change to enforcement) with the load average of each sample, cold start, scale-down (`scale-down-*`), burst |

`common.sh` holds the shared setup and sources `common-pool.sh` (warm pods, claims, keeper, pool size), `common-run.sh` (verdicts, timing lock, scenario begin and end, statistics), `common-probe.sh` (proxy, probes, store access through the toolbox) and `common-admission.sh` (pod documents, admission checks). `names-chart.env` holds every name that comes from the chart, the keeper or the adapter: change a name there only. Every variable has a default and can be set in the environment.

## Rules of the scripts

- Timed scripts (`m-*`) run one at a time: `run-all.sh` takes the lock directory `TIMING_LOCK`. Each `record` adds the load average. A script that cannot take the lock within `TIMING_LOCK_WAIT_SECONDS` (default 21600) records the failing check `timing-lock` and stops; the shell lock has no take-over (it holds no process id), so after a script that was killed, remove the folder by hand.
- `run-all.sh` counts pass and fail from the `check` calls (`SCENARIO_TALLY`). A script that exits non-zero with no failing check recorded, a script that records no check, and a script name that is not in its lists each give a failing check.
- A negative check (a connection that must fail) runs through `probe_open_to` or `probe_count_to` (`common-probe.sh`): a failed exec, an empty output or a row count different from the number of targets gives `probe-failed`, never 0. Each negative row has a positive control from the same pod (`probe_control`: a loopback listener answers and `localhost` resolves; `s03` starts a listener in each pod).
- A measurement that has no sample, or fewer samples than planned, gives a failing `-ran` check.
- Scenario pods are claimed with a label change, as the adapter does. Pods that a script claims are deleted at its end.
- The check image (`sbx-battery-check:1`, `../check-image/Dockerfile`) adds `nc` to the `rancher/k3s` image; the chart check Job needs it.
- The pool image (`sbx-battery-agent:1`, `../agent-image/Dockerfile`) holds `python3`, `git` and `cargo`. The image contract (`/bin/sh`, `cat`, `find`) is proved in `rust-checks.md`.
- A pod that the runner cannot make (uid 0) is made with `kubectl --as=system:serviceaccount:sbx-system:br-sandbox-netcheck create`: the chart lets that identity make root pods (rule 10), and nothing else. No privileged pod is used for it.

## `internal_cidrs` of each row

Every row of this battery runs with the same installed value (the details of the checks print it):
`10.42.0.0/16, 10.43.0.0/16, <node address on the cluster network>/32, <node address on the servers network>/32, <cluster network range>`. It is the pod range, the service range, each node address as /32 on both Docker networks and the k3d network range. The servers range (`172.31.77.0/24`) is not in it, so the test servers stay reachable. The chart default (RFC1918 and others) is replaced, because it holds `172.16.0.0/12`. The proxy deny list and the `egress-proxy` policy are rendered from this value plus `network.pod_cidr`, `network.service_cidr` and the node address lists.

| Rows | Value |
|---|---|
| `s04`, `s05`, `s07`, `s08` (each of its four rows), `s10`, `s11`, `s13` (each of its eight combinations), `m-netpol-delay`, `m-window` | the value above |

## Elevated access

| Where | What | Why |
|---|---|---|
| `../extras.sh`, pod `rec-node` in `sbx-testsvc` | A pod with `hostNetwork: true` (no `privileged`, no host path, no added capability) that runs `servers/probe.py` on ports 80, 443, 8080 | A recorder that listens on both node addresses, so a connection of the proxy to a node address is counted |
| `s08`, `s05` | `kubectl --as=system:serviceaccount:sbx-system:br-sandbox-netcheck` | Root test pods and the proof that the exception of rule 10 is the identity |
| `s13-pivot-barriers.sh` | `kubectl replace` of the ConfigMap `br-sandbox-proxy-config` and of the NetworkPolicy `egress-proxy` of the `pivot` allow pair, restored from a saved copy at the end | One barrier at a time is removed with our own edit (no chart value); no `helm upgrade` runs during it |
| `s12-closed-cluster.sh` | `docker exec` into the k3d node container for `kubectl` when the API port of the closed cluster is not published | The closed cluster is on an internal Docker network |

## Chart layout the scenarios expect

The values file gives these (client, mode) pairs; the chart makes one namespace and one pool for each, with the pool id `{client}-{mode}` and the ReplicaSet `warm-{client}-{mode}`.

| Pair | Namespaces | Used by |
|---|---|---|
| `ca` `none`, `ca` `full` (no credential set) | `sbx-ca-none`, `sbx-ca-full`; no egress namespace | `s03`, `s04`, `s07`, `s08` |
| `cb` `none`, `cb` `full` (no credential set) | `sbx-cb-none`, `sbx-cb-full`; no egress namespace | `s03` |
| `ca` `allow`, `cb` `allow` | `sbx-ca-allow` + `sbx-ca-allow-egress`, same for `cb` | `s03`, `s04`, `s05`, `s07`, `s08`, `m-netpol-delay`, `m-window` |
| `canary` `allow` with one credential set | `sbx-canary-allow` + `-egress` | `s10` |
| `platform` `allow` with two credential sets | `sbx-platform-allow` + `-egress` | `s11` |
| `pivot` `allow` (names of the internal targets on the allowlist), `pivot` `full` with one credential set | `sbx-pivot-allow` + `-egress`, `sbx-pivot-full` + `-egress` | `s08`, `s13` |

- `egress.enabled: true`; `runner.service_account` and `runner.namespace` give `RUNNER_SA` and `RUNNER_NS` (the chart makes no runner ServiceAccount; impersonation with `--as` needs none). `keeper.enabled: true` with the image `ghcr.io/botresources/br-sandbox-keeper` by digest (`KEEPER_IMAGE` in `names-chart.env`), so the keeper starts and it manages the pool sizes and deletes warm pods older than `warm_max_age_seconds`; `network_check.image` has `sh`, `kubectl`, `nc`.
- The release namespace (`HELM_NAMESPACE`, default `default`) is not the system namespace: the chart renders the Namespace `sbx-system` itself.
- The pod quota is `max_sandboxes` plus `quota.extra_pods`.
- Proxy pods, the credential Secrets, the CA key Secret and the reload Secret live in the egress namespace only; `../chart.sh install` makes them after the chart. The proxy address in a sandbox is the fixed ClusterIP of the Service `br-sandbox-proxy` (`HTTPS_PROXY`), the CA certificate is the file `/etc/br-sandbox-ca/ca.crt`, the placeholders are in the variables of `credential_set.secrets[].env_name`.
- `s07` makes its own test pod with `br.sandbox/role=proxy` in the egress namespace (stand-in listener on the tunnel port, on the four loopback ports of the proxy and on 80 and 443), so the network rules and the rights are proved on a pod that listens on every port; the pod declares no container port. It is deleted at the end.
- The chart check Job (`br-sandbox-netcheck-{run_id}`) fails when a probe reaches the tunnel port of an own proxy; `s04` counts that path apart (pod address and ClusterIP) and fails on every other mismatch.
- The local image `sbx-battery-agent:1` is not kept in the Docker store after the last row that needs it (`s01` uses it for the plain `docker run` side); the node holds it. Rebuild it from `../agent-image/Dockerfile` before a rerun of `s01`.
- The proxy denies internal addresses and resolves only through `network.public_resolvers`. The test servers are Docker containers outside the pod, service and node ranges; their names are resolved by the DNS stub; the proxy trusts the test CA through `egress.proxy.upstream_ca` (ConfigMap `sbx-test-ca`).

## Assumptions on the set-up

The creds file gives `GITEA_TOKEN`, `S3_*`; the chart exposes the names of `names-chart.env`; the agent image and the check image are loaded in the node; the tools image, the proxy image and the keeper image are pulled by digest. The Secret `store-credentials` of the system namespace holds `S3_ACCESS_KEY` and `S3_SECRET_KEY` of the creds file as `AWS_ACCESS_KEY_ID` and `AWS_SECRET_ACCESS_KEY` (made by `../chart.sh install`).
