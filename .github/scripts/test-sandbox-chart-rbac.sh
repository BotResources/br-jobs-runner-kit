#!/usr/bin/env bash

ROLE='.[] | select(.kind == "Role" and .metadata.namespace == "sbx-alpha-none" and .metadata.name == "%s")'

role_filter() {
    printf '%s' "${ROLE//%s/$1}"
}

role_rules() {
    jqv "$2" "$(role_filter "$1") | .rules | map(.resources[0] + \":\" + (.verbs | sort | join(\",\"))) | sort | join(\" \")"
}

CLUSTER_ROLE='.[] | select(.kind == "ClusterRole" and .metadata.name == "%s")'

cluster_role_filter() {
    printf '%s' "${CLUSTER_ROLE//%s/$1}"
}

cluster_role_rules() {
    jqv "$2" "$(cluster_role_filter "$1") | .rules | map(.resources[0] + \":\" + (.verbs | sort | join(\",\"))) | sort | join(\" \")"
}

check_runner_role_and_binding() {
    section "runner ClusterRole: pods, exec, replicasets read, the config"
    t "the runner holds pods, pods/exec, replicasets read and its config only" test "$(cluster_role_rules br-sandbox-runner keeper)" = "configmaps:get pods/exec:create,get pods:create,delete,get,list,patch,watch replicasets:get,list,watch"
    t "the runner ClusterRole names the ConfigMap it reads" jqt keeper "$(cluster_role_filter br-sandbox-runner) | .rules[] | select(.resources == [\"configmaps\"]) | .resourceNames == [\"br-sandbox-config\"]"
    t "the runner binding names the service account of the scope" jqt ex '.[] | select(.kind == "RoleBinding" and .metadata.name == "br-sandbox-runner" and .metadata.namespace == "sbx-beta-none") | .subjects == [{"kind": "ServiceAccount", "name": "runner", "namespace": "runners"}]'
    t "the runner binding names the ClusterRole, and the render holds no Role of the runner" jqt keeper '([.[] | select(.kind == "RoleBinding" and .metadata.name == "br-sandbox-runner") | .roleRef.kind] | unique == ["ClusterRole"]) and ([.[] | select(.kind == "Role" and .metadata.name == "br-sandbox-runner")] | length == 0)'
    local render
    for render in keeper cred keeperegress; do
        t "in the ${render} render no Role grants secrets" jqt "$render" '[.[] | select(.kind == "Role" or .kind == "ClusterRole") | .rules[].resources[] | select(. == "secrets" or . == "*")] | length == 0'
        t "in the ${render} render no Role grants a wildcard verb" jqt "$render" '[.[] | select(.kind == "Role" or .kind == "ClusterRole") | .rules[].verbs[] | select(. == "*")] | length == 0'
    done
}

check_keeper_rights() {
    section "keeper rights"
    t "the controller ClusterRole has pods without patch, replicasets with patch and create, status patch, events create" test "$(cluster_role_rules br-sandbox-controller keeper)" = "configmaps:get events:create networkpolicies:get,list,watch pods:delete,get,list,watch replicasets/status:patch replicasets:create,delete,get,list,patch,watch resourcequotas:get,list,watch sandboxpools/status:patch sandboxpools:get,list,watch"
    t "the keeper reads namespaces and the admission policy bindings cluster-wide, nothing more cluster-wide" jqt keeper '[.[] | select(.kind == "ClusterRole" and (.metadata.name | endswith("keeper-namespaces"))) | .rules] == [[{"apiGroups": [""], "resources": ["namespaces"], "verbs": ["get", "list", "watch"]}, {"apiGroups": ["admissionregistration.k8s.io"], "resources": ["validatingadmissionpolicybindings"], "verbs": ["get", "list", "watch"]}]]'
    t "the keeper holds leases in its own namespace" jqt keeper '.[] | select(.kind == "Role" and .metadata.name == "br-sandbox-keeper-lease" and .metadata.namespace == "sbx-system") | .rules == [{"apiGroups": ["coordination.k8s.io"], "resources": ["leases"], "verbs": ["get", "create", "update"]}]'
    t "without the operator no keeper Deployment, account, document, role or cluster binding is rendered" jqt fence '[.[] | select((.kind | IN("Deployment", "ServiceAccount", "ConfigMap", "Role", "ClusterRole", "ClusterRoleBinding")) and ((.metadata.name // "") | test("keeper")))] | length == 0'
    t "the keeper Deployment has two replicas, a restricted security context and no token mount of sandbox" jqt keeper '.[] | select(.kind == "Deployment") | .spec.replicas == 2 and .spec.template.spec.containers[0].securityContext.runAsNonRoot == true'
    t "the keeper image is mirrored like every image" jqt mirror '.[] | select(.kind == "Deployment") | .spec.template.spec.containers[0].image | startswith("mirror.example.test/")'
    t "operator.enabled needs an image" test "$(helm template t "$CHART" -f "$EXAMPLE" --set operator.enabled=true 2>&1 | grep -c 'BR_SANDBOX_MISSING_VALUE{key=operator.image}')" -ge 1
    t "the keeper egress policy names the API server and the store" jqt keeper '.[] | select(.kind == "NetworkPolicy" and .metadata.name == "br-sandbox-egress-keeper") | [.spec.egress[].to[].ipBlock.cidr | select(. != null)] | sort == ["10.0.0.11/32", "10.0.1.0/24"]'
    fails_with "BR_SANDBOX_MISSING_VALUE{key=network.store_cidrs}" -f "$OPERATOR_VALUES" -f "$(vals nostore 'del(.network.store_cidrs)')"
}

