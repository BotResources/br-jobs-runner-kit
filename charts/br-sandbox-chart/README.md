# br-sandbox-chart

The Helm chart of the sandbox. One installation per cluster. It renders everything that is declarative: namespaces, quotas, admission policy, network policies, warm pools, roles, the keeper and the network check. Licence: Apache-2.0. The chart is released by the workflow `chart-release`; it is not a library of the Libraries registry.

It does not claim a pod, execute a command, save or restore (the library `br-runner-sandbox`), size a pool or delete a save (the library `br-sandbox-keeper`). It creates no bucket, no Secret and no credential value.

## What a render holds

| Object | Scope | Gate |
|---|---|---|
| Namespace `{prefix}-system`, ConfigMap `br-sandbox-config` of the system namespace | once per installation | always |
| Namespace `{prefix}-{client}-{mode}`, ServiceAccount `sandbox` (no token, no Role), ResourceQuota `br-sandbox-quota`, LimitRange `br-sandbox-limits`, ConfigMap `br-sandbox-config` | each (client, mode) | always |
| ReplicaSet `warm-{client}-{mode}` | each (client, mode) | always; it has no `spec.replicas`, so a pool starts with one replica (the ReplicaSet default) until the keeper runs |
| NetworkPolicy `default-deny`, `egress-full` or `egress-allow` | each (client, mode) | always |
| Namespace `{sandbox namespace}-egress`, NetworkPolicies `default-deny`, `ingress-tunnel` and `egress-proxy`, ResourceQuota `br-sandbox-egress-quota` | each (client, mode) that uses a proxy | mode `allow`; mode `full` when `credential_set` is not empty |
| ValidatingAdmissionPolicy and binding `{prefix}-{client}-{mode}-pod` | each (client, mode) | always |
| Role and RoleBinding `br-sandbox-runner` | each (client, mode) | always |
| Role and RoleBinding `br-sandbox-keeper` | each (client, mode), sandbox namespace | `keeper.enabled` |
| Keeper Deployment, ServiceAccount, lease Role, ClusterRole `namespaces` read, NetworkPolicies `default-deny` and `egress-keeper` | system namespace | `keeper.enabled` |
| ServiceAccount, ClusterRole and Role `br-sandbox-netcheck`, ConfigMap `br-sandbox-netcheck`, Job `br-sandbox-netcheck-{run_id}`, CronJob, NetworkPolicy `egress-netcheck` | system namespace and each (client, mode) | `network_check.enabled` |

The system namespace is `{prefix}-system`. It carries the label `br.sandbox/system=true` and not `br.sandbox/managed`, so the Kyverno exclusion does not touch it.

## Layout: two namespaces for a mode with a proxy

A (client, mode) that uses a proxy has two namespaces. Mode `allow` always has them; mode `full` has them when `clients.<client>.modes.full.credential_set` is not empty. Mode `none` and mode `full` without a credential set have the sandbox namespace only.

| | Sandbox namespace `{prefix}-{client}-{mode}` | Egress namespace `{prefix}-{client}-{mode}-egress` |
|---|---|---|
| Holds | the sandbox pods, the warm pool, the CA certificate (not secret) | the proxy pods, the credential Secret, the CA key Secret, the sealed Secrets of the proxy |
| Label | `br.sandbox/managed=true`, `br.sandbox/mode`, `br.sandbox/client` | `br.sandbox/egress=true`, `br.sandbox/mode`, `br.sandbox/client`; never `br.sandbox/managed` |
| Pod Security | `baseline` enforced, `restricted` warn and audit | the same |
| Role and RoleBinding | `br-sandbox-runner`, `br-sandbox-keeper`, `br-sandbox-netcheck` | none |
| Policies | `default-deny`, `egress-allow` or `egress-full` | `default-deny`, `ingress-tunnel`, `egress-proxy` |
| Quota | `br-sandbox-quota` and `br-sandbox-limits` | `br-sandbox-egress-quota` (pods only, `egress.quota.pods`) |
| Admission policy | one per namespace, `role=sandbox` pods only | none |

