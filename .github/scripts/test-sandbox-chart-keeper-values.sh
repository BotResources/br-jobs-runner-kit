#!/usr/bin/env bash

NONDEFAULT_VALUES="$CHART/ci/keeper-nondefault-values.yaml"
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
    jq -r --arg ns "$2" '.[] | select(.kind == "ConfigMap" and .metadata.name == "br-sandbox-config" and .metadata.namespace == $ns) | .data["config.yaml"]' "$WORK/$1.json" | yq -o=json '.' - | flat "$3"
}

check_each_shared_value_reaches_both_documents_as_set() {
    section "a value that the keeper document and the adapter document both hold is one value, set away from its default"
    local row path keeper_expr adapter_expr want
    for row in \
        '.idle_ttl@[.pools[].idle_ttl_seconds] | unique | join(",")@.idle_ttl' \
        '.idle_keep_min_state_bytes@[.pools[].idle_keep_min_state_bytes] | unique | join(",")@.idle_keep_min_state_bytes' \
        '.warm_max_age_seconds@.warm_max_age_seconds@.warm_max_age_seconds' \
        '.save.ttl_seconds@.save_ttl_seconds@.save.ttl_seconds' \
        '.store.endpoint@.store_endpoint@.store.endpoint' \
        '.store.bucket@.store_bucket@.store.bucket' \
        '.store.region@.store_region@.store.region' \
        '.store.path_style@.store_path_style@.store.path_style'; do
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

check_each_pool_value_reaches_both_documents_as_set() {
    section "a pool value is one value in the keeper pool entry and in the adapter document of the namespace, set away from its default"
    local row namespace source pair key keeper_key want
    for row in 'sbx-alpha-allow|.pool' 'sbx-alpha-none|.pool' 'sbx-beta-none|.pool' 'sbx-alpha-full|.clients.alpha.modes.full.pool'; do
        IFS='|' read -r namespace source <<<"$row"
        for pair in floor:min_replicas max_sandboxes:max_replicas refill_seconds:refill_seconds margin:margin; do
            key="${pair%%:*}"
            keeper_key="${pair##*:}"
            want="$(overlay_value "${source}.${key}")"
            t "${namespace}: ${key} is not the default" test "$want" != "$(base_value ".pool.${key}")"
            t "${namespace}: ${key} is the value of its own pool, so a pool with an override differs from the others" test "$want" != "$(overlay_value ".pool.${key}")" -o "$source" = ".pool"
            t "${namespace}: the keeper pool entry holds ${key} as ${keeper_key}" test "$(keeper_json nondefault ".pools[] | select(.namespace == \"${namespace}\") | .${keeper_key}")" = "$want"
            t "${namespace}: the adapter document holds ${key}" test "$(adapter_json nondefault "$namespace" ".pool.${key}")" = "$want"
        done
    done
}

check_the_identity_of_each_pool_is_one_value_in_both_documents() {
    section "the label prefix and the identity of each pool are one value in the keeper document and in the adapter document"
    local namespace id
    for namespace in $SANDBOX_NAMESPACES; do
        id="${namespace#sbx-}"
        t "${namespace}: the label prefix is the same" test "$(keeper_json nondefault '.label_prefix')" = "$(adapter_json nondefault "$namespace" '.label_prefix')"
        t "${namespace}: the pool id is the same" test "$(keeper_json nondefault ".pools[] | select(.namespace == \"${namespace}\") | .id")" = "$(adapter_json nondefault "$namespace" '.pool.pool_id')"
        t "${namespace}: the namespace is the same" test "$(keeper_json nondefault ".pools[] | select(.id == \"${id}\") | .namespace")" = "$(adapter_json nondefault "$namespace" '.pool.namespace')"
        t "${namespace}: the ReplicaSet name is the same, and the render holds that ReplicaSet" test "$(keeper_json nondefault ".pools[] | select(.namespace == \"${namespace}\") | .replicaset")" = "$(adapter_json nondefault "$namespace" '.pool.replicaset')" -a "$(jqv nondefault "[.[] | select(.kind == \"ReplicaSet\" and .metadata.namespace == \"${namespace}\") | .metadata.name] | join(\",\")")" = "$(adapter_json nondefault "$namespace" '.pool.replicaset')"
    done
}

check_the_keeper_only_values_reach_the_keeper_document_as_set() {
    section "a value that only the keeper document holds is set away from its default and reaches the document"
    local pair path key want
    for pair in .keeper.lease.duration_seconds:lease_duration_seconds .keeper.lease.renew_deadline_seconds:renew_deadline_seconds .keeper.lease.retry_period_seconds:retry_period_seconds; do
        path="${pair%%:*}"
        key="${pair##*:}"
        want="$(overlay_value "$path")"
        t "${path} is not the default" test "$want" != "$(base_value "$path")"
        t "${key} is the value of ${path}" test "$(keeper_json nondefault ".${key}")" = "$want"
    done
    want="$(overlay_value '.keeper.port')"
    t ".keeper.port is not the default" test "$want" != "$(base_value '.keeper.port')"
    t "the listen address is all addresses on keeper.port" test "$(keeper_json nondefault '.listen_address')" = "0.0.0.0:${want}"
    want="$(overlay_value '.save.tombstone_ttl_days')"
    t ".save.tombstone_ttl_days is not the default" test "$want" != "$(base_value '.save.tombstone_ttl_days')"
    t "the tombstone age is the days of save.tombstone_ttl_days times 86400" test "$(keeper_json nondefault '.tombstone_ttl_seconds')" = "$((want * 86400))"
    t "the adapter document holds the tombstone age in days, not in seconds" test "$(adapter_json nondefault sbx-alpha-none '.save.tombstone_ttl_days')" = "$want"
}

check_the_managed_selector_is_the_label_of_the_sandbox_namespaces() {
    section "the managed selector of the keeper document matches the label of every sandbox namespace and of no other namespace"
    local name selector key value
    for name in keeper nondefault egress_keeper; do
        selector="$(keeper_json "$name" '.managed_namespace_selector')"
        key="${selector%%=*}"
        value="${selector#*=}"
        t "render ${name}: the selector is a key and a value" test -n "$key" -a -n "$value" -a "$key=$value" = "$selector"
        t "render ${name}: every namespace of a pool entry carries the label ${selector}" jqt "$name" --arg k "$key" --arg v "$value" "(.[] | select(.kind == \"ConfigMap\" and .metadata.name == \"br-sandbox-keeper-config\") | .data[\"config.json\"] | fromjson | [.pools[].namespace]) as \$pools | ([.[] | select(.kind == \"Namespace\" and (.metadata.name | IN(\$pools[])))] | length) == (\$pools | length) and ([.[] | select(.kind == \"Namespace\" and (.metadata.name | IN(\$pools[])))] | all(.metadata.labels[\$k] == \$v))"
        t "render ${name}: no namespace outside the pool entries carries the label ${selector}" jqt "$name" --arg k "$key" --arg v "$value" "(.[] | select(.kind == \"ConfigMap\" and .metadata.name == \"br-sandbox-keeper-config\") | .data[\"config.json\"] | fromjson | [.pools[].namespace]) as \$pools | [.[] | select(.kind == \"Namespace\" and (.metadata.name | IN(\$pools[]) | not))] | all(.metadata.labels[\$k] != \$v)"
        t "render ${name}: the system namespace is a namespace that the selector does not match" jqt "$name" --arg k "$key" --arg v "$value" '[.[] | select(.kind == "Namespace" and .metadata.name == "sbx-system")] | length == 1 and all(.metadata.labels[$k] != $v)'
        t "render ${name}: every admission binding selects the namespaces by the same label" jqt "$name" --arg k "$key" --arg v "$value" '[.[] | select(.kind == "ValidatingAdmissionPolicyBinding")] | length > 0 and all(.spec.matchResources.namespaceSelector.matchLabels[$k] == $v)'
    done
    t "the egress render holds egress namespaces that the selector does not match" jqt egress_keeper '[.[] | select(.kind == "Namespace" and (.metadata.name | endswith("-egress")))] | length > 0'
}
