#!/usr/bin/env bash

EGRESS_NAMESPACE_LIMIT='The fence check of the keeper does not cover the `{namespace}-egress` namespace: an egress namespace has no managed label and the keeper does not judge it, so a NetworkPolicy added by hand there is not seen (reported by the operator of a test cluster; no record in this repository; not proved here: with the managed label the same policy makes the keeper delete the ReplicaSet and report the namespace as not fenced).'
PROOF_BOUND='A probe reads `refused` for any non-zero exit of `nc` (reset, drop, a name that does not resolve), and the targets are the classes that the script discovers plus `network_check.targets`, so the statement holds for the targets of the Job and no wider.'

safe_sysctls() {
    jqv ex '[.[] | select(.kind == "ValidatingAdmissionPolicy" and .metadata.name == "br-sandbox-pod") | .spec.validations[] | select(.message | startswith("BR_SANDBOX_SYSCTLS:")) | .expression | scan("'"'"'([a-z0-9_.]+\\.[a-z0-9_.]+)'"'"'") | .[0]] | .[]'
}

sysctl_mutation() {
    printf '.spec.securityContext.sysctls = [{"name": "%s", "value": "1"}]' "$1"
}

check_the_install_lists_the_objects_of_a_proxied_mode() {
    section "INSTALL.md lists the objects that the platform supplies for a proxied mode, with the names and keys that the templates read"
    local text
    text="$CHART/INSTALL.md"
    render proxied_upstream -f "$EXAMPLE" -f "$EGRESS_VALUES" -f "$CHART/ci/fence-values.yaml" --set egress.proxy.upstream_ca.config_map=upstream-roots
    t "the render of a proxied entry holds no Secret: the platform makes them" jqt cred '[.[] | select(.kind == "Secret")] | length == 0'
    t "the proxy Deployments read the reload key from the key token of the Secret named by egress.proxy.reload.key_secret" jqt cred '[.[] | select(.kind == "Deployment" and .metadata.name == "br-sandbox-proxy") | .spec.template.spec.containers[].env[] | select(.name == "IRON_MANAGEMENT_API_KEY") | .valueFrom.secretKeyRef | [.name, .key]] | unique == [["proxy-reload", "token"]]'
    t "the proxy Deployments read the CA key from the key ca.key of the Secret named by proxy.ca.key_secret of each entry" jqt cred '[.[] | select(.kind == "Deployment" and .metadata.name == "br-sandbox-proxy") | .spec.template.spec.volumes[] | select(.name == "ca-key") | .secret | [.secretName, .items[0].key]] | sort == [["sbx-alpha-allow-ca-key", "ca.key"], ["sbx-alpha-full-ca-key", "ca.key"]]'
    t "the proxy Deployment of the entry with a credential set mounts the Secret named by secret_ref, and the proxy config reads one file for each key of the set" jqt cred '([.[] | select(.kind == "Deployment" and .metadata.name == "br-sandbox-proxy") | .spec.template.spec.volumes[] | select(.name == "credentials") | .secret.secretName] == ["sbx-alpha-full-credentials"]) and ([.[] | select(.kind == "ConfigMap" and .metadata.name == "br-sandbox-proxy-config" and .metadata.namespace == "sbx-alpha-full-egress") | .data["proxy.yaml"] | contains("/etc/iron-proxy/credentials/git") and contains("/etc/iron-proxy/credentials/registry")] == [true])'
    t "the release renders the ConfigMaps of the CA certificate with the key ca.crt, in the egress and in the sandbox namespace of each proxied entry" jqt cred '[.[] | select(.kind == "ConfigMap" and (.metadata.name == "br-sandbox-proxy-ca" or .metadata.name == "br-sandbox-ca")) | [.metadata.name, .metadata.namespace, (.data | keys | join(","))]] | sort == [["br-sandbox-ca", "sbx-alpha-allow", "ca.crt"], ["br-sandbox-ca", "sbx-alpha-full", "ca.crt"], ["br-sandbox-proxy-ca", "sbx-alpha-allow-egress", "ca.crt"], ["br-sandbox-proxy-ca", "sbx-alpha-full-egress", "ca.crt"]]'
    t "the optional upstream ConfigMap is named by the value and read at the key ca.crt by default, and the chart renders no such ConfigMap" jqt proxied_upstream '([.[] | select(.kind == "Deployment" and .metadata.name == "br-sandbox-proxy") | .spec.template.spec.volumes[] | select(.name == "upstream-ca") | [.configMap.name, .configMap.items[0].key]] | unique == [["upstream-roots", "ca.crt"]]) and ([.[] | select(.kind == "ConfigMap" and .metadata.name == "upstream-roots")] | length == 0)'
    t "INSTALL.md has a section on the objects that the platform supplies, before the network check" test "$(grep -n '^## Objects the platform supplies for a proxied mode' "$text" | cut -d: -f1)" -lt "$(grep -n '^## Network check of a sandbox namespace' "$text" | cut -d: -f1)"
    t "INSTALL.md says that the release renders no Secret and that the platform makes them in the egress namespace" has_all "$text" 'It renders no Secret. The platform creates the Secrets below in the egress namespace before the proxy pod starts'
    t "INSTALL.md names the reload Secret with its key" grep -qF '| Secret, the reload key | `egress.proxy.reload.key_secret` | `token` | the platform |' "$text"
    t "INSTALL.md names the proxy CA key Secret with its key" grep -qF '| Secret, the proxy CA key | `namespaces[].proxy.ca.key_secret` | `ca.key` | the platform |' "$text"
    t "INSTALL.md names the credential Secret with its keys" grep -qF '| Secret, the credentials (only with a `credential_set`) | `namespaces[].credential_set.secret_ref` | one key for each `credential_set.secrets[].key`, mounted as a file in the proxy pod | the platform |' "$text"
    t "INSTALL.md names the two ConfigMaps of the CA certificate and the optional upstream ConfigMap" has_all "$text" '| ConfigMap, the CA certificate for the proxy | `br-sandbox-proxy-ca` in the egress namespace | `ca.crt` | the release that holds the entry, from `namespaces[].proxy.ca.certificate` |' '| ConfigMap, the CA certificate for the sandbox pods | `br-sandbox-ca` in the sandbox namespace | `ca.crt` | the release that holds the entry, from `namespaces[].proxy.ca.certificate` |' '| ConfigMap, the roots for upstream servers (optional) | `egress.proxy.upstream_ca.config_map` | `egress.proxy.upstream_ca.key`, default `ca.crt` | the platform |'
    t "INSTALL.md says that the pull secrets must exist in the egress namespace and that the start is not tested" has_all "$text" 'The pull secrets of `pull_secrets` must exist in the egress namespace too. Whether the proxy pod starts with these objects on a cluster is not tested here.'
}