No identity of the chart (the runner, the keeper, the network check, the sandbox ServiceAccount) has a Role or a RoleBinding in an egress namespace, so none of them reaches a proxy pod by `pods/exec` or `pods` patch, and none reads a Secret there. The controller of the proxy Deployment is the only identity that makes or changes a pod there; it comes with the proxy change of this chart. The one cluster-wide right on pods is the read (`get`, `list`) of the network check ClusterRole; no identity has a cluster-wide right on `secrets`. A runner namespace that equals an egress namespace fails the render with `BR_SANDBOX_SHARED_NAMESPACE`.

The egress namespace does not carry `br.sandbox/managed=true`, on purpose: the admission policy of a sandbox namespace binds on that label and refuses every pod that is not `role=sandbox`, so it would refuse the proxy pods. The consequence for a Kyverno exclusion (or any policy that selects the managed namespaces): the exclusion text that names `br.sandbox/managed=true` does not cover the egress namespace. The cluster default network policy (the generated policy per namespace with allow rules for DNS, intra-namespace traffic and ingress controllers) also acts in the egress namespace: without an exclusion that selects `br.sandbox/egress=true`, its allow rules add to `ingress-tunnel` and `egress-proxy`, so the statements above (ingress only from the sandbox pods, no cluster DNS) are not what the cluster enforces there. Any other exclusion the project needs there (user namespaces, runtime class) selects `br.sandbox/egress=true` too. The egress namespace has no `hostUsers: false` requirement from the chart; the proxy change sets its own pod security. A consumer that lists namespaces by `br.sandbox/client` or `br.sandbox/mode` also sees the egress namespaces and selects `br.sandbox/managed=true` to get the sandbox ones.

**Where the secrets live.** The credential Secret, the CA key Secret and every sealed Secret of the proxy are created in the egress namespace by the deploy tool; the chart renders none. The CA certificate (not secret) stays in the sandbox namespace, for the sandbox pods: the chart renders nothing of it now, and the proxy change places it there. The pull secrets of `pull_secrets` must exist in the egress namespace too: an image pull reads the Secret of the namespace of the pod.

**Name.** `{sandbox namespace}-egress` is a working name. It comes from the helper `br-sandbox-chart.egress.namespace`, the only place that writes the suffix; a change of the pattern is a change of that helper. A name over 63 characters fails the render with `BR_SANDBOX_NAME_TOO_LONG`, and a name equal to another namespace of the render (the system namespace, a sandbox namespace, a runner namespace) fails with `BR_SANDBOX_SHARED_NAMESPACE`. One egress namespace belongs to one (client, mode): two clients never share one.

**What the egress namespace renders now.** Its Namespace, its default-deny, its two allow rules and its quota. The proxy Deployment, Service, ConfigMap and Secrets are not rendered yet: they come in a later change of this chart through the slot `br-sandbox-chart.slot.egress.objects`, which receives the root, the namespace item and the name of the egress namespace (`dict "root" . "ns" $ns "namespace" $name`). Until then no proxy pod exists, so a sandbox of a mode with a proxy has no egress at all (fail closed).

**Rules of the egress namespace.** `default-deny` on every pod, both directions. `ingress-tunnel`: proxy pods take traffic only from the pods `br.sandbox/role=sandbox` of the sandbox namespace of the same (client, mode) (namespace and pod selector in the same item), TCP on `egress.proxy.port`; no other port, so the reload port of the proxy is reached by no sandbox. `egress-proxy`: proxy pods reach `0.0.0.0/0` (and `::/0` with `dual_stack`) except three groups: the internal ranges (`network.internal_cidrs`), the pod and service networks, and the node, node public and load balancer addresses. The rule names no pod and no namespace, and no cluster DNS: no cluster DNS is reachable from the egress namespace, so the proxy of the later change must resolve names through public resolvers.

