#!/usr/bin/env bash

NP='.[] | select(.kind == "NetworkPolicy")'

policy_names() {
    jqv ex "[${NP} | select(.metadata.namespace == \"$1\") | .metadata.name] | sort | join(\",\")"
}

check_network_policy_none_has_deny_only() {
    section "network policy none: default-deny only, no resolver"
    t "none has exactly the policy default-deny" test "$(policy_names sbx-alpha-none)" = "default-deny"
    t "default-deny selects every pod and both directions" jqt ex "${NP} | select(.metadata.namespace == \"sbx-alpha-none\") | select(.spec.podSelector == {} and (.spec.policyTypes | sort) == [\"Egress\", \"Ingress\"] and (.spec | has(\"egress\") | not) and (.spec | has(\"ingress\") | not))"
    t "none pods use dnsPolicy None and nameserver 127.0.0.1" jqt ex '[.[] | select(.kind == "ReplicaSet" and .metadata.namespace == "sbx-alpha-none") | .spec.template.spec | select(.dnsPolicy == "None" and .dnsConfig.nameservers == ["127.0.0.1"])] | length == 1'
}

check_network_policy_allow_reaches_only_the_proxy() {
    section "network policy allow: only the proxy pods of the namespace, TCP on the proxy port"
    t "allow has default-deny and egress-allow" test "$(policy_names sbx-alpha-allow)" = "default-deny,egress-allow"
    t "egress-allow has one rule: proxy pods, TCP 8888, nothing else" jqt ex "${NP} | select(.metadata.name == \"egress-allow\") | select(.spec.policyTypes == [\"Egress\"] and (.spec.egress | length) == 1 and .spec.egress[0].to == [{\"podSelector\": {\"matchLabels\": {\"br.sandbox/role\": \"proxy\"}}}] and .spec.egress[0].ports == [{\"protocol\": \"TCP\", \"port\": 8888}])"
    t "egress-allow selects the sandbox pods by role, not by pool" jqt ex "${NP} | select(.metadata.name == \"egress-allow\") | .spec.podSelector == {\"matchLabels\": {\"br.sandbox/role\": \"sandbox\"}}"
    render port -f "$EXAMPLE" --set egress.proxy.port=9999
    t "the proxy port is a value" jqt port '.[] | select(.kind == "NetworkPolicy" and .metadata.name == "egress-allow") | .spec.egress[0].ports[0].port == 9999'
}

check_network_policy_full_opens_public_addresses_only() {
    section "network policy full: public addresses except every internal list, no cluster DNS"
    t "full has default-deny and egress-full" test "$(policy_names sbx-alpha-full)" = "default-deny,egress-full"
    t "single stack: one rule, one ipBlock 0.0.0.0/0, no pod or namespace selector" jqt ex "${NP} | select(.metadata.name == \"egress-full\") | select((.spec.egress | length) == 1 and (.spec.egress[0].to | length) == 1 and .spec.egress[0].to[0].ipBlock.cidr == \"0.0.0.0/0\" and (.spec.egress[0].to[0] | has(\"podSelector\") or has(\"namespaceSelector\") | not) and (.spec.egress[0] | has(\"ports\") | not))"
    t "full pods resolve through the public resolvers only" jqt ex '.[] | select(.kind == "ReplicaSet" and .metadata.namespace == "sbx-alpha-full") | .spec.template.spec | select(.dnsPolicy == "None" and .dnsConfig.nameservers == ["198.51.100.53", "198.51.100.54"])'
    render dual -f "$EXAMPLE" --set network.ipv6=dual_stack
    t "dual stack adds ::/0 without fc00::/7 and fe80::/10" jqt dual '.[] | select(.kind == "NetworkPolicy" and .metadata.name == "egress-full") | .spec.egress[1].to[0].ipBlock | select(.cidr == "::/0" and (.except | index("fc00::/7")) != null and (.except | index("fe80::/10")) != null)'
}

check_network_policy_mode_is_unknown_or_from_values() {
    section "network policy an unknown mode fails, no template reads the cluster"
    fails_with "BR_SANDBOX_MODE_UNKNOWN{client=alpha,mode=weird}" -f "$(vals weird '.clients.alpha.modes.weird = .clients.alpha.modes.none')"
    t "no template reads cluster state or the environment" test -z "$(grep -rnE 'lookup |\.Capabilities|getHostByName|[ (]env "' "$CHART/templates" || true)"
}

