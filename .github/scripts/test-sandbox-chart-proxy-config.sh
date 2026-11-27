#!/usr/bin/env bash

SET0='.clients.alpha.modes.full.credential_set.secrets[0]'

cfgt() {
    local render="$1" ns="$2"
    shift 2
    jq -r --arg ns "$ns" '.[] | select(.kind == "ConfigMap" and .metadata.name == "br-sandbox-proxy-config" and .metadata.namespace == $ns) | .data["proxy.yaml"]' "$WORK/${render}.json" | yq -o=json '.' - | jq -e "$@" >/dev/null
}

cfgv() {
    local render="$1" ns="$2"
    shift 2
    jq -r --arg ns "$ns" '.[] | select(.kind == "ConfigMap" and .metadata.name == "br-sandbox-proxy-config" and .metadata.namespace == $ns) | .data["proxy.yaml"]' "$WORK/${render}.json" | yq -o=json '.' - | jq -r "$@"
}

render_proxy_config_fixtures() {
    render ipliteral -f "$EXAMPLE" -f "$EGRESS_VALUES" --set 'network.internal_cidrs={10.0.0.0/8,192.168.0.0/16,100.64.0.0/10,169.254.0.0/16,127.0.0.0/8}'
    render egressdual6 -f "$EXAMPLE" -f "$EGRESS_VALUES" --set network.ipv6=dual_stack
    render cidronly -f "$(valsx cidronly '.clients.alpha.modes.allow.allow = {"cidrs": ["192.0.2.10/32"]}')"
    render cidrhost -f "$(valsx cidrhost "${SET0}.hosts = [\"192.0.2.0/24\"]")"
    render caother -f "$EXAMPLE" -f "$EGRESS_VALUES" --set 'egress.ca.env_names={SSL_CERT_FILE}'
}

check_the_proxy_config_has_the_two_transforms_in_order_and_the_standalone_shape() {
    section "proxy config: standalone, transforms allowlist then secrets, no DNS service, loopback listeners"
    t "mode allow has the allowlist transform alone" cfgt cred sbx-alpha-allow-egress '[.transforms[].name] == ["allowlist"]'
    t "mode full with a credential set has allowlist then secrets" cfgt cred sbx-alpha-full-egress '[.transforms[].name] == ["allowlist", "secrets"]'
    t "the config keys are the standalone keys and no other" cfgt cred sbx-alpha-full-egress 'keys == ["dns", "log", "management", "metrics", "proxy", "tls", "transforms"]'
    t "the DNS service is off and the proxy keys are the listeners and the deny list" cfgt cred sbx-alpha-full-egress '.dns == {"enabled": false} and (.proxy | keys == ["http_listen", "https_listen", "tunnel_listen", "upstream_deny_cidrs"])'
    t "the http, https, metrics and reload listeners are loopback: no sandbox reaches them" cfgt cred sbx-alpha-full-egress '[.proxy.http_listen, .proxy.https_listen, .metrics.listen, .management.listen] | all(startswith("127.0.0.1:"))'
    t "the tunnel listens on all addresses at the port of the value" cfgt cred sbx-alpha-full-egress '.proxy.tunnel_listen == "0.0.0.0:8888"'
    t "the tunnel port is the value egress.proxy.port" cfgt egresssized sbx-alpha-allow-egress '.proxy.tunnel_listen == "0.0.0.0:9999"'
    t "the reload listener, its port and the other listeners are all distinct" cfgt cred sbx-alpha-full-egress '[.proxy.http_listen, .proxy.https_listen, .metrics.listen, .management.listen, .proxy.tunnel_listen] | (map(split(":")[-1]) | unique | length) == 5'
    t "the reload key is read from the environment and never written in the config" cfgt cred sbx-alpha-full-egress '.management.api_key_env == "IRON_MANAGEMENT_API_KEY" and (tostring | test("proxy-reload") | not)'
    t "the TLS mode is mitm with the mounted CA certificate and key" cfgt cred sbx-alpha-full-egress '.tls == {"mode": "mitm", "ca_cert": "/etc/iron-proxy/ca/ca.crt", "ca_key": "/etc/iron-proxy/ca-key/ca.key"}'
    t "the Deployment mounts the files at the paths the config names" jqt cred ".[] | select(.kind == \"Deployment\" and .metadata.namespace == \"sbx-alpha-full-egress\") | .spec.template.spec.containers[0] | [.volumeMounts[] | select(.name == \"ca\" or .name == \"ca-key\" or .name == \"config\" or .name == \"credentials\") | .mountPath] | sort == [\"/etc/iron-proxy/ca\", \"/etc/iron-proxy/ca-key\", \"/etc/iron-proxy/config\", \"/etc/iron-proxy/credentials\"]"
    t "the Service and the container expose the tunnel port only: the reload port is on no object" jqt cred '[.[] | select((.kind == "Deployment" or .kind == "Service") and (.metadata.namespace | endswith("-egress"))) | (.spec.template.spec.containers[0].ports // .spec.ports) | length] == [1, 1, 1, 1]'
}