## Values

Required keys have no default: a missing key stops the render with `BR_SANDBOX_MISSING_VALUE{key=...}`. Keys are snake_case; keys that are Kubernetes fields keep the Kubernetes name (`runtimeClassName`, `runAsUser`, `runAsGroup`, `terminationGracePeriodSeconds`, `imagePullSecrets` names, resource maps). `values.schema.json` has `additionalProperties: false` on every block with fixed keys: a key outside its place fails the render. Maps keyed by a name are checked entry by entry (`clients`, `clients.<client>.modes`, `pool.node_selector` with string values). Two objects take any key: `keeper.settings` and `clients.<client>.modes.<mode>.credential_set`. All durations are in seconds. This table is the contract for the units `egress` and `keeper`.

| Key | Default | Meaning |
|---|---|---|
| `installation.namespace_prefix` | required | Prefix of every namespace, at most 22 characters. With a client name of 32 characters at most, a sandbox namespace name stays under 63; the egress namespace of a mode with a proxy adds `-egress` (7 characters) and a name over 63 fails with `BR_SANDBOX_NAME_TOO_LONG`. |
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
| `pool.{floor,max_sandboxes,refill_seconds,margin}` | 1, 20, 3, 2 | Read by the keeper. The chart does not apply `floor`: it sets no replica count, so without the keeper each pool runs one pod. |
| `pool.terminationGracePeriodSeconds`, `pool.node_selector` | `1`, `{}` | |
| `quota.extra_pods` | `4` | Pods of the quota = `max_sandboxes` + this (network check probe pod, slack; the proxy pods count in the egress namespace). |
| `network.{pod_cidr,service_cidr}` | required | |
| `network.{node_addresses,node_public_addresses,load_balancer_addresses}` | required lists | `[]` must be written on purpose; no default exists. |
| `network.public_resolvers` | required if a mode is `allow` or `full` | Nameservers of the pods of mode `full`. |
| `network.internal_cidrs` | RFC1918, 100.64/10, 169.254/16, 127/8 | |
| `network.ipv6` | `single_stack` | `dual_stack` adds `::/0` except `fc00::/7`, `fe80::/10` and the IPv6 addresses of the lists. |
| `network.api_server_addresses` | required with the keeper or the network check; at least one | Egress of the keeper and of the network check (API server). |
| `network.store_cidrs` | required with the keeper only; at least one | Egress of the keeper to the store. |
| `clients.<client>.modes.<mode>` | required | `mode` is `none`, `allow` or `full`, else `BR_SANDBOX_MODE_UNKNOWN`. A key present is a namespace. |
| `...modes.<mode>.pool` | `{}` | Overrides any key of `pool` for this namespace. |
| `...modes.<mode>.runner.{service_account,namespace}` | required | Subject of the runner Role. |
| `...modes.<mode>.allow.{domains,cidrs}` | `[]` | For mode `allow`; consumed by the egress proxy of a later change of this chart. A non-empty list fails the render with `BR_SANDBOX_EGRESS_DISABLED` while `egress.enabled` is false, and with `BR_SANDBOX_ALLOW_MODE` in a mode other than `allow`. |
| `...modes.<mode>.credential_set` | `{}` | One set per namespace; its content is read by a later change of this chart. A non-empty value fails the render with `BR_SANDBOX_EGRESS_DISABLED` while `egress.enabled` is false, and with `BR_SANDBOX_CREDENTIAL_SET_MODE` for mode `none`. Non-empty in mode `full`: the (client, mode) gets an egress namespace and its sandbox reaches only the proxy. |
| `egress.enabled`, `egress.proxy.port` | `false`, `8888` | `enabled: true` lets the allow lists and credential sets render. The proxy objects are a later change of this chart; `port` is the tunnel port of both namespaces. |
| `egress.quota.pods` | `4` | Pods of the quota of an egress namespace. |
| `keeper.{enabled,image,replicas,settings,resources}`, `keeper.lease.{duration_seconds,renew_deadline_seconds,retry_period_seconds}` | `false`, required if enabled, `2`, `{}`, 50m/64Mi and 500m/256Mi, 15/10/2 | `settings` goes untouched into the system config; the library `br-sandbox-keeper` adds its keys to the schema. |
| `network_check.{enabled,image,targets,run_id,cron}` | `true`, required if enabled, `[]`, `""`, `""` | A new `run_id` starts a new Job; an empty `run_id` gives the Job name suffix `initial`. `targets` are `host:port` entries (the bus, the database). |

