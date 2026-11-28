# Install br-sandbox-chart 0.4.0

Status words: tested offline (the test is named, all in `test-sandbox-chart-operator-docs.sh` unless another file is named), read from a page or a file (named), not tested on a cluster. **No command in this file was run on a cluster by the tests of this repository.** The offline suite renders the same values files with `helm template` and lints them with `helm lint --strict` (the lint loop of `ci.yml`); that proves the render, and nothing about an API server.

0.4.0 is a breaking minor with no upgrade path from 0.3.x: remove a 0.3.x release before the install. Chart 0.4.x needs keeper 0.2.x (see Compatibility in `README.md`). The chart has two parts. The operator part is installed once per cluster. The fence part is applied to each namespace of the list `namespaces`, by the name and the mode of the namespace. A project then declares its own namespace and its `SandboxPool`.

## Values files

The commands below run from a checkout of this repository, because the folder `ci/` is not in the packaged chart (`.helmignore`). The fixtures give the shape of the platform values: `ci/example-values.yaml` (all the constants of the platform: installation prefix, tools image, store, network addresses, limits, scopes), `ci/platform-values.yaml` (the list `namespaces` is empty) and `ci/fence-values.yaml` (the operator part and the network check are off). A real platform copies `ci/example-values.yaml` and replaces every value (the fixture holds `registry.example.test` hosts and the addresses of a test network). `operator.image` is the image of the keeper 0.2.1 or a later 0.2.x (keeper 0.2.0 has a defect in the replicas of a pool and must not be deployed, see Compatibility in `README.md`); the fixture sets the image of keeper 0.2.1, `ghcr.io/botresources/br-sandbox-keeper@sha256:2cf56ab2e5daa673e86aa325b880578be2630e803b81b0924a9f52a580a0705b` (a manifest list for `linux/amd64` and `linux/arm64`, read from the release record of the keeper and not inspected by the offline suite), the same value as in the README. The owner gives `network.api_server_addresses` and `network.store_cidrs` of the real cluster. The two releases must be given the same platform constants, `grants.writers` included.

## Operator part, once per cluster

```
helm install br-sandbox charts/br-sandbox-chart \
  -f charts/br-sandbox-chart/ci/example-values.yaml \
  -f charts/br-sandbox-chart/ci/platform-values.yaml \
  --set crds.install=true
```

This renders the system namespace, the keeper, the ClusterRoles `br-sandbox-controller` and `br-sandbox-runner`, the three policies `br-sandbox-pod`, `br-sandbox-namespace` and `br-sandbox-fence` with their bindings (and `br-sandbox-pod-update`), the ConfigMap `br-sandbox-grants` and the definition of `SandboxPool`, and no sandbox namespace. The definition has the annotation `helm.sh/resource-policy: keep`, so `helm uninstall` leaves it (read from the Helm documentation, not tested on a cluster). If someone restores the binding `br-sandbox-pod` with `kubectl apply`, the next `helm upgrade` of the operator release fails on a server-side-apply conflict and needs `--force-conflicts` (reported by the operator of a test cluster; no record in this repository; not proved here). Only one release sets `crds.install`; the chart cannot see another release. The render of this command is tested offline (`check_the_install_commands_render`, `test-sandbox-chart-operator-parts.sh` for what it holds); the order of the objects inside the release is the order of Helm and is not tested. The Kyverno exclusions of the README (Needs of the cluster) must exist before any managed namespace exists. With the fixture values (`network_check.enabled: true`) the command also renders the check Job `br-sandbox-netcheck-initial`, its ServiceAccount, its ClusterRole and the NetworkPolicy `br-sandbox-egress-netcheck` in the system namespace; with `namespaces: []` that Job probes no sandbox namespace and proves nothing (see Network check of a sandbox namespace below).

The list `grants.writers` (default `[]`) is rendered by this part into the ConfigMap `br-sandbox-grants`, which the fence part does not render. The fixture `ci/platform-values.yaml` lists `system:serviceaccount:gitops:platform`.

