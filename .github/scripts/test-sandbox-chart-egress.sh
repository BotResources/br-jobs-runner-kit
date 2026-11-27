#!/usr/bin/env bash

EGRESS_NAMESPACES='[.[] | select(.kind == "Namespace" and (.metadata.labels["br.sandbox/egress"] // "") == "true") | .metadata.name]'
TUNNEL_SOURCE='{"from": [{"namespaceSelector": {"matchLabels": {"kubernetes.io/metadata.name": "%s"}}, "podSelector": {"matchLabels": {"br.sandbox/role": "sandbox"}}}], "ports": [{"protocol": "TCP", "port": %s}]}'
PROXY_ONLY='{"to": [{"namespaceSelector": {"matchLabels": {"kubernetes.io/metadata.name": "%s"}}, "podSelector": {"matchLabels": {"br.sandbox/role": "proxy"}}}], "ports": [{"protocol": "TCP", "port": %s}]}'

render_egress_fixtures() {
    render cred -f "$EXAMPLE" -f "$EGRESS_VALUES" -f "$CHART/ci/fence-values.yaml"
    render two -f "$(valsx two '.namespaces += [{"name": "sbx-beta-allow", "mode": "allow", "allow": {"domains": ["git.example.test"]}, "proxy": {"cluster_ip": "10.43.0.60", "ca": {"certificate": "-----BEGIN CERTIFICATE-----\nZmFrZS1jYS1hbGxvdw==\n-----END CERTIFICATE-----\n", "key_secret": "sbx-beta-allow-ca-key"}}}]')"
    render egressdual -f "$(vals dualegress '.network.ipv6 = "dual_stack"')" -f "$CHART/ci/fence-values.yaml"
    render tunnel -f "$EXAMPLE" -f "$CHART/ci/fence-values.yaml" --set egress.proxy.port=9999 --set egress.quota.pods=7
    render keeperegress -f "$EXAMPLE" -f "$OPERATOR_VALUES" -f "$EGRESS_VALUES"
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
    fails_with "BR_SANDBOX_SHARED_NAMESPACE{first=egress,second=runner-of-sbx-alpha,namespace=sbx-alpha-allow-egress}" -f "$(vals runneregress '.scopes.sbx-alpha.runner.namespace = "sbx-alpha-allow-egress"')"
}

check_no_cluster_wide_right_on_secrets_and_pods_beyond_the_network_check_read() {
    section "cluster-wide rights: none on secrets, none on pods but the read of the network check"
    local render
    for render in keeper cred keeperegress; do
        t "in the ${render} render no ClusterRole rule names secrets or a wildcard resource" jqt "$render" '[.[] | select(.kind == "ClusterRole") | .rules[].resources[] | select(. == "secrets" or . == "*")] | length == 0'
    done
    t "the keeperegress render holds the keeper and the proxy Deployments, so the checks above read both" jqt keeperegress '[.[] | select(.kind == "Deployment") | .metadata.name] | index("br-sandbox-proxy") != null and (map(select(test("keeper"))) | length) > 0'
    t "the only cluster-wide pod right is get and list in the network check ClusterRole" jqt keeper '[.[] | select(.kind == "ClusterRoleBinding") | .roleRef.name] as $bound | [.[] | select(.kind == "ClusterRole" and (.metadata.name | IN($bound[]))) | select(any(.rules[]; .resources | any(. == "pods" or startswith("pods/")))) | {name: .metadata.name, rules: [.rules[] | select(.resources | any(. == "pods" or startswith("pods/")))]}] == [{"name": "sbx-system-netcheck", "rules": [{"apiGroups": [""], "resources": ["nodes", "services", "endpoints", "pods"], "verbs": ["get", "list"]}]}]'
    t "every ClusterRoleBinding names a ClusterRole of the render and a ServiceAccount" jqt keeper '[.[] | select(.kind == "ClusterRole") | .metadata.name] as $c | [.[] | select(.kind == "ClusterRoleBinding") | select((.roleRef.name as $r | $c | index($r) != null) and (.subjects | all(.kind == "ServiceAccount")) | not)] | length == 0'
    t "every RoleBinding names a Role of the render or one of the two ClusterRoles of the fence, br-sandbox-runner and br-sandbox-controller" jqt keeper '[.[] | select(.kind == "ClusterRole") | .metadata.name] as $c | [.[] | select(.kind == "RoleBinding") | select(.roleRef.kind != "Role" and ((.roleRef.name | IN("br-sandbox-runner", "br-sandbox-controller")) and (.roleRef.name | IN($c[])) | not))] | length == 0'
}

