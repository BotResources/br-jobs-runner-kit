#!/usr/bin/env bash

NONDEFAULT_VALUES="$CHART/ci/operator-nondefault-values.yaml"
SANDBOX_NAMESPACES="sbx-alpha-allow sbx-alpha-full sbx-alpha-none sbx-beta-none"

flat() {
    jq -r "$1"
}

overlay_value() {
    yq -o=json "$1" "$NONDEFAULT_VALUES" | flat '.'
}

base_value() {
    yq ea 'select(fileIndex == 0) * select(fileIndex == 1)' "$CHART/values.yaml" "$EXAMPLE" | yq -o=json "$1" - | flat '.'
}

keeper_json() {
    keeper_map "$1" | flat "$2"
}

adapter_json() {
    jq -r --arg ns "$2" '.[] | select(.kind == "ConfigMap" and .metadata.name == "br-sandbox-config" and .metadata.namespace == $ns) | .data["config.json"]' "$WORK/$1.json" | flat "$3"
}

check_each_shared_value_reaches_both_documents_as_set() {
    section "a value that the keeper document and the adapter document both hold is one value, set away from its default"
    local row path keeper_expr adapter_expr want
    for row in \
        '.idle_ttl@.idle_ttl_seconds@.idle_ttl_seconds' \
        '.idle_keep_min_state_bytes@.idle_keep_min_state_bytes@.idle_keep_min_state_bytes' \
        '.save.ttl_seconds@.save_ttl_seconds@.save_ttl_seconds' \
        '.store.endpoint@.store_endpoint@.store_endpoint' \
        '.store.bucket@.store_bucket@.store_bucket' \
        '.store.region@.store_region@.store_region' \
        '.store.path_style@.store_path_style@.store_path_style'; do
        IFS='@' read -r path keeper_expr adapter_expr <<<"$row"
        want="$(overlay_value "$path")"
        t "${path}: the overlay value is not the value of the defaults or of the example" test "$want" != "$(base_value "$path")"
        t "${path}: the keeper document holds the overlay value" test "$(keeper_json nondefault "$keeper_expr")" = "$want"
        local namespace
        for namespace in $SANDBOX_NAMESPACES; do
            t "${path}: the adapter document of ${namespace} holds the overlay value" test "$(adapter_json nondefault "$namespace" "$adapter_expr")" = "$want"
        done
    done
}

check_each_platform_constant_reaches_the_pod_template_and_the_quota_as_set() {
    section "limits and pool values reach the pod template and the quota of every namespace as set, and no adapter document holds them"
    local namespace
    t "limits.pods is not the default" test "$(overlay_value '.limits.pods')" != "$(base_value '.limits.pods')"
    t "pool.terminationGracePeriodSeconds is not the default" test "$(overlay_value '.pool.terminationGracePeriodSeconds')" != "$(base_value '.pool.terminationGracePeriodSeconds')"
    t "limits.size.cpu.limit is not the default" test "$(overlay_value '.limits.size.cpu.limit')" != "$(base_value '.limits.size.cpu.limit')"
    for namespace in $SANDBOX_NAMESPACES; do
        t "${namespace}: the quota holds limits.pods plus the extra pods" test "$(jqv nondefault ".[] | select(.kind == \"ResourceQuota\" and .metadata.namespace == \"${namespace}\") | .spec.hard.pods")" = "$(($(overlay_value '.limits.pods') + 4))"
        t "${namespace}: the pod template holds the grace period, the node selector and the size as set" jqt nondefault ".[] | select(.kind == \"ConfigMap\" and .metadata.name == \"br-sandbox-pod-template\" and .metadata.namespace == \"${namespace}\") | (.data[\"pod-template.json\"] | fromjson.spec) | .terminationGracePeriodSeconds == 5 and .nodeSelector == {\"pool\": \"sandbox\"} and .containers[0].resources.limits.cpu == \"3\" and .containers[0].resources.limits.memory == \"4Gi\""
        t "${namespace}: the adapter document holds no pool key, no warm age and no tombstone age" test "$(adapter_json nondefault "$namespace" '[has("floor"), has("max_sandboxes"), has("refill_seconds"), has("margin"), has("warm_max_age_seconds"), has("tombstone_ttl_seconds"), has("pool"), has("save"), has("limits"), has("pods")] | any')" = false
    done
}