## Fence part, for the namespaces of the platform

```
helm install br-sandbox-fence charts/br-sandbox-chart \
  -f charts/br-sandbox-chart/ci/example-values.yaml \
  -f charts/br-sandbox-chart/ci/fence-values.yaml
```

This renders, for each entry of `namespaces`, the objects of the fence (README, What a render holds) and no cluster-scoped object other than Namespace objects (`check_the_install_commands_render`). With `render_namespace: false` (the default) the chart renders no managed Namespace: the fence is applied to a namespace that the project made. The fixture `ci/example-values.yaml` sets `render_namespace: true`, so the fence render of the command above holds the managed Namespace of each entry. The entry gives the name and the mode; the `client_id` and the runner identity come from the table `scopes`. A namespace whose name matches no scope fails the render (`BR_SANDBOX_NO_SCOPE`). With the stamper (`stamper.enabled`, off by default, see `STAMPER.md`) the same objects are generated when the namespace is made, and this release is not needed for the modes `none` and `full`; the stamper is not recorded as run in this repository (`STAMPER.md`). One namespace has one maker of its fence: a fence release, or the stamper. The stamper acts on every scope of the scope table, and also on namespaces that exist. Do not turn the stamper on while a fence release fences a namespace of a scope of the table. The effect is not tested: the stamper can take the objects, overwrite them, or delete them when it is turned off. Use a scope table for the stamper whose scopes have no namespace fenced by the fence release, and compare it with the output of `kubectl get namespaces -l br.sandbox/managed=true` before turning the stamper on. The policy engine needs rights that the chart does not render: apply them first, then turn the stamper on (`STAMPER.md`, item 3: with the default rights `helm upgrade` fails with the message `validate-policy.kyverno.svc denied ... kyverno-background-controller requires permissions create,update,delete for resource v1/ServiceAccount in namespace {{ request.object.metadata.name }}`, and `helm upgrade --dry-run=server` does not show it). With these rights, a person who takes control of the background controller of the policy engine can write service accounts, quotas, limit ranges, ConfigMaps, RoleBindings and NetworkPolicies in every namespace, and can bind the two ClusterRoles of the sandbox there. Not tested: to turn the stamper off, or to remove the policy engine, while stamped namespaces exist. The policy engine can then delete the fence objects that it made; a namespace with no fence gets no pool. Remove the pools first. The modes `allow` and `full` with a credential set need inputs of the platform that are not decided (`LIMITS.md`).

The identity that applies the fence part (in the command above, `system:serviceaccount:gitops:platform`) must be listed in `grants.writers` of the operator part. The policy `br-sandbox-fence` refuses every write of a NetworkPolicy, ResourceQuota, LimitRange, Role or RoleBinding of a managed namespace, of the ServiceAccount `sandbox` and of the ConfigMaps `br-sandbox-*`, whatever the name of the object, unless the user is in `grants.writers`, and a validating policy is not bypassed by cluster-admin. With the default `[]` the install of the fence part is refused, object by object, with `BR_SANDBOX_FENCE`. The fixture `ci/fence-values.yaml` lists the same identity as `ci/platform-values.yaml`, because the two releases take the same platform constants. That an API server refuses the install in this way is not tested on a cluster. A run reported by the operator of a test cluster (no record in this repository; not proved here) says that a project identity's create of a RoleBinding and its delete of a binding were refused by RBAC (it holds no such right), not by `BR_SANDBOX_FENCE` or another policy code; the refusal by the policy is not proved on a cluster; the offline test applies the policy model to the objects of the fence render.

## Project side

The project declares two objects. Mode `none` is the only mode that a project gets without a grant of the platform (`BR_SANDBOX_MODE_NOT_GRANTED`, `POLICIES.md`). The name must be a scope of the platform or start with a scope and a dash (`BR_SANDBOX_SCOPE`); in these examples the scope is `sbx-alpha`. The chart renders the Pod Security labels of a sandbox namespace only with `render_namespace: true`. Here the project makes the Namespace, so its owner sets them, as in the example: no policy and no stamper of the chart sets or requires them, and the policy model admits the Namespace without them (`check_the_project_manifests_of_the_install`; not tested on a cluster). These two manifests were not applied to a cluster. The offline suite checks that the policy model admits the Namespace, that the `SandboxPool` holds only keys of the definition, and that the pod that the keeper makes from the template with this pool is admitted.