check_the_ca_certificate_stays_in_the_sandbox_namespace() {
    section "the CA certificate is a ConfigMap in both namespaces, the Secrets are not rendered"
    t "no Secret is rendered" jqt cred '[.[] | select(.kind == "Secret")] | length == 0'
    t "the CA certificate ConfigMap of the sandbox namespace holds the value of the mode and no key" jqt cred '[.[] | select(.kind == "ConfigMap" and .metadata.name == "br-sandbox-ca")] | map({(.metadata.namespace): .data}) | add | keys == ["sbx-alpha-allow", "sbx-alpha-full"] and (.["sbx-alpha-allow"]["ca.crt"] | test("ZmFrZS1jYS1hbGxvdw")) and (.["sbx-alpha-full"]["ca.crt"] | test("ZmFrZS1jYS1mdWxs")) and ([.[] | .["ca.crt"] | test("PRIVATE")] | any | not)'
    t "the egress namespaces hold the Namespace, the policies, the quota, two ConfigMaps, the Deployment and the Service" jqt cred "${EGRESS_NAMESPACES} as \$e | [.[] | select((.metadata.namespace as \$n | \$e | index(\$n) != null) or (.kind == \"Namespace\" and (.metadata.name as \$n | \$e | index(\$n) != null))) | .kind] | unique == [\"ConfigMap\", \"Deployment\", \"Namespace\", \"NetworkPolicy\", \"ResourceQuota\", \"Service\"]"
    t "the README places the CA certificate in the sandbox namespace" grep -qF 'The CA certificate (not secret) stays in the sandbox namespace' "$README"
    t "the README places the CA key only in the Secret of the egress namespace" grep -qF 'The CA key exists only in the Secret of the egress namespace' "$README"
}

check_the_sandbox_policy_reaches_only_the_proxy_pods_of_the_egress_namespace() {
    section "sandbox policy, modes allow and full with credentials: the proxy pods of the egress namespace only"
    local want
    want="$(printf "$PROXY_ONLY" sbx-alpha-allow-egress 8888)"
    t "allow: one rule, namespace and pod selector in the same item, TCP on the tunnel port, no DNS" jqt cred "$(policy_json sbx-alpha-allow br-sandbox-egress-allow) | .spec.policyTypes == [\"Egress\"] and .spec.podSelector == {\"matchLabels\": {\"br.sandbox/role\": \"sandbox\"}} and .spec.egress == [${want}]"
    want="$(printf "$PROXY_ONLY" sbx-alpha-full-egress 8888)"
    t "full with credentials: the same rule towards its own egress namespace, no public address, no DNS" jqt cred "$(policy_json sbx-alpha-full br-sandbox-egress-full) | .spec.policyTypes == [\"Egress\"] and .spec.egress == [${want}]"
    t "full with credentials has br-sandbox-default-deny and br-sandbox-egress-full only" test "$(jqv cred '[.[] | select(.kind == "NetworkPolicy" and .metadata.namespace == "sbx-alpha-full") | .metadata.name] | sort | join(",")')" = "br-sandbox-default-deny,br-sandbox-egress-full"
    want="$(printf "$PROXY_ONLY" sbx-alpha-allow-egress 9999)"
    t "the tunnel port is the value egress.proxy.port" jqt tunnel "$(policy_json sbx-alpha-allow br-sandbox-egress-allow) | .spec.egress == [${want}]"
    t "full with credentials resolves nothing outside: the pod resolver is the local address" jqt cred '.[] | select(.kind == "ConfigMap" and .metadata.name == "br-sandbox-pod-template" and .metadata.namespace == "sbx-alpha-full") | (.data["pod-template.json"] | fromjson).spec.dnsConfig.nameservers == ["127.0.0.1"]'
    t "the sandbox policy of a mode with a proxy names no ipBlock" jqt cred '[.[] | select(.kind == "NetworkPolicy" and (.metadata.namespace == "sbx-alpha-allow" or .metadata.namespace == "sbx-alpha-full")) | .. | objects | select(has("ipBlock"))] | length == 0'
}

