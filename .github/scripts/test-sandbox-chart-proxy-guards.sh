#!/usr/bin/env bash

ALLOW_NS='sbx-alpha-allow-egress'
FULL_NS='sbx-alpha-full-egress'
ALLOW_SET='{"secret_ref": "sbx-alpha-allow-credentials", "secrets": [{"placeholder": "placeholder-allow-a", "env_name": "ALLOW_ACCESS", "key": "a", "hosts": ["git.example.test"]}]}'
CERT_HEAD='-----BEGIN CERTIFICATE-----'
CERT_TAIL='-----END CERTIFICATE-----'
KEY_BLOCK='-----BEGIN PRIVATE KEY-----\nc2VjcmV0\n-----END PRIVATE KEY-----'

render_proxy_guard_fixtures() {
    render allowcred -f "$(valsx allowcred ".clients.alpha.modes.allow.credential_set = ${ALLOW_SET}")"
    render narrowed -f "$(valsx narrowed '.clients.alpha.modes.allow.allow.domains = ["git.example.test"]')"
    render widecidr -f "$(valsx widecidr '.clients.alpha.modes.allow.allow.cidrs = ["192.0.2.0/24", "198.51.100.0/24"]')"
    render otherhost -f "$(valsx otherhost "${SET0}.hosts = [\"git2.example.test\"]")"
    render otherca -f "$(valsx otherca ".clients.alpha.modes.allow.proxy.ca.certificate = \"${CERT_HEAD}\\nb3RoZXItY2E=\\n${CERT_TAIL}\\n\"")"
    render chain -f "$(valsx chain ".clients.alpha.modes.allow.proxy.ca.certificate = \"${CERT_HEAD}\\nYQ==\\n${CERT_TAIL}\\n${CERT_HEAD}\\nYg==\\n${CERT_TAIL}\\n\"")"
    render unrelated -f "$EXAMPLE" -f "$EGRESS_VALUES" --set egress.proxy.resources.requests.cpu=250m --set egress.proxy.replicas=2
    render upstreamca -f "$EXAMPLE" -f "$EGRESS_VALUES" --set egress.proxy.upstream_ca.config_map=upstream-roots
    render upstreamcakey -f "$EXAMPLE" -f "$EGRESS_VALUES" --set egress.proxy.upstream_ca.config_map=upstream-roots --set egress.proxy.upstream_ca.key=roots.pem
    render caonly -f "$EXAMPLE" -f "$EGRESS_VALUES" --set 'egress.ca.env_names={SSL_CERT_FILE}'
}

annotation() {
    jqv "$1" ".[] | select(.kind == \"Deployment\" and .metadata.namespace == \"$2\") | .spec.template.metadata.annotations[\"$3\"]"
}

check_a_config_change_changes_the_pod_template() {
    section "a change of the proxy config or of the CA certificate changes the annotations of the pod template"
    local config='br.sandbox/proxy-config-sha256' ca='br.sandbox/proxy-ca-sha256'
    t "the annotations are SHA-256 hex values on the pod template of each proxy" jqt cred '[.[] | select(.kind == "Deployment" and .metadata.name == "br-sandbox-proxy") | .spec.template.metadata.annotations | [.["br.sandbox/proxy-config-sha256"], .["br.sandbox/proxy-ca-sha256"]] | all(test("^[a-f0-9]{64}$"))] | length == 2 and all'
    t "the two proxies have two different config annotations" test "$(annotation cred "$ALLOW_NS" "$config")" != "$(annotation cred "$FULL_NS" "$config")"
    t "an unrelated change (replicas, requests) keeps both annotations" test "$(annotation cred "$ALLOW_NS" "$config")$(annotation cred "$ALLOW_NS" "$ca")" = "$(annotation unrelated "$ALLOW_NS" "$config")$(annotation unrelated "$ALLOW_NS" "$ca")"
    t "a narrower allow list changes the config annotation of that proxy" test "$(annotation cred "$ALLOW_NS" "$config")" != "$(annotation narrowed "$ALLOW_NS" "$config")"
    t "a narrower allow list keeps the CA annotation and the other proxy" test "$(annotation cred "$ALLOW_NS" "$ca")$(annotation cred "$FULL_NS" "$config")" = "$(annotation narrowed "$ALLOW_NS" "$ca")$(annotation narrowed "$FULL_NS" "$config")"
    t "another allow.cidrs list changes the config annotation" test "$(annotation cred "$ALLOW_NS" "$config")" != "$(annotation widecidr "$ALLOW_NS" "$config")"
    t "another credential host changes the config annotation of the proxy of the set only" test "$(annotation cred "$FULL_NS" "$config")" != "$(annotation otherhost "$FULL_NS" "$config")" -a "$(annotation cred "$ALLOW_NS" "$config")" = "$(annotation otherhost "$ALLOW_NS" "$config")"
    t "another CA certificate changes the CA annotation and not the config annotation" test "$(annotation cred "$ALLOW_NS" "$ca")" != "$(annotation otherca "$ALLOW_NS" "$ca")" -a "$(annotation cred "$ALLOW_NS" "$config")" = "$(annotation otherca "$ALLOW_NS" "$config")"
    t "the Deployment object itself carries no checksum annotation" jqt cred '[.[] | select(.kind == "Deployment" and .metadata.name == "br-sandbox-proxy") | .metadata | has("annotations")] | any | not'
    t "the README says a change of the config or of the CA certificate rolls the pods and a Secret content change does not" grep -qF 'does not change the pod template and does not roll the pods' "$README"
}