check_the_policy_text_holds_the_guards_and_the_lists() {
    section "POLICIES.md: what the model proves, what the exact text pins, and the sysctl lists"
    local name text="$CHART/POLICIES.md"
    t "the safe list in the rule text holds 12 names" test "$(safe_sysctls | wc -l | tr -d ' ')" -eq 12
    for name in $(safe_sysctls); do
        t "the model admits the safe sysctl ${name}" test "$(created ex "$NS" "$(sysctl_mutation "$name")")" = admit
    done
    for name in net.ipv4.tcp_slow_start_after_idle net.ipv4.tcp_notsent_lowat; do
        t "the model refuses ${name}, a name that the Baseline page adds in 1.37" test "$(created ex "$NS" "$(sysctl_mutation "$name")")" = BR_SANDBOX_SYSCTLS
        t "the rule text does not name ${name}" test "$(safe_sysctls | grep -cF "$name")" -eq 0
    done
    t "the three policies fail closed: failurePolicy Fail" test "$(jq -r '[.[] | select(.kind == "ValidatingAdmissionPolicy") | .spec.failurePolicy] | unique | join(",")' "$WORK/ex.json")" = Fail
    t "the rules that read an optional field guard it with has() in the rendered text" jqt ex '[.[] | select(.kind == "ValidatingAdmissionPolicy" and .metadata.name == "br-sandbox-pod") | .spec.validations[] | select(.message | startswith("BR_SANDBOX_SYSCTLS:") or startswith("BR_SANDBOX_WINDOWS_HOST_PROCESS:") or startswith("BR_SANDBOX_PROC_MOUNT:")) | .expression | contains("!has(")] == [true, true, true]'
    t "POLICIES.md says that the two sysctl names of 1.37 are refused, and points to the test" grep -qF 'the two names that the page adds in 1.37 are refused (stricter than the page; neither name is in the rule text, `check_the_policy_text_holds_the_guards_and_the_lists`)' "$text"
    t "POLICIES.md marks the defaulting sentence as read from the Kubernetes documentation and not tested on a cluster" grep -qF 'That the API server sets no other default on those fields is read from the Kubernetes documentation and is not tested on a cluster' "$text"
    t "POLICIES.md says that the model proves the refusals and the accepted shapes of the rendered pods" grep -qF 'the model proves the refusals and the accepted shapes of the rendered pods' "$text"
    t "POLICIES.md says that the model cannot see a has() guard, what a CEL evaluator gives, and that the exact text pins the guards" has_all "$text" 'it cannot see a `has()` guard. With a guard removed, a CEL evaluator gives `no such key` on a pod that leaves the field out, and under `failurePolicy: Fail` the pod is refused' 'The exact CEL text of each rule (the golden file above) pins the guards, and in this suite it is the only barrier against a lost guard.'
    t "POLICIES.md says that the exact text catches a lost has() guard and that neither catches a CEL fault" grep -qF 'the model catches a changed bound or a lost binding, the exact text catches a lost `has()` guard, and neither catches a CEL fault.' "$text"
    t "POLICIES.md no longer holds the sentences that no test proved" test "$(grep -cE 'the nodes of an older version would refuse them anyway|these rules read only fields that the chart and the API server leave unset|Nothing that these rules read is out of the model|each allowed near value|a lost guard or a lost binding' "$text")" -eq 0
}

