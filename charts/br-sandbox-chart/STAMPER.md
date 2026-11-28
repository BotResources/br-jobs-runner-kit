# Stamper of br-sandbox-chart

Status words: read from a file or a page (the file or the page is named), tested offline (the test is named, all in `test-sandbox-chart-stamper.sh`), inferred (a reading, no file, no page and no test shows it), not tested on a cluster.

## What it is

The stamper is one Kyverno `ClusterPolicy`, `br-sandbox-stamper`, of `kyverno.io/v1`. For a Namespace that carries the labels `br.sandbox/managed=true` and `br.sandbox/mode=none` or `full`, and whose name is a scope of the platform table `scopes` or starts with the scope and a dash, it generates the namespaced objects of the fence part: ServiceAccount `sandbox`, ResourceQuota `br-sandbox-quota`, LimitRange `br-sandbox-limits`, NetworkPolicy `br-sandbox-default-deny` (and `br-sandbox-egress-full` for mode `full`), ConfigMaps `br-sandbox-config` and `br-sandbox-pod-template`, RoleBindings `br-sandbox-runner` and `br-sandbox-keeper` (`check_the_stamper_has_one_rule_for_each_scope_mode_and_object`). The objects come from the same named templates as the fence part (`_fence.tpl`), and the policy text repeats no object text (`check_the_generated_objects_equal_the_fence_part`).

It takes three inputs only: the name of the Namespace, its mode label and the constants of the platform (the values and the table `scopes`). `client_id` and the runner subject come from the table by the name, never from a label (`check_client_and_runner_come_from_the_scope_table`). The only variable of the policy is the name of the Namespace, and no rule reads a label or another field of the request (`check_the_stamper_renders_one_cluster_object_and_no_namespace_input`). The policy is the same text with no namespace in `namespaces`, with three and with five (same test), so the number of cluster-scoped objects does not depend on the number of namespaces.

## Off by default; what the tests prove and what they do not

`stamper.enabled` is `false` and a render with the default renders no `ClusterPolicy` (`check_the_stamper_is_off_by_default`). The text of the policy is tested offline: for a namespace of each scope and mode, the objects that the rules generate, with the name put in the variable, equal the objects that the fence part renders for the same entry, and the comparison sees a changed, a missing and a wrongly chosen object (`check_the_generated_objects_equal_the_fence_part`). The text is valid YAML and `helm lint --strict` passes on `ci/stamper-values.yaml`. The text was not run through the Kyverno command line and was not applied to a cluster: **no cluster run of the stamper is recorded in this repository**. The project side is proved only once a cluster run of the install in `INSTALL.md` has passed; none is recorded at this version. The automatic stamping passed once on a test cluster with the policy engine (reported by the operator of that cluster; no record in this repository), for one namespace in mode none. Not proved: the platform cluster and the rights of the policy engine there, a second project at the same time, mode full, a namespace already fenced by the fence release, a policy removal while stamped namespaces exist.

Inferred and not tested on a cluster:
- that Kyverno replaces the variable inside a string of a ConfigMap (the adapter document and the pod template carry the name in JSON text). The Kyverno page on generate rules (policy types, cluster policy, generate; documentation of release 1.18) says that variable templating is supported for all fields in `data`; it does not show a JSON string inside a ConfigMap. The operator of a test cluster reports (no record in this repository; not proved here) that the stamped ConfigMaps equalled those of the fence part, except `client_id`;
- that `names` and `selector` in one `resources` block are combined with AND (no page and no test read here shows it);
- that one rule generates one object: generate policies are commonly written with one object for each rule (inferred, no test shows it), so the chart renders one rule for each scope, mode and object and does not use `foreach`. The Kyverno page says that for `foreach` a change of the target of a rule is not synchronized; the chart avoids it;
- that a rule name of 63 characters at most is accepted (the chart fails the render beyond it with `BR_SANDBOX_STAMPER_RULE_NAME_TOO_LONG`, `check_the_stamper_refuses_what_it_cannot_stamp`);
- that a new scope adds rules to the existing policy without a delete and recreate. the owner of the platform reports (no record in this repository; not proved here) that a change of the `match` or `exclude` of an existing generate rule is rejected at admission (reported, not read from a page here); adding a rule is not read and not tested;
- which value wins for the label `app.kubernetes.io/managed-by` on a generated object: the chart gives the same labels as the fence part (`Helm`) so that the two forms are equal; Kyverno adds its own labels;
- that `synchronize: true` reverts an edit of a generated object and that a policy delete removes the objects (the owner of the platform reports (no record in this repository; not proved here) it for the cluster default network policy: a delete cascades into the generated objects; reported, not tested here). For the stamper this means that a delete and recreate of the policy removes `br-sandbox-default-deny` from every sandbox namespace for the time of the gap, while pods run. The chart does not set `orphanDownstreamOnPolicyDelete`, a field that the Kyverno page shows; the choice is open;
- that a window exists between the creation of a Namespace and its fence (Kyverno acts after the admission). In it a project identity can create pods; the pod policy still judges them, and the keeper reports `NamespaceNotFenced` and makes no pod while the fence is absent.

