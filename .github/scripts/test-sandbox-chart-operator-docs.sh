#!/usr/bin/env bash

changelog_040() {
    awk '/^## 0.4.0$/ {on=1; next} /^## / {on=0} on' "$CHART/CHANGELOG.md"
}

check_the_changelog_040_states_the_breaking_changes() {
    section "the CHANGELOG 0.4.0 states what breaks, and each sentence has its test"
    local text
    text="$(changelog_040)"
    t "the CHANGELOG has the heading 0.4.0 once, above 0.3.0" test "$(grep -cx '## 0.4.0' "$CHART/CHANGELOG.md")" -eq 1 -a "$(grep -nx '## 0.4.0' "$CHART/CHANGELOG.md" | cut -d: -f1)" -lt "$(grep -nx '## 0.3.0' "$CHART/CHANGELOG.md" | cut -d: -f1)"
    t "it says that the minor is breaking and that 0.4.x needs keeper 0.2.x and 0.3.x needs keeper 0.1.x" grep -qF 'Chart 0.4.x needs keeper 0.2.x, and chart 0.3.x needs keeper 0.1.x: keeper 0.2 refuses a document with `pools` at start.' <<<"$text"
    t "it says that there is no upgrade path from 0.3.x, that a 0.3.x release is removed first, and that an upgrade in place was not tested" grep -qF 'No upgrade path from 0.3.x: remove a 0.3.x release before the install. An upgrade in place was not tested.' <<<"$text"
    t "it says that keeper.* becomes operator.* and that clients becomes namespaces and scopes" grep -qF '`keeper.*` becomes `operator.*`' <<<"$text"
    t "it names the two faults of the removed roots" test "$(grep -cF 'BR_SANDBOX_KEEPER_VALUES_REMOVED{key=keeper}' <<<"$text")" -eq 1 -a "$(grep -cF 'BR_SANDBOX_CLIENTS_REMOVED{key=clients}' <<<"$text")" -eq 1
    fails_with "BR_SANDBOX_KEEPER_VALUES_REMOVED{key=keeper}" -f "$EXAMPLE" --set keeper.enabled=true
    fails_with "BR_SANDBOX_CLIENTS_REMOVED{key=clients}" -f "$EXAMPLE" --set clients.alpha.cpu=1
    t "it says that client_id and the runner identity come from the scope that matches the name, never from a label" grep -qF 'come from the scope that matches its name, never from a label.' <<<"$text"
    t "it says that the chart renders no ReplicaSet and that the keeper makes warm-{namespace}" grep -qF 'the keeper makes `warm-{namespace}` from the `SandboxPool` of the namespace' <<<"$text"
    t "no render of the suite holds a ReplicaSet" jqt ex '[.[] | select(.kind == "ReplicaSet")] | length == 0'
    t "it says that every NetworkPolicy of the chart carries the prefix br-sandbox-" grep -qF 'every NetworkPolicy of the chart is named with the prefix `br-sandbox-`' <<<"$text"
    t "every NetworkPolicy of the example render carries the prefix br-sandbox-" jqt ex '[.[] | select(.kind == "NetworkPolicy") | .metadata.name | select(startswith("br-sandbox-") | not)] | length == 0'
    t "it says that the controller and runner rights are two ClusterRoles bound in each namespace" grep -qF 'The rights of the controller and of the runner are the ClusterRoles `br-sandbox-controller` and `br-sandbox-runner`, bound in each namespace by a RoleBinding.' <<<"$text"
    t "the two ClusterRoles exist and each sandbox namespace holds the two RoleBindings" jqt ex '([.[] | select(.kind == "ClusterRole") | .metadata.name] | contains(["br-sandbox-controller", "br-sandbox-runner"])) and ([.[] | select(.kind == "RoleBinding" and .metadata.namespace == "sbx-alpha-none") | .metadata.name] | contains(["br-sandbox-keeper", "br-sandbox-runner"]))'
    t "it says that the network check is off by default and needs the operator part" grep -qF '`network_check.enabled` is `false` by default, and the network check needs `operator.enabled`.' <<<"$text"
    t "it lists the new objects" grep -qF 'the pod policy `br-sandbox-pod` with the bindings `br-sandbox-pod` and `br-sandbox-pod-update`; the namespace policy `br-sandbox-namespace`; the fence-object policy `br-sandbox-fence`; the ConfigMap `br-sandbox-grants`; the optional stamper (`stamper.enabled`, default `false`); in the system namespace the policies `br-sandbox-default-deny` and `br-sandbox-egress-keeper`.' <<<"$text"
    t "the new objects exist in the operator render" jqt ex '([.[] | select(.kind == "ValidatingAdmissionPolicy") | .metadata.name] | sort) == ["br-sandbox-fence", "br-sandbox-namespace", "br-sandbox-pod"] and ([.[] | select(.kind == "ValidatingAdmissionPolicyBinding") | .metadata.name] | sort) == ["br-sandbox-fence", "br-sandbox-namespace", "br-sandbox-pod", "br-sandbox-pod-update"] and ([.[] | select(.kind == "ConfigMap" and .metadata.name == "br-sandbox-grants")] | length == 1) and ([.[] | select(.kind == "ConfigMap" and .metadata.name == "br-sandbox-pod-template") | .data | keys] | all(. == ["pod-template.json"]))'
    t "the system namespace holds the two network policies" jqt ex '[.[] | select(.kind == "NetworkPolicy" and .metadata.namespace == "sbx-system") | .metadata.name] | sort == ["br-sandbox-default-deny", "br-sandbox-egress-keeper", "br-sandbox-egress-netcheck"]'
    t "the stamper and the definition are off by default" test "$(yq '.stamper.enabled' "$CHART/values.yaml")" = false -a "$(yq '.crds.install' "$CHART/values.yaml")" = false
    t "it says that there are 15 rules, names rules 14 and 15, and says that kubectl debug is refused" grep -qF 'Admission rules: 15 rules in all. Rule 14 (`BR_SANDBOX_SERVICE_ACCOUNT`) requires the ServiceAccount `sandbox`; rule 15 (`BR_SANDBOX_EPHEMERAL_CONTAINERS`) refuses ephemeral containers, so `kubectl debug` on a sandbox pod is refused.' <<<"$text"
    t "the pod policy holds 15 rules, with the codes of rules 14 and 15" jqt ex '[.[] | select(.kind == "ValidatingAdmissionPolicy" and .metadata.name == "br-sandbox-pod") | .spec.validations | length, ([.[].message | select(startswith("BR_SANDBOX_SERVICE_ACCOUNT:") or startswith("BR_SANDBOX_EPHEMERAL_CONTAINERS:"))] | length)] == [15, 2]'
    t "it says that agent_container is agent and required by the keeper, and that the agent pulls with Always, with its cost and its limit" grep -qF 'The agent container of the pod template has `imagePullPolicy: Always`: the cost is one registry call at each pod start, and the limit is that a start can fail when the registry does not answer (not tested on a cluster).' <<<"$text"
    t "the agent container of every template has imagePullPolicy Always and the name agent, in the keeper and the adapter documents too" test "$(jq '[.[] | select(.kind == "ConfigMap" and .metadata.name == "br-sandbox-pod-template") | .data["pod-template.json"] | fromjson | .spec.containers[0] | [.name, .imagePullPolicy]] | unique' "$WORK/ex.json" -c)" = '[["agent","Always"]]' -a "$(jq -r '[.[] | select(.kind == "ConfigMap" and (.metadata.name == "br-sandbox-keeper-config" or .metadata.name == "br-sandbox-config")) | .data | to_entries[0].value | fromjson | .agent_container] | unique | join(",")' "$WORK/ex.json")" = agent
    t "it says that registry_mirror does not apply to the image of a pool" grep -qF '`registry_mirror` does not apply to the image of a pool: the agent of a pod runs exactly the `image` of its `SandboxPool`, as written.' <<<"$text"
    t "the pod that the keeper makes with the image of the pool as written is admitted in the mirror render" test "$(created mirror sbx-alpha-none)" = admit
    t "it says what is not proved offline" grep -qF 'Not proved offline, and not tested on a cluster: that the CEL of the three policies compiles, that the API server takes the `SandboxPool` as the parameter of the pod policy, the grants and the fence-object control, the stamper, and the rights on the API server. The rows of the battery hold them.' <<<"$text"
    t "it names the test files, all of which exist" test "$(grep -oE 'test-sandbox-chart-[a-z-]+\.sh|`-[a-z-]+\.sh`' <<<"$text" | tr -d '`' | sed 's#^-#test-sandbox-chart-#' | sort -u | while read -r f; do test -f "$SCRIPTS/$f" || echo "$f"; done | wc -l | tr -d ' ')" = 0
    t "the CHANGELOG 0.3.0 and the earlier texts keep their heading" test "$(grep -c '^## 0\.[0-3]\.' "$CHART/CHANGELOG.md")" -ge 8
}

