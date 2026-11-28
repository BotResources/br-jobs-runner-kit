#!/usr/bin/env bash

RIGHTS_TABLE='[
  {"scope": "namespace", "group": "", "resource": "pods", "names": [""], "verbs": ["get", "list", "watch", "delete"]},
  {"scope": "namespace", "group": "apps", "resource": "replicasets", "names": [""], "verbs": ["get", "list", "watch", "patch", "create", "delete"]},
  {"scope": "namespace", "group": "apps", "resource": "replicasets/status", "names": [""], "verbs": ["patch"]},
  {"scope": "namespace", "group": "", "resource": "events", "names": [""], "verbs": ["create"]},
  {"scope": "namespace", "group": "sandbox.botresources.ai", "resource": "sandboxpools", "names": [""], "verbs": ["get", "list", "watch"]},
  {"scope": "namespace", "group": "sandbox.botresources.ai", "resource": "sandboxpools/status", "names": [""], "verbs": ["patch"]},
  {"scope": "namespace", "group": "", "resource": "configmaps", "names": ["br-sandbox-pod-template"], "verbs": ["get"]},
  {"scope": "namespace", "group": "", "resource": "resourcequotas", "names": [""], "verbs": ["get", "list", "watch"]},
  {"scope": "namespace", "group": "networking.k8s.io", "resource": "networkpolicies", "names": [""], "verbs": ["get", "list", "watch"]},
  {"scope": "lease", "group": "coordination.k8s.io", "resource": "leases", "names": [""], "verbs": ["get", "create", "update"]},
  {"scope": "cluster", "group": "", "resource": "namespaces", "names": [""], "verbs": ["get", "list", "watch"]},
  {"scope": "cluster", "group": "admissionregistration.k8s.io", "resource": "validatingadmissionpolicybindings", "names": [""], "verbs": ["get", "list", "watch"]}
]'

CONTROLLER_RIGHTS='
. as $all
| [$all[] | select(.kind == "RoleBinding" or .kind == "ClusterRoleBinding")
  | select(any(.subjects[]?; .kind == "ServiceAccount" and .name == "br-sandbox-keeper" and .namespace == $system))] as $bindings
