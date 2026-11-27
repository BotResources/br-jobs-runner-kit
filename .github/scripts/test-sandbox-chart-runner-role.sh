#!/usr/bin/env bash

RUNNER_ROLE_RULES='[
  {"apiGroups": [""], "resources": ["pods"], "verbs": ["get", "list", "watch", "patch", "create", "delete"]},
  {"apiGroups": [""], "resources": ["pods/exec"], "verbs": ["create", "get"]},
  {"apiGroups": ["apps"], "resources": ["replicasets"], "verbs": ["get", "list", "watch"]},
  {"apiGroups": [""], "resources": ["configmaps"], "resourceNames": ["br-sandbox-config"], "verbs": ["get"]}
]'

RUNNER_ROLES='[.[] | select(.kind == "Role" and .metadata.name == "br-sandbox-runner")]'

RULES_NAMING_EXEC='[.[] | select(.kind == "Role" or .kind == "ClusterRole") | select(any(.rules[]; .resources | any(. == "pods/exec" or . == "pods/*" or . == "*"))) | .metadata.name] | unique'

check_the_runner_role_holds_the_ruled_table() {
    section "the runner Role holds exactly the ruled rights, pods/exec with create and get"
    local render
    for render in ex keeper mirror nondefault; do
        t "in the ${render} render each runner Role has exactly the four rules of the table" jqt "$render" "${RUNNER_ROLES} | length > 0 and all(.[]; .rules == ${RUNNER_ROLE_RULES})"
        t "in the ${render} render the pods/exec rule of each runner Role has the verbs create and get only" jqt "$render" "${RUNNER_ROLES} | length > 0 and all(.[]; [.rules[] | select(.resources == [\"pods/exec\"])] == [{\"apiGroups\": [\"\"], \"resources\": [\"pods/exec\"], \"verbs\": [\"create\", \"get\"]}])"
    done
    t "the example render has one runner Role in each of the four sandbox namespaces" jqt ex "${RUNNER_ROLES} | length == 4"
    t "the runner Role names no resource beyond pods, pods/exec, replicasets and configmaps" jqt ex "[${RUNNER_ROLES}[].rules[].resources[]] | unique == [\"configmaps\", \"pods\", \"pods/exec\", \"replicasets\"]"
    t "the runner Role has no pods/log, no pods/attach, no pods/portforward and no secrets" jqt ex "[${RUNNER_ROLES}[].rules[].resources[] | select(. == \"pods/log\" or . == \"pods/attach\" or . == \"pods/portforward\" or . == \"secrets\" or . == \"*\")] | length == 0"
    for render in ex keeper mirror nondefault; do
        t "in the ${render} render only the runner Role names pods/exec" jqt "$render" "${RULES_NAMING_EXEC} == [\"br-sandbox-runner\"]"
    done
}

check_the_runner_role_table_detector_flags_each_mutation() {
    section "the table check of the runner Role refuses a Role without get, with a third verb, or with pods/log"
    local probe="$WORK/runner-role-probe.json" mutation name
    local -a mutations=(
        'the pods/exec rule without get|del(.rules[1].verbs[] | select(. == "get"))'
        'the pods/exec rule with the verb delete|.rules[1].verbs += ["delete"]'
        'the pods/exec rule with the verb update|.rules[1].verbs += ["update"]'
        'a rule on pods/log|.rules += [{"apiGroups": [""], "resources": ["pods/log"], "verbs": ["get"]}]'
    )
    jq -n --argjson rules "$RUNNER_ROLE_RULES" '[{kind: "Role", metadata: {name: "br-sandbox-runner"}, rules: $rules}]' >"$WORK/runner-role-good.json"
    t "the table check accepts the Role of the table" test "$(jq "${RUNNER_ROLES} | all(.[]; .rules == ${RUNNER_ROLE_RULES})" "$WORK/runner-role-good.json")" = true
    for mutation in "${mutations[@]}"; do
        name="${mutation%%|*}"
        jq "map(${mutation#*|})" "$WORK/runner-role-good.json" >"$probe"
        t "the table check refuses ${name}" test "$(jq "${RUNNER_ROLES} | all(.[]; .rules == ${RUNNER_ROLE_RULES})" "$probe")" = false
    done
}