readme_and_siblings() {
    cat "$CHART/README.md" "$CHART/POLICIES.md" "$CHART/STAMPER.md" "$CHART/INSTALL.md" "$CHART/LIMITS.md"
}

install_block() {
    awk -v want="$1" '/^```/ { if (fenced) { fenced = 0; in_block = 0 } else { fenced = 1; n++; in_block = (n == want) } next } in_block' "$CHART/INSTALL.md"
}

install_command_files() {
    install_block "$1" | grep -oE 'charts/br-sandbox-chart/ci/[a-z-]+\.yaml' | sed 's#^charts/br-sandbox-chart/##'
}

check_the_readme_states_the_compatibility_and_the_rights() {
    section "README: compatibility of the chart and the keeper, rights of the keeper, mirror, links"
    local readme
    readme="$(cat "$CHART/README.md")"
    t "the README holds the row of chart 0.3.x with keeper 0.1.x" grep -qxF '| 0.3.x | 0.1.x |' "$CHART/README.md"
    t "the README holds the row of chart 0.4.x with keeper 0.2.x" grep -qxF '| 0.4.x | 0.2.x |' "$CHART/README.md"
    t "the README says that keeper 0.2 refuses a document with pools at start and that the pairs are read from the keeper README" grep -qF 'Keeper 0.2 refuses a document with `pools` at start, so it does not work with chart 0.3.x or earlier; chart 0.4.x renders no `pools` (the pools are `SandboxPool` resources). The pairs are read from the README of keeper 0.2.0.' <<<"$readme"
    t "the document of the keeper holds no pools in every render of the suite" jqt ex '[.[] | select(.kind == "ConfigMap" and .metadata.name == "br-sandbox-keeper-config") | .data["config.json"] | fromjson | has("pools")] == [false]'
    t "the README says that the join tests with the type of keeper 0.2 pass only once keeper 0.2.0 is on main" grep -qF 'which load the rendered document with the type of keeper 0.2 and pass only once keeper 0.2.0 is on `main`.' <<<"$readme"
    t "the two join test files exist" test -f "$ROOT/crates/br-sandbox-keeper/tests/chart_config.rs" -a -f "$ROOT/crates/br-sandbox-keeper/tests/chart_values.rs"
    t "the README says that 0.4.0 has no upgrade path and that an upgrade in place was not tested" grep -qF 'Chart 0.4.0 is a breaking minor with no upgrade path from 0.3.x: remove a 0.3.x release before the install; an upgrade in place was not tested.' <<<"$readme"
    t "the README says that the rights of a person who takes control of the keeper are the pods that the admission policy of the namespace accepts, with the network of the namespace" grep -qF 'A person who takes control of the keeper can start pods that the admission policy of the namespace accepts, up to the quota, in the managed namespaces. A pod that such a person starts has the network access of its namespace (in a namespace of mode `full`, the Internet). That person cannot change a fence object, a policy, a quota or a namespace, and cannot mount a token.' <<<"$readme"
    t "the keeper has no write on a fence kind, a policy, a quota or a namespace, and no secrets" test "$(jq -c '[.[] | select(.kind == "ClusterRole" and .metadata.name == "br-sandbox-controller") | .rules[] | select(any(.resources[]; IN("networkpolicies", "resourcequotas", "limitranges", "roles", "rolebindings", "serviceaccounts", "namespaces", "secrets", "validatingadmissionpolicies", "validatingadmissionpolicybindings", "sandboxpools")) and any(.verbs[]; IN("create", "update", "patch", "delete", "deletecollection", "*", "get", "list", "watch") and (. | IN("get", "list", "watch") | not)))] | length' "$WORK/ex.json")" = 0
    t "the pod policy refuses an agent that mounts a secret or a token" test "$(created ex sbx-alpha-full '.spec.automountServiceAccountToken = true')" = BR_SANDBOX_TOKEN_OR_RUNTIME
    t "mode full renders the policy br-sandbox-egress-full with an Internet rule" jqt ex '[.[] | select(.kind == "NetworkPolicy" and .metadata.namespace == "sbx-alpha-full" and .metadata.name == "br-sandbox-egress-full")] | length == 1'
    t "the README row of registry_mirror says that it does not apply to the image of a pool" grep -qF '| `registry_mirror` | `""` | Prefix of every image of the chart: `{mirror}/{reference}`. It does not apply to the image of a pool: the agent of a pod runs exactly the `image` of its `SandboxPool`, as written. |' <<<"$readme"
    t "the pod that carries the image of the pool as written is admitted in the mirror render" test "$(created mirror sbx-alpha-none)" = admit
    t "the README links INSTALL.md and LIMITS.md and says that no command was run on a cluster" grep -qF '`INSTALL.md` holds the install commands of the two parts (the operator part once per cluster, the fence part for the entries of `namespaces`) and the two manifests that a project declares (a Namespace and a `SandboxPool`); none was run on a cluster by the tests of this repository.' <<<"$readme"
    t "the README Tests section names the docs test file" grep -qF 'are checked by `test-sandbox-chart-operator-docs.sh`' "$CHART/README.md"
    t "the docs test file named in the README exists" test -f "$SCRIPTS/test-sandbox-chart-operator-docs.sh"
    t "the files named in the README exist" test -f "$CHART/INSTALL.md" -a -f "$CHART/LIMITS.md" -a -f "$CHART/POLICIES.md" -a -f "$CHART/STAMPER.md"
    t "no text file of the chart holds more than about 300 lines" test "$(wc -l "$CHART"/*.md | awk '$2 != "total" && $1 > 320' | wc -l | tr -d ' ')" = 0
}