check_a_credential_set_in_mode_allow() {
    section "a credential set in mode allow: allowlist of the mode, then secrets, Secret in the proxy pod only"
    t "the transforms are allowlist then secrets" cfgt allowcred "$ALLOW_NS" '[.transforms[].name] == ["allowlist", "secrets"]'
    t "the allowlist is the allow list of the mode, not the wildcard" cfgt allowcred "$ALLOW_NS" '.transforms[0].config == {"domains": ["git.example.test", "*.packages.example.test"], "cidrs": ["192.0.2.0/24"]}'
    t "the secrets transform reads the key file, binds the placeholder to its host and requires TLS" cfgt allowcred "$ALLOW_NS" '.transforms[1].config.secrets == [{"source": {"type": "file", "path": "/etc/iron-proxy/credentials/a"}, "replace": {"proxy_value": "placeholder-allow-a", "match_headers": ["Authorization"], "match_query": false, "match_path": false, "match_body": false, "require": false}, "rules": [{"host": "git.example.test", "require_tls": true}]}]'
    t "the credential Secret is mounted read-only in the proxy pod of mode allow, named by the value" jqt allowcred "$(proxy_pod "$ALLOW_NS") | (.volumes[] | select(.name == \"credentials\") | .secret.secretName) == \"sbx-alpha-allow-credentials\" and (.containers[0].volumeMounts[] | select(.name == \"credentials\") | .readOnly == true and .mountPath == \"/etc/iron-proxy/credentials\")"
    t "the sandbox pod of mode allow holds the placeholder and no Secret" jqt allowcred '.[] | select(.kind == "ReplicaSet" and .metadata.namespace == "sbx-alpha-allow") | .spec.template.spec | ([.containers[0].env[] | select(.name == "ALLOW_ACCESS") | .value] == ["placeholder-allow-a"]) and (tostring | test("secret|credentials|ca-key"; "i") | not)'
    t "no Secret is rendered and the mode keeps its egress rule to the proxy only" jqt allowcred '([.[] | select(.kind == "Secret")] | length == 0) and ([.[] | select(.kind == "NetworkPolicy" and .metadata.namespace == "sbx-alpha-allow" and .metadata.name == "egress-allow")] | length == 1)'
    t "the README says the set works in mode allow after the allowlist" grep -qF 'The set works in mode `allow` (after the allowlist, so the host must be on the allow list too) and in mode `full` (every public name).' "$README"
    t "the README does not limit the credential set to mode full" test -z "$(grep -F 'One set per namespace, mode `full`' "$README" || true)"
}

check_the_reload_listener_is_a_loopback_constant() {
    section "the reload listener is a constant on the loopback address, not a value"
    t "the config holds the loopback reload and metrics listeners" cfgt cred "$FULL_NS" '.management.listen == "127.0.0.1:9092" and .metrics.listen == "127.0.0.1:9090"'
    fails_with "values don't meet the specifications" -f "$EXAMPLE" -f "$EGRESS_VALUES" --set egress.proxy.reload.listen=127.0.0.1:9092
    fails_with "values don't meet the specifications" -f "$EXAMPLE" -f "$EGRESS_VALUES" --set egress.proxy.reload.listen=0.0.0.0:9092
    t "no template, value or schema key reads a reload listen value" test -z "$({ grep -rnE 'reload\.listen' "$CHART/templates" "$CHART/values.yaml"; grep -n '"listen"' "$CHART/values.schema.json"; } || true)"
    t "no object of the egress render binds a listener to all addresses but the tunnel" test "$(grep -c '0\.0\.0\.0:' "$WORK/egress_yaml.yaml" | tr -d ' ')" = 2
    t "the README says the reload API is reachable only inside the proxy pod and that nothing in the chart calls it" grep -qF 'nothing in the chart calls it' "$README"
}