check_the_deny_list_equals_the_policy_deny_list() {
    section "upstream_deny_cidrs: the deny list of the policy, plus loopback, nothing else"
    local want
    want="$(jqv cred '.[] | select(.kind == "NetworkPolicy" and .metadata.namespace == "sbx-alpha-allow-egress" and .metadata.name == "br-sandbox-egress-proxy") | .spec.egress[0].to[0].ipBlock.except' | jq -c '. + ["::1/128"]')"
    t "the config deny list is exactly that list, in that order" cfgt cred sbx-alpha-allow-egress ".proxy.upstream_deny_cidrs == ${want}"
    t "the deny list holds the internal ranges, the pod and service ranges, the node, public node and load balancer addresses" cfgt cred sbx-alpha-allow-egress '.proxy.upstream_deny_cidrs | contains(["10.0.0.0/8", "10.42.0.0/16", "10.43.0.0/16", "10.0.0.11/32", "203.0.113.11/32", "203.0.113.20/32", "127.0.0.0/8"])'
    t "dual stack adds the IPv6 ranges of the policy" cfgt egressdual6 sbx-alpha-allow-egress '.proxy.upstream_deny_cidrs | contains(["fc00::/7", "fe80::/10", "::1/128"])'
    t "single stack has no IPv6 range but the loopback" cfgt cred sbx-alpha-allow-egress '[.proxy.upstream_deny_cidrs[] | select(contains(":"))] == ["::1/128"]'
    t "an internal_cidrs override changes the deny list of the proxy the same way as the policy" cfgt ipliteral sbx-alpha-allow-egress '(.proxy.upstream_deny_cidrs | index("172.16.0.0/12")) == null and (.proxy.upstream_deny_cidrs | index("10.0.0.0/8")) != null and (.proxy.upstream_deny_cidrs | index("127.0.0.0/8")) != null'
    t "the same override drops the range from the policy except list" jqt ipliteral '.[] | select(.kind == "NetworkPolicy" and .metadata.namespace == "sbx-alpha-allow-egress" and .metadata.name == "br-sandbox-egress-proxy") | .spec.egress[0].to[0].ipBlock.except | index("172.16.0.0/12") == null and index("10.0.0.0/8") != null'
}