check_the_documents_state_the_egress_namespace_limit() {
    section "README.md, LIMITS.md and POLICIES.md: the fence check of the keeper does not cover the egress namespace"
    local file
    for file in README.md LIMITS.md POLICIES.md; do
        t "${file} carries the limit on the egress namespace" grep -qF "$EGRESS_NAMESPACE_LIMIT" "$CHART/$file"
    done
    t "an egress namespace carries br.sandbox/egress and never br.sandbox/managed, so the keeper does not judge it" jqt cred '[.[] | select(.kind == "Namespace" and (.metadata.name | endswith("-egress"))) | .metadata.labels | select(.["br.sandbox/egress"] == "true" and (has("br.sandbox/managed") | not))] | length == 2'
}

check_limits_lists_the_safe_sysctls() {
    section "LIMITS.md: a sysctl outside the safe list is refused, and the list is the one up to Kubernetes 1.32"
    local names
    names="$(safe_sysctls | sed 's/.*/`&`/' | paste -sd, - | sed 's/,/, /g')"
    t "LIMITS.md lists the 12 names of the rule, in the order of the rule, and says the list is stricter than the newest Baseline list" grep -qF "A pod that sets a sysctl outside the 12 names of the safe list is refused (\`BR_SANDBOX_SYSCTLS\`): ${names}. The safe list is the one up to Kubernetes 1.32, stricter than the newest Baseline list." "$CHART/LIMITS.md"
}

