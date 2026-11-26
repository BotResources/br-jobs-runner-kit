# br-sandbox-chart

The Helm chart of the sandbox. One installation per cluster. It renders everything that is declarative: namespaces, quotas, admission policy, network policies, warm pools, roles, the keeper and the network check. Licence: Apache-2.0. The chart is released by the workflow `chart-release`; it is not a library of the Libraries registry.

It does not claim a pod, run a command, save or restore (the library `br-runner-sandbox`), size a pool or delete a save (the library `br-sandbox-keeper`). It creates no bucket, no Secret and no credential value.

## What a render holds

| Object | Scope | Gate |
|---|---|---|
| Namespace `{prefix}-{client}-{mode}`, ServiceAccount `sandbox` (no token, no Role), ResourceQuota `br-sandbox-quota`, LimitRange `br-sandbox-limits`, ConfigMap `br-sandbox-config` | each (client, mode) | always |
| ReplicaSet `warm-{client}-{mode}` | each (client, mode) | always, never sets `spec.replicas` |
| NetworkPolicy `default-deny`, `egress-full` or `egress-allow` | each (client, mode) | always |
| ValidatingAdmissionPolicy and binding `{prefix}-{client}-{mode}-pod` | each (client, mode) | always |
| Role and RoleBinding `br-sandbox-runner` | each (client, mode) | always |
| Role and RoleBinding `br-sandbox-keeper`, keeper Deployment, ServiceAccount, lease Role, ClusterRole `namespaces` read, NetworkPolicies `default-deny` and `egress-keeper` | system namespace | `keeper.enabled` |
| ServiceAccount, ClusterRole and Role `br-sandbox-netcheck`, ConfigMap `br-sandbox-netcheck`, Job `br-sandbox-netcheck-{run_id}`, CronJob, NetworkPolicy `egress-netcheck` | system namespace and each (client, mode) | `network_check.enabled` |

The system namespace is `{prefix}-system`. It does not carry `br.sandbox/managed`, so the Kyverno exclusion does not touch it.

## Values

Required keys have no default: a missing key stops the render with `BR_SANDBOX_MISSING_VALUE{key=...}`. Keys are snake_case; keys that are Kubernetes fields keep the Kubernetes name (`runtimeClassName`, `runAsUser`, `runAsGroup`, `terminationGracePeriodSeconds`, `imagePullSecrets` names, resource maps). `values.schema.json` has `additionalProperties: false` on every block: a key outside its place fails the render. All durations are in seconds. This table is the contract for the units `egress` and `keeper`.

