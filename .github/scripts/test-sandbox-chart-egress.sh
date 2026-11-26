#!/usr/bin/env bash

EGRESS_NAMESPACES='[.[] | select(.kind == "Namespace" and (.metadata.labels["br.sandbox/egress"] // "") == "true") | .metadata.name]'
TUNNEL_SOURCE='{"from": [{"namespaceSelector": {"matchLabels": {"kubernetes.io/metadata.name": "%s"}}, "podSelector": {"matchLabels": {"br.sandbox/role": "sandbox"}}}], "ports": [{"protocol": "TCP", "port": %s}]}'
PROXY_ONLY='{"to": [{"namespaceSelector": {"matchLabels": {"kubernetes.io/metadata.name": "%s"}}, "podSelector": {"matchLabels": {"br.sandbox/role": "proxy"}}}], "ports": [{"protocol": "TCP", "port": %s}]}'

render_egress_fixtures() {
    render cred -f "$(vals cred '.egress.enabled = true | .clients.alpha.modes.full.credential_set = {"git": "set-a"}')"
    render two -f "$(vals two '.clients.beta.modes.allow = .clients.beta.modes.none')"
    render egressdual -f "$(vals dualegress '.network.ipv6 = "dual_stack"')"
    render tunnel -f "$EXAMPLE" --set egress.proxy.port=9999 --set egress.quota.pods=7
}

policy_json() {
    printf '(.[] | select(.kind == "NetworkPolicy" and .metadata.namespace == "%s" and .metadata.name == "%s"))' "$1" "$2"
}

check_no_identity_of_the_chart_has_a_right_in_the_egress_namespace() {
    section "egress namespace: no Role, no RoleBinding, no subject, no rule naming it"
    local name
    for name in ex keeper cred two; do
        t "render ${name}: the egress namespaces exist and no Role or RoleBinding lives in them" jqt "$name" "${EGRESS_NAMESPACES} as \$e | (\$e | length) > 0 and ([.[] | select((.kind == \"Role\" or .kind == \"RoleBinding\") and (.metadata.namespace as \$n | \$e | index(\$n) != null))] | length == 0)"
        t "render ${name}: no subject of a RoleBinding or a ClusterRoleBinding is in an egress namespace" jqt "$name" "${EGRESS_NAMESPACES} as \$e | (\$e | length) > 0 and ([.[] | select(.kind == \"RoleBinding\" or .kind == \"ClusterRoleBinding\") | .subjects[]? | select(.namespace as \$n | \$e | index(\$n) != null)] | length == 0)"
        t "render ${name}: no rule of a Role or ClusterRole names an egress namespace" jqt "$name" "${EGRESS_NAMESPACES} as \$e | (\$e | length) > 0 and ([.[] | select(.kind == \"Role\" or .kind == \"ClusterRole\") | .rules[]? | (.resourceNames // [])[] | select(. as \$r | \$e | index(\$r) != null)] | length == 0)"
    done
    fails_with "BR_SANDBOX_SHARED_NAMESPACE{client_a=alpha,client_b=runner-of-alpha,namespace=sbx-alpha-allow-egress}" -f "$(vals runneregress '.clients.alpha.modes.none.runner.namespace = "sbx-alpha-allow-egress"')"
}

check_no_cluster_wide_right_on_secrets_and_pods_beyond_the_network_check_read() {
    section "cluster-wide rights: none on secrets, none on pods but the read of the network check"
    t "no ClusterRole rule names secrets or a wildcard resource" jqt keeper '[.[] | select(.kind == "ClusterRole") | .rules[].resources[] | select(. == "secrets" or . == "*")] | length == 0'
    t "the only cluster-wide pod right is get and list in the network check ClusterRole" jqt keeper '[.[] | select(.kind == "ClusterRole") | select(any(.rules[]; .resources | any(. == "pods" or startswith("pods/")))) | {name: .metadata.name, rules: [.rules[] | select(.resources | any(. == "pods" or startswith("pods/")))]}] == [{"name": "sbx-system-netcheck", "rules": [{"apiGroups": [""], "resources": ["nodes", "services", "endpoints", "pods"], "verbs": ["get", "list"]}]}]'
    t "every ClusterRoleBinding names a ClusterRole of the render and a ServiceAccount" jqt keeper '[.[] | select(.kind == "ClusterRole") | .metadata.name] as $c | [.[] | select(.kind == "ClusterRoleBinding") | select((.roleRef.name as $r | $c | index($r) != null) and (.subjects | all(.kind == "ServiceAccount")) | not)] | length == 0'
    t "every RoleBinding names a Role of the render, never a ClusterRole" jqt keeper '[.[] | select(.kind == "RoleBinding") | select(.roleRef.kind != "Role")] | length == 0'
}