check_the_install_commands_render() {
    section "INSTALL.md: the two commands name existing values files and render what the text says"
    local file
    t "the operator command is the block 1 and the fence command is the block 2" test "$(install_block 1 | grep -c '^helm install br-sandbox ')" = 1 -a "$(install_block 2 | grep -c '^helm install br-sandbox-fence ')" = 1
    for file in $(install_command_files 1) $(install_command_files 2); do
        t "the values file ${file} exists" test -f "$CHART/${file#ci/}" -o -f "$ROOT/charts/br-sandbox-chart/${file}"
    done
    t "the operator command names the example, the platform values and crds.install" test "$(install_command_files 1 | tr '\n' ' ')" = "ci/example-values.yaml ci/platform-values.yaml " -a "$(install_block 1 | grep -c -- '--set crds.install=true')" = 1
    t "the fence command names the example and the fence values" test "$(install_command_files 2 | tr '\n' ' ')" = "ci/example-values.yaml ci/fence-values.yaml "
    render install_operator -f "$CHART/ci/example-values.yaml" -f "$CHART/ci/platform-values.yaml" --set crds.install=true
    render install_fence -f "$CHART/ci/example-values.yaml" -f "$CHART/ci/fence-values.yaml"
    t "the operator command renders the definition, the keeper, the three policies with their bindings and the grants" jqt install_operator '([.[] | select(.kind == "CustomResourceDefinition")] | length == 1) and ([.[] | select(.kind == "Deployment" and .metadata.name == "br-sandbox-keeper")] | length == 1) and ([.[] | select(.kind == "ValidatingAdmissionPolicy") | .metadata.name] | sort == ["br-sandbox-fence", "br-sandbox-namespace", "br-sandbox-pod"]) and ([.[] | select(.kind == "ValidatingAdmissionPolicyBinding") | .metadata.name] | sort == ["br-sandbox-fence", "br-sandbox-namespace", "br-sandbox-pod", "br-sandbox-pod-update"]) and ([.[] | select(.kind == "ConfigMap" and .metadata.name == "br-sandbox-grants")] | length == 1)'
    t "the operator command renders no sandbox namespace and no object of a sandbox namespace" jqt install_operator '[.[] | select((.metadata.namespace // "sbx-system") != "sbx-system" or (.kind == "Namespace" and .metadata.name != "sbx-system"))] | length == 0'
    t "the definition of the operator command has the annotation helm.sh/resource-policy keep" jqt install_operator '[.[] | select(.kind == "CustomResourceDefinition") | .metadata.annotations["helm.sh/resource-policy"]] == ["keep"]'
    t "without crds.install the same command renders no definition" test "$(helm template t "$CHART" -f "$CHART/ci/example-values.yaml" -f "$CHART/ci/platform-values.yaml" | grep -c '^kind: CustomResourceDefinition')" = 0
    t "the fence command renders no cluster-scoped object other than the Namespace objects that the fixture asks for" jqt install_fence '[.[] | .kind] | unique - ["Namespace", "ConfigMap", "LimitRange", "NetworkPolicy", "ResourceQuota", "RoleBinding", "ServiceAccount"] | length == 0'
    t "the fence command renders the fence of each entry of the list" jqt install_fence '[.[] | select(.kind == "ResourceQuota" and .metadata.name == "br-sandbox-quota") | .metadata.namespace] | sort == ["sbx-alpha-allow", "sbx-alpha-full", "sbx-alpha-none", "sbx-beta-none"]'
    render install_fence_plain -f "$CHART/ci/example-values.yaml" -f "$CHART/ci/fence-values.yaml" --set render_namespace=false
    t "the fence render with render_namespace false holds no managed Namespace" jqt install_fence_plain '[.[] | select(.kind == "Namespace" and (.metadata.labels | has("br.sandbox/managed")))] | length == 0'
    fails_with "BR_SANDBOX_NO_SCOPE" -f "$CHART/ci/example-values.yaml" -f "$CHART/ci/fence-values.yaml" --set-json 'namespaces=[{"name":"other-none","mode":"none"}]'
    t "the stamper is off by default" test "$(yq '.stamper.enabled' "$CHART/values.yaml")" = false
}

