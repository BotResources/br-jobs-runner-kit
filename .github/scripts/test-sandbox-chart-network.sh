#!/usr/bin/env bash

NP='.[] | select(.kind == "NetworkPolicy")'

policy_names() {
    jqv ex "[${NP} | select(.metadata.namespace == \"$1\") | .metadata.name] | sort | join(\",\")"
}

check_network_policy_none_has_deny_only() {
    section "network policy none: br-sandbox-default-deny only, no resolver"
    t "none has exactly the policy br-sandbox-default-deny" test "$(policy_names sbx-alpha-none)" = "br-sandbox-default-deny"
    t "br-sandbox-default-deny selects every pod and both directions" jqt ex "${NP} | select(.metadata.namespace == \"sbx-alpha-none\") | select(.spec.podSelector == {} and (.spec.policyTypes | sort) == [\"Egress\", \"Ingress\"] and (.spec | has(\"egress\") | not) and (.spec | has(\"ingress\") | not))"
    t "none pods use dnsPolicy None and nameserver 127.0.0.1" jqt ex '[.[] | select(.kind == "ConfigMap" and .metadata.name == "br-sandbox-pod-template" and .metadata.namespace == "sbx-alpha-none") | (.data["pod-template.json"] | fromjson).spec | select(.dnsPolicy == "None" and .dnsConfig.nameservers == ["127.0.0.1"])] | length == 1'
}

check_network_policy_allow_reaches_only_the_proxy() {
    section "network policy allow: only the proxy pods of the egress namespace, TCP on the proxy port"
    t "allow has br-sandbox-default-deny and br-sandbox-egress-allow" test "$(policy_names sbx-alpha-allow)" = "br-sandbox-default-deny,br-sandbox-egress-allow"
    t "br-sandbox-egress-allow has one rule: proxy pods of the egress namespace in the same item, TCP 8888, nothing else" jqt ex "${NP} | select(.metadata.name == \"br-sandbox-egress-allow\") | select(.spec.policyTypes == [\"Egress\"] and (.spec.egress | length) == 1 and .spec.egress[0].to == [{\"namespaceSelector\": {\"matchLabels\": {\"kubernetes.io/metadata.name\": \"sbx-alpha-allow-egress\"}}, \"podSelector\": {\"matchLabels\": {\"br.sandbox/role\": \"proxy\"}}}] and .spec.egress[0].ports == [{\"protocol\": \"TCP\", \"port\": 8888}])"
    t "br-sandbox-egress-allow selects the sandbox pods by role, not by pool" jqt ex "${NP} | select(.metadata.name == \"br-sandbox-egress-allow\") | .spec.podSelector == {\"matchLabels\": {\"br.sandbox/role\": \"sandbox\"}}"
    render port -f "$EXAMPLE" --set egress.proxy.port=9999
    t "the proxy port is a value" jqt port '.[] | select(.kind == "NetworkPolicy" and .metadata.name == "br-sandbox-egress-allow") | .spec.egress[0].ports[0].port == 9999'
}

check_network_policy_full_opens_public_addresses_only() {
    section "network policy full: public addresses except every internal list, no cluster DNS"
    t "full has br-sandbox-default-deny and br-sandbox-egress-full" test "$(policy_names sbx-alpha-full)" = "br-sandbox-default-deny,br-sandbox-egress-full"
    t "single stack: one rule, one ipBlock 0.0.0.0/0, no pod or namespace selector" jqt ex "${NP} | select(.metadata.name == \"br-sandbox-egress-full\") | select((.spec.egress | length) == 1 and (.spec.egress[0].to | length) == 1 and .spec.egress[0].to[0].ipBlock.cidr == \"0.0.0.0/0\" and (.spec.egress[0].to[0] | has(\"podSelector\") or has(\"namespaceSelector\") | not) and (.spec.egress[0] | has(\"ports\") | not))"
    t "full pods resolve through the public resolvers only" jqt ex '.[] | select(.kind == "ConfigMap" and .metadata.name == "br-sandbox-pod-template" and .metadata.namespace == "sbx-alpha-full") | (.data["pod-template.json"] | fromjson).spec | select(.dnsPolicy == "None" and .dnsConfig.nameservers == ["198.51.100.53", "198.51.100.54"])'
    render dual -f "$EXAMPLE" --set network.ipv6=dual_stack
    t "dual stack adds ::/0 without fc00::/7 and fe80::/10" jqt dual '.[] | select(.kind == "NetworkPolicy" and .metadata.name == "br-sandbox-egress-full") | .spec.egress[1].to[0].ipBlock | select(.cidr == "::/0" and (.except | index("fc00::/7")) != null and (.except | index("fe80::/10")) != null)'
}