## What the stamper does not stamp

- Mode `allow` and a `full` entry with a credential set: `stamper.enabled` with such an entry in `namespaces` is a render fault, `BR_SANDBOX_MODE_NOT_STAMPED{namespace,mode}`. No rule matches the mode `allow` (`check_the_stamper_has_one_rule_for_each_scope_mode_and_object`, `check_the_stamper_refuses_what_it_cannot_stamp`). Where the inputs of the proxied modes come from is not decided.
- The Role and RoleBinding `br-sandbox-netcheck` of the network check: the stamper makes the objects of the fence part of the list above only, so the network check does not cover a stamped namespace (`check_the_generated_objects_equal_the_fence_part` compares the list above and ignores this pair).
- The ClusterRoles `br-sandbox-controller` and `br-sandbox-runner`: the stamped RoleBindings name them, and the operator part renders them, so `stamper.enabled` needs `operator.enabled` (`BR_SANDBOX_OPERATOR_PART_REQUIRED{key=stamper}`). The table `scopes` must not be empty (`BR_SANDBOX_STAMPER_NO_SCOPES`).

## The stamper is a writer of the fence

The policy `br-sandbox-fence` refuses every write of a fence object unless the identity is in `grants.writers`. The objects are written by the background controller of Kyverno, so `stamper.identity` (default `system:serviceaccount:kyverno:kyverno-background-controller`, the name that the platform owner gave for the background controller of the policy engine) must be in `grants.writers`; the chart does not add it: a render with `stamper.enabled` and the identity absent fails with `BR_SANDBOX_STAMPER_NOT_A_WRITER{identity}` and the policy model admits that identity and refuses a project identity (`check_the_identity_of_the_stamper_is_a_writer_of_the_fence`). That Kyverno writes as this identity on a cluster is reported by the platform owner and is not tested.

## Conditions for use: items of the package, for the owner of the platform

These are facts about the cluster that the platform owner reported (this repository does not hold them and no test checks them) and conditions that follow. The chart changes nothing on the cluster. Each one is a decision of the owner of the platform.

1. **The cluster default network policy.** A `ClusterPolicy` of the cluster generates a NetworkPolicy named `default-deny` in every Namespace that is not in its `exclude` list of names. That policy allows egress to the Internet except the three private ranges, DNS, the same namespace and the API server, and ingress from the same namespace, the ingress controller and three platform namespaces. NetworkPolicies add up (a documented behaviour of Kubernetes), so a sandbox namespace of mode `none` that is not excluded would have Internet egress, whatever `br-sandbox-default-deny` says. The chart names its policy with the prefix `br-sandbox-`, so the two objects are distinct. The owner of the platform reports (no record in this repository; not proved here) that the `exclude` list is frozen after creation: a change is rejected at admission, and the way is a delete and recreate of the policy, with a gap of 5 to 15 seconds in which every namespace that the policy covers has no `default-deny`. Condition: the sandbox namespaces must be excluded before any managed namespace exists on the cluster. Whether an exclusion by label is accepted in that rule is not known; the owner reports an exclusion by name.
2. **The scope labelling policy.** The mutate rules of a policy of the cluster label each namespace whose name is `<product>-<env>` or starts with it and a dash with a scope label, and a generate rule of the same policy makes a NetworkPolicy in every Namespace that carries the label, open to the peers of the same scope. A sandbox namespace under such a name would be labelled and would be open to the services of its product. Condition: these rules must exclude the Namespaces with `br.sandbox/managed=true`. The owner of the platform reports (no record in this repository; not proved here) that editing `names` on a mutate rule is not subject to the frozen-rule limit of generate rules; that an `exclude` can be added to a mutate rule without a delete and recreate is inferred.
3. **Rights of Kyverno.** Needed, not inferred, on one cluster: the operator of a test cluster reports (Kyverno chart 3.8.0, application 1.18.0; no record in this repository; not proved here) that with the default rights `helm upgrade` of the operator release with `stamper.enabled` failed with the message of the webhook of the policy engine, `validate-policy.kyverno.svc denied ... kyverno-background-controller requires permissions create,update,delete for resource v1/ServiceAccount in namespace {{ request.object.metadata.name }}`, and that the same check holds for ResourceQuotas, LimitRanges, ConfigMaps and RoleBindings, and for the right to bind the two ClusterRoles of the sandbox (the Kyverno page on generate rules says that to generate a RoleBinding the controller must be bound to the same role first). The chart does not render this ClusterRole: a right of the policy engine is a grant of the owner of the platform. Order: apply the rights first, then turn the stamper on. `helm upgrade --dry-run=server` passed in that run and did not show the refusal, so it is no check of the rights, and the failed upgrade left a failed revision in the history. To leave it, apply the rights and run `helm upgrade` again with the same values, which makes a new revision; `helm history RELEASE` lists the revisions with their status and `helm rollback RELEASE REVISION` returns to an earlier one (read from the Helm documentation; not tested). What worked on that cluster, as a generic example for the owner to apply (the name is a placeholder; the label makes Kyverno aggregate the role into the background controller). The stamper also generates NetworkPolicies (`br-sandbox-default-deny`, and `br-sandbox-egress-full` for the mode `full`). In the test, the policy engine at version 1.18.0 (chart 3.8.0) stamped the NetworkPolicy with its default rights; the added ClusterRole does not hold that kind. Another version or another install of the policy engine can need it. The example ends with a rule on `networkpolicies` for that reason only: it is not in the ClusterRole of the test, and it was not run on a cluster:

```yaml
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRole
metadata:
  name: sandbox-stamper-rights
  labels:
    rbac.kyverno.io/aggregate-to-background-controller: "true"
rules:
  - apiGroups: [""]
    resources: [serviceaccounts, resourcequotas, limitranges, configmaps]
    verbs: [get, list, watch, create, update, patch, delete]
  - apiGroups: ["rbac.authorization.k8s.io"]
    resources: [rolebindings]
    verbs: [get, list, watch, create, update, patch, delete]
  - apiGroups: ["rbac.authorization.k8s.io"]
    resources: [clusterroles]
    verbs: [bind]
    resourceNames: [br-sandbox-controller, br-sandbox-runner]
  - apiGroups: ["networking.k8s.io"]
    resources: [networkpolicies]
    verbs: [get, list, watch, create, update, patch, delete]
```

The runner ClusterRole holds `pods/exec` (`rbac.yaml` of this chart). Trust item for the owner of the platform. With these rights, a person who takes control of the background controller of the policy engine can write service accounts, quotas, limit ranges, ConfigMaps, RoleBindings and NetworkPolicies in every namespace, and can bind the two ClusterRoles of the sandbox there. These rights are not limited to the sandbox namespaces by RBAC (inferred), so that person could also start a command in a pod of any namespace through the bound runner role. The owner accepts this or asks for another stamper. The rights table of the keeper is not changed by it.
4. **The install** of the operator part and of the policy as a platform tool, and the Kyverno form: the Kyverno documentation marks `ClusterPolicy` as deprecated and the newer `GeneratingPolicy` as stable in 1.18, with no removal date (read from the pages). The chart uses the `ClusterPolicy` form because the owner of the platform reports (no record in this repository; not proved here) that the policies of the cluster use it; the newer form is a later change.

## One maker for one namespace

One namespace has one maker of its fence: a fence release, or the stamper. The stamper acts on every scope of the scope table, and also on namespaces that exist. Do not turn the stamper on while a fence release fences a namespace of a scope of the table. The effect is not tested: the stamper can take the objects, overwrite them, or delete them when it is turned off.

The policy has a rule for every key of `scopes`; the rules match a Namespace by the name `<scope>` or `<scope>-*` and by the labels, with `generateExisting` and `synchronize` set to `true`. A scope that a fence release already fences is then targeted by the stamper, with the object names that the fence release owns: two makers of one object name. Use a scope table for the stamper whose scopes have no namespace fenced by the fence release. Before turning the stamper on, list the managed namespaces and compare them with the namespaces of the fence releases:

```
kubectl get namespaces -l br.sandbox/managed=true
```

Not tested: to turn the stamper off, or to remove the policy engine, while stamped namespaces exist. The policy engine can then delete the fence objects that it made; a namespace with no fence gets no pool. Remove the pools first.

A project identity can add the label `br.sandbox/mode` alone on a namespace that is not managed: the namespace policy accepts it, and it has no fence, no pool and no effect (the stamper needs both labels; reported by the operator of a test cluster, no record in this repository, not proved here).