check_the_egress_namespace_has_its_own_policies() {
    section "egress namespace policies: br-sandbox-default-deny, tunnel ingress from its own sandbox namespace only, barriers on egress"
    t "the egress namespace holds br-sandbox-default-deny, br-sandbox-egress-proxy and br-sandbox-ingress-tunnel" test "$(jqv cred '[.[] | select(.kind == "NetworkPolicy" and .metadata.namespace == "sbx-alpha-allow-egress") | .metadata.name] | sort | join(",")')" = "br-sandbox-default-deny,br-sandbox-egress-proxy,br-sandbox-ingress-tunnel"
    t "br-sandbox-default-deny selects every pod, both directions, no rule" jqt cred "$(policy_json sbx-alpha-allow-egress br-sandbox-default-deny) | .spec.podSelector == {} and (.spec.policyTypes | sort) == [\"Egress\", \"Ingress\"] and (.spec | has(\"ingress\") or has(\"egress\") | not)"
    local want
    want="$(printf "$TUNNEL_SOURCE" sbx-alpha-allow 8888)"
    t "br-sandbox-ingress-tunnel: from the sandbox namespace of the same client and mode, role sandbox, the tunnel port only" jqt cred "$(policy_json sbx-alpha-allow-egress br-sandbox-ingress-tunnel) | .spec.policyTypes == [\"Ingress\"] and .spec.podSelector == {\"matchLabels\": {\"br.sandbox/role\": \"proxy\"}} and .spec.ingress == [${want}]"
    want="$(printf "$TUNNEL_SOURCE" sbx-alpha-full 8888)"
    t "the egress namespace of full with credentials takes its own sandbox namespace" jqt cred "$(policy_json sbx-alpha-full-egress br-sandbox-ingress-tunnel) | .spec.ingress == [${want}]"
    want="$(printf "$TUNNEL_SOURCE" sbx-beta-allow 8888)"
    t "two clients: each egress namespace takes its own sandbox namespace and no other" jqt two "$(policy_json sbx-beta-allow-egress br-sandbox-ingress-tunnel) | .spec.ingress == [${want}]"
    want="$(printf "$TUNNEL_SOURCE" sbx-alpha-allow 9999)"
    t "the reload port is reached by nobody: the only ingress port is the tunnel port, a value" jqt tunnel "[.[] | select(.kind == \"NetworkPolicy\" and (.metadata.namespace | endswith(\"-egress\"))) | .spec.ingress[]?] | all(.ports == [{\"protocol\": \"TCP\", \"port\": 9999}])"
    t "no policy of an egress namespace but br-sandbox-ingress-tunnel has an ingress rule" jqt cred '[.[] | select(.kind == "NetworkPolicy" and (.metadata.namespace | endswith("-egress")) and (.spec | has("ingress"))) | .metadata.name] | unique == ["br-sandbox-ingress-tunnel"]'
    t "br-sandbox-egress-proxy opens the public addresses except every internal list and selects no pod or namespace" jqt cred "$(policy_json sbx-alpha-allow-egress br-sandbox-egress-proxy) | .spec.policyTypes == [\"Egress\"] and .spec.podSelector == {\"matchLabels\": {\"br.sandbox/role\": \"proxy\"}} and (.spec.egress | length) == 1 and .spec.egress[0].to[0].ipBlock.cidr == \"0.0.0.0/0\" and ([\"10.0.0.0/8\", \"172.16.0.0/12\", \"192.168.0.0/16\", \"100.64.0.0/10\", \"169.254.0.0/16\", \"127.0.0.0/8\", \"10.42.0.0/16\", \"10.43.0.0/16\", \"10.0.0.11/32\", \"10.0.0.12/32\", \"203.0.113.11/32\", \"203.0.113.12/32\", \"203.0.113.20/32\"] - .spec.egress[0].to[0].ipBlock.except == []) and ([.spec.egress[] | .. | objects | select(has(\"podSelector\") or has(\"namespaceSelector\"))] | length == 0)"
    t "dual stack adds ::/0 without fc00::/7 and fe80::/10" jqt egressdual "$(policy_json sbx-alpha-allow-egress br-sandbox-egress-proxy) | .spec.egress[1].to[0].ipBlock | .cidr == \"::/0\" and (.except | index(\"fc00::/7\")) != null and (.except | index(\"fe80::/10\")) != null"
    t "the egress namespace has a quota on pods, a value" jqt tunnel '.[] | select(.kind == "ResourceQuota" and .metadata.namespace == "sbx-alpha-allow-egress") | .spec.hard == {"pods": "7"}'
}

