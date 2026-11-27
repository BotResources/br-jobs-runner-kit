#!/usr/bin/env bash

keeper_map() {
    jq -r '.[] | select(.kind == "ConfigMap" and .metadata.namespace == "sbx-system" and .metadata.name == "br-sandbox-keeper-config") | .data["config.json"]' "$WORK/$1.json"
}

keeper_doc() {
    keeper_map "$1" | jq -r "$2"
}

check_keeper_document() {
    section "the keeper document"
    t "the keeper ConfigMap holds one JSON document under config.json" test "$(keeper_map keeper | jq -r 'type')" = object
    t "the keeper settings reach the top level of the keeper document" test "$(keeper_doc keeper '.scale_down_cooldown_seconds')" = 120
    t "the keeper document has no nested settings key" test "$(keeper_doc keeper 'has("settings")')" = false
    t "the system namespace holds no ConfigMap br-sandbox-config when the keeper is on" jqt keeper '[.[] | select(.kind == "ConfigMap" and .metadata.namespace == "sbx-system" and .metadata.name == "br-sandbox-config")] | length == 0'
    t "without the keeper the system namespace holds no keeper or sandbox config" jqt ex '[.[] | select(.kind == "ConfigMap" and .metadata.namespace == "sbx-system" and (.metadata.name | IN("br-sandbox-config", "br-sandbox-keeper-config")))] | length == 0'
    t "the keeper reads config.json from the mounted ConfigMap" jqt keeper '.[] | select(.kind == "Deployment" and .metadata.name == "br-sandbox-keeper") | .spec.template.spec as $pod | ($pod.containers[0].env[] | select(.name == "BR_SANDBOX_CONFIG") | .value) == "/etc/br-sandbox/config.json" and ($pod.volumes[] | select(.name == "config") | .configMap.name) == "br-sandbox-keeper-config"'
    t "the store credential is in the two variables and not in the document" test "$(keeper_map keeper | grep -ciE 'secret|access_key|credential')" = 0
    t "the label prefix and the managed selector are the label contract" test "$(keeper_doc keeper '[.label_prefix, .managed_namespace_selector] | join(" ")')" = "br.sandbox br.sandbox/managed=true"
    t "the store keys are the values of store" test "$(keeper_doc keeper '[.store_bucket, .store_endpoint, .store_region, .store_path_style] | join(" ")')" = "saves https://store.example.test example-1 true"
    t "the idle time and the idle threshold are the one value of each pool entry" test "$(keeper_doc keeper '[.pools[] | [.idle_ttl_seconds, .idle_keep_min_state_bytes] | join("/")] | unique | join(" ")')" = "120/10000000"
    t "one pool entry for each ReplicaSet, with its namespace, name and pool id" test "$(keeper_doc keeper '[.pools[] | [.namespace, .replicaset, .id] | join(" ")] | sort | join(",")')" = "sbx-alpha-allow warm-alpha-allow alpha-allow,sbx-alpha-full warm-alpha-full alpha-full,sbx-alpha-none warm-alpha-none alpha-none,sbx-beta-none warm-beta-none beta-none"
    t "each pool entry names a ReplicaSet and a namespace that the render holds" test "$(jq -r --argjson pools "$(keeper_doc keeper '.pools')" '[.[] | select(.kind == "ReplicaSet") | .metadata.namespace + "/" + .metadata.name] | sort == ([$pools[] | .namespace + "/" + .replicaset] | sort)' "$WORK/keeper.json")" = true
    t "the floor and the maximum of a pool are min_replicas and max_replicas" test "$(keeper_doc keeper '.pools[] | select(.id == "alpha-full") | [.min_replicas, .max_replicas] | join("/")')" = "1/10"
    t "the lease keys are the values of operator.lease" test "$(keeper_doc keeper '[.lease_duration_seconds, .renew_deadline_seconds, .retry_period_seconds] | join("/")')" = "15/10/2"
    t "the tombstone age is save.tombstone_ttl_days in seconds" test "$(keeper_doc keeper '.tombstone_ttl_seconds')" = 31536000
    t "the warm pod age is the value warm_max_age_seconds" test "$(keeper_doc keeper '.warm_max_age_seconds')" = 1800
}

check_keeper_port() {
    section "the keeper port"
    t "the listen address is all addresses on operator.port" test "$(keeper_doc keeper '.listen_address')" = "0.0.0.0:8080"
    t "the container port, the probes and the listen address use one port" jqt keeper '.[] | select(.kind == "Deployment" and .metadata.name == "br-sandbox-keeper") | .spec.template.spec.containers[0] | .ports == [{"name": "http", "containerPort": 8080, "protocol": "TCP"}] and .livenessProbe.httpGet == {"path": "/livez", "port": "http"} and .readinessProbe.httpGet == {"path": "/readyz", "port": "http"}'
    render port_set -f "$EXAMPLE" -f "$OPERATOR_VALUES" --set operator.port=9100
    t "another operator.port moves the listen address and the container port" test "$(keeper_doc port_set '.listen_address')" = "0.0.0.0:9100" -a "$(jqv port_set '.[] | select(.kind == "Deployment" and .metadata.name == "br-sandbox-keeper") | .spec.template.spec.containers[0].ports[0].containerPort')" = 9100
    local port
    for port in 0 65536; do
        t "operator.port ${port} fails the schema" test "$(helm template t "$CHART" -f "$EXAMPLE" -f "$OPERATOR_VALUES" --set "operator.port=${port}" 2>&1 | grep -c 'operator')" -ge 1
    done
}

check_keeper_settings() {
    section "the keeper settings"
    local key
    for key in label_prefix managed_namespace_selector store_bucket store_endpoint store_region store_path_style listen_address save_ttl_seconds tombstone_ttl_seconds warm_max_age_seconds lease_duration_seconds renew_deadline_seconds retry_period_seconds pools; do
        fails_with "BR_SANDBOX_KEEPER_SETTING_COMPUTED{key=${key}}" -f "$EXAMPLE" -f "$OPERATOR_VALUES" --set "operator.settings.${key}=1"
    done
    render lease_ns -f "$EXAMPLE" -f "$OPERATOR_VALUES" --set operator.settings.lease_namespace=other
    t "a key that the chart does not compute passes to the document" test "$(keeper_doc lease_ns '.lease_namespace')" = other
    t "the config checksum of the keeper pod is the checksum of the document" test "$(jqv keeper '.[] | select(.kind == "Deployment" and .metadata.name == "br-sandbox-keeper") | .spec.template.metadata.annotations["br.sandbox/keeper-config-sha256"]')" = "$(keeper_map keeper | tr -d '\n' | sha256sum | cut -d' ' -f1)"
    t "a changed setting changes the checksum of the keeper pod" test "$(jqv lease_ns '.[] | select(.kind == "Deployment" and .metadata.name == "br-sandbox-keeper") | .spec.template.metadata.annotations["br.sandbox/keeper-config-sha256"]')" != "$(jqv keeper '.[] | select(.kind == "Deployment" and .metadata.name == "br-sandbox-keeper") | .spec.template.metadata.annotations["br.sandbox/keeper-config-sha256"]')"
}