check_the_identity_of_each_pool_is_one_value_in_both_documents() {
    section "the label prefix and the pool id are one value in the keeper document, the adapter document and the pod template"
    local namespace
    for namespace in $SANDBOX_NAMESPACES; do
        t "${namespace}: the label prefix is the same" test "$(keeper_json nondefault '.label_prefix')" = "$(adapter_json nondefault "$namespace" '.label_prefix')"
        t "${namespace}: the pool id of the adapter document is the name of the namespace" test "$(adapter_json nondefault "$namespace" '.pool_id')" = "$namespace"
        t "${namespace}: the pool id label of the pod template is the name of the namespace" test "$(jqv nondefault ".[] | select(.kind == \"ConfigMap\" and .metadata.name == \"br-sandbox-pod-template\" and .metadata.namespace == \"${namespace}\") | (.data[\"pod-template.json\"] | fromjson).metadata.labels[\"br.sandbox/pool-id\"]")" = "$namespace"
    done
}

check_the_keeper_only_values_reach_the_keeper_document_as_set() {
    section "a value that only the keeper document holds is set away from its default and reaches the document"
    local pair path key want
    want="$(overlay_value '.warm_max_age_seconds')"
    t ".warm_max_age_seconds is not the default" test "$want" != "$(base_value '.warm_max_age_seconds')"
    t "the keeper document holds the warm age as set" test "$(keeper_json nondefault '.warm_max_age_seconds')" = "$want"
    for pair in .operator.lease.duration_seconds:lease_duration_seconds .operator.lease.renew_deadline_seconds:renew_deadline_seconds .operator.lease.retry_period_seconds:retry_period_seconds; do
        path="${pair%%:*}"
        key="${pair##*:}"
        want="$(overlay_value "$path")"
        t "${path} is not the default" test "$want" != "$(base_value "$path")"
        t "${key} is the value of ${path}" test "$(keeper_json nondefault ".${key}")" = "$want"
    done
    want="$(overlay_value '.operator.port')"
    t ".operator.port is not the default" test "$want" != "$(base_value '.operator.port')"
    t "the listen address is all addresses on operator.port" test "$(keeper_json nondefault '.listen_address')" = "0.0.0.0:${want}"
    want="$(overlay_value '.save.tombstone_ttl_days')"
    t ".save.tombstone_ttl_days is not the default" test "$want" != "$(base_value '.save.tombstone_ttl_days')"
    t "the tombstone age is the days of save.tombstone_ttl_days times 86400" test "$(keeper_json nondefault '.tombstone_ttl_seconds')" = "$((want * 86400))"
    t "the adapter document holds no tombstone age" test "$(adapter_json nondefault sbx-alpha-none '[has("tombstone_ttl_seconds"), has("tombstone_ttl_days"), has("warm_max_age_seconds")] | any')" = false
}

check_the_managed_selector_is_the_label_of_the_sandbox_namespaces() {
    section "the managed selector of the keeper document matches the label of every sandbox namespace and of no other namespace"
    local name selector key value
    for name in keeper nondefault egress_keeper; do
        selector="$(keeper_json "$name" '.managed_namespace_selector')"
        key="${selector%%=*}"
        value="${selector#*=}"
        t "render ${name}: the selector is a key and a value" test -n "$key" -a -n "$value" -a "$key=$value" = "$selector"
        t "render ${name}: every namespace with a pod template carries the label ${selector}" jqt "$name" --arg k "$key" --arg v "$value" "[.[] | select(.kind == \"ConfigMap\" and .metadata.name == \"br-sandbox-pod-template\") | .metadata.namespace] as \$pools | ([.[] | select(.kind == \"Namespace\" and (.metadata.name | IN(\$pools[])))] | length) == (\$pools | length) and ([.[] | select(.kind == \"Namespace\" and (.metadata.name | IN(\$pools[])))] | all(.metadata.labels[\$k] == \$v))"
        t "render ${name}: no namespace without a pod template carries the label ${selector}" jqt "$name" --arg k "$key" --arg v "$value" "[.[] | select(.kind == \"ConfigMap\" and .metadata.name == \"br-sandbox-pod-template\") | .metadata.namespace] as \$pools | [.[] | select(.kind == \"Namespace\" and (.metadata.name | IN(\$pools[]) | not))] | all(.metadata.labels[\$k] != \$v)"
        t "render ${name}: the system namespace is a namespace that the selector does not match" jqt "$name" --arg k "$key" --arg v "$value" '[.[] | select(.kind == "Namespace" and .metadata.name == "sbx-system")] | length == 1 and all(.metadata.labels[$k] != $v)'
        t "render ${name}: every admission binding selects the namespaces by the same label" jqt "$name" --arg k "$key" --arg v "$value" '[.[] | select(.kind == "ValidatingAdmissionPolicyBinding")] | length > 0 and all(.spec.matchResources.namespaceSelector.matchLabels[$k] == $v)'
    done
    t "the egress render holds egress namespaces that the selector does not match" jqt egress_keeper '[.[] | select(.kind == "Namespace" and (.metadata.name | endswith("-egress")))] | length > 0'
}