check_the_admission_policy_accepts_the_sandbox_role_only() {
    section "admission policy: role=sandbox only, no proxy case, no controller value"
    t "the role rule is one case" expression_is_exactly BR_SANDBOX_ROLE
    t "no policy, binding or value names a proxy case or a controller user; only the non-root rule of the pod policy and the writer clauses of the namespace policy read the user" test -z "$(grep -rnE 'proxyCreator|proxy_controller|replicaset-controller' "$CHART/templates" "$CHART/values.yaml" "$CHART/values.schema.json" || true)$(grep -rnE 'userInfo' "$CHART/templates" "$CHART/values.yaml" "$CHART/values.schema.json" | grep -v -e '_vap.tpl:' -e '_vap_namespace.tpl:' -e 'admission-namespace.yaml:' || true)"
    t "no admission block in the values" test "$(yq '.admission' "$CHART/values.yaml")" = null
    t "the bindings select the managed label, which no egress namespace carries" jqt cred '[.[] | select(.kind == "ValidatingAdmissionPolicyBinding") | .spec.matchResources.namespaceSelector.matchLabels] | all(. == {"br.sandbox/managed": "true"}) and ([.[] | select(.kind == "Namespace" and (.metadata.name | endswith("-egress")) and (.metadata.labels["br.sandbox/managed"] == "true"))] | length == 0)'
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
    t "full without a credential set has no egress namespace and keeps br-sandbox-egress-full to the public addresses" jqt ex '([.[] | select(.kind == "Namespace" and .metadata.name == "sbx-alpha-full-egress")] | length == 0) and ([.[] | select(.kind == "NetworkPolicy" and .metadata.namespace == "sbx-alpha-full" and .metadata.name == "br-sandbox-egress-full") | .spec.egress[0].to[0].ipBlock.cidr] == ["0.0.0.0/0"])'
    t "an empty credential set is no credential set" render emptyset -f "$(valsx emptyset '(.namespaces[] | select(.name == "sbx-alpha-full")).credential_set = {}')"
    t "an empty credential set renders no egress namespace for full" jqt emptyset '[.[] | select(.kind == "Namespace" and .metadata.name == "sbx-alpha-full-egress")] | length == 0'
    t "a credential set of full renders its egress namespace: three policies, the quota, two ConfigMaps, the Deployment and the Service" jqt cred '[.[] | select(.metadata.namespace == "sbx-alpha-full-egress") | .kind] | sort == ["ConfigMap", "ConfigMap", "Deployment", "NetworkPolicy", "NetworkPolicy", "NetworkPolicy", "ResourceQuota", "Service"]'
    t "mode none never has an egress namespace" jqt cred '[.[] | select(.kind == "Namespace" and (.metadata.name | test("none-egress$")))] | length == 0'
    fails_with "BR_SANDBOX_CREDENTIAL_SET_MODE{namespace=sbx-beta-none,mode=none}" -f "$(valsx credsetnone "(.namespaces[] | select(.name == \"sbx-beta-none\")).credential_set = ${CREDENTIAL_SET}")"
    fails_with "BR_SANDBOX_EGRESS_DISABLED{namespace=sbx-alpha-full,mode=full,key=credential_set}" -f "$(vals credsetoff "(.namespaces[] | select(.name == \"sbx-alpha-full\")).credential_set = ${CREDENTIAL_SET}")"
}

check_the_egress_namespace_name_is_a_helper_with_a_bound() {
    section "egress namespace name: one helper, 63 characters at most, a clash fails"
    t "the helper br-sandbox-chart.egress.namespace holds the suffix and no other template writes it" test "$(grep -rlF -e '%s-egress' "$CHART/templates")" = "$CHART/templates/_helpers.tpl"
    t "the name is the sandbox namespace and -egress" jqt ex "${EGRESS_NAMESPACES} == [\"sbx-alpha-allow-egress\"]"
    local long fits
    long="sbx-alpha-$(printf 'c%.0s' {1..47})"
    fails_with "BR_SANDBOX_NAME_TOO_LONG{namespace=${long},egress=${long}-egress}" -f "$(vals long ".namespaces[1].name = \"${long}\"")"
    fits="sbx-alpha-$(printf 'c%.0s' {1..46})"
    render fits -f "$(vals fits ".namespaces[1].name = \"${fits}\"")"
    fits="${fits}-egress"
    t "an egress name of 63 characters is accepted" jqt fits "${EGRESS_NAMESPACES} == [\"${fits}\"] and (\"${fits}\" | length) == 63"
    fails_with "BR_SANDBOX_SHARED_NAMESPACE{first=system,second=egress,namespace=sbx-alpha-allow-egress}" -f "$(vals clash '.installation.system_namespace = "sbx-alpha-allow-egress"')"
}