check_the_allowlist_renders_domains_and_cidrs_of_the_mode() {
    section "allowlist: the domains and the cidrs of the mode, the wildcard for full"
    t "mode allow renders its domains and its cidrs, in order" cfgt cred sbx-alpha-allow-egress '.transforms[0] == {"name": "allowlist", "config": {"domains": ["git.example.test", "*.packages.example.test"], "cidrs": ["192.0.2.0/24"]}}'
    t "mode allow with a cidr and no domain renders the cidr and no domain" cfgt cidronly sbx-alpha-allow-egress '.transforms[0].config == {"domains": [], "cidrs": ["192.0.2.10/32"]}'
    t "an IP-literal target needs only a cidr entry: the cidr list is independent of the domain list and of the deny list when the range is not denied" cfgt ipliteral sbx-alpha-allow-egress '.transforms[0].config.cidrs == ["192.0.2.0/24"] and (.proxy.upstream_deny_cidrs | index("192.0.2.0/24")) == null'
    t "mode full renders the wildcard domain and no cidr" cfgt cred sbx-alpha-full-egress '.transforms[0] == {"name": "allowlist", "config": {"domains": ["*"]}}'
    t "the config of the proxy has no dependency on a name: the DNS service is off in every proxy" jqt cred '[.[] | select(.kind == "ConfigMap" and .metadata.name == "br-sandbox-proxy-config") | .data["proxy.yaml"] | test("dns:\\n  enabled: false")] | all'
}

check_the_secrets_transform_renders_the_credential_set() {
    section "secrets transform: source file, placeholder, match fields, rules"
    t "the first secret reads its key file, swaps the placeholder in the Authorization header on its host, TLS required" cfgt cred sbx-alpha-full-egress '.transforms[1].config.secrets[0] == {"source": {"type": "file", "path": "/etc/iron-proxy/credentials/git"}, "replace": {"proxy_value": "placeholder-git-a", "match_headers": ["Authorization"], "match_query": false, "match_path": false, "match_body": false, "require": false}, "rules": [{"host": "git.example.test", "require_tls": true}]}'
    t "the second secret takes its headers, query, require and plain-text rule from the values" cfgt cred sbx-alpha-full-egress '.transforms[1].config.secrets[1] == {"source": {"type": "file", "path": "/etc/iron-proxy/credentials/registry"}, "replace": {"proxy_value": "placeholder-registry-a", "match_headers": ["X-Registry-Auth"], "match_query": true, "match_path": false, "match_body": false, "require": true}, "rules": [{"host": "*.packages.example.test", "require_tls": false}]}'
    t "a host with a slash becomes a cidr rule" cfgt cidrhost sbx-alpha-full-egress '.transforms[1].config.secrets[0].rules == [{"cidr": "192.0.2.0/24", "require_tls": true}]'
    t "the config holds no secret value: only placeholders and file paths" test -z "$(jq -r '.[] | select(.kind == "ConfigMap") | .data | tostring' "$WORK/cred.json" | grep -E 'ghp_|Bearer ' || true)"
    t "the placeholders are the only credential-looking values in the sandbox pods" jqt cred "[${SANDBOX_PODS}[] | .spec.containers[0].env[]? | select(.name == \"GIT_ACCESS\" or .name == \"REGISTRY_ACCESS\") | .value] | sort == [\"placeholder-git-a\", \"placeholder-registry-a\"]"
}