Render errors: `BR_SANDBOX_MISSING_VALUE`, `BR_SANDBOX_MODE_UNKNOWN`, `BR_SANDBOX_SHARED_NAMESPACE` (two equal names among the system, sandbox and egress namespaces, or a runner namespace equal to an egress namespace), `BR_SANDBOX_NAME_TOO_LONG` (a namespace name over 63 characters), `BR_SANDBOX_SAVE_AGE_INVALID`, `BR_SANDBOX_POOL_INVALID` (floor above `max_sandboxes`), `BR_SANDBOX_EGRESS_DISABLED`, `BR_SANDBOX_CREDENTIAL_SET_MODE`, `BR_SANDBOX_ALLOW_MODE` (an `allow` list in mode `none` or `full`), `BR_SANDBOX_QUANTITY_INVALID` (a resource quantity with an unknown suffix), `BR_SANDBOX_RUNNER_IS_POD_IDENTITY` (a `runner.service_account` named `sandbox` with a `runner.namespace` that is a sandbox namespace: the runner identity is never the identity of the sandbox pods).

Constants, not keys (a value that weakens one is a different chart): label prefix `br.sandbox`, `hostUsers: false`, seccomp `RuntimeDefault`, the seven capabilities, no service account token, no ingress, no port, Pod Security `baseline` enforced, `restricted` warn and audit.

## Slots

Later changes of this chart (the egress proxy and the keeper objects) fill these named templates (file `templates/_slots.tpl`). **They render nothing now.**

| Slot | Called from | Content later |
|---|---|---|
| `br-sandbox-chart.slot.egress.env` | the agent container (YAML list) | `HTTPS_PROXY`, CA paths, placeholders |
| `br-sandbox-chart.slot.egress.volumes` | the pod (YAML list) | CA ConfigMap |
| `br-sandbox-chart.slot.egress.volume_mounts` | the agent container (YAML list) | CA mount |
| `br-sandbox-chart.slot.egress.objects` | `templates/egress.yaml`, once per egress namespace, with `dict "root" . "ns" $ns "namespace" <egress namespace name>` | proxy Deployment, Service, ConfigMap and Secret mount, all in the egress namespace; the proxy controller identity, the only identity with a right there |
| `br-sandbox-chart.slot.keeper.objects` | `templates/config.yaml` | anything else the keeper needs |

## Modes

| Mode | NetworkPolicies of the namespace | Pod DNS |
|---|---|---|
| `none` | `default-deny` only (ingress and egress, every pod) | `dnsPolicy: None`, nameserver `127.0.0.1` |
| `allow` | `default-deny` and `egress-allow`: sandbox pods reach only the pods `br.sandbox/role=proxy` of the egress namespace of the same (client, mode), TCP on `egress.proxy.port`; namespace and pod are selected in the same rule item; no DNS | same as `none` |
| `full` | `default-deny` and `egress-full`. With a credential set: the same rule as mode `allow`, nothing else, and the pod resolver is `127.0.0.1`. Without: `0.0.0.0/0` except internal, pod, service, node, node public and load balancer lists; with `dual_stack` also `::/0` except `fc00::/7`, `fe80::/10` and the IPv6 addresses. No rule names a pod or a namespace, so no cluster DNS | nameservers `network.public_resolvers` |