check_the_proof_sentences_carry_the_bound() {
    section "INSTALL.md and LIMITS.md: the proof of the route holds for the targets of the Job and no wider"
    t "INSTALL.md and LIMITS.md carry the bound sentence, in the same words" test "$(grep -cF "$PROOF_BOUND" "$CHART/INSTALL.md" "$CHART/LIMITS.md" | grep -c ':1$')" -eq 2
    t "the bound sentence follows the proof sentence in both files" test "$(grep -cF "This repository records no such run at this version. ${PROOF_BOUND}" "$CHART/INSTALL.md" "$CHART/LIMITS.md" | grep -c ':1$')" -eq 2
    t "the script reads refused for any non-zero exit of nc" has_all "$CHART/files/network-check.sh" 'if ! nc -w 2 "${1%:*}" "${1##*:}" </dev/null >/dev/null 2>&1; then'
    t "INSTALL.md says that the control_unavailable value is the control field of the probe object" grep -qF 'else the probe object carries `control_unavailable` in its `control` field and the Job fails' "$CHART/INSTALL.md"
    t "INSTALL.md no longer says that the probe reads control_unavailable" test "$(grep -cF 'else the probe reads `control_unavailable`' "$CHART/INSTALL.md")" -eq 0
    t "the script prints control_unavailable in the control field of the object" has_all "$CHART/files/network-check.sh" 'control_unavailable'
    t "the README says that a completed Job with an empty list of namespaces proves nothing" grep -qF 'A completed Job of that release with an empty `namespaces` list proves nothing for any sandbox namespace.' "$CHART/README.md"
}

check_the_readme_counts_the_new_rules_and_names_the_cluster_role() {
    section "README.md: eight rules are new in 0.4.0, and the ClusterRole of the check is named after the system namespace"
    local readme codes
    readme="$CHART/README.md"
    render nc_role -f "$EXAMPLE" -f "$CHART/ci/platform-values.yaml"
    codes="$(grep -o 'Eight rules are new in 0.4.0.*' "$readme" | sed 's/The pod policy does not depend.*//' | grep -oE 'BR_SANDBOX_[A-Z_]+' | tr '\n' ' ' || true)"
    t "the README says that eight rules are new, and no longer says six" test "$(grep -cF 'Eight rules are new in 0.4.0: ' "$readme")" -eq 1 -a "$(grep -cF 'Six rules are new' "$readme")" -eq 0
    t "the eight codes that the README names are the last eight rules of the policy, in order" test "$codes" = "$(jqv ex '[.[] | select(.kind == "ValidatingAdmissionPolicy" and .metadata.name == "br-sandbox-pod") | .spec.validations[-8:][].message | split(":")[0]] | join(" ")') "
    t "the pod policy holds 13 rules before them" jqt ex '[.[] | select(.kind == "ValidatingAdmissionPolicy" and .metadata.name == "br-sandbox-pod") | .spec.validations[:-8] | length] == [13]'
    t "the ClusterRole of the check renders as the system namespace and -netcheck" jqt nc_role '([.[] | select(.kind == "Namespace" and .metadata.labels["br.sandbox/system"] == "true") | .metadata.name][0]) as $system | [.[] | select(.kind == "ClusterRole" and (.metadata.name | endswith("-netcheck"))) | .metadata.name] == [$system + "-netcheck"]'
    t "the README row names the ClusterRole {system namespace}-netcheck and not br-sandbox-netcheck" test "$(grep -cF 'ServiceAccount and Role `br-sandbox-netcheck`, ClusterRole `{system namespace}-netcheck`' "$readme")" -eq 1 -a "$(grep -cF 'ClusterRole and Role `br-sandbox-netcheck`' "$readme")" -eq 0
}