check_network_policy_mode_is_unknown_or_from_values() {
    section "network policy an unknown mode fails, no template reads the cluster"
    fails_with "BR_SANDBOX_MODE_UNKNOWN{namespace=sbx-alpha-none,mode=weird}" -f "$(vals weird '(.namespaces[] | select(.name == "sbx-alpha-none")).mode = "weird"')"
    t "no template reads cluster state or the environment" test -z "$(grep -rnE 'lookup |\.Capabilities|getHostByName|[ (]env "' "$CHART/templates" || true)"
}

check_one_namespace_per_entry() {
    section "one namespace per entry, br-sandbox-default-deny in each"
    t "four sandbox namespaces, the egress namespace of mode allow and the system namespace" test "$(jqv ex '[.[] | select(.kind == "Namespace") | .metadata.name] | sort | join(",")')" = "sbx-alpha-allow,sbx-alpha-allow-egress,sbx-alpha-full,sbx-alpha-none,sbx-beta-none,sbx-system"
    t "every sandbox namespace has br-sandbox-default-deny on both directions" jqt ex '[.[] | select(.kind == "Namespace" and .metadata.labels["br.sandbox/managed"] == "true") | .metadata.name] as $n | [.[] | select(.kind == "NetworkPolicy" and .metadata.name == "br-sandbox-default-deny" and (.spec.policyTypes | sort) == ["Egress", "Ingress"]) | .metadata.namespace] as $d | ($n - $d) == []'
    t "no policy of a sandbox or system namespace has an ingress rule" jqt ex "[${NP} | select((.metadata.namespace | endswith(\"-egress\") | not) and (.spec | has(\"ingress\")))] | length == 0"
    fails_with "BR_SANDBOX_SHARED_NAMESPACE{first=system,second=sandbox,namespace=sbx-alpha-none}" -f "$(vals shared '.installation.system_namespace = "sbx-alpha-none"')"
}

check_network_check_except_lists_hold_every_internal_range() {
    section "the network check except lists hold internal, pod, service, node, node public and load balancer"
    t "full except list has every configured range" jqt ex '.[] | select(.kind == "NetworkPolicy" and .metadata.name == "br-sandbox-egress-full") | .spec.egress[0].to[0].ipBlock.except as $e | ["10.0.0.0/8", "172.16.0.0/12", "192.168.0.0/16", "100.64.0.0/10", "169.254.0.0/16", "127.0.0.0/8", "10.42.0.0/16", "10.43.0.0/16", "10.0.0.11/32", "10.0.0.12/32", "203.0.113.11/32", "203.0.113.12/32", "203.0.113.20/32"] - $e == []'
    render v6 -f "$EXAMPLE" --set network.ipv6=dual_stack --set 'network.node_public_addresses={203.0.113.11,2001:db8::5}'
    t "an IPv6 node address is excepted in the IPv6 rule only" jqt v6 '.[] | select(.kind == "NetworkPolicy" and .metadata.name == "br-sandbox-egress-full") | (.spec.egress[1].to[0].ipBlock.except | index("2001:db8::5/128")) != null and (.spec.egress[0].to[0].ipBlock.except | index("2001:db8::5/128")) == null'
    fails_with "BR_SANDBOX_MISSING_VALUE{key=network.node_addresses}" -f "$(vals nonode 'del(.network.node_addresses)')"
    fails_with "BR_SANDBOX_MISSING_VALUE{key=network.node_public_addresses}" -f "$(vals nopublic 'del(.network.node_public_addresses)')"
    fails_with "BR_SANDBOX_MISSING_VALUE{key=network.load_balancer_addresses}" -f "$(vals nolb 'del(.network.load_balancer_addresses)')"
    t "an empty list written on purpose renders" render emptylb -f "$(vals emptylb '.network.load_balancer_addresses = []')"
    t "the check job reads every address of the three lists" jqt ex '.[] | select(.kind == "ConfigMap" and .metadata.name == "br-sandbox-netcheck" and .metadata.namespace == "sbx-system") | .data["node-addresses"] | split("\n") | (["10.0.0.11", "203.0.113.11", "203.0.113.20"] - .) == []'
}