No policy of a sandbox namespace or of the system namespace has an ingress rule; the only ingress rule of the chart is `ingress-tunnel` of an egress namespace. The sandbox side selects `br.sandbox/role=sandbox`, never the pool label, so a claimed pod keeps its path. A mode with a proxy has no egress until a later change of this chart renders the proxy (fail closed). A tool can send DNS to any public host in mode `full`; only the pod resolver is fixed.

A bound host that sends a credential value back shows it to the sandbox (no scrub exists; the proxy has no response scrub). The chart claims nothing else about credentials.

## Label contract

One prefix, `br.sandbox`. The adapter, the keeper and the policies use exactly these names.

| Name | Kind | Set by | Value |
|---|---|---|---|
| `br.sandbox/managed` | namespace label | chart | `true`; the label of the Kyverno exclusion and of the policy binding |
| `br.sandbox/egress` | namespace label | chart | `true` on an egress namespace only; never together with `br.sandbox/managed` |
| `br.sandbox/mode`, `br.sandbox/client` | namespace label | chart | mode, client (sandbox and egress namespaces) |
| `br.sandbox/role` | pod label | pod template | `sandbox` in a sandbox namespace, `proxy` in an egress namespace (set by the egress proxy of a later change of this chart); never changes |
| `br.sandbox/pool` | pod label | template, then claim | `warm`, then `claimed` (one way); `probe` for a check pod |
| `br.sandbox/pool-id` | pod label | template | `{client}-{mode}`; never changes |
| `br.sandbox/thread`, `br.sandbox/agent`, `br.sandbox/creds` | pod label | claim | ids |
| `br.sandbox/clean`, `br.sandbox/claimed-at`, `br.sandbox/idle-since`, `br.sandbox/state-bytes`, `br.sandbox/state-turn`, `br.sandbox/credsets` | pod annotation | `br-runner-sandbox` | |
| `br.sandbox/system` | pod label | chart | `keeper` or `netcheck`, in the system namespace |
| `br.sandbox/system` | namespace label | chart | `true` on the system namespace only |

The selector of the ReplicaSet is `pool=warm` and `pool-id`. The ReplicaSet has no `spec.replicas`, on purpose: the keeper is the only writer of the replica count, because a value rendered by the chart would be reset at each `helm upgrade`. A pool starts with one replica (the ReplicaSet default) until the keeper runs. The `floor` value is read by the keeper; the chart does not apply it.

## The pod

Agent container `agent`: the project image, command `{tools.mount_path}/bin/pause`, user `runAsUser` with `runAsNonRoot: true` (the init container `tools-init` runs as the same user with the same flag), env `HOME=/home/agent`, volumes `tools` (read-only), `/work`, `/home/agent`, `/skills` (read-only). Init container `tools-init` (tools image): `/sbx/bin/bsdtar -x -p --no-same-owner -f /sbx/tools.tar -C {tools.mount_path}`. All containers drop ALL and add `CHOWN, DAC_OVERRIDE, FOWNER, FSETID, SETUID, SETGID, KILL`. Volumes are `emptyDir` with a size limit (a `configMap` only for the check pod).

**Seed container (contract for the library `br-runner-sandbox`).** Name `seed` (key `tools.seed_container.name`), image `br-sandbox-tools`, command `/sbx/bin/pause`, root user inside the user namespace, `readOnlyRootFilesystem`, no port, no shared process namespace, same seven capabilities. The library `br-runner-sandbox` writes the seed through `pods/exec` into this container, mount path `/skills`, program `/sbx/bin/bsdtar` (in the agent container the same program is `{tools.mount_path}/bin/bsdtar`, default `/opt/sbx/bin/bsdtar`). The agent mounts `/skills` read-only, so a non-root agent cannot change the seed. `tools.seed_container.enabled: false` removes the container; then `/skills` is writable by the agent and the seed is only the weak form. Its resources are small keys of their own: whether they count in the size of the pool is open.