check_the_project_manifests_of_the_install() {
    section "INSTALL.md: the Namespace and the SandboxPool of the project, against the policy model and the definition"
    local namespace pool grants spec_keys size_keys definition
    namespace="$(install_block 3 | yq -o=json -I=0 '.')"
    pool="$(install_block 4 | yq -o=json -I=0 '.')"
    grants="$(grants_of ex)"
    definition="$CHART/files/sandboxpool-crd.yaml"
    t "block 3 is a Namespace of mode none with the managed label, and block 4 a SandboxPool of that namespace" test "$(jq -r '[.kind, .metadata.labels["br.sandbox/managed"], .metadata.labels["br.sandbox/mode"]] | join(",")' <<<"$namespace")" = "Namespace,true,none" -a "$(jq -r '[.kind, .apiVersion, .metadata.namespace] | join(",")' <<<"$pool")" = "SandboxPool,sandbox.botresources.ai/v1alpha1,$(jq -r '.metadata.name' <<<"$namespace")"
    t "the policy model admits the Namespace of the project, created by a project identity" test "$(ns_verdict ex CREATE "$namespace" null "$PROJECT_USER" "$grants")" = admit
    t "the same Namespace with the mode full is refused to a project identity with no grant" test "$(ns_verdict ex CREATE "$(jq -c '.metadata.labels["br.sandbox/mode"] = "full"' <<<"$namespace")" null "$PROJECT_USER" "$grants")" = BR_SANDBOX_MODE_NOT_GRANTED
    spec_keys="$(jq -c '.spec | keys' <<<"$pool")"
    size_keys="$(jq -c '.spec.size | keys' <<<"$pool")"
    t "every key of the spec of the SandboxPool is a property of the definition" test "$(yq -o=json '.spec.versions[0].schema.openAPIV3Schema.properties.spec.properties' "$definition" | jq -c --argjson keys "$spec_keys" '[$keys[] | select(in($ARGS.named.p // {}) | not)] | length' --argjson p "$(yq -o=json '.spec.versions[0].schema.openAPIV3Schema.properties.spec.properties' "$definition")")" = 0
    t "every key of the size is a property of the size of the definition" test "$(jq -nc --argjson keys "$size_keys" --argjson p "$(yq -o=json '.spec.versions[0].schema.openAPIV3Schema.properties.spec.properties.size.properties' "$definition")" '[$keys[] | select(in($p) | not)] | length')" = 0
    t "the only required field of the definition is image" test "$(yq -o=json -I=0 '.spec.versions[0].schema.openAPIV3Schema.properties.spec.required' "$definition")" = '["image"]'
    t "the definition has one version, v1alpha1" test "$(yq -o=json -I=0 '[.spec.versions[].name]' "$definition")" = '["v1alpha1"]'
    t "the pod that the keeper makes from the template of the namespace with this pool is admitted" test "$(created ex sbx-alpha-none . "[$(jq -c '{metadata: {namespace: .metadata.namespace, name: .metadata.name}, spec: .spec}' <<<"$pool")]")" = admit
    t "the INSTALL text says that the two manifests were not applied to a cluster" grep -qF 'These two manifests were not applied to a cluster.' "$CHART/INSTALL.md"
}