| Key | Default | Meaning |
|---|---|---|
| `installation.namespace_prefix` | required | Prefix of every namespace, at most 22 characters (with a client name of 32 characters at most, a namespace name stays under 63). |
| `installation.system_namespace` | `{prefix}-system` | Keeper and network check. |
| `image` | required | One registry link of the project, with a tag or a digest. The pool and the policy use it. |
| `image_pull_policy` | computed | `Always` for a tag, `IfNotPresent` for a digest. A value overrides it. |
| `registry_mirror` | `""` | Prefix of every image: `{mirror}/{reference}`. |
| `pull_secrets` | `[]` | Names of image pull secrets, put on every pod. |
| `tools.image.{repository,digest}` | required | Image `br-sandbox-tools`, digest only. |
| `tools.mount_path`, `tools.size_limit` | `/opt/sbx`, `64Mi` | Where the agent sees `bin/bsdtar` and `bin/pause`. |
| `tools.init_resources` | 10m/16Mi/8Mi, 100m/64Mi/64Mi | Resources of the init container (cpu, memory, ephemeral-storage). |
| `tools.seed_container.{enabled,name,resources}` | `true`, `seed`, 10m/8Mi/8Mi, 100m/32Mi/32Mi | The seed container, see below. Its resources are not part of `pool.size`. |
| `seed.size_limit` | `256Mi` | Size of the `/skills` volume. |
| `runtimeClassName` | `""` | Passed to the pod; the policy requires the same value. Not measured. |
| `runAsUser`, `runAsGroup` | `10001` | Numeric user of the agent. The image needs no user entry. |
| `restore_timeout_seconds` | `30` | Read by `br-runner-sandbox`. |
| `idle_ttl`, `idle_keep_min_state_bytes` | `120`, `10000000` | Read by the keeper and `br-runner-sandbox`. |
| `warm_max_age_seconds` | `1800` | The keeper deletes a warm pod older than this. |
| `save.ttl_seconds` | required, above 0 | `BR_SANDBOX_SAVE_AGE_INVALID` otherwise. One value for the keeper and `br-runner-sandbox`. |
| `save.paths`, `save.tombstone_ttl_days` | `[/work, /home/agent]`, `365` | |
| `store.{endpoint,bucket,region,path_style,credentials_secret}` | required | Passed to the config. `credentials_secret` names a Secret of the keeper namespace with the keys `AWS_ACCESS_KEY_ID` and `AWS_SECRET_ACCESS_KEY`; the chart renders no Secret. |
| `work.size_limit`, `home.size_limit` | `3Gi`, `512Mi` | `emptyDir` `sizeLimit` of `/work` and `/home/agent`. No other volume type exists. |
| `pool.size.{cpu,memory,disk}.{request,limit}` | 500m/2, 1Gi/2Gi, 2Gi/5Gi | The one size key of each resource. `disk` is `ephemeral-storage`. |
| `pool.{floor,max_sandboxes,refill_seconds,margin}` | 1, 20, 3, 2 | Read by the keeper. |
| `pool.terminationGracePeriodSeconds`, `pool.node_selector` | `1`, `{}` | |
| `quota.extra_pods` | `4` | Pods of the quota = `max_sandboxes` + this (proxy, network check, slack). |
| `network.{pod_cidr,service_cidr}` | required | |
| `network.{node_addresses,node_public_addresses,load_balancer_addresses}` | required lists | `[]` must be written on purpose; no default exists. |
| `network.public_resolvers` | required if a mode is `allow` or `full` | Nameservers of the pods of mode `full`. |
| `network.internal_cidrs` | RFC1918, 100.64/10, 169.254/16, 127/8 | |
| `network.ipv6` | `single_stack` | `dual_stack` adds `::/0` except `fc00::/7`, `fe80::/10` and the IPv6 addresses of the lists. |
| `network.{api_server_addresses,store_cidrs}` | required with keeper or network check; at least one | Egress of the keeper (API server, store) and the network check (API server). |
| `admission.proxy_controller_user` | `system:serviceaccount:kube-system:replicaset-controller` | The only user allowed to create a `role=proxy` pod. |
| `clients.<client>.modes.<mode>` | required | `mode` is `none`, `allow` or `full`, else `BR_SANDBOX_MODE_UNKNOWN`. A key present is a namespace. |
| `...modes.<mode>.pool` | `{}` | Overrides any key of `pool` for this namespace. |
| `...modes.<mode>.runner.{service_account,namespace}` | required | Subject of the runner Role. |
| `...modes.<mode>.allow.{domains,cidrs}` | `[]` | For mode `allow`; consumed by the egress proxy of a later change of this chart. |
| `...modes.<mode>.credential_set` | `{}` | SLOT, filled by a later change of this chart; one set per namespace. |
| `egress.enabled`, `egress.proxy.port` | `false`, `8888` | SLOT. `enabled: true` fails the render (`BR_SANDBOX_SLOT_EMPTY`) until a later change of this chart fills its slots. |
| `keeper.{enabled,image,replicas,settings,resources}`, `keeper.lease.{duration_seconds,renew_deadline_seconds,retry_period_seconds}` | `false`, required if enabled, `2`, `{}`, 50m/64Mi and 500m/256Mi, 15/10/2 | `settings` goes untouched into the system config; the library `br-sandbox-keeper` adds its keys to the schema. |
| `network_check.{enabled,image,targets,run_id,cron}` | `true`, required if enabled, `[]`, `""`, `""` | A new `run_id` starts a new Job. `targets` are `host:port` entries (the bus, the database). |