check_one_namespace_per_client_and_mode() {
    section "namespaces per client and mode, default-deny in each"
    t "four sandbox namespaces plus the system namespace" test "$(jqv ex '[.[] | select(.kind == "Namespace") | .metadata.name] | sort | join(",")')" = "sbx-alpha-allow,sbx-alpha-full,sbx-alpha-none,sbx-beta-none,sbx-system"
    t "every sandbox namespace has default-deny on both directions" jqt ex '[.[] | select(.kind == "Namespace" and .metadata.labels["br.sandbox/managed"] == "true") | .metadata.name] as $n | [.[] | select(.kind == "NetworkPolicy" and .metadata.name == "default-deny" and (.spec.policyTypes | sort) == ["Egress", "Ingress"]) | .metadata.namespace] as $d | ($n - $d) == []'
    t "no policy has an ingress rule" jqt ex "[${NP} | select(.spec | has(\"ingress\"))] | length == 0"
    fails_with "BR_SANDBOX_SHARED_NAMESPACE{client_a=system,client_b=alpha,namespace=sbx-alpha-none}" -f "$(vals shared '.installation.system_namespace = "sbx-alpha-none"')"
}

check_network_check_except_lists_hold_every_internal_range() {
    section "the network check except lists hold internal, pod, service, node, node public and load balancer"
    t "full except list has every configured range" jqt ex '.[] | select(.kind == "NetworkPolicy" and .metadata.name == "egress-full") | .spec.egress[0].to[0].ipBlock.except as $e | ["10.0.0.0/8", "172.16.0.0/12", "192.168.0.0/16", "100.64.0.0/10", "169.254.0.0/16", "127.0.0.0/8", "10.42.0.0/16", "10.43.0.0/16", "10.0.0.11/32", "10.0.0.12/32", "203.0.113.11/32", "203.0.113.12/32", "203.0.113.20/32"] - $e == []'
    render v6 -f "$EXAMPLE" --set network.ipv6=dual_stack --set 'network.node_public_addresses={203.0.113.11,2001:db8::5}'
    t "an IPv6 node address is excepted in the IPv6 rule only" jqt v6 '.[] | select(.kind == "NetworkPolicy" and .metadata.name == "egress-full") | (.spec.egress[1].to[0].ipBlock.except | index("2001:db8::5/128")) != null and (.spec.egress[0].to[0].ipBlock.except | index("2001:db8::5/128")) == null'
    fails_with "BR_SANDBOX_MISSING_VALUE{key=network.node_addresses}" -f "$(vals nonode 'del(.network.node_addresses)')"
    fails_with "BR_SANDBOX_MISSING_VALUE{key=network.node_public_addresses}" -f "$(vals nopublic 'del(.network.node_public_addresses)')"
    fails_with "BR_SANDBOX_MISSING_VALUE{key=network.load_balancer_addresses}" -f "$(vals nolb 'del(.network.load_balancer_addresses)')"
    t "an empty list written on purpose renders" render emptylb -f "$(vals emptylb '.network.load_balancer_addresses = []')"
    t "the check job reads every address of the three lists" jqt ex '.[] | select(.kind == "ConfigMap" and .metadata.name == "br-sandbox-netcheck" and .metadata.namespace == "sbx-system") | .data["node-addresses"] | split("\n") | (["10.0.0.11", "203.0.113.11", "203.0.113.20"] - .) == []'
}

check_no_project_value_in_the_chart() {
    section "no host, registry or client name outside the fixtures"
    local allowed='(kubernetes\.io|k8s\.io|json-schema\.org|example\.test|cluster\.local)$'
    local found
    found="$(cd "$CHART" && hosts_of Chart.yaml README.md CHANGELOG.md values.yaml values.schema.json templates/* files/* | grep -vE "$allowed" | grep -v '^github\.com$' || true)"
    t "no host in the chart files outside the allowed suffixes" test -z "$found"
    t "no client or project name in the chart files" test -z "$(cd "$CHART" && grep -rniE "ghcr|docker\.io|quay\.io${FORBIDDEN_NAMES:+|${FORBIDDEN_NAMES}}" README.md CHANGELOG.md values.yaml values.schema.json templates files || true)"
    t "the fixtures hold only example.test hosts" test -z "$(cd "$CHART/ci" && hosts_of ./*.yaml | grep -v 'example\.test$' || true)"
    t "the render holds no host that is not a value or a Kubernetes name" test -z "$(hosts_of "$WORK/mirror_yaml.yaml" | grep -vE "$allowed" || true)"
}