## Runner role

The Role `br-sandbox-runner` of each sandbox namespace is the contract for the adapter. It binds the `runner.service_account` of `runner.namespace`. Rules: `pods` get, list, watch, patch, create, delete; `pods/exec` create; `replicasets` get, list, watch (the adapter reads the condition of the pool); `configmaps` get on `br-sandbox-config` only (the adapter reads the config). The keeper Role of the same namespace (with `keeper.enabled`) has `pods` get, list, watch, delete; `replicasets` get, list, watch, patch; `replicasets/status` patch; `events` create. It has no `pods` patch. No identity of the chart has a Role in an egress namespace.

## Admission policy

One policy and binding per (client, mode); values (size, runtime class, repositories) are inlined. The binding matches the namespace by `br.sandbox/managed=true` and by name. It matches every pod, `CREATE` and `UPDATE`, and fails closed.

- A pod passes if it is `role=sandbox` and follows the rules below. Any other role, or none, is refused (`BR_SANDBOX_ROLE`), `role=proxy` included: a proxy pod does not live in a sandbox namespace, it lives in the egress namespace, which the policy does not bind. The role cannot change (`BR_SANDBOX_ROLE_IMMUTABLE`).
- Rules of a sandbox pod: 1 `hostUsers` false (`USERNS_REQUIRED`); 2 no host network, PID, IPC, port (`HOST_NAMESPACE`); 3 only `emptyDir` and `configMap` volumes, no `secretKeyRef`, no `secretRef` (`VOLUME_FORBIDDEN`); 4 drop ALL, add only the seven (`CAPABILITY`); 5 no privilege, no escalation, seccomp `RuntimeDefault` on the pod and, when a container sets a profile, on that container (`PRIVILEGE`); 6 cpu, memory and ephemeral-storage requests and limits on every container, not above the size of the pool (`RESOURCES`); 7 every image from an allowed repository (project image, tools image, check image, each with the mirror) with a tag or a digest (`IMAGE`); 8 no token, runtime class equals the chart value (`TOKEN_OR_RUNTIME`); 9 on update the security contexts, resources, volumes and `hostUsers` stay (`IMMUTABLE`); 10 every container of the three lists (`containers`, `initContainers`, `ephemeralContainers`) sets `runAsNonRoot: true` and a `runAsUser` above 0 in its own `securityContext`, and the pod does not share its process namespace (`RUN_AS_NON_ROOT`).
- Patch rules (`BR_SANDBOX_LABELS`): a patch changes only `br.sandbox/` keys of labels and annotations; `role` and `pool-id` never change; a claimed pod never leaves `claimed`.
- Init and seed containers are containers of the pod and follow rules 4 to 7 and 10.
- Rule 10 has two exceptions, both exact. The seed container passes by its name (`tools.seed_container.name`) and by its image, which is exactly the tools image that the chart renders (the same string, with the mirror prefix when there is one); it must still run as `runAsUser` 0, have a read-only root file system and no port, and rule 4 still limits it to the seven capabilities. A container with the seed name and another image is refused. The pods of the network check, which run as uid 0, pass by the user name of the ServiceAccount that makes them, `system:serviceaccount:{system namespace}:br-sandbox-netcheck`, and by no label or name; they still must not share the process namespace. With `network_check.enabled: false` the rule has no such exception, and with `tools.seed_container.enabled: false` it has no seed exception. The policy also watches `UPDATE` of `pods/ephemeralcontainers`, so an ephemeral container as root is refused.

**What the policy does not hold.** The seed is written by the adapter through `pods/exec` into the seed container, which runs as root. No rule protects the seed against an adapter that is compromised: it can write any content to `/skills`. The policy holds the shape of the pods, not the content of their files.

The refusals of an unlabelled pod, of a `role=proxy` pod and of a relabel of the role, with the runner service account, in a sandbox namespace, are proved by a cluster test, and so is the refusal of rule 10 by the runner service account. That test is not part of the offline suite of this chart and has not been executed at this version. The CEL of the admission policy is not compiled offline, so at this version it has been neither compiled nor executed on a cluster.

