# Install br-sandbox-chart 0.4.0

Status words: tested offline (the test is named, all in `test-sandbox-chart-operator-docs.sh` unless another file is named), read from a page or a file (named), not tested on a cluster. **No command in this file was run on a cluster by the tests of this repository.** The offline suite renders the same values files with `helm template` and lints them with `helm lint --strict` (the lint loop of `ci.yml`); that proves the render, and nothing about an API server.

0.4.0 is a breaking minor with no upgrade path from 0.3.x: remove a 0.3.x release before the install. Chart 0.4.x needs keeper 0.2.x (see Compatibility in `README.md`). The chart has two parts. The operator part is installed once per cluster. The fence part is applied to each namespace of the list `namespaces`, by the name and the mode of the namespace. A project then declares its own namespace and its `SandboxPool`.

## Values files

The commands below run from a checkout of this repository, because the folder `ci/` is not in the packaged chart (`.helmignore`). The fixtures give the shape of the platform values: `ci/example-values.yaml` (all the constants of the platform: installation prefix, tools image, store, network addresses, limits, scopes), `ci/platform-values.yaml` (the list `namespaces` is empty) and `ci/fence-values.yaml` (the operator part and the network check are off). A real platform copies `ci/example-values.yaml` and replaces every value (the fixture holds `registry.example.test` hosts, a placeholder keeper image and the addresses of a test network). `operator.image` is the image of the keeper 0.2.x; the value in the fixture is a placeholder. The owner gives `network.api_server_addresses` and `network.store_cidrs` of the real cluster. The two releases must be given the same platform constants, `grants.writers` included.

## Operator part, once per cluster

```
helm install br-sandbox charts/br-sandbox-chart \
  -f charts/br-sandbox-chart/ci/example-values.yaml \
  -f charts/br-sandbox-chart/ci/platform-values.yaml \
  --set crds.install=true
```

This renders the system namespace, the keeper, the ClusterRoles `br-sandbox-controller` and `br-sandbox-runner`, the three policies `br-sandbox-pod`, `br-sandbox-namespace` and `br-sandbox-fence` with their bindings (and `br-sandbox-pod-update`), the ConfigMap `br-sandbox-grants` and the definition of `SandboxPool`, and no sandbox namespace. The definition has the annotation `helm.sh/resource-policy: keep`, so `helm uninstall` leaves it (read from the Helm documentation, not tested on a cluster). Only one release sets `crds.install`; the chart cannot see another release. The render of this command is tested offline (`check_the_install_commands_render`, `test-sandbox-chart-operator-parts.sh` for what it holds); the order of the objects inside the release is the order of Helm and is not tested. The Kyverno exclusions of the README (Needs of the cluster) must exist before any managed namespace exists.

The list `grants.writers` (default `[]`) is rendered by this part into the ConfigMap `br-sandbox-grants`, which the fence part does not render. The fixture `ci/platform-values.yaml` lists `system:serviceaccount:gitops:platform`.

## Fence part, for the namespaces of the platform

```
helm install br-sandbox-fence charts/br-sandbox-chart \
  -f charts/br-sandbox-chart/ci/example-values.yaml \
  -f charts/br-sandbox-chart/ci/fence-values.yaml
```

This renders, for each entry of `namespaces`, the objects of the fence (README, What a render holds) and no cluster-scoped object other than Namespace objects (`check_the_install_commands_render`). With `render_namespace: false` (the default) the chart renders no managed Namespace: the fence is applied to a namespace that the project made. The fixture `ci/example-values.yaml` sets `render_namespace: true`, so the fence render of the command above holds the managed Namespace of each entry. The entry gives the name and the mode; the `client_id` and the runner identity come from the table `scopes`. A namespace whose name matches no scope fails the render (`BR_SANDBOX_NO_SCOPE`). With the stamper (`stamper.enabled`, off by default, see `STAMPER.md`) the same objects are generated when the namespace is made, and this release is not needed for the modes `none` and `full`; the stamper is not proved on a cluster. The modes `allow` and `full` with a credential set need inputs of the platform that are not decided (`LIMITS.md`).

The identity that applies the fence part (in the command above, `system:serviceaccount:gitops:platform`) must be listed in `grants.writers` of the operator part. The policy `br-sandbox-fence` refuses every write of a NetworkPolicy, ResourceQuota, LimitRange, Role or RoleBinding of a managed namespace, of the ServiceAccount `sandbox` and of the ConfigMaps `br-sandbox-*`, whatever the name of the object, unless the user is in `grants.writers`, and a validating policy is not bypassed by cluster-admin. With the default `[]` the install of the fence part is refused, object by object, with `BR_SANDBOX_FENCE`. The fixture `ci/fence-values.yaml` lists the same identity as `ci/platform-values.yaml`, because the two releases take the same platform constants. That an API server refuses the install in this way is not tested on a cluster; the offline test applies the policy model to the objects of the fence render.

## Project side

The project declares two objects. Mode `none` is the only mode that a project gets without a grant of the platform (`BR_SANDBOX_MODE_NOT_GRANTED`, `POLICIES.md`). The name must be a scope of the platform or start with a scope and a dash (`BR_SANDBOX_SCOPE`); in these examples the scope is `sbx-alpha`. These two manifests were not applied to a cluster. The offline suite checks that the policy model admits the Namespace, that the `SandboxPool` holds only keys of the definition, and that the pod that the keeper makes from the template with this pool is admitted.

```yaml
apiVersion: v1
kind: Namespace
metadata:
  name: sbx-alpha-none
  labels:
    br.sandbox/managed: "true"
    br.sandbox/mode: none
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

The only required field of the definition is `image`. The size of a container is bound by `size` and by the maximum `limits.size` of the platform; a namespace with no `SandboxPool` admits no pod. The keeper makes the ReplicaSet `warm-sbx-alpha-none` from this resource and from the ConfigMap `br-sandbox-pod-template`, once the fence is there. That the keeper does so is a fact of keeper 0.2, not tested in this repository.

## Order

The order that the design takes is: the definition and the operator part (one release), then the stamper or the fence part, then the namespace and the resource of the project. Other orders are inferred to fail closed and are not tested: a resource before the definition has no kind to match; a keeper before the definition is not ready; a namespace before its fence has no pool (`NamespaceNotFenced`). Keeper 0.2 refuses the document of chart 0.3.x at start because it holds `pools` (read from the README of keeper 0.2.0); this chart states nothing about keeper 0.1 with the document of 0.4.0.