check_no_project_value_in_the_chart() {
    section "no host or registry outside the fixtures"
    local allowed='(kubernetes\.io|k8s\.io|kyverno\.io|json-schema\.org|example\.test|cluster\.local|sandbox\.botresources\.ai)$'
    local found
    local default_repository='^      repository: docker\.io/ironsh/iron-proxy$'
    found="$(cd "$CHART" && hosts_of Chart.yaml README.md POLICIES.md STAMPER.md CHANGELOG.md values.schema.json templates/* files/* <(grep -vE "$default_repository" values.yaml) | grep -vE "$allowed" | grep -v '^github\.com$' || true)"
    t "no host in the chart files outside the allowed suffixes" test -z "$found"
    t "no registry host in the chart files" test -z "$(cd "$CHART" && { grep -rniE "ghcr|docker\.io|quay\.io" README.md POLICIES.md STAMPER.md CHANGELOG.md values.schema.json templates files; grep -vE "$default_repository" values.yaml | grep -niE "ghcr|docker\.io|quay\.io"; } || true)"
    t "the only registry name of the values is the default repository of the proxy image" test "$(grep -cE 'docker\.io|ghcr|quay\.io' "$CHART/values.yaml")" -eq 1
    t "the fixtures hold only example.test hosts" test -z "$(cd "$CHART/ci" && hosts_of ./*.yaml | grep -v 'example\.test$' || true)"
    t "the render holds no host that is not a value or a Kubernetes name" test -z "$(hosts_of "$WORK/mirror_yaml.yaml" | grep -vE "$allowed" || true)"
}

CLOSED_NETWORK_POLICY_NAMES='["br-sandbox-default-deny", "br-sandbox-egress-allow", "br-sandbox-egress-full"]'

check_the_network_policy_names_of_a_sandbox_namespace_are_the_closed_list() {
    section "the network policy names of a sandbox namespace are the closed list, every policy of the chart has the prefix"
    local render
    for render in ex cred egress_keeper; do
        t "render ${render}: every NetworkPolicy of the render is named with the prefix br-sandbox-" jqt "$render" '[.[] | select(.kind == "NetworkPolicy") | .metadata.name | select(startswith("br-sandbox-") | not)] | length == 0'
        t "render ${render}: the policies of the sandbox namespaces are inside the closed list" jqt "$render" --argjson closed "$CLOSED_NETWORK_POLICY_NAMES" '[.[] | select(.kind == "ConfigMap" and .metadata.name == "br-sandbox-config") | .metadata.namespace] as $managed | [.[] | select(.kind == "NetworkPolicy" and (.metadata.namespace | IN($managed[]))) | .metadata.name] | unique | length > 0 and (. - $closed) == []'
    done
    t "the policies of the sandbox namespaces of the renders with and without a proxy, together, are the closed list and no other name" test "$(jq -rs --argjson closed "$CLOSED_NETWORK_POLICY_NAMES" '[.[] | .[] | select(.kind == "ConfigMap" and .metadata.name == "br-sandbox-config") | .metadata.namespace] as $managed | [.[] | .[] | select(.kind == "NetworkPolicy" and (.metadata.namespace | IN($managed[]))) | .metadata.name] | unique == $closed' "$WORK/ex.json" "$WORK/cred.json")" = true
}

limit_range_is() {
    jqt "$1" --arg request_cpu "$2" --arg request_memory "$3" --arg request_disk "$4" --arg limit_cpu "$5" --arg limit_memory "$6" --arg limit_disk "$7" '
        .[] | select(.kind == "LimitRange" and .metadata.namespace == "sbx-alpha-none") | .spec.limits | length == 1 and (.[0]
        | .type == "Container"
        and .defaultRequest == {cpu: $request_cpu, memory: $request_memory, "ephemeral-storage": $request_disk}
        and .default == {cpu: $limit_cpu, memory: $limit_memory, "ephemeral-storage": $limit_disk}
        and .max == {cpu: $limit_cpu, memory: $limit_memory, "ephemeral-storage": $limit_disk})'
}