check_one_size_key_per_resource() {
    section "size lives in pool.size only"
    t "no template reads cpu, memory or disk outside a size" test -z "$(grep -rnE '(^|[^e])\.(cpu|memory|disk)\.(request|limit)' "$CHART/templates" || true)"
    local key
    for key in size.cpu=1 pool.cpu=1 tools.memory=1 clients.alpha.cpu=1 clients.alpha.modes.none.pool.cpu=1 clients.alpha.modes.none.memory=1 quota.cpu=1; do
        if helm template t "$CHART" -f "$EXAMPLE" --set "$key" >/dev/null 2>"$WORK/size.err"; then fail "a size key at ${key} passed the schema"; else pass "a size key at ${key} fails the schema"; fi
    done
    render big -f "$EXAMPLE" --set pool.size.cpu.limit=3 --set pool.size.disk.limit=8Gi
    local normal changed
    normal="$(jq 'map(select(.kind != "ReplicaSet" and .kind != "ResourceQuota" and .kind != "LimitRange" and .kind != "ValidatingAdmissionPolicy" and .kind != "ConfigMap"))' "$WORK/ex.json")"
    changed="$(jq 'map(select(.kind != "ReplicaSet" and .kind != "ResourceQuota" and .kind != "LimitRange" and .kind != "ValidatingAdmissionPolicy" and .kind != "ConfigMap"))' "$WORK/big.json")"
    t "a bigger size changes only quota, limit range, policy, config and pod resources" test "$normal" = "$changed"
    t "the pod differs only in resources" test "$(jq -S '[.[] | select(.kind == "ReplicaSet") | del(.spec.template.spec.containers[].resources)]' "$WORK/ex.json")" = "$(jq -S '[.[] | select(.kind == "ReplicaSet") | del(.spec.template.spec.containers[].resources)]' "$WORK/big.json")"
    t "the bigger size reaches the quota, the limit range and the policy" test "$(jq -r '[.[] | select(.kind == "ResourceQuota" or .kind == "LimitRange" or .kind == "ValidatingAdmissionPolicy")] | tostring | test("limits.cpu")' "$WORK/big.json")" = true
    t "the pool of a namespace carries its own size" jqt ex '.[] | select(.kind == "ReplicaSet" and .metadata.namespace == "sbx-alpha-full") | .spec.template.spec.containers[0].resources.limits.cpu == "4"'
}

check_image_is_one_value() {
    section "a second image changes the image value only"
    render other -f "$EXAMPLE" --set image=registry.example.test/agents/other:2.0
    helm template t "$CHART" -f "$EXAMPLE" | sed 's|registry.example.test/agents/base:1.0|IMAGE|; s|registry.example.test/agents/base|IMAGE_REPOSITORY|' >"$WORK/n4a.yaml"
    helm template t "$CHART" -f "$EXAMPLE" --set image=registry.example.test/agents/other:2.0 | sed 's|registry.example.test/agents/other:2.0|IMAGE|; s|registry.example.test/agents/other|IMAGE_REPOSITORY|' >"$WORK/n4b.yaml"
    t "two renders differ only where the image is written" cmp -s "$WORK/n4a.yaml" "$WORK/n4b.yaml"
    t "the image reaches the pool and the policy" test "$(grep -c 'agents/other' <(helm template t "$CHART" -f "$EXAMPLE" --set image=registry.example.test/agents/other:2.0))" -ge 8
    t "a tag pulls at each creation" jqt ex '[.[] | select(.kind == "ReplicaSet") | .spec.template.spec.containers[0].imagePullPolicy] | unique == ["Always"]'
    render digest -f "$EXAMPLE" --set image=registry.example.test/agents/base@sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc
    t "a digest pulls if not present" jqt digest '[.[] | select(.kind == "ReplicaSet") | .spec.template.spec.containers[0].imagePullPolicy] | unique == ["IfNotPresent"]'
    t "the tools image is by digest and pulled if not present" jqt ex '[.[] | select(.kind == "ReplicaSet") | .spec.template.spec.initContainers[0] | select(.imagePullPolicy == "IfNotPresent" and (.image | test("@sha256:[a-f0-9]{64}$")))] | length == 4'
    render forced -f "$EXAMPLE" --set image_pull_policy=Never
    t "image_pull_policy overrides the computed policy" jqt forced '[.[] | select(.kind == "ReplicaSet") | .spec.template.spec.containers[0].imagePullPolicy] | unique == ["Never"]'
    fails_with "values don't meet the specifications" -f "$EXAMPLE" --set image=registry.example.test/agents/base
    fails_with "BR_SANDBOX_MISSING_VALUE{key=image}" -f "$(vals noimage 'del(.image)')"
    fails_with "BR_SANDBOX_MISSING_VALUE{key=tools.image.digest}" -f "$(vals nodigest 'del(.tools.image.digest)')"
}