check_the_proxy_leaks_nothing_of_the_vendor_platform() {
    section "no token, no tenancy, no telemetry, no vendor host, no response scrub"
    local pattern='iron\.sh|IRON_PROXY_TOKEN|tenancy|otel|OTEL_EXPORTER|RESPONSE_RETRY|response_retry|scrub'
    t "no template and no value names a vendor service, a token, a tenancy, telemetry or a response step" test -z "$(grep -rniE "$pattern" "$CHART/templates" "$CHART/values.yaml" "$CHART/values.schema.json" "$CHART/ci" || true)"
    t "no object of the egress render names them either" test -z "$(grep -niE "$pattern" "$WORK/egress_yaml.yaml" || true)"
    t "the egress render names no host but the example hosts and the Kubernetes names, and the product image is the one pinned reference" test "$(hosts_of <(grep -vE '^\s+image: "?docker\.io/ironsh/iron-proxy@sha256:[a-f0-9]{64}"?$' "$WORK/egress_yaml.yaml") | grep -vE '^(admissionregistration\.k8s\.io|app\.kubernetes\.io|bus\.example\.test|db\.example\.test|git\.example\.test|kubernetes\.default\.svc\.cluster\.local|kubernetes\.io|networking\.k8s\.io|packages\.example\.test|pod-security\.kubernetes\.io|rbac\.authorization\.k8s\.io|registry\.example\.test|store\.example\.test)$' | wc -l | tr -d ' ')" = 0
    t "the proxy container has no environment but the reload key" jqt cred '[.[] | select(.kind == "Deployment" and .metadata.name == "br-sandbox-proxy") | .spec.template.spec.containers[0].env | map(.name)] | all(. == ["IRON_MANAGEMENT_API_KEY"])'
    t "the transforms of every proxy are among the two the product needs" jqt cred '[.[] | select(.kind == "ConfigMap" and .metadata.name == "br-sandbox-proxy-config") | .data["proxy.yaml"] | test("name: (allowlist|secrets)")] | all'
}

check_the_sandbox_side_holds_the_proxy_address_the_ca_and_the_placeholders() {
    section "sandbox side: proxy env, CA mount, placeholders, no real value, no DNS"
    local env='.[] | select(.kind == "ReplicaSet" and .metadata.namespace == "%s") | .spec.template.spec.containers[0].env'
    t "mode full: the proxy variables, the empty no-proxy, the CA variables, then the placeholders" jqt cred "$(printf "$env" sbx-alpha-full) | map({(.name): .value}) | add | .HTTPS_PROXY == \"http://10.43.0.51:8888\" and .HTTP_PROXY == .HTTPS_PROXY and .https_proxy == .HTTPS_PROXY and .http_proxy == .HTTPS_PROXY and .NO_PROXY == \"\" and .no_proxy == \"\" and .SSL_CERT_FILE == \"/etc/br-sandbox-ca/ca.crt\" and .REQUESTS_CA_BUNDLE == .SSL_CERT_FILE and .NODE_EXTRA_CA_CERTS == .SSL_CERT_FILE and .GIT_SSL_CAINFO == .SSL_CERT_FILE and .CURL_CA_BUNDLE == .SSL_CERT_FILE and .GIT_ACCESS == \"placeholder-git-a\" and .REGISTRY_ACCESS == \"placeholder-registry-a\""
    t "mode allow: the proxy address of its own mode and no placeholder" jqt cred "$(printf "$env" sbx-alpha-allow) | map({(.name): .value}) | add | .HTTPS_PROXY == \"http://10.43.0.50:8888\" and (has(\"GIT_ACCESS\") | not)"
    t "mode none and the other client have no proxy variable and no CA mount" jqt cred '[.[] | select(.kind == "ReplicaSet" and (.metadata.namespace == "sbx-alpha-none" or .metadata.namespace == "sbx-beta-none")) | .spec.template.spec | [.containers[].env[]?.name | select(test("PROXY|proxy|CA_BUNDLE|SSL_CERT|EXTRA_CA|CAINFO"))] + [.volumes[].name | select(. == "egress-ca")] | length] == [0, 0]'
    t "the CA ConfigMap is mounted read-only at the mount path and is the only added volume" jqt cred '.[] | select(.kind == "ReplicaSet" and .metadata.namespace == "sbx-alpha-full") | .spec.template.spec | ([.volumes[] | select(.name == "egress-ca")] == [{"name": "egress-ca", "configMap": {"name": "br-sandbox-ca"}}]) and ([.containers[0].volumeMounts[] | select(.name == "egress-ca")] == [{"name": "egress-ca", "mountPath": "/etc/br-sandbox-ca", "readOnly": true}])'
    t "the CA variable names are values and the mount path is the constant /etc/br-sandbox-ca" jqt caother '.[] | select(.kind == "ReplicaSet" and .metadata.namespace == "sbx-alpha-full") | .spec.template.spec.containers[0] | ([.env[] | select(.value == "/etc/br-sandbox-ca/ca.crt") | .name] == ["SSL_CERT_FILE"]) and ([.volumeMounts[] | select(.name == "egress-ca") | .mountPath] == ["/etc/br-sandbox-ca"])'
    fails_with "values don't meet the specifications" -f "$EXAMPLE" -f "$EGRESS_VALUES" --set egress.ca.mount_path=/etc/other-ca
    t "the ConfigMap of the sandbox namespace holds the certificate and no key" jqt cred '[.[] | select(.kind == "ConfigMap" and .metadata.name == "br-sandbox-ca") | .data | keys] == [["ca.crt"], ["ca.crt"]]'
    t "no sandbox env or mount refers to a Secret, the credential key files or the proxy mount paths" jqt cred "${SANDBOX_PODS} | (tostring | test(\"iron-proxy|/credentials|secretKeyRef\") | not)"
    t "the sandbox resolver is still the local address with the proxy address in the env" jqt cred '.[] | select(.kind == "ReplicaSet" and .metadata.namespace == "sbx-alpha-full") | .spec.template.spec.dnsConfig.nameservers == ["127.0.0.1"]'
}

