#!/usr/bin/env bash

RULES_REACHING_EPHEMERAL_CONTAINERS='
def reaches:
    (.apiGroups // [] | any(. == "" or . == "*"))
    and (.resources // [] | any(. == "*" or test("^(pods|\\*)/(ephemeralcontainers|\\*)$")))
    and ((.verbs // []) | length > 0);
[.[] | select(.kind == "Role" or .kind == "ClusterRole") | .rules[]? | select(reaches)] | length
'

ROLES_AND_RULES_OF_THE_RENDER='[.[] | select(.kind == "Role" or .kind == "ClusterRole") | .rules[]?] | length > 0'

check_the_ephemeral_containers_detector_flags_every_reaching_rule() {
    section "the detector of rules reaching pods/ephemeralcontainers"
    local resource group verbs probe="$WORK/ephemeral-probe.json"
    for resource in pods/ephemeralcontainers '*' 'pods/*' '*/ephemeralcontainers' '*/*'; do
        jq -nc --arg r "$resource" '[{kind: "Role", rules: [{apiGroups: [""], resources: [$r], verbs: ["get"]}]}]' >"$probe"
        t "a Role with the resource ${resource} is flagged" test "$(jq "$RULES_REACHING_EPHEMERAL_CONTAINERS" "$probe")" = 1
        jq -nc --arg r "$resource" '[{kind: "ClusterRole", rules: [{apiGroups: ["*"], resources: [$r], verbs: ["*"]}]}]' >"$probe"
        t "a ClusterRole with the resource ${resource}, all groups, all verbs, is flagged" test "$(jq "$RULES_REACHING_EPHEMERAL_CONTAINERS" "$probe")" = 1
    done
    for verbs in '["update"]' '["patch"]' '["get"]' '["*"]'; do
        jq -nc --argjson v "$verbs" '[{kind: "Role", rules: [{apiGroups: [""], resources: ["pods/ephemeralcontainers"], verbs: $v}]}]' >"$probe"
        t "the verbs ${verbs} on pods/ephemeralcontainers are flagged" test "$(jq "$RULES_REACHING_EPHEMERAL_CONTAINERS" "$probe")" = 1
    done
    for resource in pods pods/log pods/exec pods/status replicasets nodes; do
        jq -nc --arg r "$resource" '[{kind: "Role", rules: [{apiGroups: [""], resources: [$r], verbs: ["get", "list", "watch", "create", "delete", "patch", "update"]}]}]' >"$probe"
        t "a Role with the resource ${resource} only is not flagged" test "$(jq "$RULES_REACHING_EPHEMERAL_CONTAINERS" "$probe")" = 0
    done
    for group in apps batch coordination.k8s.io; do
        jq -nc --arg g "$group" '[{kind: "Role", rules: [{apiGroups: [$g], resources: ["*"], verbs: ["*"]}]}]' >"$probe"
        t "a wildcard rule of the group ${group} does not reach a core subresource" test "$(jq "$RULES_REACHING_EPHEMERAL_CONTAINERS" "$probe")" = 0
    done
}

check_no_role_of_the_chart_reaches_pods_ephemeralcontainers() {
    section "no Role and no ClusterRole of any render holds a verb on pods/ephemeralcontainers"
    render ephemeral_nocheck -f "$EXAMPLE" --set network_check.enabled=false
    render ephemeral_keeper_nocheck -f "$EXAMPLE" -f "$OPERATOR_VALUES" --set network_check.enabled=false
    local name
    for name in ex mirror keeper cred two egressdual tunnel ephemeral_nocheck ephemeral_keeper_nocheck; do
        [[ " ex mirror keeper ephemeral_keeper_nocheck " != *" $name "* ]] || t "render ${name}: the render holds Roles with rules" jqt "$name" "$ROLES_AND_RULES_OF_THE_RENDER"
        t "render ${name}: no rule reaches pods/ephemeralcontainers" test "$(jqv "$name" "$RULES_REACHING_EPHEMERAL_CONTAINERS")" = 0
    done
    t "the render with the network check holds its namespaced Roles" jqt ex '[.[] | select(.kind == "Role" and .metadata.name == "br-sandbox-netcheck")] | length == 4'
    t "the render without the network check holds none" jqt ephemeral_nocheck '[.[] | select(.metadata.name // "" | test("netcheck"))] | length == 0'
}
