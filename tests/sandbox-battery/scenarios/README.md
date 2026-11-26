# Sandbox battery: scenarios

One script for each check that runs with `kubectl` and `helm`. The checks that need the Rust adapter are in `rust-checks.md`. The scripts are written against the specs; the chart, the keeper and the adapter must exist to run them. Run all: `./run-all.sh`. Run some: `./run-all.sh s01-docker-parity m-burst`.

| Script | Check |
|---|---|
| `s01-docker-parity.sh` | Script of five commands against plain Docker (`docker-parity-*`), SIGTERM, no orphan, `pause` as pid 1 |
| `s02-two-agents.sh` | Two agents of one thread (`two-agents-*`) |
| `s03-two-clients.sh` | Two clients, two namespaces, cross connection fails (`two-clients-*`) |
| `s04-network-modes.sh` | Three network modes, no internal target, `allow` only listed sites, the check Job (`network-modes-*`) |
| `s05-admission.sh` | Admission policy checked with the identity of a runner |
| `s06-seed-read-only.sh` | Seed read-only for the non-root agent (`seed-*`) |
| `s10-proxy-canary.sh` | Canary through the proxy with TLS (`canary-*`); the echo case is recorded as `canary-echo-not-met` |
| `s11-platform.sh` | Platform scenario: clone, packages, build, push in mode `allow` |
| `s12-closed-cluster.sh` | Render with mirror and pull secrets, vendor host search, full flow on the closed cluster (`closed-cluster-*`) |
| `m-*.sh` | Measurements: restore stalls, NetworkPolicy delay, cold start, scale-down (`scale-down-*`), burst |

`common.sh` holds the shared functions. `names-chart.env` holds every name that comes from the chart, the keeper or the adapter (release, namespaces, labels, values keys, pool ids, proxy port, vendor hosts): change a name there only. Every variable has a default and can be set in the environment.

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

## Assumptions on the set-up and the chart

The main ones: the creds file gives `GITEA_TOKEN`, `S3_*`; the chart exposes the names of `names-chart.env`; the pool image has `python3`, `git` and `cargo` for the pool `platform`.