check_the_proxy_render_errors() {
    section "render errors of the proxy and the credential set"
    fails_with "BR_SANDBOX_CREDENTIAL_HOSTS_EMPTY{client=alpha,mode=full,item=0}" -f "$(valsx errhosts "${SET0}.hosts = []")"
    fails_with "BR_SANDBOX_CREDENTIAL_PLACEHOLDER_EMPTY{client=alpha,mode=full,item=0}" -f "$(valsx errph "${SET0}.placeholder = \"\"")"
    fails_with "BR_SANDBOX_CREDENTIAL_REQUIRE_TLS{client=alpha,mode=full,item=0}" -f "$(valsx errtls "${SET0}.require = true")"
    fails_with "BR_SANDBOX_CREDENTIAL_PLACEHOLDER_DUPLICATE{client=alpha,mode=full,item=1,first=0}" -f "$(valsx errdupph "${SET0}.placeholder as \$p | .clients.alpha.modes.full.credential_set.secrets[1].placeholder = \$p")"
    fails_with "BR_SANDBOX_CREDENTIAL_ENV_DUPLICATE{client=alpha,mode=full,item=1,env_name=GIT_ACCESS}" -f "$(valsx errdupenv '.clients.alpha.modes.full.credential_set.secrets[1].env_name = "GIT_ACCESS"')"
    fails_with "BR_SANDBOX_CREDENTIAL_KEY_DUPLICATE{client=alpha,mode=full,item=1,key=git}" -f "$(valsx errdupkey '.clients.alpha.modes.full.credential_set.secrets[1].key = "git"')"
    fails_with "BR_SANDBOX_PROXY_ADDRESS_DUPLICATE{client_a=alpha,client_b=alpha,cluster_ip=10.43.0.50}" -f "$(valsx errdupip '.clients.alpha.modes.full.proxy.cluster_ip = "10.43.0.50"')"
    fails_with "BR_SANDBOX_MISSING_VALUE{key=clients.alpha.modes.allow.proxy.cluster_ip}" -f "$(valsx errnoip 'del(.clients.alpha.modes.allow.proxy.cluster_ip)')"
    fails_with "BR_SANDBOX_MISSING_VALUE{key=clients.alpha.modes.allow.proxy.ca.certificate}" -f "$(valsx errnoca 'del(.clients.alpha.modes.allow.proxy.ca.certificate)')"
    fails_with "BR_SANDBOX_MISSING_VALUE{key=clients.alpha.modes.allow.proxy.ca.key_secret}" -f "$(valsx errnokey 'del(.clients.alpha.modes.allow.proxy.ca.key_secret)')"
    fails_with "BR_SANDBOX_MISSING_VALUE{key=egress.proxy.reload.key_secret}" -f "$EXAMPLE" -f "$EGRESS_VALUES" --set egress.proxy.reload.key_secret=
    fails_with "BR_SANDBOX_PROXY_PORT_CONFLICT{port=8080}" -f "$EXAMPLE" -f "$EGRESS_VALUES" --set egress.proxy.port=8080
    fails_with "BR_SANDBOX_PROXY_PORT_CONFLICT{port=8443}" -f "$EXAMPLE" -f "$EGRESS_VALUES" --set egress.proxy.port=8443
    fails_with "BR_SANDBOX_PROXY_PORT_CONFLICT{port=9090}" -f "$EXAMPLE" -f "$EGRESS_VALUES" --set egress.proxy.port=9090
    fails_with "BR_SANDBOX_PROXY_PORT_CONFLICT{port=9092}" -f "$EXAMPLE" -f "$EGRESS_VALUES" --set egress.proxy.port=9092
    fails_with "BR_SANDBOX_PROXY_QUOTA{replicas=" -f "$EXAMPLE" -f "$EGRESS_VALUES" --set egress.proxy.replicas=50
    fails_with "BR_SANDBOX_PROXY_QUOTA{replicas=4,pods=4}" -f "$EXAMPLE" -f "$EGRESS_VALUES" --set egress.quota.pods=4 --set egress.proxy.replicas=4
    t "a credential set with a plain-text rule and require false renders" render plainok -f "$EXAMPLE" -f "$EGRESS_VALUES"
    t "egress disabled needs no proxy value: the example renders with no proxy key" render noegress -f "$EXAMPLE"
}