Render errors: `BR_SANDBOX_MISSING_VALUE`, `BR_SANDBOX_MODE_UNKNOWN`, `BR_SANDBOX_SHARED_NAMESPACE` (two equal names, the system namespace included), `BR_SANDBOX_SAVE_AGE_INVALID`, `BR_SANDBOX_POOL_INVALID` (floor above `max_sandboxes`), `BR_SANDBOX_SLOT_EMPTY`.

Constants, not keys (a value that weakens one is a different chart): label prefix `br.sandbox`, `hostUsers: false`, seccomp `RuntimeDefault`, the seven capabilities, no service account token, no ingress, no port, Pod Security `baseline` enforced, `restricted` warn and audit.

## Slots

Later changes of this chart (the egress proxy and the keeper objects) fill these named templates (file `templates/_slots.tpl`). **They render nothing now.**

| Slot | Called from | Content later |
|---|---|---|
| `br-sandbox-chart.slot.egress.env` | the agent container (YAML list) | `HTTPS_PROXY`, CA paths, placeholders |
| `br-sandbox-chart.slot.egress.volumes` | the pod (YAML list) | CA ConfigMap |
| `br-sandbox-chart.slot.egress.volume_mounts` | the agent container (YAML list) | CA mount |
| `br-sandbox-chart.slot.egress.objects` | no caller yet | proxy Deployment, Service, ConfigMap, Secret mount, proxy NetworkPolicies, proxy Role |
| `br-sandbox-chart.slot.keeper.objects` | `templates/config.yaml` | anything else the keeper needs |

## Modes

| Mode | NetworkPolicies of the namespace | Pod DNS |
|---|---|---|
| `none` | `default-deny` only (ingress and egress, every pod) | `dnsPolicy: None`, nameserver `127.0.0.1` |
| `allow` | `default-deny` and `egress-allow`: sandbox pods reach only pods `br.sandbox/role=proxy` of the namespace, TCP on `egress.proxy.port` | same as `none` |
| `full` | `default-deny` and `egress-full`: `0.0.0.0/0` except internal, pod, service, node, node public and load balancer lists; with `dual_stack` also `::/0` except `fc00::/7`, `fe80::/10` and the IPv6 addresses. No rule names a pod or a namespace, so no cluster DNS | nameservers `network.public_resolvers` |

No policy has an ingress rule. The sandbox side selects `br.sandbox/role=sandbox`, never the pool label, so a claimed pod keeps its path. Mode `allow` has no egress until a later change of this chart renders the proxy (fail closed). A tool can send DNS to any public host in mode `full`; only the pod resolver is fixed.

A bound host that sends a credential value back shows it to the sandbox (no scrub exists; the proxy has no response scrub). The chart claims nothing else about credentials.

## Label contract

One prefix, `br.sandbox`. The adapter, the keeper and the policies use exactly these names.

| Name | Kind | Set by | Value |
|---|---|---|---|
| `br.sandbox/managed` | namespace label | chart | `true`; the label of the Kyverno exclusion and of the policy binding |
| `br.sandbox/mode`, `br.sandbox/client` | namespace label | chart | mode, client |
| `br.sandbox/role` | pod label | pod template | `sandbox` (or `proxy`, set by the egress proxy of a later change of this chart); never changes |
| `br.sandbox/pool` | pod label | template, then claim | `warm`, then `claimed` (one way); `probe` for a check pod |
| `br.sandbox/pool-id` | pod label | template | `{client}-{mode}`; never changes |
| `br.sandbox/thread`, `br.sandbox/agent`, `br.sandbox/creds` | pod label | claim | ids |
| `br.sandbox/clean`, `br.sandbox/claimed-at`, `br.sandbox/idle-since`, `br.sandbox/state-bytes`, `br.sandbox/state-turn`, `br.sandbox/credsets` | pod annotation | `br-runner-sandbox` | |
| `br.sandbox/system` | pod label | chart | `keeper` or `netcheck`, in the system namespace |

The selector of the ReplicaSet is `pool=warm` and `pool-id`. The ReplicaSet has no `spec.replicas`: the keeper is the only writer (one replica until it writes).

## The pod