check_the_network_check_identity_reads_pod_specs_cluster_wide() {
    section "network check identity: cluster-wide read of pods, nodes, services and endpoints, stated in the README"
    t "the netcheck ClusterRole holds get and list on pods, nodes, services and endpoints, nothing else" jqt keeper '[.[] | select(.kind == "ClusterRole" and .metadata.name == "sbx-system-netcheck") | .rules] == [[{"apiGroups": [""], "resources": ["nodes", "services", "endpoints", "pods"], "verbs": ["get", "list"]}]]'
    t "a ClusterRoleBinding gives that ClusterRole to the netcheck ServiceAccount only" jqt keeper '[.[] | select(.kind == "ClusterRoleBinding" and .roleRef.name == "sbx-system-netcheck") | .subjects] == [[{"kind": "ServiceAccount", "name": "br-sandbox-netcheck", "namespace": "sbx-system"}]]'
    t "the README says the network check identity reads the pod specs of the whole cluster" grep -qF 'The network check identity reads the pod specs of the whole cluster (a cluster-wide `get` and `list` on `pods`, `nodes`, `services` and `endpoints`)' "$README"
}

check_the_proxy_pod_exists_only_when_egress_is_enabled_and_only_in_an_egress_namespace() {
    section "no proxy pod while egress is disabled; with egress enabled it lives in an egress namespace only"
    local name
    for name in ex keeper tunnel; do
        t "render ${name} (egress disabled): no workload holds the role proxy" jqt "$name" '[.[] | select(.kind == "Deployment" or .kind == "StatefulSet" or .kind == "DaemonSet" or .kind == "ReplicaSet" or .kind == "Pod" or .kind == "Job" or .kind == "CronJob") | select(([.. | objects | select(has("br.sandbox/role")) | .["br.sandbox/role"]] | index("proxy")) != null)] | length == 0'
        t "render ${name} (egress disabled): no Deployment, Pod or Service lives in an egress namespace" jqt "$name" "${EGRESS_NAMESPACES} as \$e | [.[] | select((.kind == \"Deployment\" or .kind == \"Pod\" or .kind == \"Service\") and (.metadata.namespace as \$n | \$e | index(\$n) != null))] | length == 0"
    done
    for name in cred two; do
        t "render ${name}: the role proxy is on workloads of egress namespaces only" jqt "$name" "${EGRESS_NAMESPACES} as \$e | [.[] | select(([.. | objects | select(has(\"br.sandbox/role\")) | .[\"br.sandbox/role\"]] | index(\"proxy\")) != null) | select(.kind == \"Deployment\") | .metadata.namespace] | length > 0 and all(. as \$n | \$e | index(\$n) != null)"
        t "render ${name}: the only Deployments of an egress namespace are proxy Deployments, one per namespace" jqt "$name" "${EGRESS_NAMESPACES} as \$e | ([.[] | select(.kind == \"Deployment\" and (.metadata.namespace as \$n | \$e | index(\$n) != null)) | .metadata.namespace] | sort) == (\$e | sort)"
        t "render ${name}: no Deployment, Service or Pod lives in a sandbox, system or mode-none namespace but the keeper's" jqt "$name" "${EGRESS_NAMESPACES} as \$e | [.[] | select((.kind == \"Deployment\" or .kind == \"Pod\" or .kind == \"Service\") and (.metadata.namespace as \$n | \$e | index(\$n) == null))] | length == 0"
    done
    t "the README says the proxy Deployment lives in the egress namespace and never in a sandbox namespace" grep -qF 'A proxy Deployment lives in an egress namespace and never in a sandbox namespace.' "$README"
    t "the README says that with egress disabled no proxy pod exists and a sandbox of a mode with a proxy has no egress once the policies are enforced" grep -qF 'With `egress.enabled: false` no proxy pod exists, so a sandbox of a mode with a proxy has no egress at all once the network plugin enforces the policies (fail closed in that steady state, see The enforcement window).' "$README"
}