## Image contract

**The agent image.** Any Linux image with `/bin/sh`, `cat` and `find`, for the architecture of the nodes. Nothing else: no agent binary, no `tar`, no user entry, no `ps`. The tools that the sandbox needs (`bsdtar`, `pause`) come from `br-sandbox-tools`. The project puts one registry link in `image`. Example: `br-sandbox-python` (Python 3.12, pandas, openpyxl, python-docx, python-pptx, LibreOffice), the test and reference image of this repository only; each project defines its image.

**A new push under one tag.** A tag pulls at each pod creation (`imagePullPolicy: Always`). A warm pod is used one time, so the pool takes the new image as it refills, and the keeper deletes a warm pod older than `warm_max_age_seconds` (default 1800). A quiet pool has the new image in 30 minutes at most. A digest is advised for production; with a digest the pull policy is `IfNotPresent`.

**The check image.** `network_check.image` needs `sh`, `kubectl`, `nc` and the POSIX tools `cat`, `sed`, `tr`, `mktemp`. It runs the check Job and the check pods. No image of this repository has them; the project or the cluster test sets the value.

## Network check

The Job `br-sandbox-netcheck-{run_id}` (and an optional CronJob) lists the nodes, services and pods through the API, builds targets (the `targets` values, a ClusterIP, a pod IP, each node address on 10250 and 6443, the API service, the cluster DNS service, a `.svc.cluster.local` name), and probes from the Job pod (the control) and from a probe pod of each namespace. The probe pod is made from the pool pod template (no token, root inside the user namespace); rule 10 of the admission policy lets it through by the identity of the check ServiceAccount. One JSON object per probe: `{net, mode, target, kind, expected, observed, control}`; `observed` is `refused`, `reached` or `no_result`; `control` is `ok` or `control_unavailable`. The Job fails on any `reached`, any `control_unavailable`, a missing result, a node address that is not in the three node lists, or a target class with no target. The classes are `targets` (at least one entry of `network_check.targets`), `node_kubelet`, `node_api`, `api_service`, `dns_service`, `cluster_name` and `pod` (a pod IP with a declared container port); a missing class is one object with `kind` `target_class` and `observed` `missing`. The CONNECT probes through the proxy come with the egress proxy of a later change of this chart.

The network check identity reads the pod specs of the whole cluster (a cluster-wide `get` and `list` on `pods`, `nodes`, `services` and `endpoints`) to pick its targets; its rights to create and delete pods and to read pod logs are bound in the sandbox namespaces only.

## Why