quota_is() {
    jqt "$1" --arg request_cpu "$2" --arg request_memory "$3" --arg request_disk "$4" --arg limit_cpu "$5" --arg limit_memory "$6" --arg limit_disk "$7" '
        .[] | select(.kind == "ResourceQuota" and .metadata.namespace == "sbx-alpha-none") | .spec.hard
        | .["requests.cpu"] == $request_cpu and .["requests.memory"] == $request_memory and .["requests.ephemeral-storage"] == $request_disk
        and .["limits.cpu"] == $limit_cpu and .["limits.memory"] == $limit_memory and .["limits.ephemeral-storage"] == $limit_disk'
}

policy_size_is() {
    jqt "$1" --arg request_cpu "$2" --arg request_memory "$3" --arg request_disk "$4" --arg limit_cpu "$5" --arg limit_memory "$6" --arg limit_disk "$7" '
        .[] | select(.kind == "ValidatingAdmissionPolicy" and .metadata.name == "br-sandbox-pod") | .spec.validations[]
        | select(.message | startswith("BR_SANDBOX_RESOURCES:")) | .expression
        | contains("quantity(c.resources.requests[\u0027cpu\u0027]).isGreaterThan(quantity(\u0027" + $request_cpu + "\u0027))")
        and contains("quantity(c.resources.requests[\u0027memory\u0027]).isGreaterThan(quantity(\u0027" + $request_memory + "\u0027))")
        and contains("quantity(c.resources.requests[\u0027ephemeral-storage\u0027]).isGreaterThan(quantity(\u0027" + $request_disk + "\u0027))")
        and contains("quantity(c.resources.limits[\u0027cpu\u0027]).isGreaterThan(quantity(\u0027" + $limit_cpu + "\u0027))")
        and contains("quantity(c.resources.limits[\u0027memory\u0027]).isGreaterThan(quantity(\u0027" + $limit_memory + "\u0027))")
        and contains("quantity(c.resources.limits[\u0027ephemeral-storage\u0027]).isGreaterThan(quantity(\u0027" + $limit_disk + "\u0027))")'
}

check_one_size_key_per_resource() {
    section "size lives in limits.size only"
    t "no template reads cpu, memory or disk outside a size" test -z "$(grep -rnE '(^|[^e])\.(cpu|memory|disk)\.(request|limit)' "$CHART/templates" || true)"
    local key
    for key in size.cpu=1 pool.cpu=1 tools.memory=1 limits.cpu=1 limits.size.cpu=1 namespaces[0].cpu=1 quota.cpu=1; do
        if helm template t "$CHART" -f "$EXAMPLE" --set "$key" >/dev/null 2>"$WORK/size.err"; then fail "a size key at ${key} passed the schema"; else pass "a size key at ${key} fails the schema"; fi
    done
    render big -f "$EXAMPLE" --set limits.size.cpu.limit=3 --set limits.size.disk.limit=8Gi
    local normal changed
    normal="$(jq 'map(select(.kind != "ConfigMap" and .kind != "ResourceQuota" and .kind != "LimitRange" and .kind != "ValidatingAdmissionPolicy"))' "$WORK/ex.json")"
    changed="$(jq 'map(select(.kind != "ConfigMap" and .kind != "ResourceQuota" and .kind != "LimitRange" and .kind != "ValidatingAdmissionPolicy"))' "$WORK/big.json")"
    t "a bigger size changes only quota, limit range, policy, config and pod resources" test "$normal" = "$changed"
    t "the pod differs only in resources" test "$(jq -S "${SANDBOX_PODS} | map(del(.spec.containers[].resources))" "$WORK/ex.json")" = "$(jq -S "${SANDBOX_PODS} | map(del(.spec.containers[].resources))" "$WORK/big.json")"
    render bigger -f "$EXAMPLE" --set limits.size.cpu.request=700m --set limits.size.cpu.limit=3 --set limits.size.memory.request=3Gi --set limits.size.memory.limit=5Gi --set limits.size.disk.request=6Gi --set limits.size.disk.limit=9Gi
    t "the default size reaches the limit range of its namespace" limit_range_is ex 500m 1Gi 2Gi 2 2Gi 5Gi
    t "the new size reaches the limit range: default request, default and max follow it" limit_range_is bigger 700m 3Gi 6Gi 3 5Gi 9Gi
    t "the default size reaches the quota of its namespace" quota_is ex 12200m 24736Mi 49312Mi 50 49792Mi 123520Mi
    t "the new size reaches the quota: each of the six quantities follows it" quota_is bigger 17000m 73888Mi 147616Mi 74 123520Mi 221824Mi
    t "the default size reaches the size rule of the policy" policy_size_is ex 500m 1Gi 2Gi 2 2Gi 5Gi
    t "the new size reaches the size rule of the policy: each of the six bounds follows it" policy_size_is bigger 700m 3Gi 6Gi 3 5Gi 9Gi
    t "the pod template of each namespace carries limits.size" jqt ex "${SANDBOX_PODS} | map(.spec.containers[0].resources.limits.cpu) | unique == [\"2\"]"
}

