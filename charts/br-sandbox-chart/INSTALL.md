# Install br-sandbox-chart 0.4.0

Status words: tested offline (the test is named, all in `test-sandbox-chart-operator-docs.sh` unless another file is named), read from a page or a file (named), not tested on a cluster. **No command in this file was run on a cluster by the tests of this repository.** The offline suite renders the same values files with `helm template` and lints them with `helm lint --strict` (the lint loop of `ci.yml`); that proves the render, and nothing about an API server.

0.4.0 is a breaking minor with no upgrade path from 0.3.x: remove a 0.3.x release before the install. Chart 0.4.x needs keeper 0.2.x (see Compatibility in `README.md`). The chart has two parts. The operator part is installed once per cluster. The fence part is applied to each namespace of the list `namespaces`, by the name and the mode of the namespace. A project then declares its own namespace and its `SandboxPool`.

## Values files

The commands below run from a checkout of this repository, because the folder `ci/` is not in the packaged chart (`.helmignore`). The fixtures give the shape of the platform values: `ci/example-values.yaml` (all the constants of the platform: installation prefix, tools image, store, network addresses, limits, scopes), `ci/platform-values.yaml` (the list `namespaces` is empty) and `ci/fence-values.yaml` (the operator part and the network check are off). A real platform copies `ci/example-values.yaml` and replaces every value (the fixture holds `registry.example.test` hosts and the addresses of a test network). `operator.image` is the image of the keeper 0.2.x; the fixture sets the image of keeper 0.2.0, `ghcr.io/botresources/br-sandbox-keeper@sha256:ce338934062a0ebf8d27ddf56b9361ec509036c80679d202623a301f23a0c244` (a manifest list for `linux/amd64` and `linux/arm64`, read from the release record of the keeper and not inspected by the offline suite), the same value as in the README. The owner gives `network.api_server_addresses` and `network.store_cidrs` of the real cluster. The two releases must be given the same platform constants, `grants.writers` included.

## Operator part, once per cluster

```
helm install br-sandbox charts/br-sandbox-chart \
  -f charts/br-sandbox-chart/ci/example-values.yaml \
  -f charts/br-sandbox-chart/ci/platform-values.yaml \
  --set crds.install=true
```

This renders the system namespace, the keeper, the ClusterRoles `br-sandbox-controller` and `br-sandbox-runner`, the three policies `br-sandbox-pod`, `br-sandbox-namespace` and `br-sandbox-fence` with their bindings (and `br-sandbox-pod-update`), the ConfigMap `br-sandbox-grants` and the definition of `SandboxPool`, and no sandbox namespace. The definition has the annotation `helm.sh/resource-policy: keep`, so `helm uninstall` leaves it (read from the Helm documentation, not tested on a cluster). If someone restores the binding `br-sandbox-pod` with `kubectl apply`, the next `helm upgrade` of the operator release fails on a server-side-apply conflict and needs `--force-conflicts` (observed once on a cluster; not tested by the offline suite). Only one release sets `crds.install`; the chart cannot see another release. The render of this command is tested offline (`check_the_install_commands_render`, `test-sandbox-chart-operator-parts.sh` for what it holds); the order of the objects inside the release is the order of Helm and is not tested. The Kyverno exclusions of the README (Needs of the cluster) must exist before any managed namespace exists. The network check of a sandbox namespace is not run by this install: see Network check of a sandbox namespace below.

The list `grants.writers` (default `[]`) is rendered by this part into the ConfigMap `br-sandbox-grants`, which the fence part does not render. The fixture `ci/platform-values.yaml` lists `system:serviceaccount:gitops:platform`.

## Fence part, for the namespaces of the platform

```
helm install br-sandbox-fence charts/br-sandbox-chart \
  -f charts/br-sandbox-chart/ci/example-values.yaml \
  -f charts/br-sandbox-chart/ci/fence-values.yaml
```

This renders, for each entry of `namespaces`, the objects of the fence (README, What a render holds) and no cluster-scoped object other than Namespace objects (`check_the_install_commands_render`). With `render_namespace: false` (the default) the chart renders no managed Namespace: the fence is applied to a namespace that the project made. The fixture `ci/example-values.yaml` sets `render_namespace: true`, so the fence render of the command above holds the managed Namespace of each entry. The entry gives the name and the mode; the `client_id` and the runner identity come from the table `scopes`. A namespace whose name matches no scope fails the render (`BR_SANDBOX_NO_SCOPE`). With the stamper (`stamper.enabled`, off by default, see `STAMPER.md`) the same objects are generated when the namespace is made, and this release is not needed for the modes `none` and `full`; the stamper is not proved on a cluster. The modes `allow` and `full` with a credential set need inputs of the platform that are not decided (`LIMITS.md`).