```yaml
apiVersion: v1
kind: Namespace
metadata:
  name: sbx-alpha-none
  labels:
    br.sandbox/managed: "true"
    br.sandbox/mode: none
    pod-security.kubernetes.io/enforce: baseline
    pod-security.kubernetes.io/warn: restricted
    pod-security.kubernetes.io/audit: restricted
```

```yaml
apiVersion: sandbox.botresources.ai/v1alpha1
kind: SandboxPool
metadata:
  name: pool
  namespace: sbx-alpha-none
spec:
  image: registry.example.test/agents/base:1.0
  size:
    cpu:
      request: 250m
      limit: "1"
    memory:
      request: 512Mi
      limit: 1Gi
    disk:
      request: 1Gi
      limit: 2Gi
  minReplicas: 1
  maxReplicas: 10
  margin: 2
  refillSeconds: 3
```

The only required field of the definition file is `image`, but keeper 0.2.0 also requires `size`, `minReplicas` and `maxReplicas` in a `SandboxPool`: a resource without them is `InvalidSpec` and gets no pool (the README of keeper 0.2.0 lists the four as required, and its code gives a spec that cannot be read the reason `InvalidSpec`; not tested here). The size of a container is bound by `size` and by the maximum `limits.size` of the platform; a namespace with no `SandboxPool` admits no pod. The keeper makes the ReplicaSet `warm-sbx-alpha-none` from this resource and from the ConfigMap `br-sandbox-pod-template`, once the fence is there. That the keeper does so is a fact of keeper 0.2, not tested in this repository.

## Order

The order that the design takes is: the definition and the operator part (one release), then the stamper or the fence part, then the namespace and the resource of the project. Other orders are inferred to fail closed and are not tested: a resource before the definition has no kind to match; a keeper before the definition is not ready; a namespace before its fence has no pool (`NamespaceNotFenced`). Keeper 0.2 refuses the document of chart 0.3.x at start because it holds `pools` (read from the README of keeper 0.2.0); this chart states nothing about keeper 0.1 with the document of 0.4.0. If a managed namespace exists before its fence part is installed, the keeper logs forbidden list and watch errors about once a second until the RoleBinding `br-sandbox-keeper` of the fence part arrives, and then it recovers: install the fence part right after the namespace is labelled, or in the same step (reported by the operator of a test cluster; no record in this repository; not proved here).

## Objects the platform supplies for a proxied mode

An entry in mode `allow`, or in mode `full` with a `credential_set`, has an egress namespace `{name}-egress` with a proxy Deployment, and needs `egress.enabled: true`. The release whose `namespaces` holds the entry (the fence release in the two-release install) renders the egress namespace, the Deployment and the two ConfigMaps of the CA certificate. It renders no Secret. The platform creates the Secrets below in the egress namespace before the proxy pod starts, with the names that its values give and with these keys (read from the templates and tested offline, `check_the_install_lists_the_objects_of_a_proxied_mode`):

| Object | Name | Keys | Who makes it |
|---|---|---|---|
| Secret, the reload key | `egress.proxy.reload.key_secret` | `token` | the platform |
| Secret, the proxy CA key | `namespaces[].proxy.ca.key_secret` | `ca.key` | the platform |
| Secret, the credentials (only with a `credential_set`) | `namespaces[].credential_set.secret_ref` | one key for each `credential_set.secrets[].key`, mounted as a file in the proxy pod | the platform |
| ConfigMap, the CA certificate for the proxy | `br-sandbox-proxy-ca` in the egress namespace | `ca.crt` | the release that holds the entry, from `namespaces[].proxy.ca.certificate` |
| ConfigMap, the CA certificate for the sandbox pods | `br-sandbox-ca` in the sandbox namespace | `ca.crt` | the release that holds the entry, from `namespaces[].proxy.ca.certificate` |
| ConfigMap, the roots for upstream servers (optional) | `egress.proxy.upstream_ca.config_map` | `egress.proxy.upstream_ca.key`, default `ca.crt` | the platform |