| Thing | Why it is the way it is |
|---|---|
| Namespace per (client, mode) | A mode cannot mix; the namespace checks hold by the namespace and not by a label. |
| The policy matches every pod | A pod without `role=sandbox` would escape and a runner with `pods create` could mount a Secret. |
| Proxy pods and their Secrets in a second namespace | The runner holds `pods/exec` and `pods` patch in the sandbox namespace; a proxy pod there would let a compromised runner read the credential value and the CA key. No identity of the chart has a right in the egress namespace. |
| Egress namespace without `br.sandbox/managed` | The admission policy of a sandbox namespace accepts `role=sandbox` pods only; it must not bind the proxy namespace. |
| Policy per namespace | The size and the repositories are inlined, so a pool size is a single key. |
| `hostUsers: false` fixed | Root in the pod is a high host user; the seven capabilities hold only inside. |
| `pause` from the tools mount | The agent image has no reaper; an orphan stayed alive after SIGTERM with `sleep infinity`. |
| Seed in a second container | `pods/exec` cannot choose a user; a root writer is needed so that a non-root agent sees a read-only seed. |
| No `spec.replicas` | The keeper is the only writer of the replica count; a value rendered by the chart would be reset at each `helm upgrade`. A pool starts with one replica (the ReplicaSet default) until the keeper runs; `floor` is read by the keeper, not applied by the chart. |
| Quota = size x (`max_sandboxes` + `extra_pods`) + seed container resources x `max_sandboxes` (when `tools.seed_container.enabled`) | The refill must not be refused: each sandbox pod holds the agent and the seed container, and the extra pods hold one size each. The init container does not add: a pod counts the larger of its init containers and the sum of its containers. |
| Seed exception by name and image | A name alone would let a pod run any image as root in a container called like the seed. |
| Check exception by user name | The probe pods run as root, so rule 10 admits them by the ServiceAccount that makes them; a label or a name can be set by any identity that creates a pod. |
| Selects `role`, never `pool` | A claim changes `pool`; the network path must not move. |
| `default-deny` in the system namespace only with keeper or check | An empty chart adds no policy there. |
| `egress-netcheck` allows all egress | The check must try to reach internal targets; the Job holds a read role, so it runs on demand. |
| Mirror is a prefix | `{mirror}/{reference}`; a mirror that rewrites the host is done by the mirror itself. |
| Label rule covers annotations | A runner may patch only `br.sandbox/` keys; a CNI that annotates the main pod resource would need an exception (to check on a cluster; no such check has been executed at this version). |

## Images and licences

| Image | Used for | SPDX of its content |
|---|---|---|
| `br-sandbox-tools` | init and seed container | `bsdtar` (libarchive) BSD-2-Clause; musl MIT; `pause` of ours Apache-2.0; GCC runtime code statically linked GPL-3.0-or-later WITH GCC-exception-3.1 |
| `br-sandbox-python` | test and reference agent image of this repository | Python PSF-2.0; pandas BSD-3-Clause; openpyxl MIT; python-docx MIT; python-pptx MIT; LibreOffice MPL-2.0 (its repository holds the table) |
| `br-sandbox-keeper` | keeper | Apache-2.0 (ours) |
| check image | check Job | chosen by the project |
| proxy image | egress proxy (a later change of this chart), runs in the egress namespace | Apache-2.0 (iron-proxy) |

The chart itself is Apache-2.0 and ships no GPL program. Kubernetes (Apache-2.0), Helm (Apache-2.0) and Kyverno are not shipped.

## Needs of the cluster (not done by the chart)

`kubelet` in `/etc/subuid` and `/etc/subgid` on each node; the Kyverno exclusion for namespaces with `br.sandbox/managed=true` (without it mode `none` is not possible) and for namespaces with `br.sandbox/egress=true` (the default network policy of the cluster would add its allow rules there, and the egress namespace would no longer follow `ingress-tunnel` and `egress-proxy`); the bucket with its lifecycle rule; the sealed Secret of `store.credentials_secret`. Each Secret named in `pull_secrets` in every sandbox namespace and in the system namespace, and in every egress namespace (an image pull reads the Secret of the namespace of the pod; the chart renders no Secret). Policy objects need Kubernetes 1.30 or later (ValidatingAdmissionPolicy v1).

A node needs a kubelet with subordinate id ranges (`/etc/subuid` and `/etc/subgid`) for user namespaces. The chart ships no node label for this and requires none. A node without the prerequisite refuses the pod at start, so the pool stays below its floor. A project that wants to restrict the pool to prepared nodes sets `pool.node_selector` to a label of its own; the value reaches the node selector of every pod of the pool.

## Tests

`bash .github/scripts/test-sandbox-chart.sh` (offline: `helm template`, `yq`, `jq`) proves the checks that need no cluster. The cluster-test rows (the CEL compiles and refuses each bad pod, the three tests per mode, the namespace-to-namespace test, the userns, the label-to-policy delay) are not part of this suite and have not been executed at this version; they are to be executed on a test cluster.

The check of foreign values in the chart files greps registries and host names only. A client name or a project name in the chart is checked by review, not by this suite.