check_the_credential_secrets_live_in_the_egress_namespace() {
    section "no Secret in a sandbox namespace or an egress namespace, stated in the README"
    local name
    for name in cred two keeper; do
        t "render ${name}: no Secret in any sandbox, egress or system namespace" jqt "$name" '[.[] | select(.kind == "Secret")] | length == 0'
    done
    t "the README says the credential Secret, the CA key Secret and the sealed Secrets are created in the egress namespace by the deploy tool" grep -qF 'The credential Secret, the CA key Secret and every sealed Secret of the proxy are created in the egress namespace by the deploy tool; the chart renders none.' "$README"
}

check_an_allow_list_outside_mode_allow_fails_the_render() {
    section "an allow list in a mode other than allow fails when egress is enabled"
    fails_with "BR_SANDBOX_ALLOW_MODE{namespace=sbx-alpha-none,mode=none}" -f "$(valsx allownone '(.namespaces[] | select(.name == "sbx-alpha-none")).allow.domains = ["git.example.test"]')"
    fails_with "BR_SANDBOX_ALLOW_MODE{namespace=sbx-alpha-full,mode=full}" -f "$(valsx allowfull '(.namespaces[] | select(.name == "sbx-alpha-full")).allow.cidrs = ["192.0.2.0/24"]')"
    fails_with "BR_SANDBOX_EGRESS_DISABLED{namespace=sbx-alpha-full,mode=full,key=allow}" -f "$(vals allowfulloff '(.namespaces[] | select(.name == "sbx-alpha-full")).allow.cidrs = ["192.0.2.0/24"]')"
    t "an allow list in mode allow renders when egress is enabled" render allowok -f "$(valsx allowok '(.namespaces[] | select(.name == "sbx-alpha-allow")).allow.domains = ["git.example.test"]')"
    t "empty allow lists in mode none and full render when egress is enabled" render allowempty -f "$(valsx allowempty '(.namespaces[] | select(.name == "sbx-alpha-none")).allow = {"domains": [], "cidrs": []} | (.namespaces[] | select(.name == "sbx-alpha-full")).allow = {"domains": [], "cidrs": []}')"
    t "the README names BR_SANDBOX_ALLOW_MODE" grep -qF 'BR_SANDBOX_ALLOW_MODE' "$README"
}