The pull secrets of `pull_secrets` must exist in the egress namespace too. Whether the proxy pod starts with these objects on a cluster is not tested here.

## Network check of a sandbox namespace

In the two-release install, chart 0.4.0 runs no network check of a sandbox namespace. In that shape nothing of the chart proves on the cluster that the network plugin enforces the policies of a sandbox namespace. The fence check of the keeper proves the names and the shape, not the enforcement.

The check probes the `namespaces` of the release that holds `network_check.enabled`, and it needs `operator.enabled`. The operator release above has `namespaces: []` (the fixture `ci/platform-values.yaml` sets it; the release of an owner that lists namespaces holds them), and the fence release cannot turn the check on (`BR_SANDBOX_OPERATOR_PART_REQUIRED{key=network_check}`). A Job with an empty `namespaces` list exits 0 and proves nothing: with the fixture values (`network_check.enabled: true`) the operator install above renders the Job `br-sandbox-netcheck-initial`, which probes no namespace and completes, so a completed `br-sandbox-netcheck-initial` is not evidence for any sandbox namespace. One route needs no change of the chart: one dedicated probe namespace in the `namespaces` of the operator release, in four steps. A Job that ran is not run again, so the check is a new Job each time (a new `network_check.run_id`). The probe namespace is used only for the proof. It carries the fence part from the operator release, so it must not be in the `namespaces` of the fence release: no object of the fence release has the kind, namespace and name of an object of the route, so Helm owns no object twice (tested offline, `check_the_netcheck_route_renders_the_check_of_one_probe_namespace`). The name is a scope of the table `scopes`, a dash and `netcheck`: it follows the name rule of the real sandbox namespaces, so that the rules of a cluster that act by the name of a namespace act on the probe namespace as on a real one. A name whose prefix is not a scope of the table `scopes` of the install values fails the render with `BR_SANDBOX_NO_SCOPE{namespace=...}` (tested offline).

The commands of the four steps use these placeholders. Each is written in angle brackets, and the owner replaces each one:

- `<chart>`: the path of the chart folder, or the chart reference, that the owner gave to the install commands above.
- `<release>`: the name of the operator release (`br-sandbox` in the install above).
- `<system-namespace>`: the system namespace of the release: `{prefix}-system` (`installation.namespace_prefix`), or `installation.system_namespace` when it is set (`sbx-system` with the fixture values). The Job of the check and its pod are in it, and not in the probe namespace (step 3).
- `<install-values>`: a values file of the operator release, the same files that the owner gave to the install of the operator part. Write one `-f` line for each file. The files of `ci/` are the fixtures of the CI render, not the values of a real cluster. If the install values set `network.internal_cidrs`, the list must hold `169.254.0.0/16` or an entry that covers it (a well-formed IPv4 CIDR), else each `helm` command of the route stops at the render with `BR_SANDBOX_LINK_LOCAL_MISSING`.
- `<scope>`: a key of the table `scopes` of the install values. The probe namespace is `<scope>-netcheck`.
- `<route-values>` and `<route-run-values>`: files of the owner, copies of `ci/netcheck-route-values.yaml` and of `ci/netcheck-route-run-values.yaml` (steps 1 and 3).
- `<host>:<port>`: the target that the owner chooses (step 3).
- `<image>` and `<size>`: the image and the size of the pool of the probe namespace (step 2).

**Step 1: the probe namespace, with the check off.** Copy `ci/netcheck-route-values.yaml` to a file of the owner, `<route-values>`, and replace the namespace name in the copy: `sbx-alpha-netcheck` becomes `<scope>-netcheck`. The file holds `namespaces: [{name: sbx-alpha-netcheck, mode: none}]`, `render_namespace: true` (the release renders the Namespace with its managed labels) and `network_check.enabled: false`, so the release renders no Job and no Job fails before a pool exists. Before the upgrade, read the next paragraph: if the install values list namespaces, it removes the `namespaces` key from the copy.

