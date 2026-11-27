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