check_the_ca_certificate_value_holds_certificates_only() {
    section "the CA certificate value: CERTIFICATE blocks only"
    fails_with "values don't meet the specifications" -f "$(valsx keyinside ".clients.alpha.modes.allow.proxy.ca.certificate = \"${CERT_HEAD}\\n${KEY_BLOCK}\\n${CERT_TAIL}\\n\"")"
    fails_with "values don't meet the specifications" -f "$(valsx keyalone ".clients.alpha.modes.allow.proxy.ca.certificate = \"${KEY_BLOCK}\\n\"")"
    fails_with "values don't meet the specifications" -f "$(valsx keyafter ".clients.alpha.modes.allow.proxy.ca.certificate = \"${CERT_HEAD}\\nYQ==\\n${CERT_TAIL}\\n${KEY_BLOCK}\\n\"")"
    t "a chain of two CERTIFICATE blocks renders" jqt chain '[.[] | select(.kind == "ConfigMap" and .metadata.name == "br-sandbox-ca" and .metadata.namespace == "sbx-alpha-allow") | .data["ca.crt"] | [match("BEGIN CERTIFICATE"; "g")] | length] == [2]'
    t "no ConfigMap of a render with a chain holds a private key" jqt chain '[.[] | select(.kind == "ConfigMap") | .data | tostring | test("PRIVATE")] | any | not'
}

check_the_proxy_address_is_an_ipv4_address() {
    section "proxy.cluster_ip is IPv4: an IPv6 value fails the schema"
    fails_with "values don't meet the specifications" -f "$(valsx ipv6ip '.clients.alpha.modes.allow.proxy.cluster_ip = "fd00::50"')"
    fails_with "values don't meet the specifications" -f "$(valsx ipv6mapped '.clients.alpha.modes.allow.proxy.cluster_ip = "::ffff:10.43.0.50"')"
    t "no template brackets an address" test -z "$(grep -nF '[%s]' "$CHART"/templates/_proxy_*.tpl || true)"
    t "the README says an IPv6 value fails the schema" grep -qF 'an IPv6 value fails the schema' "$README"
}

check_credential_items_are_checked_against_the_names_of_the_chart() {
    section "credential items: no variable of the chart, no host that binds every host"
    local name
    for name in HTTPS_PROXY HTTP_PROXY https_proxy http_proxy NO_PROXY no_proxy HOME SSL_CERT_FILE REQUESTS_CA_BUNDLE NODE_EXTRA_CA_CERTS GIT_SSL_CAINFO CURL_CA_BUNDLE; do
        fails_with "BR_SANDBOX_CREDENTIAL_ENV_RESERVED{client=alpha,mode=full,item=0,env_name=${name}}" -f "$(valsx "reserved${name}" "${SET0}.env_name = \"${name}\"")"
    done
    t "with env_names reduced to SSL_CERT_FILE, a CA variable name that the chart no longer sets is free" render freeenv -f "$(valsx freeenv "${SET0}.env_name = \"REQUESTS_CA_BUNDLE\"")" --set 'egress.ca.env_names={SSL_CERT_FILE}'
    fails_with "BR_SANDBOX_CREDENTIAL_ENV_RESERVED{client=alpha,mode=full,item=0,env_name=SSL_CERT_FILE}" -f "$(valsx caonlyres "${SET0}.env_name = \"SSL_CERT_FILE\"")" --set 'egress.ca.env_names={SSL_CERT_FILE}'
    fails_with "BR_SANDBOX_CREDENTIAL_HOST_WILDCARD{client=alpha,mode=full,item=0,host=*}" -f "$(valsx hoststar "${SET0}.hosts = [\"*\"]")"
    fails_with "BR_SANDBOX_CREDENTIAL_HOST_WILDCARD{client=alpha,mode=full,item=0,host=*}" -f "$(valsx hostmixed "${SET0}.hosts = [\"git.example.test\", \"*\"]")"
    fails_with "BR_SANDBOX_CREDENTIAL_HOST_WILDCARD{client=alpha,mode=full,item=0,host=0.0.0.0/0}" -f "$(valsx hostzero "${SET0}.hosts = [\"0.0.0.0/0\"]")"
    fails_with "BR_SANDBOX_CREDENTIAL_HOST_WILDCARD{client=alpha,mode=full,item=0,host=::/0}" -f "$(valsx hostzero6 "${SET0}.hosts = [\"::/0\"]")"
    t "a pattern that is not the bare wildcard still renders as a host rule" cfgt cred "$FULL_NS" '.transforms[1].config.secrets[1].rules == [{"host": "*.packages.example.test", "require_tls": false}]'
    t "the README names both errors" test "$(grep -c 'BR_SANDBOX_CREDENTIAL_ENV_RESERVED\|BR_SANDBOX_CREDENTIAL_HOST_WILDCARD' "$README")" -ge 2
}