check_config_contract() {
    section "the ConfigMap read by br-runner-sandbox and the keeper"
    t "the save age is one value for the keeper and br-runner-sandbox" test "$(jq -r '.[] | select(.kind == "ConfigMap" and .metadata.namespace == "sbx-system" and .metadata.name == "br-sandbox-keeper-config") | .data["config.json"] | fromjson | .save_ttl_seconds' "$WORK/keeper.json")" = "$(jq -r '.[] | select(.kind == "ConfigMap" and .metadata.namespace == "sbx-alpha-none" and .metadata.name == "br-sandbox-config") | .data["config.json"] | fromjson | .save_ttl_seconds' "$WORK/keeper.json")"
    fails_with "BR_SANDBOX_SAVE_AGE_INVALID" -f "$(vals zero '.save.ttl_seconds = 0')"
    fails_with "BR_SANDBOX_SAVE_AGE_INVALID" -f "$(vals neg '.save.ttl_seconds = -5')"
    local key
    for key in store.endpoint store.bucket store.region store.path_style store.credentials_secret save.ttl_seconds installation.namespace_prefix network.pod_cidr network.service_cidr; do
        fails_with "BR_SANDBOX_MISSING_VALUE{key=${key}}" -f "$(vals "no_${key}" "del(.${key})")"
    done
    fails_with "BR_SANDBOX_MISSING_VALUE{key=network.public_resolvers}" -f "$(vals nores 'del(.network.public_resolvers)')"
    fails_with "BR_SANDBOX_MISSING_VALUE{key=scopes.sbx-alpha.runner.namespace}" -f "$(vals norunner 'del(.scopes.sbx-alpha.runner.namespace)')"
    fails_with "BR_SANDBOX_MISSING_VALUE{key=network_check.image}" -f "$(vals nonetcheck 'del(.network_check.image)')"
}

BINDING_SUBJECTS_OF_THE_POD_IDENTITY='
[.[] | select(.kind == "Namespace" and .metadata.labels["br.sandbox/managed"] == "true") | .metadata.name] as $sandbox_namespaces
| [.[] | select(.kind == "RoleBinding" or .kind == "ClusterRoleBinding") | .subjects[]?
  | select(.kind == "ServiceAccount" and .name == "sandbox" and (.namespace | IN($sandbox_namespaces[])))]
'

check_the_pod_identity_has_no_binding() {
    section "the sandbox service account of a sandbox namespace is the subject of no binding"
    local render
    for render in ex keeper mirror; do
        t "in the ${render} render no RoleBinding and no ClusterRoleBinding names it" jqt "$render" "${BINDING_SUBJECTS_OF_THE_POD_IDENTITY} | length == 0"
    done
    t "the render holds bindings, so the check reads something" jqt keeper '[.[] | select(.kind == "RoleBinding" or .kind == "ClusterRoleBinding")] | length > 0'
    t "the service account sandbox exists in each managed namespace" jqt ex '[.[] | select(.kind == "ServiceAccount" and .metadata.name == "sandbox")] | length == 4'
}

check_the_runner_identity_is_not_the_pod_identity() {
    section "the runner service account is never the sandbox service account of a sandbox namespace"
    fails_with "BR_SANDBOX_RUNNER_IS_POD_IDENTITY{scope=sbx-alpha,namespace=sbx-alpha-none}" -f "$(vals runnerownns '.scopes.sbx-alpha.runner = {"service_account": "sandbox", "namespace": "sbx-alpha-none"}')"
    fails_with "BR_SANDBOX_RUNNER_IS_POD_IDENTITY{scope=sbx-beta,namespace=sbx-alpha-full}" -f "$(vals runnerothersandbox '.scopes.sbx-beta.runner = {"service_account": "sandbox", "namespace": "sbx-alpha-full"}')"
    t "a runner service account named sandbox outside a sandbox namespace renders" helm template t "$CHART" -f "$(vals sandboxoutside '.scopes.sbx-beta.runner = {"service_account": "sandbox", "namespace": "runners"}')"
    t "a runner service account of another name in a sandbox namespace renders" helm template t "$CHART" -f "$(vals runnerinside '.scopes.sbx-beta.runner = {"service_account": "runner", "namespace": "sbx-alpha-none"}')"
}