check_the_credential_set_needs_both_keys_and_one_item() {
    section "credential_set: secret_ref and secrets are given together, secrets has at least one item"
    fails_with "properties 'secrets' required, if 'secret_ref' exists" -f "$(valsx setrefonly 'del(.clients.alpha.modes.full.credential_set.secrets)')"
    fails_with "properties 'secret_ref' required, if 'secrets' exists" -f "$(valsx setsecretsonly 'del(.clients.alpha.modes.full.credential_set.secret_ref)')"
    fails_with "minItems: got 0, want 1" -f "$(valsx setemptysecrets '.clients.alpha.modes.full.credential_set.secrets = []')"
    t "a set with secret_ref and one item renders" render setone -f "$(valsx setone ".clients.alpha.modes.full.credential_set = ${CREDENTIAL_SET}")"
    t "that set renders its egress namespace" jqt setone '[.[] | select(.kind == "Namespace" and .metadata.name == "sbx-alpha-full-egress")] | length == 1'
}

check_the_quota_guard_keeps_one_pod_for_the_surge() {
    section "the quota guard: replicas plus one pod must fit egress.quota.pods"
    t "a pod quota of 4 and 3 replicas renders" render quotafit3 -f "$EXAMPLE" -f "$EGRESS_VALUES" --set egress.quota.pods=4 --set egress.proxy.replicas=3
    t "a pod quota of 5 and 4 replicas renders" render quotafit4 -f "$EXAMPLE" -f "$EGRESS_VALUES" --set egress.quota.pods=5 --set egress.proxy.replicas=4
    fails_with "BR_SANDBOX_PROXY_QUOTA{replicas=5,pods=5}" -f "$EXAMPLE" -f "$EGRESS_VALUES" --set egress.quota.pods=5 --set egress.proxy.replicas=5
    t "the README states the boundary of the quota guard" grep -qF 'with a pod quota of 4, 3 replicas render and 4 replicas fail; with a pod quota of 5, 4 replicas render.' "$README"
    t "the CHANGELOG states the boundary of the quota guard" grep -qF 'with a pod quota of 4, 3 replicas render and 4 replicas fail; with a pod quota of 5, 4 replicas render.' "$CHART/CHANGELOG.md"
}
