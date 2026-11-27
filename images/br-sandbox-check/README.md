# br-sandbox-check

The programs that the network check Job of `br-sandbox-chart` needs, and nothing else: an Alpine base and one pinned `kubectl`. The chart value `network_check.image` takes it.

The name `br-sandbox-check` is not final. Until it is confirmed, this image is built and tested by every pull request on both platforms, but it is pushed to the registry only by a push to `main`: the folder holds the empty file `PUSH_ON_MAIN_ONLY` (see [`../README.md`](../README.md)).

## Content

| Part | Version |
|---|---|
| Base | `alpine:3.24.4`, pinned by index digest (`linux/amd64` and `linux/arm64`) |
| `sh`, `nc`, `cat`, `sed`, `tr`, `mktemp`, `awk`, `sleep`, `date` | applets of the BusyBox 1.37.0 of the base; no other package is installed |
| `kubectl` | 1.34.13, the official static binary of `dl.k8s.io`, at `/usr/local/bin/kubectl`, mode 0555; each architecture is pinned by its sha256 (`ADD --checksum`) |
| `/usr/share/kubectl-LICENSE` | the Apache-2.0 text of Kubernetes v1.34.13, pinned by sha256 |

The image has no `ENTRYPOINT`, no `USER` and no file of the chart. The chart supplies the script (a ConfigMap mounted at `/netcheck`) and the command.

## What the chart needs from it

| Where | What the chart sets | What the image must do |
|---|---|---|
| Check Job | `command: sh /netcheck/network-check.sh discover`; uid and gid 10001, `runAsNonRoot`, read-only root file system, every capability dropped, no privilege escalation; `TMPDIR=/tmp` on an `emptyDir`; the script mounted read-only | find `sh`, `kubectl`, `nc`, `sed`, `tr`, `mktemp`, `awk`, `date` and `sleep`; `mktemp -d` in `/tmp`; keep no entrypoint that changes the `command` |
| Probe pod of each sandbox namespace | `command: sh /netcheck/network-check.sh probe`; uid and gid 0 inside a user namespace with the seven capabilities of the sandbox pod; no `/tmp` volume | `nc`, `sed`, `tr`, `awk`, `date`, `sleep` |

The script calls `nc -w 2 host port </dev/null` (is the port reachable: exit 0 when it connects, non-zero when refused), and `printf 'CONNECT ...' | nc -w 3 host port` (send the request, read the reply until the peer closes the connection or stays silent). The BusyBox `nc` of the base does this, and `test.sh` proves it.

## What is pinned

The base is pinned by index digest. `kubectl` and its licence text are pinned by sha256. The BusyBox applets and the other Alpine packages are the ones in the base image of that digest: they do not change between two builds.

To move to another `kubectl` version, change `KUBECTL_VERSION` in the `Dockerfile`, the two sha256 values (the files `https://dl.k8s.io/release/v<version>/bin/linux/<architecture>/kubectl.sha256`) and the sha256 of the licence file of that tag.

## Why

| Thing | Why it is the way it is |
|---|---|
| Alpine base | BusyBox gives nine of the ten programs; no package is added. |
| `kubectl` from `dl.k8s.io` with `ADD --checksum` | One pinned file for each platform, the same file for every build. The Alpine package is not used, because the package version moves with the Alpine release. |
| `kubectl` 1.34 | `kubectl` supports a server one minor version older or newer than itself; 1.34 is chosen for servers 1.33 to 1.35. The test suite has no cluster, so it does not start `kubectl` against a real API server. |
| One stage for each architecture, then `FROM kubectl-${TARGETARCH}` | `ADD --checksum` takes one value, and the two architectures have two files. |
| The licence file is `/usr/share/kubectl-LICENSE` | The parent folders of a file added from a URL are created with mode 0700, so a non-root user could not read a file in a new folder; `test.sh` reads it as uid 10001. |
| No `ENTRYPOINT` and no `USER` | The chart sets `command` and the numeric user for each pod. |

## Licences