check_image_pull_policies() {
    section "the agent container pulls at each creation, the tools image is pinned by digest"
    t "the agent container pulls at each creation" jqt ex "${SANDBOX_PODS} | map(.spec.containers[0].imagePullPolicy) | unique == [\"Always\"]"
    t "the tools image is by digest and pulled if not present" jqt ex "${SANDBOX_PODS} | map(.spec.initContainers[0] | select(.imagePullPolicy == \"IfNotPresent\" and (.image | test(\"@sha256:[a-f0-9]{64}\$\")))) | length == 4"
    render forced -f "$EXAMPLE" --set image_pull_policy=Never
    t "image_pull_policy overrides the computed policy" jqt forced "${SANDBOX_PODS} | map(.spec.containers[0].imagePullPolicy) | unique == [\"Never\"]"
    fails_with "values don't meet the specifications" -f "$EXAMPLE" --set image=registry.example.test/agents/base
    fails_with "BR_SANDBOX_MISSING_VALUE{key=tools.image.digest}" -f "$(vals nodigest 'del(.tools.image.digest)')"
}

check_no_replica_set_and_a_quota_for_the_pods() {
    section "the chart renders no ReplicaSet, the quota holds the pods of limits.pods, a pod leaves fast"
    t "no ReplicaSet is rendered" jqt keeper '[.[] | select(.kind == "ReplicaSet")] | length == 0'
    t "the quota allows limits.pods plus the extra pods" jqt ex '[.[] | select(.kind == "ResourceQuota" and (.metadata.namespace | endswith("-egress") | not))] | map(.spec.hard.pods | tonumber) | sort == [24, 24, 24, 24]'
    local namespace pods
    for namespace in sbx-alpha-allow sbx-alpha-full sbx-alpha-none sbx-beta-none; do
        pods="$(yq "select(.kind == \"ResourceQuota\" and .metadata.namespace == \"${namespace}\") | .spec.hard.pods" "$WORK/ex_yaml.yaml")"
        t "the quota of ${namespace} holds at least limits.pods pods" test "$pods" -ge 20
    done
    t "a pod leaves in one second" jqt ex "${SANDBOX_PODS} | map(.spec.terminationGracePeriodSeconds) | unique == [1]"
    t "the quota multiplies the size by the pod count and adds the seed container of each sandbox" jqt ex '.[] | select(.kind == "ResourceQuota" and .metadata.namespace == "sbx-alpha-none") | .spec.hard | .["requests.cpu"] == "12200m" and .["requests.memory"] == "24736Mi" and .["limits.ephemeral-storage"] == "123520Mi"'
    fails_with "values don't meet the specifications" -f "$EXAMPLE" --set pool.floor=30
}

check_one_pod_per_agent_nothing_shared() {
    section "no shared volume, no host namespace, no ingress"
    t "sandbox pods have only emptyDir volumes" jqt ex "${SANDBOX_PODS} | map(.spec.volumes[] | keys | map(select(. != \"name\"))) | flatten | unique == [\"emptyDir\"]"
    t "no sandbox pod shares a namespace with the host or another container" jqt ex "${SANDBOX_PODS} | map(.spec | select(has(\"hostPID\") or has(\"hostNetwork\") or has(\"hostIPC\") or has(\"shareProcessNamespace\"))) | length == 0"
    t "no sandbox container opens a port" jqt ex "${SANDBOX_PODS} | map(.spec.containers[], .spec.initContainers[] | select(has(\"ports\"))) | length == 0"
    t "no render object is a volume claim, a service or an ingress" jqt ex '[.[] | select(.kind == "PersistentVolumeClaim" or .kind == "Service" or .kind == "Ingress" or .kind == "Secret")] | length == 0'
    t "the volumes are bounded by a size limit" jqt ex "${SANDBOX_PODS} | map(.spec.volumes[] | .emptyDir.sizeLimit) | all(. != null)"
}