Agent container `agent`: the project image, command `{tools.mount_path}/bin/pause`, user `runAsUser`, env `HOME=/home/agent`, volumes `tools` (read-only), `/work`, `/home/agent`, `/skills` (read-only). Init container `tools-init` (tools image): `/sbx/bin/bsdtar -x -p --no-same-owner -f /sbx/tools.tar -C {tools.mount_path}`. All containers drop ALL and add `CHOWN, DAC_OVERRIDE, FOWNER, FSETID, SETUID, SETGID, KILL`. Volumes are `emptyDir` with a size limit (a `configMap` only for the check pod).

**Seed container (contract for the library `br-runner-sandbox`).** Name `seed` (key `tools.seed_container.name`), image `br-sandbox-tools`, command `/sbx/bin/pause`, root user inside the user namespace, `readOnlyRootFilesystem`, no port, no shared process namespace, same seven capabilities. The library `br-runner-sandbox` writes the seed through `pods/exec` into this container, mount path `/skills`, program `/sbx/bin/bsdtar` (in the agent container the same program is `{tools.mount_path}/bin/bsdtar`, default `/opt/sbx/bin/bsdtar`). The agent mounts `/skills` read-only, so a non-root agent cannot change the seed. `tools.seed_container.enabled: false` removes the container; then `/skills` is writable by the agent and the seed is only the weak form. Its resources are small keys of their own: whether they count in the size of the pool is open.

## Admission policy

One policy and binding per (client, mode); values (size, runtime class, repositories, proxy user) are inlined. The binding matches the namespace by `br.sandbox/managed=true` and by name. It matches every pod, `CREATE` and `UPDATE`, and fails closed.

- A pod passes if it is `role=sandbox` and follows the rules below, or `role=proxy` and its creator is `admission.proxy_controller_user`. A pod with neither role is refused (`BR_SANDBOX_ROLE`). The role cannot change (`BR_SANDBOX_ROLE_IMMUTABLE`).
- Rules of a sandbox pod: 1 `hostUsers` false (`USERNS_REQUIRED`); 2 no host network, PID, IPC, port (`HOST_NAMESPACE`); 3 only `emptyDir` and `configMap` volumes, no `secretKeyRef`, no `secretRef` (`VOLUME_FORBIDDEN`); 4 drop ALL, add only the seven (`CAPABILITY`); 5 no privilege, no escalation, seccomp `RuntimeDefault` (`PRIVILEGE`); 6 cpu, memory and ephemeral-storage requests and limits on every container, not above the size of the pool (`RESOURCES`); 7 every image from an allowed repository (project image, tools image, check image, each with the mirror) with a tag or a digest (`IMAGE`); 8 no token, runtime class equals the chart value (`TOKEN_OR_RUNTIME`); 9 on update the security contexts, resources, volumes and `hostUsers` stay (`IMMUTABLE`).
- Patch rules (`BR_SANDBOX_LABELS`): a patch changes only `br.sandbox/` keys of labels and annotations; `role` and `pool-id` never change; a claimed pod never leaves `claimed`.
- Init and seed containers are containers of the pod and follow rules 4 to 7.

The exact user name of the controller that creates the proxy pods is not proven offline. A wrong name fails closed: the proxy pod is refused and does not start. The cluster test proves the name on the test cluster, and proves with the runner service account that an unlabelled pod, a `role=proxy` pod and a relabel of the role are refused. The CEL text is also not compiled offline.

## Image contract

**The agent image.** Any Linux image with `/bin/sh`, `cat` and `find`, for the architecture of the nodes. Nothing else: no agent binary, no `tar`, no user entry, no `ps`. The tools that the sandbox needs (`bsdtar`, `pause`) come from `br-sandbox-tools`. The project puts one registry link in `image`. Example: `br-sandbox-python` (Python 3.12, pandas, openpyxl, python-docx, python-pptx, LibreOffice), the test and reference image of this repository only; each project defines its image.

**A new push under one tag.** A tag pulls at each pod creation (`imagePullPolicy: Always`). A warm pod is used one time, so the pool takes the new image as it refills, and the keeper deletes a warm pod older than `warm_max_age_seconds` (default 1800). A quiet pool has the new image in 30 minutes at most. A digest is advised for production; with a digest the pull policy is `IfNotPresent`.