check_the_limits_have_their_sentences_and_their_tests() {
    section "LIMITS.md: each limit has its sentence, and its test or its mark"
    local limits grants
    limits="$(cat "$CHART/LIMITS.md")"
    grants="$(grants_of ex)"
    t "LIMITS.md names the fence-check set and the fence-object set" test "$(grep -cE '^\*\*The fence-check set\*\* |^\*\*The fence-object set\*\* ' "$CHART/LIMITS.md")" = 2
    t "the fence-check set is read from the keeper README and not tested here" grep -qF 'This is read from the README of keeper 0.2.0 (section Fence); it is not tested in this repository.' <<<"$limits"
    t "the template ConfigMap is in the fence-object set only" grep -qF 'The template ConfigMap `br-sandbox-pod-template` is in the fence-object set only.' <<<"$limits"
    t "a project identity cannot write the template ConfigMap, and a writer can" test "$(fence_verdict ex UPDATE configmaps br-sandbox-pod-template "$PROJECT_USER" "$grants")" = BR_SANDBOX_FENCE -a "$(fence_verdict ex UPDATE configmaps br-sandbox-pod-template "$WRITER_USER" "$(grants_of grants)")" = admit
    t "the fence policy matches exactly the kinds that the fence-object set names" jqt ex '[.[] | select(.kind == "ValidatingAdmissionPolicy" and .metadata.name == "br-sandbox-fence") | .spec.matchConstraints.resourceRules[].resources[]] | sort == ["configmaps", "limitranges", "networkpolicies", "resourcequotas", "rolebindings", "roles", "serviceaccounts"]'
    t "a ServiceAccount other than sandbox and a ConfigMap outside the prefix are not fence objects" test "$(fence_verdict ex CREATE serviceaccounts other "$PROJECT_USER" "$grants")" = admit -a "$(fence_verdict ex CREATE configmaps other "$PROJECT_USER" "$grants")" = admit
    t "the fence part renders no network policy outside the closed list of three names" test "$(jq -c '[.[] | select(.kind == "NetworkPolicy" and (.metadata.namespace | IN("sbx-alpha-none", "sbx-alpha-allow", "sbx-alpha-full", "sbx-beta-none"))) | .metadata.name] | unique - ["br-sandbox-default-deny", "br-sandbox-egress-allow", "br-sandbox-egress-full"]' "$WORK/ex.json")" = '[]'
    t "LIMITS.md says what the check proves and what it does not" grep -qF 'The check proves names and the shape of one policy. It does not prove the content of the other fence objects' <<<"$limits"
    t "LIMITS.md holds the limit sentence of the binding of the update in the agreed words" grep -qF 'The fence check of the keeper reads the binding `br-sandbox-pod`. It does not read `br-sandbox-pod-update`. If that binding is absent, an identity with the patch right on pods can change the image of a pod after its admission, and the keeper does not see it.' <<<"$limits"
    t "POLICIES.md holds the same sentence" grep -qF 'The fence check of the keeper reads the binding `br-sandbox-pod`. It does not read `br-sandbox-pod-update`. If that binding is absent, an identity with the patch right on pods can change the image of a pod after its admission, and the keeper does not see it.' "$CHART/POLICIES.md"
    t "LIMITS.md says who can remove a binding and cites the two rights tests" grep -qF 'Only an identity with a write right on admission policy bindings at cluster scope can remove a binding: that is the platform. The runner and the keeper have no such right: no Role and no ClusterRole of any render of the chart writes an admission policy binding (`check_no_role_of_any_render_writes_an_admission_binding` in `test-sandbox-chart-pod-policy.sh`, and `check_the_controller_holds_no_forbidden_right` in `test-sandbox-chart-operator-rights.sh` for the keeper).' <<<"$limits"
    t "the two functions that LIMITS.md cites exist" test "$(grep -c '^check_no_role_of_any_render_writes_an_admission_binding()' "$SCRIPTS/test-sandbox-chart-pod-policy.sh")" = 1 -a "$(grep -c '^check_the_controller_holds_no_forbidden_right()' "$SCRIPTS/test-sandbox-chart-operator-rights.sh")" = 1
    t "no Role and no ClusterRole of the example render writes an admission policy binding" jqt ex '[.[] | select(.kind == "ClusterRole" or .kind == "Role") | .rules[] | select(any(.resources[]; . == "validatingadmissionpolicybindings" or . == "validatingadmissionpolicies" or . == "*") and any(.verbs[]; . != "get" and . != "list" and . != "watch"))] | length == 0'
    t "both bindings of the pod policy select the same namespaces and the same policy" jqt ex '[.[] | select(.kind == "ValidatingAdmissionPolicyBinding" and (.metadata.name | startswith("br-sandbox-pod"))) | [.spec.policyName, .spec.matchResources.namespaceSelector]] | unique | length == 1'
    t "LIMITS.md says that all namespaces of a scope share one client_id" grep -qF 'All sandbox namespaces of one scope share one `client_id`, so the store prefix does not separate two pools of one product and environment' <<<"$limits"
    t "two namespaces of the scope sbx-alpha render the same client_id and the scope sbx-beta another" test "$(jq -r '[.[] | select(.kind == "ConfigMap" and .metadata.name == "br-sandbox-config") | {(.metadata.namespace): (.data["config.json"] | fromjson | .client_id)}] | add | [.["sbx-alpha-none"], .["sbx-alpha-full"], .["sbx-beta-none"]] | join(",")' "$WORK/ex.json")" = "alpha,alpha,beta"
    t "LIMITS.md says that no SandboxPool means no pod, and that two resources both apply" test "$(grep -cF '**No `SandboxPool`, no pod.** A managed namespace with no `SandboxPool` admits no new pod' <<<"$limits")" -eq 1 -a "$(grep -cF '**Two resources in one namespace.** A pod must pass the rules against every `SandboxPool` of its namespace' <<<"$limits")" -eq 1
    t "a managed namespace with no resource admits no pod" test "$(created ex sbx-alpha-none . '[]')" = PARAMETER_NOT_FOUND
    t "a second resource with another image stops the pod that passes the first" test "$(created ex sbx-alpha-none . "[$(pool_of sbx-alpha-none), $(pool_of sbx-alpha-none | jq -c '.metadata.name = "second" | .spec.image = "registry.example.test/agents/other:1.0"')]")" = BR_SANDBOX_IMAGE
    t "a size above the maximum of the platform is refused at pod time under a pool that allows it" test "$(created ex sbx-alpha-none '(agent).resources.limits.memory = "3Gi"' "[$(pool_of sbx-alpha-none | jq -c '.spec.size.memory.limit = "4Gi"')]")" = BR_SANDBOX_RESOURCES
    t "the LimitRange max is limits.size and the quota holds limits.pods plus quota.extra_pods" jqt ex '([.[] | select(.kind == "LimitRange" and .metadata.namespace == "sbx-alpha-none") | .spec.limits[0].max] == [{"cpu": "2", "memory": "2Gi", "ephemeral-storage": "5Gi"}]) and ([.[] | select(.kind == "ResourceQuota" and .metadata.name == "br-sandbox-quota" and .metadata.namespace == "sbx-alpha-none") | .spec.hard.pods] == ["24"])'
    t "the chart has no value that caps maxReplicas" test "$(grep -c 'maxReplicas' "$CHART/values.yaml" "$CHART/values.schema.json" | awk -F: '{s += $2} END {print s}')" = 0
    t "only one rule of the pod policy reads the user who creates the pod: the exception of the network check" jqt ex '[.[] | select(.kind == "ValidatingAdmissionPolicy" and .metadata.name == "br-sandbox-pod") | .spec.validations[] | select(.expression | contains("request.userInfo")) | .message | split(":")[0]] == ["BR_SANDBOX_RUN_AS_NON_ROOT"]'
    t "LIMITS.md says that no rule says who may create a pod and that the window is not closed" grep -qF '**No rule on who creates pods.** No rule says who may create a pod in a managed namespace.' <<<"$limits"
    t "the bullet on who creates pods marks the claim on the keeper as a fact of keeper 0.2 that is not tested here" grep -qF 'the keeper makes no ReplicaSet before the fence is there (a fact of keeper 0.2, not tested in this repository). The window is not closed and not tested on a cluster.' <<<"$limits"
    t "LIMITS.md says that the stamper is off by default and not proved on a cluster" grep -qF '**The stamper is off by default** (`stamper.enabled` is `false`) and is not proved on a cluster' <<<"$limits"
    t "LIMITS.md says the fence-object control holds only under different identities, in the fallback shape" grep -qF 'The control holds only if the project and the platform act under different identities, or if the GitOps project of the project refuses it the fence kinds.' <<<"$limits"
    t "LIMITS.md says that a project can declare any image in its own pool" grep -qF '**A project can declare any image in its own pool.**' <<<"$limits"
    t "LIMITS.md says that the proxied modes are not decided" grep -qF 'Who gives the allow list and who approves it is not decided.' <<<"$limits"
    t "the system namespace holds the egress policy of the keeper and is not a managed namespace" jqt ex '([.[] | select(.kind == "NetworkPolicy" and .metadata.namespace == "sbx-system" and .metadata.name == "br-sandbox-egress-keeper")] | length == 1) and ([.[] | select(.kind == "Namespace" and .metadata.name == "sbx-system") | .metadata.labels | has("br.sandbox/managed")] == [false])'
}