**One list of namespaces, not two.** In Helm a later `-f` file replaces a list of an earlier file; it does not merge the two. The `namespaces` list of `<route-values>` therefore replaces the `namespaces` list of `<install-values>` (tested offline). If the install values list no namespace, use the copy as it is. If they list namespaces, delete the `namespaces` key from `<route-values>` and append the entry `{name: <scope>-netcheck, mode: none}` to the `namespaces` list of the install values, the file that every step uses: a second list would remove the other namespaces from the release. `render_namespace` is one value of the whole release: with `true` the release renders the Namespace of every entry of `namespaces`, not only that of the probe namespace; what this does to a Namespace that a project made is not tested here. In steps 3 and 4 the install values then hold the probe entry (step 3) or no longer hold it (step 4).

**Upgrade of step 1.** Then upgrade the operator release with the install values and the copy, the copy being as the paragraph above leaves it. If the install values rendered the Job `br-sandbox-netcheck-initial` (the fixture values do), this upgrade also removes it.

```
helm upgrade <release> <chart> \
  -f <install-values> \
  -f <route-values> \
  --set crds.install=true
```

**Step 2: one `SandboxPool` in the probe namespace.** The probe pod is a pod of the managed namespace, so the pod policy judges it against a `SandboxPool` of that namespace. Without a pool the first probe pod is refused for that reason only; a reader sees `The pods "br-sandbox-netcheck-probe" is invalid: ValidatingAdmissionPolicy 'br-sandbox-pod' with binding 'br-sandbox-pod' denied request: failed to configure binding: no params found for policy binding with Deny parameterNotFoundAction` (reported by the operator of a test cluster; no record in this repository; not proved here). Replace `<scope>`, `<image>` and `<size>`: `<size>` is the `size` mapping of the pool of the project side above (`cpu`, `memory` and `disk`, each with `request` and `limit`). Save the object below as the file `pool.yaml`, with the three placeholders replaced, and create the pool with the command under it, after step 1 and before step 3. The pool has `minReplicas: 0`, `maxReplicas: 1` and `margin: 0`. With `minReplicas: 0` and `margin: 0` the keeper computes no warm pod (`desired = clamp(max(ceil(rate x refill) + margin, floor), minReplicas, maxReplicas)`, read from the README of keeper 0.2.0: the formula gives 0 for a rate of 0); the default margin is 2, and with it a pool of `minReplicas: 0` and `maxReplicas: 1` gets one warm pod (reported by the operator of a test cluster; no record in this repository; not proved here), which counts against the quota of the probe namespace and starts an agent pod there. The probe namespace holds one `SandboxPool` at 0 replicas for the time of the check.

```yaml
apiVersion: sandbox.botresources.ai/v1alpha1
kind: SandboxPool
metadata:
  name: pool
  namespace: <scope>-netcheck
spec:
  image: <image>
  size: <size>
  minReplicas: 0
  maxReplicas: 1
  margin: 0
```

```
kubectl apply -f pool.yaml
```