**The check image.** `network_check.image` needs `sh`, `kubectl`, `nc` and the POSIX tools `cat`, `sed`, `tr`, `mktemp`. It runs the check Job and the check pods. No image of this repository has them; the project or the cluster test sets the value.

## Network check

The Job `br-sandbox-netcheck-{run_id}` (and an optional CronJob) lists the nodes, services and pods through the API, builds targets (the `targets` values, a ClusterIP, a pod IP, each node address on 10250 and 6443, the API service, the cluster DNS service, a `.svc.cluster.local` name), and probes from the Job pod (the control) and from a probe pod of each namespace. The probe pod is made from the pool pod template (no token, root inside the user namespace). One JSON object per probe: `{net, mode, target, kind, expected, observed, control}`; `observed` is `refused`, `reached` or `no_result`; `control` is `ok` or `control_unavailable`. The Job fails on any `reached`, any `control_unavailable`, a missing result, or a node address that is not in the three node lists. The CONNECT probes through the proxy come with the egress proxy of a later change of this chart.

## Why

| Thing | Why it is the way it is |
|---|---|
| Namespace per (client, mode) | A mode cannot mix; the namespace checks hold by the namespace and not by a label. |
| The policy matches every pod | A pod without `role=sandbox` would escape and a runner with `pods create` could mount a Secret. |
| Policy per namespace | The size and the repositories are inlined, so a pool size is a single key. |
| `hostUsers: false` fixed | Root in the pod is a high host user; the seven capabilities hold only inside. |
| `pause` from the tools mount | The agent image has no reaper; an orphan stayed alive after SIGTERM with `sleep infinity`. |
| Seed in a second container | `pods/exec` cannot choose a user; a root writer is needed so that a non-root agent sees a read-only seed. |
| No `spec.replicas` | The keeper writes it; a chart value would fight the deploy tool. |
| Quota = size x (`max_sandboxes` + `extra_pods`) | The refill must not be refused; the small seed and init resources fit in the extra pods. |
| Selects `role`, never `pool` | A claim changes `pool`; the network path must not move. |
| `default-deny` in the system namespace only with keeper or check | An empty chart adds no policy there. |
| `egress-netcheck` allows all egress | The check must try to reach internal targets; the Job holds a read role, so it runs on demand. |
| Mirror is a prefix | `{mirror}/{reference}`; a mirror that rewrites the host is done by the mirror itself. |
| Label rule covers annotations | A runner may patch only `br.sandbox/` keys; a CNI that annotates the main pod resource would need an exception (cluster test). |

## Images and licences

| Image | Used for | SPDX of its content |
|---|---|---|
| `br-sandbox-tools` | init and seed container | `bsdtar` (libarchive) BSD-2-Clause; musl MIT; `pause` of ours Apache-2.0 |
| `br-sandbox-python` | test and reference agent image of this repository | Python PSF-2.0; pandas BSD-3-Clause; openpyxl MIT; python-docx MIT; python-pptx MIT; LibreOffice MPL-2.0 (its repository holds the table) |
| `br-sandbox-keeper` | keeper | Apache-2.0 (ours) |
| check image | check Job | chosen by the project |
| proxy image | egress proxy (a later change of this chart) | Apache-2.0 (iron-proxy) |

The chart itself is Apache-2.0 and ships no GPL program. Kubernetes (Apache-2.0), Helm (Apache-2.0) and Kyverno are not shipped.

## Needs of the cluster (not done by the chart)

`kubelet` in `/etc/subuid` and `/etc/subgid` on each node; the Kyverno exclusion for namespaces with `br.sandbox/managed=true` (without it mode `none` is not possible); the bucket with its lifecycle rule; the sealed Secret of `store.credentials_secret`. Policy objects need Kubernetes 1.30 or later (ValidatingAdmissionPolicy v1).

## Tests

`bash .github/scripts/test-sandbox-chart.sh` (offline: `helm template`, `yq`, `jq`) proves the checks that need no cluster. cluster-test rows (the CEL compiles and refuses each bad pod, the three tests per mode, the namespace-to-namespace test, the userns, the label-to-policy delay) run on a test cluster.