check_the_ca_certificate_stays_in_the_sandbox_namespace() {
    section "the CA certificate and the Secrets are not rendered; the egress namespace holds only its policies and quota"
    t "no Secret and no CA or certificate object is rendered" jqt cred '[.[] | select(.kind == "Secret" or ((.kind == "ConfigMap") and (.metadata.name | test("(^|-)(ca|cert|certificate)(-|$)"))))] | length == 0'
    t "the egress namespaces hold only Namespace, NetworkPolicy and ResourceQuota" jqt cred "${EGRESS_NAMESPACES} as \$e | [.[] | select((.metadata.namespace as \$n | \$e | index(\$n) != null) or (.kind == \"Namespace\" and (.metadata.name as \$n | \$e | index(\$n) != null))) | .kind] | unique == [\"Namespace\", \"NetworkPolicy\", \"ResourceQuota\"]"
    t "the README places the CA certificate in the sandbox namespace and the CA key in the egress namespace" grep -qF 'The CA certificate (not secret) stays in the sandbox namespace' "$README"
}

check_the_sandbox_policy_reaches_only_the_proxy_pods_of_the_egress_namespace() {
    section "sandbox policy, modes allow and full with credentials: the proxy pods of the egress namespace only"
    local want
    want="$(printf "$PROXY_ONLY" sbx-alpha-allow-egress 8888)"
    t "allow: one rule, namespace and pod selector in the same item, TCP on the tunnel port, no DNS" jqt cred "$(policy_json sbx-alpha-allow egress-allow) | .spec.policyTypes == [\"Egress\"] and .spec.podSelector == {\"matchLabels\": {\"br.sandbox/role\": \"sandbox\"}} and .spec.egress == [${want}]"
    want="$(printf "$PROXY_ONLY" sbx-alpha-full-egress 8888)"
    t "full with credentials: the same rule towards its own egress namespace, no public address, no DNS" jqt cred "$(policy_json sbx-alpha-full egress-full) | .spec.policyTypes == [\"Egress\"] and .spec.egress == [${want}]"
    t "full with credentials has default-deny and egress-full only" test "$(jqv cred '[.[] | select(.kind == "NetworkPolicy" and .metadata.namespace == "sbx-alpha-full") | .metadata.name] | sort | join(",")')" = "default-deny,egress-full"
    want="$(printf "$PROXY_ONLY" sbx-alpha-allow-egress 9999)"
    t "the tunnel port is the value egress.proxy.port" jqt tunnel "$(policy_json sbx-alpha-allow egress-allow) | .spec.egress == [${want}]"
    t "full with credentials resolves nothing outside: the pod resolver is the local address" jqt cred '.[] | select(.kind == "ReplicaSet" and .metadata.namespace == "sbx-alpha-full") | .spec.template.spec.dnsConfig.nameservers == ["127.0.0.1"]'
    t "the sandbox policy of a mode with a proxy names no ipBlock" jqt cred '[.[] | select(.kind == "NetworkPolicy" and (.metadata.namespace == "sbx-alpha-allow" or .metadata.namespace == "sbx-alpha-full")) | .. | objects | select(has("ipBlock"))] | length == 0'
}

