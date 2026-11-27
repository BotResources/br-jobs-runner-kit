#!/usr/bin/env bash

STAMPER_VALUES="$CHART/ci/stamper-values.yaml"
STAMPER_IDENTITY='system:serviceaccount:kyverno:kyverno-background-controller'
STAMP='.[] | select(.kind == "ClusterPolicy" and .metadata.name == "br-sandbox-stamper")'
STAMPER_JQ='
def rebuilt($name): (.generate | {apiVersion, kind} + {metadata: ((.data.metadata // {}) + {name: .name, namespace: .namespace})} + (.data | del(.metadata))) | walk(if type == "string" then gsub("\\{\\{ request\\.object\\.metadata\\.name \\}\\}"; $name) else . end);
def group($scope; $mode): select(.match.any[0].resources.names[0] == $scope and .match.any[0].resources.selector.matchLabels["br.sandbox/mode"] == $mode);
def stamped($scope; $mode; $name): [.[] | select(.kind == "ClusterPolicy" and .metadata.name == "br-sandbox-stamper") | .spec.rules[] | group($scope; $mode) | rebuilt($name)] | sort_by(.kind + "/" + .metadata.name);
def fenced($name): [.[] | select(.metadata.namespace == $name and (.kind | IN("ServiceAccount", "ResourceQuota", "LimitRange", "NetworkPolicy", "ConfigMap", "RoleBinding")) and .metadata.name != "br-sandbox-netcheck")] | sort_by(.kind + "/" + .metadata.name);
'

render_stamper_fixtures() {
    render stamper -f "$EXAMPLE" -f "$STAMPER_VALUES"
    render stamper_off -f "$EXAMPLE" -f "$STAMPER_VALUES" --set stamper.enabled=false
    render stamper_none -f "$EXAMPLE" -f "$STAMPER_VALUES" --set-json 'namespaces=[]'
    render stamper_five -f "$EXAMPLE" -f "$STAMPER_VALUES" --set 'namespaces[3].name=sbx-alpha-x-none' --set 'namespaces[3].mode=none' --set 'namespaces[4].name=sbx-beta-y-full' --set 'namespaces[4].mode=full'
    render stamper_scoped -f "$EXAMPLE" -f "$STAMPER_VALUES" --set scopes.sbx-beta.client_id=beta-saves --set scopes.sbx-beta.runner.service_account=beta-runner --set scopes.sbx-beta.runner.namespace=beta-runners
    render_yaml stamper_yaml -f "$EXAMPLE" -f "$STAMPER_VALUES"
}

same_as_fence() {
    jq -e --arg scope "$2" --arg mode "$3" --arg name "$4" "${STAMPER_JQ}"'stamped($scope; $mode; $name) == fenced($name) and (stamped($scope; $mode; $name) | length > 0)' "$WORK/$1.json" >/dev/null
}

check_the_stamper_is_off_by_default() {
    section "stamper: off by default it renders nothing, and the values name the identity of the writer"
    t "stamper.enabled is false in the values" test "$(yq '.stamper.enabled' "$CHART/values.yaml")" = false
    t "the default identity is the background controller of the policy engine" test "$(yq '.stamper.identity' "$CHART/values.yaml")" = "$STAMPER_IDENTITY"
    local name
    for name in ex keeper platform fence nondefault egress_keeper stamper_off; do
        t "render ${name}: no ClusterPolicy" jqt "$name" '[.[] | select(.kind == "ClusterPolicy")] | length == 0'
    done
    t "an unknown key of stamper is refused by the schema" render_fails -f "$EXAMPLE" --set stamper.rules=true
    t "an identity that is not a service account name is refused by the schema" render_fails -f "$EXAMPLE" --set stamper.identity=somebody
}

check_the_stamper_has_one_rule_for_each_scope_mode_and_object() {
    section "stamper: one ClusterPolicy, the rules are grouped by scope and mode, no rule for allow"
    t "with stamper.enabled the render holds one ClusterPolicy br-sandbox-stamper of kyverno.io/v1" jqt stamper "[.[] | select(.kind == \"ClusterPolicy\")] | map([.apiVersion, .metadata.name]) == [[\"kyverno.io/v1\", \"br-sandbox-stamper\"]]"
    t "the groups are the scopes of the table times the modes none and full" jqt stamper "[${STAMP} | .spec.rules | group_by([.match.any[0].resources.names[0], .match.any[0].resources.selector.matchLabels[\"br.sandbox/mode\"]]) | map([.[0].match.any[0].resources.names[0], .[0].match.any[0].resources.selector.matchLabels[\"br.sandbox/mode\"], length])] == [[[\"sbx-alpha\", \"full\", 9], [\"sbx-alpha\", \"none\", 8], [\"sbx-beta\", \"full\", 9], [\"sbx-beta\", \"none\", 8]]]"
    t "no rule matches the mode allow or any mode but none and full" jqt stamper "[${STAMP} | .spec.rules[] | .match.any[0].resources.selector.matchLabels[\"br.sandbox/mode\"]] | unique == [\"full\", \"none\"]"
    t "each rule matches Namespaces by the scope name and its dash prefix and by the managed label" jqt stamper "[${STAMP} | .spec.rules[] | .match.any[0].resources | select(.kinds == [\"Namespace\"] and .names == [.names[0], (.names[0] + \"-*\")] and .selector.matchLabels[\"br.sandbox/managed\"] == \"true\" and (.selector.matchLabels | keys == [\"br.sandbox/managed\", \"br.sandbox/mode\"]))] | length == 34"
    t "each rule generates one object, with synchronize and generateExisting" jqt stamper "[${STAMP} | .spec.rules[] | .generate | select(.synchronize == true and .generateExisting == true and has(\"foreach\") == false and .namespace == \"{{ request.object.metadata.name }}\")] | length == 34"
    t "the mode none generates the eight objects of the fence and full adds br-sandbox-egress-full" test "$(jqv stamper "[${STAMP} | .spec.rules[] | select(.name | startswith(\"br-sandbox-sbx-alpha-\")) | (.name | split(\"-\")[4]) + \"/\" + .generate.kind + \"/\" + .generate.name] | sort | join(\",\")")" = "full/ConfigMap/br-sandbox-config,full/ConfigMap/br-sandbox-pod-template,full/LimitRange/br-sandbox-limits,full/NetworkPolicy/br-sandbox-default-deny,full/NetworkPolicy/br-sandbox-egress-full,full/ResourceQuota/br-sandbox-quota,full/RoleBinding/br-sandbox-keeper,full/RoleBinding/br-sandbox-runner,full/ServiceAccount/sandbox,none/ConfigMap/br-sandbox-config,none/ConfigMap/br-sandbox-pod-template,none/LimitRange/br-sandbox-limits,none/NetworkPolicy/br-sandbox-default-deny,none/ResourceQuota/br-sandbox-quota,none/RoleBinding/br-sandbox-keeper,none/RoleBinding/br-sandbox-runner,none/ServiceAccount/sandbox"
}

check_the_generated_objects_equal_the_fence_part() {
    section "stamper: for a namespace, the objects that the rules generate equal the objects of the fence part"
    local row scope mode name
    for row in 'sbx-alpha|none|sbx-alpha-none' 'sbx-alpha|full|sbx-alpha-full' 'sbx-beta|none|sbx-beta-none'; do
        IFS='|' read -r scope mode name <<<"$row"
        t "${name}: the objects of the rules of ${scope} and ${mode}, with the name of the namespace put in, equal the fence objects of the namespace" same_as_fence stamper "$scope" "$mode" "$name"
    done
    t "the same rules do not equal the fence objects of a namespace of another mode" test "$(jq -r --arg n sbx-alpha-full "${STAMPER_JQ}"'stamped("sbx-alpha"; "none"; $n) == fenced($n)' "$WORK/stamper.json")" = false
    t "the same rules do not equal the fence objects of a namespace of another scope" test "$(jq -r --arg n sbx-beta-none "${STAMPER_JQ}"'stamped("sbx-alpha"; "none"; $n) == fenced($n)' "$WORK/stamper.json")" = false
    jq 'map(if .kind == "ClusterPolicy" then (.spec.rules |= map(if (.generate.kind == "ResourceQuota") then .generate.data.spec.hard.pods = "99" else . end)) else . end)' "$WORK/stamper.json" >"$WORK/stamper_mut_quota.json"
    jq 'map(if .kind == "ClusterPolicy" then (.spec.rules |= map(if (.generate.name == "br-sandbox-default-deny") then .generate.data.spec.policyTypes = ["Ingress"] else . end)) else . end)' "$WORK/stamper.json" >"$WORK/stamper_mut_deny.json"
    jq 'map(if .kind == "ClusterPolicy" then (.spec.rules |= map(if (.generate.name == "br-sandbox-config") then .generate.data.data["config.json"] |= sub("alpha"; "other") else . end)) else . end)' "$WORK/stamper.json" >"$WORK/stamper_mut_config.json"
    jq 'map(if .kind == "ClusterPolicy" then (.spec.rules |= map(select(.generate.name != "br-sandbox-keeper"))) else . end)' "$WORK/stamper.json" >"$WORK/stamper_mut_missing.json"
    local probe
    for probe in quota deny config missing; do
        t "the comparison sees a rule whose data changed (${probe})" test "$(jq -r --arg n sbx-alpha-none "${STAMPER_JQ}"'stamped("sbx-alpha"; "none"; $n) == fenced($n)' "$WORK/stamper_mut_${probe}.json")" = false
    done
    t "the fence part and the rules share the named templates and the stamper repeats no object text" test -z "$(grep -lE 'kind: (ResourceQuota|LimitRange|NetworkPolicy|RoleBinding|ServiceAccount|ConfigMap)' "$CHART/templates/_stamper.tpl" "$CHART/templates/stamper.yaml" || true)"
}

check_the_stamper_renders_one_cluster_object_and_no_namespace_input() {
    section "stamper: only the policy is added, it does not depend on the namespaces, it reads no label"
    t "the stamper adds the ClusterPolicy and no other object" test "$(jq -rn --slurpfile on "$WORK/stamper.json" --slurpfile off "$WORK/stamper_off.json" '($on[0] | map(.kind + "/" + .metadata.name)) - ($off[0] | map(.kind + "/" + .metadata.name)) | join(",")')" = ClusterPolicy/br-sandbox-stamper
    t "with no namespace, three namespaces and five namespaces the policy is the same text" test "$(jqv stamper "[${STAMP}] | tojson" | sha256sum)" = "$(jqv stamper_none "[${STAMP}] | tojson" | sha256sum)" -a "$(jqv stamper "[${STAMP}] | tojson" | sha256sum)" = "$(jqv stamper_five "[${STAMP}] | tojson" | sha256sum)"
    t "the number of ClusterPolicy objects is one with no namespace, three and five" test "$(for n in stamper_none stamper stamper_five; do jqv "$n" '[.[] | select(.kind == "ClusterPolicy")] | length'; done | sort -u | tr -d '\n')" = 1
    t "the policy and every rule carry the prefix br-sandbox- and a rule name is at most 63 characters" jqt stamper "[${STAMP} | select(.metadata.name | startswith(\"br-sandbox-\")) | .spec.rules[] | select((.name | startswith(\"br-sandbox-\")) and (.name | length <= 63))] | length == 34"
    t "every rule name is unique" jqt stamper "[${STAMP} | .spec.rules[].name] | (unique | length) == length"
    t "no text of the policy reads a label of the namespace or any request field but the name" jqt stamper "[${STAMP} | .. | strings | select(contains(\"request.object\") and (contains(\"request.object.metadata.name\") | not))] | length == 0"
    t "the policy holds no variable but the name of the namespace" jqt stamper "[${STAMP} | .. | strings | gsub(\"\\\\{\\\\{ request\\\\.object\\\\.metadata\\\\.name \\\\}\\\\}\"; \"\") | select(contains(\"{{\"))] | length == 0"
    t "the variable of the name is in the namespace, the document of the adapter and the pool id of the pod template of each namespace rule" jqt stamper "[${STAMP} | .spec.rules[] | select(.generate.name == \"br-sandbox-config\" or .generate.name == \"br-sandbox-pod-template\") | (.generate.data.data | to_entries[0].value) | contains(\"{{ request.object.metadata.name }}\")] | all"
    t "no placeholder of the template engine is left in the render" test -z "$(grep -c 'STAMPTOKEN' "$WORK/stamper_yaml.yaml" | grep -v '^0$' || true)"
    t "the stamped policy is valid YAML with 34 rules" test "$(yq 'select(.kind == "ClusterPolicy") | .spec.rules | length' "$WORK/stamper_yaml.yaml")" = 34
}

check_client_and_runner_come_from_the_scope_table() {
    section "stamper: client_id and the runner subject are the values of the scope, never of a label"
    local row scope mode name client sa ns config
    for row in 'sbx-alpha|none|sbx-alpha-none|alpha|runner|runners' 'sbx-beta|none|sbx-beta-none|beta-saves|beta-runner|beta-runners' 'sbx-beta|full|sbx-beta-full|beta-saves|beta-runner|beta-runners'; do
        IFS='|' read -r scope mode name client sa ns <<<"$row"
        config="$(jq -r --arg scope "$scope" --arg mode "$mode" --arg n "$name" "${STAMPER_JQ}"'stamped($scope; $mode; $n)[] | select(.metadata.name == "br-sandbox-config") | .data["config.json"] | fromjson | .client_id + "/" + .pool_id' "$WORK/stamper_scoped.json")"
        t "${scope} ${mode}: client_id ${client} and the pool id is the name of the namespace" test "$config" = "${client}/${name}"
        t "${scope} ${mode}: the runner binding names ${ns}/${sa}" test "$(jq -r --arg scope "$scope" --arg mode "$mode" --arg n "$name" "${STAMPER_JQ}"'stamped($scope; $mode; $n)[] | select(.metadata.name == "br-sandbox-runner") | .subjects[0] | .namespace + "/" + .name' "$WORK/stamper_scoped.json")" = "${ns}/${sa}"
    done
    t "the stamped keeper binding names the ClusterRole br-sandbox-controller and the keeper of the system namespace" test "$(jq -r --arg n sbx-alpha-none "${STAMPER_JQ}"'stamped("sbx-alpha"; "none"; $n)[] | select(.metadata.name == "br-sandbox-keeper") | .roleRef.name + "/" + .subjects[0].namespace + "/" + .subjects[0].name' "$WORK/stamper.json")" = br-sandbox-controller/sbx-system/br-sandbox-keeper
    t "the scope table overlap is a fault with the stamper on" fails_with "BR_SANDBOX_SCOPES_OVERLAP{scope=sbx,other=sbx-alpha}" -f "$EXAMPLE" -f "$STAMPER_VALUES" --set scopes.sbx.runner.service_account=r --set scopes.sbx.runner.namespace=n
}

check_the_stamper_refuses_what_it_cannot_stamp() {
    section "stamper: render faults"
    fails_with "BR_SANDBOX_OPERATOR_PART_REQUIRED{key=stamper}" -f "$EXAMPLE" -f "$STAMPER_VALUES" --set operator.enabled=false --set network_check.enabled=false
    fails_with "BR_SANDBOX_STAMPER_NO_SCOPES" -f "$EXAMPLE" -f "$STAMPER_VALUES" --set-json 'scopes={}' --set-json 'namespaces=[]'
    fails_with "BR_SANDBOX_STAMPER_NOT_A_WRITER{identity=${STAMPER_IDENTITY}}" -f "$EXAMPLE" -f "$STAMPER_VALUES" --set-json 'grants.writers=[]'
    fails_with "BR_SANDBOX_STAMPER_NOT_A_WRITER{identity=system:serviceaccount:other:kyverno}" -f "$EXAMPLE" -f "$STAMPER_VALUES" --set stamper.identity=system:serviceaccount:other:kyverno
    t "another identity listed as writer makes the render pass" render stamper_other -f "$EXAMPLE" -f "$STAMPER_VALUES" --set stamper.identity=system:serviceaccount:other:kyverno --set 'grants.writers={system:serviceaccount:other:kyverno}'
    fails_with "BR_SANDBOX_MODE_NOT_STAMPED{namespace=sbx-alpha-allow,mode=allow}" -f "$EXAMPLE" --set stamper.enabled=true --set "grants.writers={${STAMPER_IDENTITY}}"
    fails_with "BR_SANDBOX_MODE_NOT_STAMPED{namespace=sbx-alpha-full,mode=full}" -f "$(valsx stampcred ".namespaces |= map(select(.name == \"sbx-alpha-full\")) | .stamper.enabled = true | .operator.enabled = true | .operator.image = \"registry.example.test/keeper/br-sandbox-keeper:0.1.0\" | .grants.writers = [\"${STAMPER_IDENTITY}\"]")"
    fails_with "BR_SANDBOX_STAMPER_RULE_NAME_TOO_LONG{rule=br-sandbox-sbx-gamma-with-a-very-long-scope-name-" -f "$EXAMPLE" -f "$STAMPER_VALUES" --set scopes.sbx-gamma-with-a-very-long-scope-name-x.runner.service_account=r --set scopes.sbx-gamma-with-a-very-long-scope-name-x.runner.namespace=n
}

check_the_identity_of_the_stamper_is_a_writer_of_the_fence() {
    section "stamper: the identity of the policy engine passes the fence policy, a project identity does not"
    local grants res
    grants="$(grants_of stamper)"
    for res in networkpolicies resourcequotas limitranges rolebindings; do
        t "the identity of the stamper writes ${res} in a managed namespace" test "$(fence_verdict stamper CREATE "$res" anything "$STAMPER_IDENTITY" "$grants")" = admit
        t "a project identity is refused ${res} in a managed namespace" test "$(fence_verdict stamper CREATE "$res" anything "$PROJECT_USER" "$grants")" = BR_SANDBOX_FENCE
    done
    t "the identity of the stamper writes the ConfigMap br-sandbox-pod-template and the ServiceAccount sandbox" test "$(fence_verdict stamper CREATE configmaps br-sandbox-pod-template "$STAMPER_IDENTITY" "$grants")$(fence_verdict stamper CREATE serviceaccounts sandbox "$STAMPER_IDENTITY" "$grants")" = admitadmit
}