check_the_label_table_says_who_sets_the_namespace_labels() {
    section "README label contract: the project sets managed and mode on its own Namespace, the chart only on request"
    local readme
    readme="$(cat "$CHART/README.md")"
    t "the managed row names the project, render_namespace and the immutability code" grep -qF '| `br.sandbox/managed` | namespace label | the project, on its own Namespace; the chart only with `render_namespace: true` |' <<<"$readme"
    t "the mode row names the project, render_namespace, the egress namespaces and the immutability code" grep -qF '| `br.sandbox/mode` | namespace label | the project, on its own Namespace; the chart only with `render_namespace: true` (egress namespaces: the chart) |' <<<"$readme"
    t "no row of the label table says that the chart sets managed or mode with no condition" test "$(grep -cE '^\| `br\.sandbox/(managed|mode)` \| namespace label \| chart \|' "$CHART/README.md")" = 0
    t "render_namespace is false by default" test "$(yq '.render_namespace' "$CHART/values.yaml")" = false
    render label_plain -f "$EXAMPLE" -f "$CHART/ci/fence-values.yaml" --set render_namespace=false
    t "a render with render_namespace false holds no Namespace with the managed label" jqt label_plain '[.[] | select(.kind == "Namespace" and (.metadata.labels | has("br.sandbox/managed")))] | length == 0'
    t "a render with render_namespace true holds the managed Namespace of each entry, with the mode of the entry" jqt ex '[.[] | select(.kind == "Namespace" and (.metadata.labels | has("br.sandbox/managed"))) | [.metadata.name, .metadata.labels["br.sandbox/mode"]]] | sort == [["sbx-alpha-allow", "allow"], ["sbx-alpha-full", "full"], ["sbx-alpha-none", "none"], ["sbx-beta-none", "none"]]'
    t "an egress namespace carries the mode set by the chart and no managed label" jqt egress_keeper '[.[] | select(.kind == "Namespace" and (.metadata.labels | has("br.sandbox/egress"))) | (.metadata.labels | has("br.sandbox/mode") and (has("br.sandbox/managed") | not))] | length > 0 and all'
    t "the policy refuses a project that changes the mode of a managed namespace, and the same change by a writer is admitted" test "$(ns_verdict grants UPDATE "$(namespace_object sbx-alpha-new true allow)" "$(namespace_object sbx-alpha-new true none)" "$PROJECT_USER" "$(grants_of grants)")" = BR_SANDBOX_LABEL_IMMUTABLE -a "$(ns_verdict grants UPDATE "$(namespace_object sbx-alpha-new true allow)" "$(namespace_object sbx-alpha-new true none)" "$WRITER_USER" "$(grants_of grants)")" = admit
}