check_the_egress_namespace_has_its_own_policies() {
    section "egress namespace policies: default-deny, tunnel ingress from its own sandbox namespace only, barriers on egress"
    t "the egress namespace holds default-deny, egress-proxy and ingress-tunnel" test "$(jqv cred '[.[] | select(.kind == "NetworkPolicy" and .metadata.namespace == "sbx-alpha-allow-egress") | .metadata.name] | sort | join(",")')" = "default-deny,egress-proxy,ingress-tunnel"
    t "default-deny selects every pod, both directions, no rule" jqt cred "$(policy_json sbx-alpha-allow-egress default-deny) | .spec.podSelector == {} and (.spec.policyTypes | sort) == [\"Egress\", \"Ingress\"] and (.spec | has(\"ingress\") or has(\"egress\") | not)"
    local want
    want="$(printf "$TUNNEL_SOURCE" sbx-alpha-allow 8888)"
    t "ingress-tunnel: from the sandbox namespace of the same client and mode, role sandbox, the tunnel port only" jqt cred "$(policy_json sbx-alpha-allow-egress ingress-tunnel) | .spec.policyTypes == [\"Ingress\"] and .spec.podSelector == {\"matchLabels\": {\"br.sandbox/role\": \"proxy\"}} and .spec.ingress == [${want}]"
    want="$(printf "$TUNNEL_SOURCE" sbx-alpha-full 8888)"
    t "the egress namespace of full with credentials takes its own sandbox namespace" jqt cred "$(policy_json sbx-alpha-full-egress ingress-tunnel) | .spec.ingress == [${want}]"
    want="$(printf "$TUNNEL_SOURCE" sbx-beta-allow 8888)"
    t "two clients: each egress namespace takes its own sandbox namespace and no other" jqt two "$(policy_json sbx-beta-allow-egress ingress-tunnel) | .spec.ingress == [${want}]"
    want="$(printf "$TUNNEL_SOURCE" sbx-alpha-allow 9999)"
    t "the reload port is reached by nobody: the only ingress port is the tunnel port, a value" jqt tunnel "[.[] | select(.kind == \"NetworkPolicy\" and (.metadata.namespace | endswith(\"-egress\"))) | .spec.ingress[]?] | all(.ports == [{\"protocol\": \"TCP\", \"port\": 9999}])"
    t "no policy of an egress namespace but ingress-tunnel has an ingress rule" jqt cred '[.[] | select(.kind == "NetworkPolicy" and (.metadata.namespace | endswith("-egress")) and (.spec | has("ingress"))) | .metadata.name] | unique == ["ingress-tunnel"]'
    t "egress-proxy opens the public addresses except every internal list and selects no pod or namespace" jqt cred "$(policy_json sbx-alpha-allow-egress egress-proxy) | .spec.policyTypes == [\"Egress\"] and .spec.podSelector == {\"matchLabels\": {\"br.sandbox/role\": \"proxy\"}} and (.spec.egress | length) == 1 and .spec.egress[0].to[0].ipBlock.cidr == \"0.0.0.0/0\" and ([\"10.0.0.0/8\", \"172.16.0.0/12\", \"192.168.0.0/16\", \"100.64.0.0/10\", \"169.254.0.0/16\", \"127.0.0.0/8\", \"10.42.0.0/16\", \"10.43.0.0/16\", \"10.0.0.11/32\", \"10.0.0.12/32\", \"203.0.113.11/32\", \"203.0.113.12/32\", \"203.0.113.20/32\"] - .spec.egress[0].to[0].ipBlock.except == []) and ([.spec.egress[] | .. | objects | select(has(\"podSelector\") or has(\"namespaceSelector\"))] | length == 0)"
    t "dual stack adds ::/0 without fc00::/7 and fe80::/10" jqt egressdual "$(policy_json sbx-alpha-allow-egress egress-proxy) | .spec.egress[1].to[0].ipBlock | .cidr == \"::/0\" and (.except | index(\"fc00::/7\")) != null and (.except | index(\"fe80::/10\")) != null"
    t "the egress namespace has a quota on pods, a value" jqt tunnel '.[] | select(.kind == "ResourceQuota" and .metadata.namespace == "sbx-alpha-allow-egress") | .spec.hard == {"pods": "7"}'
}

check_the_admission_policy_accepts_the_sandbox_role_only() {
    section "admission policy: role=sandbox only, no proxy case, no controller value"
    t "the role rule is one case" expression_is_exactly sbx-alpha-none BR_SANDBOX_ROLE
    t "no policy, binding or value names a proxy case or a controller user" test -z "$(grep -rnE 'proxyCreator|proxy_controller|replicaset-controller|userInfo' "$CHART/templates" "$CHART/values.yaml" "$CHART/values.schema.json" || true)"
    t "no admission block in the values" test "$(yq '.admission' "$CHART/values.yaml")" = null
    t "the binding of a sandbox namespace never matches an egress namespace" jqt cred "${EGRESS_NAMESPACES} as \$e | [.[] | select(.kind == \"ValidatingAdmissionPolicyBinding\") | .spec.matchResources.namespaceSelector.matchLabels | select(.[\"kubernetes.io/metadata.name\"] as \$n | \$e | index(\$n) != null)] | length == 0"
}