The identity that applies the fence part (in the command above, `system:serviceaccount:gitops:platform`) must be listed in `grants.writers` of the operator part. The policy `br-sandbox-fence` refuses every write of a NetworkPolicy, ResourceQuota, LimitRange, Role or RoleBinding of a managed namespace, of the ServiceAccount `sandbox` and of the ConfigMaps `br-sandbox-*`, whatever the name of the object, unless the user is in `grants.writers`, and a validating policy is not bypassed by cluster-admin. With the default `[]` the install of the fence part is refused, object by object, with `BR_SANDBOX_FENCE`. The fixture `ci/fence-values.yaml` lists the same identity as `ci/platform-values.yaml`, because the two releases take the same platform constants. That an API server refuses the install in this way is not tested on a cluster. In the one cluster run reported for this install, a project identity's create of a RoleBinding and its delete of a binding were refused by RBAC (it holds no such right), not by `BR_SANDBOX_FENCE` or another policy code; the refusal by the policy is not proved on a cluster; the offline test applies the policy model to the objects of the fence render.

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

The order that the design takes is: the definition and the operator part (one release), then the stamper or the fence part, then the namespace and the resource of the project. Other orders are inferred to fail closed and are not tested: a resource before the definition has no kind to match; a keeper before the definition is not ready; a namespace before its fence has no pool (`NamespaceNotFenced`). Keeper 0.2 refuses the document of chart 0.3.x at start because it holds `pools` (read from the README of keeper 0.2.0); this chart states nothing about keeper 0.1 with the document of 0.4.0. If a managed namespace exists before its fence part is installed, the keeper logs forbidden list and watch errors about once a second until the RoleBinding `br-sandbox-keeper` of the fence part arrives, and then it recovers: install the fence part right after the namespace is labelled, or in the same step (observed once on a cluster; not tested by the offline suite).

## Network check of a sandbox namespace

In the two-release install, chart 0.4.0 runs no network check of a sandbox namespace. In that shape nothing of the chart proves on the cluster that the network plugin enforces the policies of a sandbox namespace. The fence check of the keeper proves the names and the shape, not the enforcement.

The check probes the `namespaces` of the release that holds `network_check.enabled`, and it needs `operator.enabled`. The operator release above has `namespaces: []`, and the fence release cannot turn the check on (`BR_SANDBOX_OPERATOR_PART_REQUIRED{key=network_check}`). One route needs no change of the chart: upgrade the operator release with one dedicated probe namespace in its own `namespaces` list.

```
helm upgrade br-sandbox charts/br-sandbox-chart \
  -f charts/br-sandbox-chart/ci/example-values.yaml \
  -f charts/br-sandbox-chart/ci/platform-values.yaml \
  -f charts/br-sandbox-chart/ci/netcheck-route-values.yaml \
  --set crds.install=true
```

`ci/netcheck-route-values.yaml` holds three values: `namespaces: [{name: sbx-alpha-netcheck, mode: none}]`, `render_namespace: true` and `network_check.enabled: true`. The name is the name of a scope of the table `scopes` (`sbx-alpha` in the fixture), a dash and `netcheck`: it follows the name rule of the real sandbox namespaces, so that the rules of a cluster that act by the name of a namespace act on the probe namespace as on a real one. `render_namespace: true` makes the operator release render the Namespace object with its labels. The release then renders the Job `br-sandbox-netcheck-initial`, the ConfigMap, the Role and the RoleBinding `br-sandbox-netcheck` of the probe namespace, and the fence objects of that one namespace, and the check probes it. The probe namespace is a namespace used only for the proof. It carries the fence part from the operator release, so it must not be in the `namespaces` of the fence release: no object of the fence release has the namespace and the name of an object of this render, so Helm owns no object twice (tested offline, `check_the_netcheck_route_renders_the_check_of_one_probe_namespace`). The check needs a `SandboxPool` in the probe namespace (the manifest above, with the name of that namespace). This route probes one probe namespace in mode `none` that the operator release makes. It proves on this cluster that the network plugin enforces the default-deny policy of the chart, and that no rule of the cluster opens a namespace with this name. It does not probe a namespace of a project, the modes `allow` and `full`, or a fence that the stamper made. The checks of the modes `allow` and `full` on a real cluster are not this Job. To remove the route, run `helm upgrade` of the operator release again without the file. The offline tests render the route and lint it; the route was not run on a cluster by the tests of this repository at this version, and a cluster run is made by the platform test.