check_the_stamper_example_covers_the_kinds_that_the_stamper_generates() {
    section "STAMPER.md: the example ClusterRole covers every kind that the stamper generates, NetworkPolicies included"
    local kinds resources
    kinds="$(jq -c '[.[] | select(.kind == "ClusterPolicy") | .spec.rules[] | .generate.kind | select(. != null) | ascii_downcase | if endswith("y") then .[:-1] + "ies" else . + "s" end] | unique' "$WORK/stamper.json")"
    resources="$(awk '/^```yaml/{on=1;next} /^```/{on=0} on' "$CHART/STAMPER.md" | yq -o=json -I=0 '[.rules[] | select(.verbs | contains(["create"])) | .resources[]] | sort')"
    t "the stamper generates six kinds, NetworkPolicy among them" test "$kinds" = '["configmaps","limitranges","networkpolicies","resourcequotas","rolebindings","serviceaccounts"]'
    t "the example rules with the create verb name exactly those six resources" test "$resources" = "$kinds"
    t "the stamper generates the two NetworkPolicies that STAMPER.md names" test "$(jq -r '[.[] | select(.kind == "ClusterPolicy") | .spec.rules[] | .generate | select(.kind == "NetworkPolicy") | .name] | unique | join(",")' "$WORK/stamper.json")" = br-sandbox-default-deny,br-sandbox-egress-full
    t "STAMPER.md says what was seen for the NetworkPolicy right: the version of the policy engine, the default rights, that the added ClusterRole does not hold that kind, and that the example rule is there for that reason only" grep -qF 'The stamper also generates NetworkPolicies (`br-sandbox-default-deny`, and `br-sandbox-egress-full` for the mode `full`). In the test, the policy engine at version 1.18.0 (chart 3.8.0) stamped the NetworkPolicy with its default rights; the added ClusterRole does not hold that kind. Another version or another install of the policy engine can need it. The example ends with a rule on `networkpolicies` for that reason only: it is not in the ClusterRole of the test, and it was not run on a cluster:' "$CHART/STAMPER.md"
    t "STAMPER.md does not call the right on NetworkPolicy inferred" test "$(grep -cF 'has this right (inferred' "$CHART/STAMPER.md")" -eq 0
}

check_the_cluster_reports_are_marked_and_the_changelog_names_the_stamper_conditions() {
    section "README.md and CHANGELOG.md: the older cluster results are marked as reported, and the stamper conditions are in the CHANGELOG"
    t "the flannel measurement is marked as reported, with no record here" grep -qF 'A probe at once therefore failed the Job although the policy worked (reported by the operator of a test cluster; no record in this repository).' "$CHART/README.md"
    t "the sentence that says the window was measured with flannel and kube-router carries the mark: reported, no record here" grep -qF 'This was measured on a test cluster with flannel and the kube-router network policy controller (reported by the operator of a test cluster; no record in this repository): the policy started' "$CHART/README.md"
    t "the sentence that says the window was measured with that one network plugin carries the mark: reported, no record here" grep -qF 'The window was measured with that one network plugin (reported by the operator of a test cluster; no record in this repository); the time it takes' "$CHART/README.md"
    t "the measurement of the image rancher/k3s is marked as reported, with no record here" grep -qF 'the image `rancher/k3s` has no `nc` (reported by the operator of a test cluster; no record in this repository).' "$CHART/README.md"
    t "the CHANGELOG 0.4.0 holds one line on the conditions of the stamper: the rights, reported, and one maker" grep -qF -- '- Stamper conditions (`STAMPER.md`): the rights that the policy engine needs to generate the fence objects are not rendered by the chart and are needed (reported by the operator of a test cluster; no record in this repository; not proved here), and one namespace has one maker of its fence: a fence release, or the stamper.' <<<"$(changelog_040)"
    t "the CHANGELOG route line names copies of the two files and the probe namespace as <scope>-netcheck" has_all "$CHART/CHANGELOG.md" 'an upgrade of the operator release with a copy of `ci/netcheck-route-values.yaml` (one probe namespace `<scope>-netcheck` in mode `none`' 'an upgrade with a copy of `ci/netcheck-route-run-values.yaml` added'
}

LINK_LOCAL_FAULT='BR_SANDBOX_LINK_LOCAL_MISSING{key=network.internal_cidrs,range=169.254.0.0/16}'