check_the_egress_namespace_labels() {
    section "egress namespace labels: egress, client, mode, same Pod Security labels, never managed"
    t "each egress namespace carries br.sandbox/egress, client and mode, and not br.sandbox/managed" jqt cred '[.[] | select(.kind == "Namespace" and .metadata.labels["br.sandbox/egress"] == "true")] | map(select((.metadata.labels | has("br.sandbox/managed") | not) and .metadata.labels["br.sandbox/client"] == "alpha")) | map(.metadata.name + ":" + .metadata.labels["br.sandbox/mode"]) | sort == ["sbx-alpha-allow-egress:allow", "sbx-alpha-full-egress:full"]'
    t "an egress namespace has the Pod Security labels of the sandbox namespace" jqt cred '[.[] | select(.kind == "Namespace") | select(.metadata.name == "sbx-alpha-allow" or .metadata.name == "sbx-alpha-allow-egress") | .metadata.labels | with_entries(select(.key | startswith("pod-security")))] | (.[0] == .[1]) and (.[0] | length) == 3 and .[0]["pod-security.kubernetes.io/enforce"] == "baseline"'
    t "the sandbox namespaces keep br.sandbox/managed and carry no egress label" jqt cred '[.[] | select(.kind == "Namespace" and .metadata.labels["br.sandbox/managed"] == "true") | .metadata.labels | has("br.sandbox/egress")] | all(. == false)'
    t "the README states that the egress namespace does not carry the managed label, and the consequence" grep -qF 'The egress namespace does not carry `br.sandbox/managed=true`' "$README"
    t "the README names br.sandbox/egress in the label contract" grep -qF '| `br.sandbox/egress` |' "$README"
}

check_full_with_credentials_follows_the_same_layout() {
    section "mode full: an egress namespace only when a credential set is configured"
    t "full without a credential set has no egress namespace and keeps egress-full to the public addresses" jqt ex '([.[] | select(.kind == "Namespace" and .metadata.name == "sbx-alpha-full-egress")] | length == 0) and ([.[] | select(.kind == "NetworkPolicy" and .metadata.namespace == "sbx-alpha-full" and .metadata.name == "egress-full") | .spec.egress[0].to[0].ipBlock.cidr] == ["0.0.0.0/0"])'
    t "an empty credential set is no credential set" render emptyset -f "$(vals emptyset '.egress.enabled = true | .clients.alpha.modes.full.credential_set = {}')"
    t "an empty credential set renders no egress namespace for full" jqt emptyset '[.[] | select(.kind == "Namespace" and .metadata.name == "sbx-alpha-full-egress")] | length == 0'
    t "a credential set of full renders its egress namespace, quota and three policies" jqt cred '[.[] | select(.metadata.namespace == "sbx-alpha-full-egress") | .kind] | sort == ["NetworkPolicy", "NetworkPolicy", "NetworkPolicy", "ResourceQuota"]'
    t "mode none never has an egress namespace" jqt cred '[.[] | select(.kind == "Namespace" and (.metadata.name | test("none-egress$")))] | length == 0'
    fails_with "BR_SANDBOX_CREDENTIAL_SET_MODE{client=beta,mode=none}" -f "$(vals credsetnone '.egress.enabled = true | .clients.beta.modes.none.credential_set = {"git": "set-a"}')"
    fails_with "BR_SANDBOX_EGRESS_DISABLED{client=alpha,mode=full,key=credential_set}" -f "$(vals credsetoff '.clients.alpha.modes.full.credential_set = {"git": "set-a"}')"
}

check_the_egress_namespace_name_is_a_helper_with_a_bound() {
    section "egress namespace name: one helper, 63 characters at most, a clash fails"
    t "the helper br-sandbox-chart.egress.namespace holds the suffix and no other template writes it" test "$(grep -rlF -e '%s-egress' "$CHART/templates")" = "$CHART/templates/_helpers.tpl"
    t "the name is the sandbox namespace and -egress" jqt ex "${EGRESS_NAMESPACES} == [\"sbx-alpha-allow-egress\"]"
    local prefix client long fits
    prefix="$(printf 'a%.0s' {1..22})"
    client="$(printf 'c%.0s' {1..31})"
    long="${prefix}-${client}-allow-egress"
    fails_with "BR_SANDBOX_NAME_TOO_LONG{client=${client},mode=allow,namespace=${long}}" -f "$(vals long ".installation.namespace_prefix = \"${prefix}\" | .clients = {\"${client}\": .clients.alpha}")"
    prefix="$(printf 'a%.0s' {1..20})"
    client="$(printf 'c%.0s' {1..29})"
    fits="${prefix}-${client}-allow-egress"
    render fits -f "$(vals fits ".installation.namespace_prefix = \"${prefix}\" | .clients = {\"${client}\": .clients.alpha}")"
    t "an egress name of 63 characters is accepted" jqt fits "${EGRESS_NAMESPACES} == [\"${fits}\"] and (\"${fits}\" | length) == 63"
    fails_with "BR_SANDBOX_SHARED_NAMESPACE{client_a=system,client_b=alpha,namespace=sbx-alpha-allow-egress}" -f "$(vals clash '.installation.system_namespace = "sbx-alpha-allow-egress"')"
}