check_a_mode_allow_with_no_list_fails_the_render_when_egress_is_enabled() {
    section "a mode allow with no domain and no cidr fails when egress is enabled, and renders without a list when egress is disabled"
    fails_with "BR_SANDBOX_ALLOW_EMPTY{namespace=sbx-alpha-allow,mode=allow}" -f "$(valsx allowempty_deleted 'del((.namespaces[] | select(.name == "sbx-alpha-allow")).allow)')"
    fails_with "BR_SANDBOX_ALLOW_EMPTY{namespace=sbx-alpha-allow,mode=allow}" -f "$(valsx allowempty_lists '(.namespaces[] | select(.name == "sbx-alpha-allow")).allow = {"domains": [], "cidrs": []}')"
    fails_with "BR_SANDBOX_ALLOW_EMPTY{namespace=sbx-alpha-allow,mode=allow}" -f "$(vals allowempty_noproxy '.egress.enabled = true')"
    t "a mode allow with domains only renders when egress is enabled" render allowdomainsonly -f "$(valsx allowdomainsonly '(.namespaces[] | select(.name == "sbx-alpha-allow")).allow = {"domains": ["git.example.test"]}')"
    t "a mode allow with cidrs only renders when egress is enabled" render allowcidrsonly -f "$(valsx allowcidrsonly '(.namespaces[] | select(.name == "sbx-alpha-allow")).allow = {"cidrs": ["192.0.2.0/24"]}')"
    t "an empty allow list in mode none and mode full is not an error when egress is enabled" render allownoneempty -f "$(valsx allownoneempty '(.namespaces[] | select(.name == "sbx-alpha-none")).allow = {"domains": [], "cidrs": []}')"
    t "with egress disabled a mode allow with no list renders: no list can be given there" render allowoff -f "$EXAMPLE"
    t "the README names BR_SANDBOX_ALLOW_EMPTY and says it needs egress.enabled" grep -qF 'With `egress.enabled`, mode `allow` needs at least one domain or one cidr, else `BR_SANDBOX_ALLOW_EMPTY`.' "$README"
    t "the CHANGELOG names BR_SANDBOX_ALLOW_EMPTY" grep -qF 'BR_SANDBOX_ALLOW_EMPTY{client=...,mode=allow}' "$CHART/CHANGELOG.md"
    t "the CHANGELOG says that the render fault changes what 0.2.0 to 0.2.3 accepted" grep -qF 'This changes what 0.2.0 to 0.2.3 accepted' "$CHART/CHANGELOG.md"
}

check_the_cluster_default_policy_exclusion_is_stated_for_both_namespace_kinds() {
    section "README: the cluster default network policy exclusion covers managed and egress namespaces"
    t "the README says the cluster default network policy also acts in the egress namespace" grep -qF 'The cluster default network policy (the generated policy per namespace with allow rules for DNS, intra-namespace traffic and ingress controllers) also acts in the egress namespace' "$README"
    t "Needs of the cluster names the exclusion for br.sandbox/egress=true namespaces" grep -qF 'and for namespaces with `br.sandbox/egress=true`' "$README"
    t "the README no longer makes the egress exclusion conditional" test -z "$(grep -F 'If the cluster policy of the project needs an exclusion there' "$README" || true)"
}