check_a_replacing_deny_list_must_keep_the_link_local_range() {
    section "network.internal_cidrs replaces the chart default: a list without the link-local range, or a cover of it, fails the render"
    local list proxied=(-f "$EXAMPLE" -f "$EGRESS_VALUES" -f "$CHART/ci/fence-values.yaml")
    for list in '10.0.0.0/8' '10.0.0.0/8,192.168.0.0/16,100.64.0.0/10,127.0.0.0/8' '10.0.0.0/8,169.254.1.0/24' '169.254.0.0/17' '169.254.169.254' '10.0.0.0/8,fe80::/10' '168.0.0.0/8' '169.255.0.0/16' '10.0.0.0/8,::1.2.3.4/0' '10.0.0.0/8,::ffff:1.2.3.4/0' '10.0.0.0/8,0:0:0:0:0:0:1.2.3.4/0' '10.0.0.0/8,::169.254.0.0/0' '10.0.0.0/8,1.2.3/0' '10.0.0.0/8,1.2.3.4.5/0' '10.0.0.0/8,1.2/8' 'a.b.c.d/0' 'fe.fe.fe.fe/0' '168.510.0.0/16' '256.0.0.0/0' '0.0.0.0/33' '0.0.0.0/99' '169.254.0.0/32' '169.254.0.0/24' '169.254.0.256/16'; do
        fails_with "$LINK_LOCAL_FAULT" -f "$EXAMPLE" --set "network.internal_cidrs={${list}}"
    done
    fails_with "$LINK_LOCAL_FAULT" "${proxied[@]}" --set 'network.internal_cidrs={10.0.0.0/8}'
    fails_with "$LINK_LOCAL_FAULT" -f "$EXAMPLE" -f "$CHART/ci/fence-values.yaml" --set 'network.internal_cidrs={10.0.0.0/8}'
    for list in '10.0.0.0/8,169.254.0.0/16' '169.254.0.0/16' '10.0.0.0/8,169.254.0.0/15' '0.0.0.0/0' '169.0.0.0/8' '128.0.0.0/1' '169.254.5.5/16' '169.252.0.0/14' '10.0.0.0/8,169.254.0.0/016' '169.254.255.255/16' '255.255.255.255/0'; do
        t "a list ${list} holds or covers the range: the render passes" helm template t "$CHART" -f "$EXAMPLE" --set "network.internal_cidrs={${list}}"
    done
    t "the default list renders" helm template t "$CHART" -f "$EXAMPLE"
    render ll_default "${proxied[@]}"
    render ll_cover "${proxied[@]}" --set 'network.internal_cidrs={10.0.0.0/8,169.254.0.0/15}'
    render ll_full -f "$EXAMPLE" --set 'network.internal_cidrs={10.0.0.0/8,169.254.0.0/15}'
    t "the policy br-sandbox-egress-full of a mode full namespace denies the link-local range by default" jqt ex '[.[] | select(.kind == "NetworkPolicy" and .metadata.name == "br-sandbox-egress-full") | .spec.egress[0].to[0].ipBlock.except | index("169.254.0.0/16")] | length > 0 and all(. != null)'
    t "the policy br-sandbox-egress-full denies the covering entry of a replacing list" jqt ll_full '[.[] | select(.kind == "NetworkPolicy" and .metadata.name == "br-sandbox-egress-full") | .spec.egress[0].to[0].ipBlock.except | index("169.254.0.0/15")] | length > 0 and all(. != null)'
    t "the policy br-sandbox-egress-proxy of each egress namespace denies the link-local range by default" jqt ll_default '[.[] | select(.kind == "NetworkPolicy" and .metadata.name == "br-sandbox-egress-proxy") | .spec.egress[0].to[0].ipBlock.except | index("169.254.0.0/16")] | length == 2 and all(. != null)'
    t "the policy br-sandbox-egress-proxy of each egress namespace denies the covering entry of a replacing list" jqt ll_cover '[.[] | select(.kind == "NetworkPolicy" and .metadata.name == "br-sandbox-egress-proxy") | .spec.egress[0].to[0].ipBlock.except | index("169.254.0.0/15")] | length == 2 and all(. != null)'
    t "the upstream deny list of each proxy holds the link-local range by default and the covering entry of a replacing list" test "$(jq -r '[.[] | select(.kind == "ConfigMap" and .metadata.name == "br-sandbox-proxy-config") | .data["proxy.yaml"] | contains("169.254.0.0/16")] | all' "$WORK/ll_default.json")" = true -a "$(jq -r '[.[] | select(.kind == "ConfigMap" and .metadata.name == "br-sandbox-proxy-config") | .data["proxy.yaml"] | contains("169.254.0.0/15")] | all' "$WORK/ll_cover.json")" = true
    t "the three deny lists are built by one helper from network.internal_cidrs, so the one check covers them" test "$(grep -rl 'br-sandbox-chart.cidr.except' "$CHART/templates" | sed 's#.*/##' | sort | tr '\n' ' ')" = '_cidrs.tpl _fence.tpl _proxy_config.tpl egress.yaml '
    t "no template other than the validation templates names the fault" test "$(grep -rl 'BR_SANDBOX_LINK_LOCAL_MISSING' "$CHART/templates" | sed 's#.*/##' | tr '\n' ' ')" = '_validate.tpl '
    t "no fixture of ci/ sets network.internal_cidrs" test "$(grep -rl internal_cidrs "$CHART/ci" | wc -l | tr -d ' ')" -eq 0
    t "the README lists the fault in the render errors and in the values table" test "$(grep -cF "\`${LINK_LOCAL_FAULT}\`" "$CHART/README.md")" -ge 1 -a "$(grep -cF '| `network.internal_cidrs` | RFC1918, 100.64/10, 169.254/16, 127/8 | must hold `169.254.0.0/16` or a CIDR that covers it (`BR_SANDBOX_LINK_LOCAL_MISSING`) |' "$CHART/README.md")" -eq 1
    t "the README says that a replacing list keeps the private ranges of the cluster, that the chart holds the link-local range only, and how the cover is computed" has_all "$CHART/README.md" 'A list given to `network.internal_cidrs` replaces the default list: it must also keep the private ranges of the cluster, and the chart holds the link-local range only.' 'An IPv4 entry covers the range when it is a well-formed IPv4 CIDR (four decimal octets of 0 to 255 and a prefix of 0 to 32), its prefix is 16 bits or fewer and its network holds `169.254.0.0` (`169.254.0.0/15` and `0.0.0.0/0` cover it); a bare address is a /32 and does not; IPv6 entries and malformed entries are not read and cover nothing.' 'with a list that had no `169.254.0.0/16`, the address `169.254.169.254` on port 80 was open from a pod of mode `full` (reported by the operator of a test cluster; no record in this repository)'
    t "the CHANGELOG 0.4.0 names the fault in the faults bullet and in its own line" test "$(changelog_040 | grep -cF "${LINK_LOCAL_FAULT}")" -eq 2
    t "the CHANGELOG 0.4.0 line Link-local range says what an owner of an older list does: add the range or a covering entry" test "$(changelog_040 | grep -F -- "- Link-local range: " | grep -cF 'the owner adds `169.254.0.0/16`, or an entry that covers it, to the list.')" -eq 1
    t "the CHANGELOG 0.4.0 line Link-local range says that a 0.4.0 install without the range is refused at the render and that there is no upgrade from 0.3.x, and does not say that an install is refused after an upgrade" test "$(changelog_040 | grep -F -- "- Link-local range: " | grep -cF 'A 0.4.0 install whose `network.internal_cidrs` lacks that range is refused at the render, and there is no upgrade from 0.3.x:')" -eq 1 -a "$(changelog_040 | grep -cF 'is refused after the upgrade')" -eq 0
    t "the README says what an owner of a list without the range does: add the range or a covering entry" grep -qF 'the owner adds `169.254.0.0/16`, or an entry that covers it, to the list.' "$CHART/README.md"
    t "the CHANGELOG 0.4.0 line Link-local range says that a malformed entry covers nothing" test "$(changelog_040 | grep -F -- "- Link-local range: " | grep -cF 'only if it is a well-formed IPv4 CIDR (four decimal octets of 0 to 255 and a prefix of 0 to 32); a malformed entry covers nothing.')" -eq 1
    t "the CHANGELOG 0.4.0 has the line Link-local range, which names the fault" test "$(changelog_040 | grep -F -- "- Link-local range: " | grep -cF "${LINK_LOCAL_FAULT}")" -eq 1
    local proxy_shas='[.[] | select(.kind == "Deployment" and .spec.template.metadata.labels["br.sandbox/role"] == "proxy") | .metadata.name + "=" + .spec.template.metadata.annotations["br.sandbox/proxy-config-sha256"]] | sort'
    render ll_default_again "${proxied[@]}"
    t "a change of network.internal_cidrs changes the annotation br.sandbox/proxy-config-sha256 of each of the two proxy Deployments, and the same list gives the same annotation" test "$(jq -n --argjson a "$(jq -c "$proxy_shas" "$WORK/ll_default.json")" --argjson b "$(jq -c "$proxy_shas" "$WORK/ll_cover.json")" --argjson c "$(jq -c "$proxy_shas" "$WORK/ll_default_again.json")" '($a | length) == 2 and ($b | length) == 2 and $a == $c and ([$a, $b] | transpose | all((.[0] | split("=")[0]) == (.[1] | split("=")[0]) and .[0] != .[1]))')" = true
    t "the README says that a change of network.internal_cidrs changes the proxy configuration and rolls the proxy Deployments, by the annotation of the pod template" grep -qF 'A change of this list changes the proxy configuration, so `helm upgrade` rolls the proxy Deployments of the egress namespaces (the annotation `br.sandbox/proxy-config-sha256` of the pod template changes), and the entry is added to or removed from the policy lists built from it.' "$CHART/README.md"
    t "the CHANGELOG 0.4.0 line Link-local range says that a change of network.internal_cidrs changes the proxy configuration and rolls the proxy Deployments" test "$(changelog_040 | grep -F -- "- Link-local range: " | grep -cF 'A change of `network.internal_cidrs` changes the proxy configuration, so the upgrade rolls the proxy Deployments of the egress namespaces (the annotation `br.sandbox/proxy-config-sha256` changes).')" -eq 1
    t "the annotation is on the pod template of the proxy Deployment and holds the SHA-256 of the rendered proxy configuration" has_all "$CHART/templates/_proxy_objects.tpl" 'br.sandbox/proxy-config-sha256: {{ include "br-sandbox-chart.proxy.config" . | sha256sum | quote }}'
    t "the rendered proxy configuration holds the deny list that is built from network.internal_cidrs" has_all "$CHART/templates/_proxy_config.tpl" '"upstream_deny_cidrs" (include "br-sandbox-chart.proxy.deny_cidrs"' 'include "br-sandbox-chart.cidr.except"'
}

check_limits_names_an_allow_entry_in_a_private_range() {
    section "LIMITS.md: an allow.cidrs entry in a private or link-local range is not refused, and the document says which part is not tested"
    local cidr
    for cidr in 10.0.0.0/8 169.254.0.0/16 169.254.169.254; do
        yq ".namespaces[1].allow.cidrs = [\"${cidr}\"]" "$EGRESS_VALUES" >"$WORK/allow-private.yaml"
        t "an allow.cidrs entry ${cidr} renders, with the entry at the end of the allow list of the adapter document" bash -c 'helm template t "$1" -f "$2" -f "$3" | grep -qF -- "$4"' _ "$CHART" "$EXAMPLE" "$WORK/allow-private.yaml" "\"${cidr}\"]"
    done
    t "LIMITS.md carries the limit on an allow.cidrs entry in a private or link-local range, and says what is not tested" grep -qF 'An entry of `allow.cidrs` inside a private or link-local range is not refused by the chart. Not tested: which of the allow entry and the deny list of the proxy wins for such an address.' "$CHART/LIMITS.md"
}

check_limits_names_the_small_pool_behaviour_of_the_keeper() {
    section "LIMITS.md: a refused edit removes all the warm pods of a small pool"
    t "LIMITS.md carries the limit on a small pool and a refused edit of the image or the size" grep -qF 'With a pool that has `scale_down_step` warm pods or fewer (the default step is 5), one edit of the image or of the size that the namespace refuses removes all the warm pods of the pool. The pool shows `PodsRefused` and makes pods again when the edit is corrected. A claimed pod is never removed.' "$CHART/LIMITS.md"
}
