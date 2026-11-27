#!/usr/bin/env bash

TEMPLATE_RENDERS="ex fence mirror keeper nondefault egress_keeper two keeperegress rights_five"
GOLDEN="$CHART/ci/golden/pod-template.json"
GOLDEN_FILTER='del(.metadata.labels["br.sandbox/pool-id"]) | del(.spec.containers[0].image, .spec.containers[0].resources, .spec.containers[0].imagePullPolicy)'

template_of() {
    jq -c --arg ns "$2" '.[] | select(.kind == "ConfigMap" and .metadata.name == "br-sandbox-pod-template" and .metadata.namespace == $ns) | .data["pod-template.json"] | fromjson' "$WORK/$1.json"
}

template_namespaces() {
    jq -r '[.[] | select(.kind == "ConfigMap" and .metadata.name == "br-sandbox-pod-template") | .metadata.namespace] | sort | .[]' "$WORK/$1.json"
}

template_of_every_namespace_is() {
    local render="$1" filter="$2" ns
    for ns in $(template_namespaces "$render"); do
        [[ "$(template_of "$render" "$ns" | jq -r "$filter")" == true ]] || return 1
    done
}

golden_equals_the_template_of() {
    local ns="$1" want got
    want="$(jq -c --arg ns "$ns" '.[$ns]' "$GOLDEN")"
    got="$(template_of ex "$ns" | jq -c "${GOLDEN_FILTER}")"
    [[ "$want" == "$got" && "$want" != null ]]
}

check_each_namespace_holds_the_pod_template() {
    section "the pod template: one ConfigMap per namespace with the key pod-template.json"
    local name
    for name in $TEMPLATE_RENDERS; do
        t "the render ${name} holds the template of every namespace of its entries and no other key" jqt "$name" '([.[] | select(.kind == "ConfigMap" and .metadata.name == "br-sandbox-config") | .metadata.namespace] | sort) as $adapter | ([.[] | select(.kind == "ConfigMap" and .metadata.name == "br-sandbox-pod-template") | select((.data | keys) == ["pod-template.json"]) | .metadata.namespace] | sort) == $adapter and ($adapter | length) > 0'
    done
    t "the template is JSON with a metadata and a spec only" template_of_every_namespace_is ex '(keys | sort) == ["metadata", "spec"]'
    t "the template labels are the role, the pool and the pool id, which is the namespace name" jqt ex '[.[] | select(.kind == "ConfigMap" and .metadata.name == "br-sandbox-pod-template") | (.data["pod-template.json"] | fromjson | .metadata.labels) as $labels | .metadata.namespace as $ns | $labels == {"br.sandbox/role": "sandbox", "br.sandbox/pool": "warm", "br.sandbox/pool-id": $ns}] | all'
    t "the render holds no ReplicaSet that carries a template" jqt ex '[.[] | select(.kind == "ReplicaSet")] | length == 0'
}

check_the_template_equals_the_golden_of_the_previous_release() {
    section "the template equals the pod template of the previous minor but for the agent image, the agent resources and the pull policy"
    local ns
    for ns in sbx-alpha-allow sbx-alpha-full sbx-alpha-none sbx-beta-none; do
        t "the template of ${ns} equals the golden once the pool id, the agent image, the agent resources and the pull policy are removed" golden_equals_the_template_of "$ns"
    done
    t "the golden holds the four namespaces of the example" test "$(jq -c 'keys' "$GOLDEN")" = '["sbx-alpha-allow","sbx-alpha-full","sbx-alpha-none","sbx-beta-none"]'
    t "a changed seccomp type of the seed container is not equal to the golden" test "$(template_of ex sbx-alpha-none | jq -c '.spec.containers[1].securityContext.seccompProfile = {"type": "Unconfined"}' | jq -c "${GOLDEN_FILTER}")" != "$(jq -c '.["sbx-alpha-none"]' "$GOLDEN")"
}

check_the_template_passes_the_policy_model() {
    section "the pod that the keeper makes from the template passes the model of the admission policy"
    local ns
    for ns in sbx-alpha-allow sbx-alpha-full sbx-alpha-none sbx-beta-none; do
        t "the pod made from the template of ${ns}, with the image and the size of its pool, passes the model" test "$(model_result "$ns" '.')" = true
    done
    t "the agent image of the template is the tools image of the platform" template_of_every_namespace_is ex '.spec.containers[0].image == .spec.containers[1].image and .spec.containers[0].image == .spec.initContainers[0].image'
    t "the model refuses the pod made from the template with an agent image of another repository" test "$(model_result sbx-alpha-none '.spec.containers[0].image = "other.example.test/agent:1"')" = false
}

check_nothing_mounts_or_reads_the_template() {
    section "no pod mounts the template, the runner role does not reach it, the template holds no secret reference"
    local name
    for name in $TEMPLATE_RENDERS; do
        t "the render ${name}: no pod, template or Deployment mounts or names the ConfigMap br-sandbox-pod-template" test "$(jq '[.[] | select(.kind == "Deployment" or .kind == "Pod" or .kind == "ReplicaSet") | .. | objects | select(.configMap.name? == "br-sandbox-pod-template" or .configMapKeyRef.name? == "br-sandbox-pod-template")] | length' "$WORK/${name}.json")" = 0
    done
    t "no pod template mounts a ConfigMap named br-sandbox-pod-template" template_of_every_namespace_is ex '[.spec.volumes[]? | select(.configMap.name? == "br-sandbox-pod-template")] | length == 0'
    t "the runner ClusterRole has no rule on configmaps other than br-sandbox-config" jqt ex '[.[] | select(.kind == "ClusterRole" and .metadata.name == "br-sandbox-runner") | .rules[] | select(.resources | index("configmaps")) | .resourceNames] | all(. == ["br-sandbox-config"])'
    t "no Role and no ClusterRole but the controller one names the ConfigMap br-sandbox-pod-template or reads every ConfigMap" jqt ex '[.[] | select(.kind == "Role" or (.kind == "ClusterRole" and .metadata.name != "br-sandbox-controller")) | .rules[] | select(.resources | index("configmaps")) | select(((.resourceNames // []) | index("br-sandbox-pod-template")) or ((.resourceNames // []) | length == 0))] | length == 0'
    t "no template holds a secretKeyRef or a secret volume" template_of_every_namespace_is ex '[.. | objects | select(has("secretKeyRef") or has("secret"))] | length == 0'
}