check_the_upstream_trust_of_the_proxy_is_optional() {
    section "egress.proxy.upstream_ca: a ConfigMap mounted read-only and SSL_CERT_FILE, nothing when unset"
    local file='/etc/iron-proxy/trust/ca.crt'
    t "set: the ConfigMap volume holds the one key and is mounted read-only at the trust path in each proxy pod" jqt upstreamca '[.[] | select(.kind == "Deployment" and .metadata.name == "br-sandbox-proxy") | .spec.template.spec | ([.volumes[] | select(.name == "upstream-ca")] == [{"name": "upstream-ca", "configMap": {"name": "upstream-roots", "items": [{"key": "ca.crt", "path": "ca.crt"}]}}]) and ([.containers[0].volumeMounts[] | select(.name == "upstream-ca")] == [{"name": "upstream-ca", "mountPath": "/etc/iron-proxy/trust", "readOnly": true}])] | length == 2 and all'
    t "set: SSL_CERT_FILE of the proxy container is the file, beside the reload key" jqt upstreamca "[.[] | select(.kind == \"Deployment\" and .metadata.name == \"br-sandbox-proxy\") | .spec.template.spec.containers[0].env | map(.name) == [\"IRON_MANAGEMENT_API_KEY\", \"SSL_CERT_FILE\"] and ([.[] | select(.name == \"SSL_CERT_FILE\") | .value] == [\"${file}\"])] | length == 2 and all"
    t "set with a key: the key is the item, the path and the file of SSL_CERT_FILE" jqt upstreamcakey '.[] | select(.kind == "Deployment" and .metadata.namespace == "sbx-alpha-allow-egress") | .spec.template.spec | (.volumes[] | select(.name == "upstream-ca") | .configMap.items) == [{"key": "roots.pem", "path": "roots.pem"}] and ([.containers[0].env[] | select(.name == "SSL_CERT_FILE") | .value] == ["/etc/iron-proxy/trust/roots.pem"])'
    t "set: the chart renders no ConfigMap of that name and no Secret" jqt upstreamca '([.[] | select(.kind == "ConfigMap" and .metadata.name == "upstream-roots")] | length == 0) and ([.[] | select(.kind == "Secret")] | length == 0)'
    t "set: no sandbox pod names the trust ConfigMap or the trust path" jqt upstreamca "${SANDBOX_PODS} | (tostring | test(\"upstream|iron-proxy/trust\") | not)"
    t "set: the only writable mount of the proxy pod is still the emptyDir" jqt upstreamca "$(proxy_pod "$ALLOW_NS") | [.containers[0].volumeMounts[] | select(.readOnly | not) | .name] == [\"tmp\"]"
    t "unset: no volume, no mount and no SSL_CERT_FILE in a proxy pod" jqt cred '[.[] | select(.kind == "Deployment" and .metadata.name == "br-sandbox-proxy") | .spec.template.spec | ([.volumes[].name] | index("upstream-ca")) == null and ([.containers[0].volumeMounts[].name] | index("upstream-ca")) == null and ([.containers[0].env[].name] | index("SSL_CERT_FILE")) == null] | all'
    fails_with "values don't meet the specifications" -f "$EXAMPLE" -f "$EGRESS_VALUES" --set egress.proxy.upstream_ca.key=roots.pem
    fails_with "values don't meet the specifications" -f "$EXAMPLE" -f "$EGRESS_VALUES" --set egress.proxy.upstream_ca.config_map=upstream-roots --set egress.proxy.upstream_ca.secret=x
    fails_with "values don't meet the specifications" -f "$EXAMPLE" -f "$EGRESS_VALUES" --set egress.proxy.upstream_ca.config_map=upstream-roots --set egress.proxy.upstream_ca.key=a/b
    t "the README says the proxy trusts the system roots of its image and what the key does" grep -qF 'The proxy trusts the system roots of its image.' "$README"
}
