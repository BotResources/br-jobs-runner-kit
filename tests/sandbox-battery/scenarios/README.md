# Sandbox battery: scenarios

One script for each check that runs with `kubectl` and `helm`. The checks that need the Rust adapter are in `rust-checks.md`. The scripts follow the real names of `charts/br-sandbox-chart` (its README is the contract). The chart renders no proxy yet (the slot `br-sandbox-chart.slot.egress.objects` is empty): every check that needs a proxy starts with a check `...-proxy-present` and stops with a fail when no pod with `br.sandbox/role=proxy` runs in the egress namespace. Run all: `./run-all.sh`. Run some: `./run-all.sh s01-docker-parity m-burst`.

| Script | Check |
|---|---|
| `s01-docker-parity.sh` | Script of five commands against plain Docker (`docker-parity-*`), SIGTERM, no orphan, `pause` as pid 1 |
| `s02-two-agents.sh` | Two agents of one thread (`two-agents-*`) |
| `s03-two-clients.sh` | Two clients, two namespaces, cross connection fails (`two-clients-*`) |
| `s04-network-modes.sh` | Three network modes, no internal target (proxy pods and egress namespaces are not targets), the tunnel port as the one allowed path, `allow` only listed sites, the chart check Job (`network-modes-*`) |
| `s05-admission.sh` | Admission policy checked with the identity of a runner: role, Secret use, root containers, seed container name, relabel |
| `s07-egress-namespace.sh` | Egress namespace: labels, no right of the runner, keeper and sandbox identities (can-i and real attempts), only the tunnel port reachable from the sandbox of the same client and mode, policy of the sandbox namespace (`egress-*`) |
| `s06-seed-read-only.sh` | Seed read-only for the non-root agent (`seed-*`) |
| `s10-proxy-canary.sh` | Canary through the proxy with TLS (`canary-*`); the proxy has no response scrub, so the echo case is recorded as `fail` named `canary-echo-not-met` with the hit count |
| `s11-platform.sh` | Platform scenario: clone, packages, build, push in mode `allow` |
| `s12-closed-cluster.sh` | Render with mirror and pull secrets, vendor host search, full flow on the closed cluster (`closed-cluster-*`) |
| `m-*.sh` | Measurements: restore stalls, NetworkPolicy delay, cold start, scale-down (`scale-down-*`), burst |

`common.sh` holds the shared functions. `names-chart.env` holds every name that comes from the chart, the keeper or the adapter (release, namespaces, labels, values keys, proxy port, vendor hosts): change a name there only. Every variable has a default and can be set in the environment.

## Rules of the scripts

- Timed scripts (`m-*`) run one at a time: `run-all.sh` takes the lock directory `TIMING_LOCK`. Each `record` adds the load average.
- `run-all.sh` counts pass and fail from the `check` calls (`SCENARIO_TALLY`).
- Scenario pods are claimed with a label change, as the adapter does. Pods that a script claims are deleted at its end.
- The pool image of the scenarios must hold `python3` (the test image does). The image contract (`/bin/sh`, `cat`, `find`) is proved in `rust-checks.md`.

## Elevated access

| Where | What | Why |
|---|---|---|
| `s10-proxy-canary.sh`, function `capture_start` | A `tcpdump` container (`CAPTURE_IMAGE`) that joins the pid namespace of the k3d node container (`--pid container:NODE`) with `NET_ADMIN`, `SYS_ADMIN`, `SYS_PTRACE`, and enters the network namespace of the sandbox pod with `nsenter` | The packet capture inside the sandbox network (proxy canary). No `--privileged`, no `--pid=host`, no host mount: the capture goes to stdout and is written by the script. |
| `s12-closed-cluster.sh` | `docker exec` into the k3d node container for `kubectl` when the API port of the closed cluster is not published | The closed cluster is on an internal Docker network |

## Chart layout the scenarios expect

The values file (`CHART_VALUES_FILE`, `CLOSED_VALUES_FILE`) must give these (client, mode) pairs; the chart makes one namespace and one pool for each, with the pool id `{client}-{mode}` and the ReplicaSet `warm-{client}-{mode}`. The pool of a namespace is found from its name.

| Pair | Namespaces | Used by |
|---|---|---|
| `ca` `none`, `ca` `full` (no credential set) | `sbx-ca-none`, `sbx-ca-full` | most scenarios; no egress namespace |
| `ca` `allow`, `cb` `allow` | `sbx-ca-allow` + `sbx-ca-allow-egress`, same for `cb` | network, admission, egress, closed cluster |
| `cb` `none`, `cb` `full` | `sbx-cb-none`, `sbx-cb-full` | two clients |
| `canary` `allow` with one credential set | `sbx-canary-allow` + `-egress` | proxy canary |
| `platform` `allow` with one credential set | `sbx-platform-allow` + `-egress` | platform scenario |

- `egress.enabled: true`; `runner.service_account` and `runner.namespace` give `RUNNER_SA` and `RUNNER_NS` (the chart makes no runner ServiceAccount; impersonation with `--as` needs none). Keeper on (`keeper.enabled`), `network_check.image` with `sh`, `kubectl`, `nc`.
- The release namespace (`HELM_NAMESPACE`, default `default`) is not the system namespace: the chart renders the Namespace `sbx-system` itself.
- One `image` for the installation: it needs `python3`, `git` and `cargo` (the platform scenario), and `/bin/sh`, `cat`, `find`.
- The pod quota is `max_sandboxes` plus `quota.extra_pods`: `m-burst` with 40 needs `max_sandboxes` of at least 36 for `ca` `none`.
- Proxy pods, the credential Secret and the CA key live in the egress namespace only. The scenarios create the credential Secret there (`set_secret`) and restart the proxy by deleting its pods. The names of the Secret, its keys, the placeholder variables and the reload port (`PROXY_RELOAD_PORT`) are not in the chart yet: they are working names in `names-chart.env`.
- `s07` makes its own test pod with `br.sandbox/role=proxy` in the egress namespace (stand-in listener on the tunnel port, a stand-in reload port and four other ports), so the network rules and the rights are proved without a proxy; the pod declares no container port, so the chart check Job does not pick it. It is deleted at the end.
- The chart check Job (`br-sandbox-netcheck-{run_id}`) fails when a probe reaches the tunnel port of an own proxy; `s04` counts that path apart and fails on every other mismatch. The probe pods of the Job run as root inside the user namespace: that is how a root test of the network is made (the runner cannot make such a pod).
- The proxy denies internal addresses and has no cluster DNS, so a test server with an in-cluster name (`TEST_HOST_GIT`, `TEST_HOST_ECHO`) is not reachable through it. The chart has no test exception: only `network.internal_cidrs` can change, and the pod and service networks stay excluded. The scenarios that go through the proxy to the test servers need a test server outside both networks, or a test exception in the proxy.

## Assumptions on the set-up

The creds file gives `GITEA_TOKEN`, `S3_*`; the chart exposes the names of `names-chart.env`; the tools image (digest only) and the check image are loaded in the cluster.