check_pool_has_no_replicas_and_a_quota_for_it() {
    section "the keeper owns spec.replicas, the quota holds the pool, a pod leaves fast"
    t "no ReplicaSet and no keeper Deployment sets spec.replicas except the keeper's own" jqt keeper '[.[] | select(.kind == "ReplicaSet") | .spec | has("replicas")] | any | not'
    t "the quota allows max_sandboxes plus the extra pods" jqt ex '[.[] | select(.kind == "ResourceQuota")] | map(.spec.hard.pods | tonumber) | sort == [14, 24, 24, 24]'
    local namespace max pods
    for namespace in sbx-alpha-allow sbx-alpha-full sbx-alpha-none sbx-beta-none; do
        max="$(yq "select(.kind == \"ConfigMap\" and .metadata.name == \"br-sandbox-config\" and .metadata.namespace == \"${namespace}\") | .data[\"config.yaml\"] | from_yaml | .pool.max_sandboxes" "$WORK/ex_yaml.yaml")"
        pods="$(yq "select(.kind == \"ResourceQuota\" and .metadata.namespace == \"${namespace}\") | .spec.hard.pods" "$WORK/ex_yaml.yaml")"
        t "the quota of ${namespace} holds at least max_sandboxes pods" test "$pods" -ge "$max"
    done
    t "a pod leaves in one second" jqt ex '[.[] | select(.kind == "ReplicaSet") | .spec.template.spec.terminationGracePeriodSeconds] | unique == [1]'
    t "the quota multiplies the size by the pod count" jqt ex '.[] | select(.kind == "ResourceQuota" and .metadata.namespace == "sbx-alpha-none") | .spec.hard | .["requests.cpu"] == "12000m" and .["requests.memory"] == "24Gi" and .["limits.ephemeral-storage"] == "120Gi"'
    t "a floor above max_sandboxes fails" test "$(helm template t "$CHART" -f "$EXAMPLE" --set pool.floor=30 2>&1 | grep -c 'BR_SANDBOX_POOL_INVALID')" -ge 1
}

check_one_pod_per_agent_nothing_shared() {
    section "no shared volume, no host namespace, no ingress"
    t "sandbox pods have only emptyDir volumes" jqt ex "${SANDBOX_PODS} | map(.spec.volumes[] | keys | map(select(. != \"name\"))) | flatten | unique == [\"emptyDir\"]"
    t "no sandbox pod shares a namespace with the host or another container" jqt ex "${SANDBOX_PODS} | map(.spec | select(has(\"hostPID\") or has(\"hostNetwork\") or has(\"hostIPC\") or has(\"shareProcessNamespace\"))) | length == 0"
    t "no sandbox container opens a port" jqt ex "${SANDBOX_PODS} | map(.spec.containers[], .spec.initContainers[] | select(has(\"ports\"))) | length == 0"
    t "no render object is a volume claim, a service or an ingress" jqt ex '[.[] | select(.kind == "PersistentVolumeClaim" or .kind == "Service" or .kind == "Ingress" or .kind == "Secret")] | length == 0'
    t "the volumes are bounded by a size limit" jqt ex "${SANDBOX_PODS} | map(.spec.volumes[] | .emptyDir.sizeLimit) | all(. != null)"
}