check_the_container_is_named_agent() {
    section "the adapter document, the keeper document and the template name the container agent"
    local ns
    for ns in sbx-alpha-allow sbx-alpha-full sbx-alpha-none sbx-beta-none; do
        t "${ns}: the adapter document names the container agent" test "$(adapter_doc ex "$ns" '.agent_container')" = agent
        t "${ns}: the first container of the template is named agent" test "$(template_of ex "$ns" | jq -r '.spec.containers[0].name')" = agent
    done
    t "the keeper document names the container agent" test "$(keeper_doc keeper '.agent_container')" = agent
    fails_with "BR_SANDBOX_KEEPER_SETTING_COMPUTED{key=agent_container}" -f "$EXAMPLE" --set operator.settings.agent_container=other
}

check_the_fixed_names_are_rendered() {
    section "the fixed names are rendered"
    local entry
    for entry in NetworkPolicy/br-sandbox-default-deny ResourceQuota/br-sandbox-quota LimitRange/br-sandbox-limits ConfigMap/br-sandbox-pod-template ConfigMap/br-sandbox-config RoleBinding/br-sandbox-keeper RoleBinding/br-sandbox-runner ServiceAccount/sandbox; do
        t "every namespace of the example holds ${entry}" jqt ex --arg entry "$entry" '[.[] | select(.kind == "ConfigMap" and .metadata.name == "br-sandbox-pod-template") | .metadata.namespace] as $namespaces | ([.[] | select((.kind + "/" + .metadata.name) == $entry) | .metadata.namespace] | unique) as $holders | ($namespaces | length) > 0 and ($namespaces - $holders | length) == 0'
    done
    t "the example holds the controller ClusterRole, the runner ClusterRole, the keeper Deployment and the keeper ServiceAccount under their fixed names" jqt ex '[.[] | .kind + "/" + .metadata.name] | (index("ClusterRole/br-sandbox-controller") != null) and (index("ClusterRole/br-sandbox-runner") != null) and (index("Deployment/br-sandbox-keeper") != null) and (index("ServiceAccount/br-sandbox-keeper") != null)'
    t "every NetworkPolicy of the example is named with the prefix br-sandbox-" jqt ex '[.[] | select(.kind == "NetworkPolicy") | .metadata.name] | all(startswith("br-sandbox-"))'
    t "the policies of a managed namespace belong to the closed list" jqt ex '[.[] | select(.kind == "ConfigMap" and .metadata.name == "br-sandbox-pod-template") | .metadata.namespace] as $managed | [.[] | select(.kind == "NetworkPolicy" and (.metadata.namespace | IN($managed[]))) | .metadata.name] | length > 0 and all(IN("br-sandbox-default-deny", "br-sandbox-egress-allow", "br-sandbox-egress-full"))'
}

check_the_keeper_document_holds_the_keeper_wide_keys() {
    section "the keeper document holds the keeper-wide keys and no pools"
    local key
    for key in label_prefix managed_namespace_selector store_endpoint store_bucket store_region store_path_style save_ttl_seconds tombstone_ttl_seconds warm_max_age_seconds idle_ttl_seconds idle_keep_min_state_bytes agent_container lease_duration_seconds renew_deadline_seconds retry_period_seconds listen_address; do
        t "the keeper document holds the key ${key}" test "$(keeper_doc keeper "has(\"${key}\")")" = true
    done
    t "the keeper document has no key pools" test "$(keeper_doc keeper 'has("pools")')" = false
    t "the keeper document has no key naming a replica set or a pool size" test "$(keeper_doc keeper '[keys[] | select(test("pool|floor|margin|refill|replica"))] | length')" = 0
}

check_the_agent_pull_policy_is_always() {
    section "the agent container pulls with Always whatever the form of the reference"
    local name
    for name in $TEMPLATE_RENDERS; do
        t "the render ${name}: the agent container of each template has imagePullPolicy Always" test "$(jq '[.[] | select(.kind == "ConfigMap" and .metadata.name == "br-sandbox-pod-template") | .data["pod-template.json"] | fromjson | .spec.containers[0].imagePullPolicy] | (length > 0) and all(. == "Always")' "$WORK/${name}.json")" = true
    done
    t "the seed container and the init container keep the digest rule" template_of_every_namespace_is ex '.spec.containers[1].imagePullPolicy == "IfNotPresent" and .spec.initContainers[0].imagePullPolicy == "IfNotPresent"'
    render pullpolicy -f "$(vals pullpolicy '.image_pull_policy = "IfNotPresent"')"
    t "the global value image_pull_policy is the only override of the agent policy" test "$(template_of pullpolicy sbx-alpha-none | jq -r '.spec.containers[0].imagePullPolicy')" = IfNotPresent
}