**Step 3: the check, as a new Job.** Copy `ci/netcheck-route-run-values.yaml` to a file of the owner, `<route-run-values>`, and replace the target `192.0.2.10:5432` in the copy by `<host>:<port>`. The file holds `network_check.enabled: true`, a new `network_check.run_id` (here `route-1`, which gives the Job `br-sandbox-netcheck-route-1`; the Job `br-sandbox-netcheck-initial` of the first install is another Job) and one entry of `network_check.targets`. The class `targets` of the Job needs at least one entry. A target is a `host:port` that the Job pod reaches (the control: `nc -w 2` from the Job pod must succeed, else the probe object carries `control_unavailable` in its `control` field and the Job fails) and that the probe pod of the sandbox namespace must not reach (the probe must read `refused`). Choose such an address, for example the ClusterIP and the port of the store. With `targets: []` (the default) the chart renders the check and the Job fails, because the script reports the class `targets` as missing: it prints `{"net":"cluster","mode":"all","target":"targets","kind":"target_class","expected":"present","observed":"missing","control":"ok"}` and exits 1 (read from `files/network-check.sh`; the Job failing at this point is reported by the operator of a test cluster, no record in this repository, not proved here). The chart does not refuse `network_check.enabled: true` with empty `targets` at render time: only the script fails. While the check is on the release also renders the ServiceAccount, the ClusterRole and the Role of the check, the ConfigMap `br-sandbox-netcheck` of the system namespace and of the probe namespace, and a third NetworkPolicy in the system namespace, `br-sandbox-egress-netcheck`, besides `br-sandbox-default-deny` and `br-sandbox-egress-keeper`. Upgrade again with the install values and the two copies.

```
helm upgrade <release> <chart> \
  -f <install-values> \
  -f <route-values> \
  -f <route-run-values> \
  --set crds.install=true
```

The release then renders the Job `br-sandbox-netcheck-route-1`, the ConfigMap, the Role and the RoleBinding `br-sandbox-netcheck` of the probe namespace, and the check probes the probe namespace and every other entry of `namespaces`, if the install values list some (not tested here). The Job and its pod are in the system namespace, not in the probe namespace. Wait for the Job and read its report; the value of `--timeout` is the time that the owner accepts to wait, and a Job that fails never becomes `complete`, so a timeout is not a verdict: read the logs and the status of the Job.

```
kubectl wait --for=condition=complete job/br-sandbox-netcheck-route-1 -n <system-namespace> --timeout=10m
kubectl logs job/br-sandbox-netcheck-route-1 -n <system-namespace>
```

The log can hold lines that start with `Warning:` (for example about Pod Security) that `kubectl` prints; they are not result lines. The result lines are the lines that the script prints, each one a JSON object that starts with `{"net":`.

To run the check again, use another `run_id` (`route-2`, and the Job name follows it).

**Step 4: removal.** Delete the `SandboxPool` first, then upgrade the operator release with the install values only: no route copy and no `run_id`, and without the probe entry if step 1 appended it to the install values. The upgrade removes the probe namespace and the objects that the release rendered in it. The Job `br-sandbox-netcheck-route-1` is in the system namespace, not in the probe namespace; the upgrade removes it too, because the install values do not render it. With the fixture values it renders the Job `br-sandbox-netcheck-initial` again, which probes no namespace.

```
kubectl delete sandboxpool pool -n <scope>-netcheck
helm upgrade <release> <chart> \
  -f <install-values> \
  --set crds.install=true
```

When this route has been run on a cluster and the Job reports no mismatch, it proves on that cluster that the network plugin enforces the default-deny policy of the chart, and that no rule of the cluster opens a namespace with this name. This repository records no such run at this version. A probe reads `refused` for any non-zero exit of `nc` (reset, drop, a name that does not resolve), and the targets are the classes that the script discovers plus `network_check.targets`, so the statement holds for the targets of the Job and no wider. It does not probe a namespace of a project that `namespaces` does not list, or a fence that the stamper made. For an entry of `namespaces` in the mode `allow` or `full` (the install values may list some), the Job proves the refusal of the targets that it probes there; for an entry in mode `allow` when `egress.enabled` is `true`, it also proves the refusal of a `CONNECT` through the proxy to each target and to the off-list host. It does not prove the allowed traffic of the modes `allow` and `full`, and the checks of that traffic on a real cluster are not this Job. The offline tests render each step and lint it, with the fixtures as the values: `<install-values>` are `ci/example-values.yaml` and `ci/platform-values.yaml`, `<route-values>` and `<route-run-values>` are the two files of `ci/` as they are, `<scope>` is `sbx-alpha`, and `<image>` and `<size>` are those of the pool of the project side above; no step was run on a cluster by the tests of this repository at this version.