| Product | Version | SPDX | In the image |
|---|---|---|---|
| BusyBox | 1.37.0 | GPL-2.0-only | the base image; the programs `sh`, `nc`, `cat`, `sed`, `tr`, `mktemp`, `awk`, `sleep`, `date` |
| musl libc | 1.2.6 | MIT | the base image |
| Alpine base packages | 3.24.4 | GPL-2.0-only, GPL-2.0-or-later, MIT, BSD-2-Clause, MPL-2.0 (CA bundle), Apache-2.0 (OpenSSL), Zlib | the base image |
| Kubernetes `kubectl` | 1.34.13 | Apache-2.0 | `/usr/local/bin/kubectl`; text in `/usr/share/kubectl-LICENSE`. The binary holds Go modules under their own licences; they are not copied into the image. |
| `Dockerfile`, `test.sh` | n/a | Apache-2.0 | not in the image |

The label `org.opencontainers.image.licenses` lists the SPDX identifiers of the table.

## Build and test

```sh
docker buildx build --platform linux/arm64 \
  --label org.opencontainers.image.source=https://github.com/BotResources/br-jobs-runner-kit \
  --label org.opencontainers.image.version=local \
  --label org.opencontainers.image.revision=$(git rev-parse HEAD) \
  --load -t br-sandbox-check:local -f images/br-sandbox-check/Dockerfile .
images/br-sandbox-check/test.sh br-sandbox-check:local linux/arm64
```

The workflow builds each platform, loads it into the Docker of the job and executes `test.sh` on it; on a pull request nothing is pushed (see [`../README.md`](../README.md)). `test.sh` starts the image with no network, every capability dropped and `no-new-privileges`, in two shapes: the shape of the check Job (uid 10001, read-only root, `TMPDIR=/tmp` on a tmpfs, the real `charts/br-sandbox-chart/files/network-check.sh` mounted at `/netcheck`) and the shape of the probe pod (uid 0 with the seven capabilities). A fixed list of check names makes `test.sh` fail when a check did not execute or an unlisted check did.

## Tests

| Check | What it asserts |
|---|---|
| the image architecture is the platform | The image is built for the platform that the workflow asked for. |
| the image has no entrypoint | `Config.Entrypoint` is empty, so the `command` of a pod is the whole command. |
| the four labels are set | `source`, `version`, `revision` and `licenses` are not empty (`images-check-labels.sh`). |
| the Job runs as uid 10001 on a read-only root with a writable /tmp | The shape of the Job is real: uid 10001, a write to `/` fails, a write to `/tmp` works. |
| `sh`, `cat`, `sed`, `tr`, `awk`, `sleep`, `date` run | Each is found with `command -v` and does its work in the Job shape; `awk` runs the status-line program of the script. |
| kubectl runs and prints the pinned version | `kubectl version --client` prints `Client Version: v` and the `KUBECTL_VERSION` of the `Dockerfile`. |
| nc runs and has the -w option | `nc` is found and its usage lists `-w SEC`. |
| mktemp makes a directory in /tmp and fails with no /tmp volume | `mktemp -d` with `TMPDIR=/tmp` gives a directory in `/tmp`; the same call on a read-only root with no `/tmp` volume fails, so the check can fail. |
| kubectl reports an unreachable API server as an error on a read-only root | `kubectl get nodes` against a closed port exits non-zero with `connection refused`, as uid 10001 on a read-only root. |
| kubectl licence text is in the image | uid 10001 reads `/usr/share/kubectl-LICENSE`. |
| nc -w reaches a listener and exits 0 | `nc -w 2 127.0.0.1 <open port> </dev/null` exits 0. |
| nc -w on a closed port exits non-zero in under 2 seconds | The refused connection is reported at once, with exit 1. |
| nc reads the reply until the peer closes | A fake proxy that answers a `CONNECT` with a 403 line gives that line through `nc -w 3`. |
| nc -w ends a connection to a peer that never answers | A peer that accepts and stays silent for 20 seconds does not hold `nc -w 1` for more than 5 seconds. |
| network-check.sh probe gives its lines as the Job pod user and as the probe pod user | The script of the chart, in `probe` mode, against fake peers: an open port (`reached`), a closed port (`refused`), and three fake proxies that answer 403 (`refused`), 200 (`reached`) and nothing (`no_result`), as uid 10001 on a read-only root and as uid 0 with the seven capabilities. |