| [$bindings[] | . as $binding
  | ($all[] | select(.kind == $binding.roleRef.kind and .metadata.name == $binding.roleRef.name
      and (.kind == "ClusterRole" or .metadata.namespace == $binding.metadata.namespace))) as $role
  | ($binding.kind == "ClusterRoleBinding") as $cluster
  | $role.rules[] as $rule
  | $rule.apiGroups[] as $group
  | $rule.resources[] as $resource
  | ($rule.resourceNames // [""])[] as $name
  | $rule.verbs[] as $verb
  | {where: (if $cluster then "cluster" else "ns:" + $binding.metadata.namespace end), group: $group, resource: $resource, name: $name, verb: $verb}]
| unique
'

EXPECTED_RIGHTS='
([.[] | select(.kind == "RoleBinding" and .metadata.name == "br-sandbox-runner") | .metadata.namespace] | unique) as $managed
| [$table[] | . as $row
  | (if $row.scope == "cluster" then ["cluster"] elif $row.scope == "lease" then ["ns:" + $system] else [$managed[] | "ns:" + .] end)[] as $where
  | $row.names[] as $name
  | $row.verbs[] as $verb
  | {where: $where, group: $row.group, resource: $row.resource, name: $name, verb: $verb}]
| unique
'

controller_rights() {
    jq -c --arg system "$2" "${CONTROLLER_RIGHTS}" "$WORK/$1.json"
}

expected_rights() {
    jq -c --arg system "$2" --argjson table "$RIGHTS_TABLE" "${EXPECTED_RIGHTS}" "$WORK/$1.json"
}

rights_equal_the_table() {
    local name="$1" system="${2:-sbx-system}"
    [[ "$(controller_rights "$name" "$system")" == "$(expected_rights "$name" "$system")" ]]
}

rights_hold() {
    local name="$1" filter="$2"
    jq -e --arg system sbx-system "[${CONTROLLER_RIGHTS}] | .[0] | ${filter}" "$WORK/${name}.json" >/dev/null
}

render_rights_fixtures() {
    render rights_five -f "$(vals rightsfive '.namespaces += [{"name": "sbx-beta-a-none", "mode": "none"}, {"name": "sbx-beta-b-none", "mode": "none"}, {"name": "sbx-alpha-x-none", "mode": "none"}] | .operator.enabled = true | .operator.image = "ghcr.io/botresources/br-sandbox-keeper@sha256:2cf56ab2e5daa673e86aa325b880578be2630e803b81b0924a9f52a580a0705b"')"
}

check_the_controller_rights_equal_the_table() {
    section "the rights of the controller: the union of what its bindings grant equals the table"
    t "the table holds twelve rows, nine for each namespace, one for the lease, two for the cluster" test "$(jq -r '[group_by(.scope)[] | length] | sort | join(",")' <<<"$RIGHTS_TABLE")" = "1,2,9"
    t "with four namespaces the rights equal the table" rights_equal_the_table keeper
    t "with seven namespaces the rights equal the table" rights_equal_the_table rights_five
    t "the example holds the controller subject in four namespaces" test "$(jq -r '[.[] | .where | select(startswith("ns:sbx-") and . != "ns:sbx-system")] | unique | length' <<<"$(controller_rights keeper sbx-system)")" = 4
    t "with seven namespaces the rights of the controller reach seven namespaces" test "$(jq -r '[.[] | .where | select(startswith("ns:sbx-") and . != "ns:sbx-system")] | unique | length' <<<"$(controller_rights rights_five sbx-system)")" = 7
}

check_the_cluster_and_lease_rights_of_the_controller() {
    section "the cluster rights and the lease right of the controller"
    t "the cluster rights are on namespaces and on validatingadmissionpolicybindings" test "$(jq -c '[.[] | select(.where == "cluster") | .resource] | unique' <<<"$(controller_rights keeper sbx-system)")" = '["namespaces","validatingadmissionpolicybindings"]'
    t "the cluster verbs are get, list and watch" test "$(jq -c '[.[] | select(.where == "cluster") | .verb] | unique' <<<"$(controller_rights keeper sbx-system)")" = '["get","list","watch"]'
    t "the lease right is leases get, create and update in the system namespace" test "$(jq -c '[.[] | select(.where == "ns:sbx-system") | [.resource, .verb]] | sort' <<<"$(controller_rights keeper sbx-system)")" = '[["leases","create"],["leases","get"],["leases","update"]]'
}

check_the_controller_holds_no_forbidden_right() {
    section "the controller does not hold: exec, a pod create or patch, secrets, a write on the fence, a wildcard, escalate, bind, impersonate"
    local rights
    rights="$(controller_rights rights_five sbx-system)"
    t "the controller has no right on pods/exec" test "$(jq '[.[] | select(.resource == "pods/exec")] | length' <<<"$rights")" = 0
    t "the controller can neither create nor patch nor update a pod" test "$(jq '[.[] | select(.resource == "pods" and (.verb | IN("create", "patch", "update", "deletecollection")))] | length' <<<"$rights")" = 0
    t "the controller has no right on secrets" test "$(jq '[.[] | select(.resource == "secrets")] | length' <<<"$rights")" = 0
    local resource
    for resource in networkpolicies namespaces roles rolebindings clusterroles clusterrolebindings resourcequotas limitranges validatingadmissionpolicies validatingadmissionpolicybindings customresourcedefinitions sandboxpools serviceaccounts; do
        t "the controller has no write on ${resource}" test "$(jq --arg r "$resource" '[.[] | select(.resource == $r and (.verb | IN("create", "update", "patch", "delete", "deletecollection")))] | length' <<<"$rights")" = 0
    done
    t "the controller has no right on sandboxpools/finalizers" test "$(jq '[.[] | select(.resource == "sandboxpools/finalizers")] | length' <<<"$rights")" = 0
    t "the controller has none of the verbs escalate, bind and impersonate" test "$(jq '[.[] | select(.verb | IN("escalate", "bind", "impersonate"))] | length' <<<"$rights")" = 0
    t "the controller has no wildcard group, resource, name or verb" test "$(jq '[.[] | select(.group == "*" or .resource == "*" or .name == "*" or .verb == "*" or (.resource | endswith("/*")))] | length' <<<"$rights")" = 0
    t "the controller has no right in an egress namespace" test "$(jq '[.[] | select(.where | endswith("-egress"))] | length' <<<"$(controller_rights egress_keeper sbx-system)")" = 0
}

check_the_rights_detector_flags_each_mutation() {
    section "the rights check refuses a table plus one verb, minus a resource name, a wildcard, a write on networkpolicies"
    local mutation name
    local -a mutations=(
        'one more verb on pods|(.[] | select(.kind == "ClusterRole" and .metadata.name == "br-sandbox-controller") | .rules[0].verbs) += ["create"]'
        'the pod template rule without resourceNames|del(.[] | select(.kind == "ClusterRole" and .metadata.name == "br-sandbox-controller") | .rules[] | select(.resources == ["configmaps"]) | .resourceNames)'
        'a wildcard resource|(.[] | select(.kind == "ClusterRole" and .metadata.name == "br-sandbox-controller") | .rules) += [{"apiGroups": [""], "resources": ["*"], "verbs": ["get"]}]'
        'a write on networkpolicies|(.[] | select(.kind == "ClusterRole" and .metadata.name == "br-sandbox-controller") | .rules[] | select(.resources == ["networkpolicies"]) | .verbs) += ["create"]'
        'a cluster binding of the namespaced role|. + [{"kind": "ClusterRoleBinding", "metadata": {"name": "x"}, "roleRef": {"kind": "ClusterRole", "name": "br-sandbox-controller"}, "subjects": [{"kind": "ServiceAccount", "name": "br-sandbox-keeper", "namespace": "sbx-system"}]}]'
    )
    for mutation in "${mutations[@]}"; do
        name="${mutation%%|*}"
        jq -c "${mutation#*|}" "$WORK/keeper.json" >"$WORK/rights-probe.json"
        t "the rights check refuses ${name}" test "$(jq -c --arg system sbx-system "${CONTROLLER_RIGHTS}" "$WORK/rights-probe.json")" != "$(jq -c --arg system sbx-system --argjson table "$RIGHTS_TABLE" "${EXPECTED_RIGHTS}" "$WORK/rights-probe.json")"
    done
}

check_no_value_sets_a_right() {
    section "no value sets a right of the controller"
    local key
    for key in rules rbac extra_rules bind_roles roles cluster_role; do
        fails_with "values don't meet the specifications" -f "$EXAMPLE" -f "$OPERATOR_VALUES" --set "operator.${key}=1"
    done
}
