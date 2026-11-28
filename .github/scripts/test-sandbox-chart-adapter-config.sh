#!/usr/bin/env bash

adapter_map() {
    jq -r --arg ns "$2" '.[] | select(.kind == "ConfigMap" and .metadata.namespace == $ns and .metadata.name == "br-sandbox-config") | .data["config.json"]' "$WORK/$1.json"
}

adapter_doc() {
    adapter_map "$1" "$2" | jq -r "$3"
}

check_adapter_document() {
    section "the adapter document"
    local namespace
    for namespace in sbx-alpha-allow sbx-alpha-full sbx-alpha-none sbx-beta-none; do
        t "the ConfigMap of ${namespace} holds one JSON object under config.json and no other key" test "$(adapter_map cred "$namespace" | jq -r 'type')" = object -a "$(jq -r --arg ns "$namespace" '[.[] | select(.kind == "ConfigMap" and .metadata.namespace == $ns and .metadata.name == "br-sandbox-config") | .data | keys | join(",")] | join(" ")' "$WORK/cred.json")" = config.json
    done
    t "the document of a namespace names the namespace, the client, the pool id and the mode of its entry" test "$(adapter_doc cred sbx-alpha-full '[.namespace, .client_id, .pool_id, .network_mode] | join(" ")')" = "sbx-alpha-full alpha sbx-alpha-full full"
    t "the keys are flat: no value of the document is an object" test "$(adapter_doc cred sbx-alpha-none '[.[] | select(type == "object")] | length')" = 0
    t "the document holds no key of the old nested shape" test "$(adapter_doc cred sbx-alpha-none '[has("pool"), has("tools"), has("save"), has("store"), has("idle_ttl"), has("mode"), has("client")] | any')" = false
    t "the label prefix and the store keys are the same values in the keeper document and the adapter document" test "$(jq -r '.[] | select(.kind == "ConfigMap" and .metadata.namespace == "sbx-system" and .metadata.name == "br-sandbox-keeper-config") | .data["config.json"] | fromjson | [.label_prefix, .store_endpoint, .store_bucket, .store_region, .store_path_style] | join(" ")' "$WORK/keeper.json")" = "$(adapter_doc keeper sbx-alpha-none '[.label_prefix, .store_endpoint, .store_bucket, .store_region, .store_path_style] | join(" ")')"
    t "the document holds the values of the example" test "$(adapter_doc cred sbx-alpha-none '[.label_prefix, .save_ttl_seconds, .idle_ttl_seconds, .idle_keep_min_state_bytes, .store_endpoint, .store_bucket, .store_region, .store_path_style] | join(" ")')" = "br.sandbox 60 120 10000000 https://store.example.test saves example-1 true"
    t "the idle time and the idle threshold are one value for the keeper and the adapter" test "$(jq -r '.[] | select(.kind == "ConfigMap" and .metadata.namespace == "sbx-system" and .metadata.name == "br-sandbox-keeper-config") | .data["config.json"] | fromjson | [.idle_ttl_seconds, .idle_keep_min_state_bytes] | join("/")' "$WORK/keeper.json")" = "$(adapter_doc keeper sbx-alpha-none '[.idle_ttl_seconds, .idle_keep_min_state_bytes] | join("/")')"
    t "the restore timeout, the save scope and the container names are the values of the chart" test "$(adapter_doc cred sbx-alpha-none '[.restore_timeout_seconds, (.save_scope_paths | join(",")), .seed_container, .tools_path, .seed_tools_path, .seed_root] | join(" ")')" = "30 /work,/home/agent seed /opt/sbx/bin/bsdtar /sbx/bin/bsdtar /skills"
    t "the agent container of the document is the name of the agent container of the pool" test "$(adapter_doc cred sbx-alpha-none '.agent_container')" = "$(jqv cred '.[] | select(.kind == "ConfigMap" and .metadata.name == "br-sandbox-pod-template" and .metadata.namespace == "sbx-alpha-none") | (.data["pod-template.json"] | fromjson).spec.containers[0].name')"
    t "the seed container, the seed root and the seed tools path of the document are those of the pool pod" jqt cred '.[] | select(.kind == "ConfigMap" and .metadata.name == "br-sandbox-pod-template" and .metadata.namespace == "sbx-alpha-none") | (.data["pod-template.json"] | fromjson).spec as $pod | ($pod.containers[1].name == "seed") and ($pod.containers[1].volumeMounts[0].mountPath == "/skills") and ($pod.initContainers[0].command[0] == "/sbx/bin/bsdtar") and ($pod.containers[0].volumeMounts[] | select(.name == "tools") | .mountPath == "/opt/sbx")'
    t "the allow list of the document is the domains then the cidrs of the allow value of the mode" test "$(adapter_doc cred sbx-alpha-allow '.allowlist | join(",")')" = "git.example.test,*.packages.example.test,192.0.2.0/24"
    t "the allow list of the document is the allow list of the proxy of the namespace" test "$(adapter_doc cred sbx-alpha-allow '.allowlist | sort | join(",")')" = "$(jq -r '.[] | select(.kind == "ConfigMap" and .metadata.name == "br-sandbox-proxy-config" and .metadata.namespace == "sbx-alpha-allow-egress") | .data["proxy.yaml"]' "$WORK/cred.json" | yq '.transforms[] | select(.name == "allowlist") | .config | (.domains + .cidrs) | sort | join(",")')"
    t "a mode with no allow list renders no allowlist key" test "$(adapter_doc cred sbx-alpha-full 'has("allowlist")')" = false -a "$(adapter_doc cred sbx-alpha-none 'has("allowlist")')" = false -a "$(adapter_doc ex sbx-alpha-allow 'has("allowlist")')" = false
    t "with egress enabled the mode allow document holds an allowlist; with egress disabled it holds none" test "$(jq -r '[.[] | select(.kind == "ConfigMap" and .metadata.name == "br-sandbox-config") | .data["config.json"] | fromjson | select(.network_mode == "allow" and (has("allowlist") | not))] | length' "$WORK/cred.json")" = 0 -a "$(jq -r '[.[] | select(.kind == "ConfigMap" and .metadata.name == "br-sandbox-config") | .data["config.json"] | fromjson | select(.network_mode == "allow" and (has("allowlist") | not))] | length' "$WORK/ex.json")" = 1
    t "a document holds none of the keys that stay out of it" test "$(adapter_map cred sbx-alpha-allow | grep -ciE 'store_credentials|access_key|secret|kube_context|credentials_secret')" = 0
    t "a key that the chart has no value for is left to the adapter default" test "$(adapter_doc cred sbx-alpha-none '[has("save_zstd_level"), has("open_timeout_seconds"), has("save_timeout_seconds"), has("claim_max_attempts"), has("cold_create")] | any')" = false
    t "the config.yaml key is not rendered in any sandbox namespace" jqt cred '[.[] | select(.kind == "ConfigMap" and .metadata.name == "br-sandbox-config") | .data | has("config.yaml")] | any | not'
}

check_adapter_document_follows_the_values() {
    section "the adapter document follows the values"
    render adapter_values -f "$EXAMPLE" --set save.ttl_seconds=90 --set restore_timeout_seconds=45 --set idle_ttl=300 --set idle_keep_min_state_bytes=5 --set tools.mount_path=/opt/other --set tools.seed_container.name=seeder --set 'save.paths={/work}' --set store.path_style=false
    t "each value reaches its key" test "$(adapter_doc adapter_values sbx-alpha-none '[.save_ttl_seconds, .restore_timeout_seconds, .idle_ttl_seconds, .idle_keep_min_state_bytes, .tools_path, .seed_container, (.save_scope_paths | join(",")), .store_path_style] | join(" ")')" = "90 45 300 5 /opt/other/bin/bsdtar seeder /work false"
    t "the pod of the same render holds the same tools mount path and seed container name" jqt adapter_values '.[] | select(.kind == "ConfigMap" and .metadata.name == "br-sandbox-pod-template" and .metadata.namespace == "sbx-alpha-none") | (.data["pod-template.json"] | fromjson).spec | (.containers[1].name == "seeder") and (.containers[0].volumeMounts[] | select(.name == "tools") | .mountPath == "/opt/other")'
}
