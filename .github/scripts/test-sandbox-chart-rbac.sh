#!/usr/bin/env bash

ROLE='.[] | select(.kind == "Role" and .metadata.namespace == "sbx-alpha-none" and .metadata.name == "%s")'

role_rules() {
    # shellcheck disable=SC2059
    jqv "$2" "$(printf "$ROLE" "$1") | .rules | map(.resources[0] + \":\" + (.verbs | sort | join(\",\"))) | sort | join(\" \")"
}

check_runner_role_and_binding() {
    section "runner Role: pods, exec, replicasets read, the config"
    t "the runner holds pods, pods/exec, replicasets read and its config only" test "$(role_rules br-sandbox-runner ex)" = "configmaps:get pods/exec:create pods:create,delete,get,list,patch,watch replicasets:get,list,watch"
    t "the runner Role names the ConfigMap it reads" jqt ex "$(printf "$ROLE" br-sandbox-runner) | .rules[] | select(.resources == [\"configmaps\"]) | .resourceNames == [\"br-sandbox-config\"]"
    t "the runner binding names the service account of the values" jqt ex '.[] | select(.kind == "RoleBinding" and .metadata.name == "br-sandbox-runner" and .metadata.namespace == "sbx-beta-none") | .subjects == [{"kind": "ServiceAccount", "name": "runner", "namespace": "runners"}]'
    t "no Role of the render grants secrets" jqt keeper '[.[] | select(.kind == "Role" or .kind == "ClusterRole") | .rules[].resources[] | select(. == "secrets" or . == "*")] | length == 0'
    t "no Role of the render grants a wildcard verb" jqt keeper '[.[] | select(.kind == "Role" or .kind == "ClusterRole") | .rules[].verbs[] | select(. == "*")] | length == 0'
}

check_keeper_rights() {
    section "keeper rights"
    t "the keeper Role has pods without patch, replicasets with patch, status patch, events create" test "$(jq -r "$(printf "$ROLE" br-sandbox-keeper) | .rules | map(.resources[0] + \":\" + (.verbs | sort | join(\",\"))) | sort | join(\" \")" "$WORK/keeper.json")" = "events:create pods:delete,get,list,watch replicasets/status:patch replicasets:get,list,patch,watch"
    t "the keeper reads namespaces cluster-wide, nothing more cluster-wide" jqt keeper '[.[] | select(.kind == "ClusterRole" and (.metadata.name | endswith("keeper-namespaces"))) | .rules] == [[{"apiGroups": [""], "resources": ["namespaces"], "verbs": ["get", "list", "watch"]}]]'
    t "the keeper holds leases in its own namespace" jqt keeper '.[] | select(.kind == "Role" and .metadata.name == "br-sandbox-keeper-lease" and .metadata.namespace == "sbx-system") | .rules == [{"apiGroups": ["coordination.k8s.io"], "resources": ["leases"], "verbs": ["get", "create", "update"]}]'
    t "without the keeper no keeper object is rendered" jqt ex '[.[] | select((.metadata.name // "") | test("keeper"))] | length == 0'
    t "the keeper Deployment has two replicas, a restricted security context and no token mount of sandbox" jqt keeper '.[] | select(.kind == "Deployment") | .spec.replicas == 2 and .spec.template.spec.containers[0].securityContext.runAsNonRoot == true'
    t "the keeper image is mirrored like every image" jqt mirror '.[] | select(.kind == "Deployment") | .spec.template.spec.containers[0].image | startswith("mirror.example.test/")'
    t "keeper.enabled needs an image" test "$(helm template t "$CHART" -f "$EXAMPLE" --set keeper.enabled=true 2>&1 | grep -c 'BR_SANDBOX_MISSING_VALUE{key=keeper.image}')" -ge 1
    t "the keeper settings reach the system config" test "$(yq 'select(.kind == "ConfigMap" and .metadata.namespace == "sbx-system" and .metadata.name == "br-sandbox-config") | .data["config.yaml"] | from_yaml | .settings.scale_down_cooldown_seconds' "$WORK/keeper_yaml.yaml")" = 120
    t "the keeper egress policy names the API server and the store" jqt keeper '.[] | select(.kind == "NetworkPolicy" and .metadata.name == "egress-keeper") | [.spec.egress[].to[].ipBlock.cidr | select(. != null)] | sort == ["10.0.0.11/32", "10.0.1.0/24"]'
    fails_with "BR_SANDBOX_MISSING_VALUE{key=network.store_cidrs}" -f "$KEEPER_VALUES" -f "$(vals nostore 'del(.network.store_cidrs)')"
}

check_config_contract() {
    section "the ConfigMap read by br-runner-sandbox and the keeper"
    local config
    config="$(yq 'select(.kind == "ConfigMap" and .metadata.namespace == "sbx-alpha-none" and .metadata.name == "br-sandbox-config") | .data["config.yaml"]' "$WORK/ex_yaml.yaml")"
    printf '%s' "$config" >"$WORK/config.yaml"
    t "the config holds the keys of the contract" test "$(yq '[.restore_timeout_seconds, .idle_ttl, .idle_keep_min_state_bytes, .warm_max_age_seconds, .save.ttl_seconds, .save.paths[0], .tools.seed_container.name, .tools.bsdtar_path, .label_prefix, .pool.replicaset, .store.bucket] | join(",")' "$WORK/config.yaml")" = "30,120,10000000,1800,60,/work,seed,/opt/sbx/bin/bsdtar,br.sandbox,warm-alpha-none,saves"
    t "the save age is one value for the keeper and br-runner-sandbox" test "$(yq 'select(.kind == "ConfigMap" and .metadata.namespace == "sbx-system" and .metadata.name == "br-sandbox-config") | .data["config.yaml"] | from_yaml | .save.ttl_seconds' "$WORK/ex_yaml.yaml")" = 60
    fails_with "BR_SANDBOX_SAVE_AGE_INVALID" -f "$(vals zero '.save.ttl_seconds = 0')"
    fails_with "BR_SANDBOX_SAVE_AGE_INVALID" -f "$(vals neg '.save.ttl_seconds = -5')"
    local key
    for key in store.endpoint store.bucket store.region store.path_style store.credentials_secret save.ttl_seconds installation.namespace_prefix network.pod_cidr network.service_cidr; do
        fails_with "BR_SANDBOX_MISSING_VALUE{key=${key}}" -f "$(vals "no_${key}" "del(.${key})")"
    done
    fails_with "BR_SANDBOX_MISSING_VALUE{key=network.public_resolvers}" -f "$(vals nores 'del(.network.public_resolvers)')"
    fails_with "BR_SANDBOX_MISSING_VALUE{key=clients.alpha.modes.none.runner.namespace}" -f "$(vals norunner 'del(.clients.alpha.modes.none.runner.namespace)')"
    fails_with "BR_SANDBOX_MISSING_VALUE{key=network_check.image}" -f "$(vals nonetcheck 'del(.network_check.image)')"
}
