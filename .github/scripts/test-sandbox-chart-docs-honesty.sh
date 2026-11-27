#!/usr/bin/env bash

all_chart_text() {
    cat "$CHART"/*.md
}

check_the_project_side_is_not_claimed_as_proved() {
    section "STAMPER.md and LIMITS.md: the project side is proved only once a cluster run has passed, and none is recorded"
    local sentence='The project side is proved only once a cluster run of the install in `INSTALL.md` has passed; none is recorded at this version.'
    t "STAMPER.md holds the conditional sentence" grep -qF "$sentence" "$CHART/STAMPER.md"
    t "LIMITS.md holds the conditional sentence" grep -qF "$sentence" "$CHART/LIMITS.md"
    t "no document says that the project side is proved before a run" test "$(all_chart_text | grep -ciE 'until then the project side is proved|project side is proved by')" -eq 0
    t "no document names a plan row or a battery" test "$(all_chart_text | grep -ciE '\(P[0-9]+\)|battery')" -eq 0
}

check_the_network_check_covers_the_namespaces_of_its_release() {
    section "the network check probes the namespaces of the release that holds it: none in the two-release install"
    local exact
    render nc_two_operator -f "$EXAMPLE" -f "$CHART/ci/platform-values.yaml" --set network_check.enabled=true
    render nc_single -f "$EXAMPLE" -f "$OPERATOR_VALUES" --set network_check.enabled=true
    t "operator release with namespaces [] and the check on: the ConfigMap of the system namespace lists no namespace" jqt nc_two_operator '[.[] | select(.kind == "ConfigMap" and .metadata.name == "br-sandbox-netcheck" and .metadata.namespace == "sbx-system") | .data.namespaces | gsub("\\s"; "")] == [""]'
    t "operator release with namespaces [] and the check on: no object br-sandbox-netcheck in any other namespace" jqt nc_two_operator '[.[] | select(.metadata.name == "br-sandbox-netcheck" and .metadata.namespace != null and .metadata.namespace != "sbx-system")] | length == 0'
    fails_with "BR_SANDBOX_OPERATOR_PART_REQUIRED{key=network_check}" -f "$EXAMPLE" -f "$CHART/ci/fence-values.yaml" --set network_check.enabled=true
    t "single release with the check on: the ConfigMap of the system namespace lists the four namespaces" jqt nc_single '[.[] | select(.kind == "ConfigMap" and .metadata.name == "br-sandbox-netcheck" and .metadata.namespace == "sbx-system") | .data.namespaces | split("\n") | map(select(length > 0))] | map(length) == [4]'
    t "single release with the check on: each of the four namespaces holds the ConfigMap, the Role and the RoleBinding br-sandbox-netcheck" jqt nc_single '[.[] | select(.metadata.name == "br-sandbox-netcheck" and .metadata.namespace != null and .metadata.namespace != "sbx-system") | .kind] | sort | group_by(.) | map(length) == [4, 4, 4]'
    exact='In the two-release install, chart 0.4.0 runs no network check of a sandbox namespace. In that shape nothing of the chart proves on the cluster that the network plugin enforces the policies of a sandbox namespace. The fence check of the keeper proves the names and the shape, not the enforcement.'
    t "INSTALL.md carries the exact sentences" grep -qF "$exact" "$CHART/INSTALL.md"
    t "LIMITS.md carries the exact sentences" grep -qF "$exact" "$CHART/LIMITS.md"
    t "LIMITS.md says the key namespaces of the ConfigMap is empty and no probe pod starts" grep -qF 'the key `namespaces` of its ConfigMap is empty, no probe pod starts' "$CHART/LIMITS.md"
    t "the README states the limit where it describes the probe pod of each namespace and names the route" grep -qF 'so the check probes no sandbox namespace there (the key `namespaces` of its ConfigMap is empty and no probe pod starts), and the fence release cannot turn the check on' "$CHART/README.md"
    t "no document says another route without naming it" test "$(all_chart_text | grep -ciE 'by another route|check them by')" -eq 0
}

check_the_netcheck_route_renders_the_check_of_one_probe_namespace() {
    section "the route of INSTALL.md: one probe namespace in the operator release, with the check on"
    local route="$CHART/ci/netcheck-route-values.yaml"
    render route -f "$EXAMPLE" -f "$CHART/ci/platform-values.yaml" -f "$route"
    render route_fence -f "$EXAMPLE" -f "$CHART/ci/fence-values.yaml"
    t "the route file lists one namespace of mode none and turns the check on" test "$(yq -o=json -I=0 '[[.namespaces[] | .name + "/" + .mode], .render_namespace, .network_check.enabled, (keys | length)]' "$route")" = '[["sbx-alpha-netcheck/none"],true,true,3]'
    t "the probe namespace matches a scope of the fixture" test "$(yq '.scopes | keys | .[]' "$EXAMPLE" | grep -c '^sbx-alpha$')" -eq 1
    t "the Job of the check exists in the system namespace" jqt route '[.[] | select(.kind == "Job" and .metadata.name == "br-sandbox-netcheck-initial" and .metadata.namespace == "sbx-system")] | length == 1'
    t "the ConfigMap, the Role and the RoleBinding br-sandbox-netcheck exist in the probe namespace" jqt route '[.[] | select(.metadata.name == "br-sandbox-netcheck" and .metadata.namespace == "sbx-alpha-netcheck") | .kind] | sort == ["ConfigMap", "Role", "RoleBinding"]'
    t "the ConfigMap of the system namespace lists the probe namespace and no other" jqt route '[.[] | select(.kind == "ConfigMap" and .metadata.name == "br-sandbox-netcheck" and .metadata.namespace == "sbx-system") | .data.namespaces | split("\n") | map(select(length > 0))] == [["sbx-alpha-netcheck none"]]'
    t "the probe namespace holds the fence objects" jqt route '[.[] | select(.metadata.namespace == "sbx-alpha-netcheck") | .kind] | contains(["ResourceQuota", "LimitRange", "NetworkPolicy", "ServiceAccount", "RoleBinding"])'
    t "no namespace of the fence release of the install is in the route render" jqt route '[.[] | (.metadata.namespace // "") , (if .kind == "Namespace" then .metadata.name else "" end)] | map(select(. == "sbx-alpha-none" or . == "sbx-alpha-allow" or . == "sbx-alpha-full" or . == "sbx-beta-none")) | length == 0'
    t "no object of the fence release has the kind, namespace and name of an object of the route render" test "$(jq -rn --slurpfile a "$WORK/route.json" --slurpfile b "$WORK/route_fence.json" '[$a[0][] | [.kind, (.metadata.namespace // ""), .metadata.name] | join("/")] as $x | [$b[0][] | [.kind, (.metadata.namespace // ""), .metadata.name] | join("/")] | map(select(. as $y | $x | index($y))) | length')" -eq 0
    t "helm lint --strict passes on the operator release with the route" helm lint --strict "$CHART" -f "$EXAMPLE" -f "$CHART/ci/platform-values.yaml" -f "$route"
    t "INSTALL.md names the route command with the three values files and crds.install" test "$(install_block 5 | grep -c -e 'helm upgrade br-sandbox ' -e 'ci/netcheck-route-values.yaml' -e 'ci/platform-values.yaml' -e 'ci/example-values.yaml' -e 'crds.install=true')" -eq 5
    ruling='This route probes one probe namespace in mode `none` that the operator release makes. It proves on this cluster that the network plugin enforces the default-deny policy of the chart, and that no rule of the cluster opens a namespace with this name. It does not probe a namespace of a project, the modes `allow` and `full`, or a fence that the stamper made.'
    t "INSTALL.md and LIMITS.md carry the ruling words after the route" test "$(grep -cF "$ruling" "$CHART/INSTALL.md" "$CHART/LIMITS.md" | grep -c ':1$')" -eq 2
    t "INSTALL.md writes the three values of the route, the name rule and its reason" grep -qF '`namespaces: [{name: sbx-alpha-netcheck, mode: none}]`, `render_namespace: true` and `network_check.enabled: true`. The name is the name of a scope of the table `scopes`' "$CHART/INSTALL.md"
    t "INSTALL.md says why the name follows the rule of real namespaces" grep -qF 'so that the rules of a cluster that act by the name of a namespace act on the probe namespace as on a real one' "$CHART/INSTALL.md"
    t "INSTALL.md says that the checks of allow and full on a real cluster are not this Job, and how to remove the route" test "$(grep -cF 'The checks of the modes `allow` and `full` on a real cluster are not this Job. To remove the route, run `helm upgrade` of the operator release again without the file.' "$CHART/INSTALL.md")" -eq 1
    t "INSTALL.md says that the route was not run by the tests of this repository at this version" grep -qF 'the route was not run on a cluster by the tests of this repository at this version, and a cluster run is made by the platform test' "$CHART/INSTALL.md"
    t "INSTALL.md says that the probe namespace is used only for the proof, carries the fence part of the operator release, and that the route was not run by the tests of this repository" test "$(grep -cF 'The probe namespace is a namespace used only for the proof. It carries the fence part from the operator release' "$CHART/INSTALL.md")" -eq 1 -a "$(grep -cF 'the route was not run on a cluster by the tests of this repository at this version, and a cluster run is made by the platform test' "$CHART/INSTALL.md")" -eq 1
}

check_the_changelog_040_holds_the_values_the_behaviour_and_the_faults() {
    section "the CHANGELOG 0.4.0 lists the moved values, the pod policy at an update and every new fault"
    local text code
    text="$(changelog_040)"
    t "it says that pool.size becomes limits.size and pool.max_sandboxes becomes limits.pods" grep -qF '`pool.size` becomes `limits.size` and `pool.max_sandboxes` becomes `limits.pods`' <<<"$text"
    t "the schema refuses pool.size and pool.max_sandboxes" bash -c '! helm template t "$1" -f "$2" --set pool.max_sandboxes=3 >/dev/null 2>&1 && ! helm template t "$1" -f "$2" --set-json "pool.size={\"cpu\":{}}" >/dev/null 2>&1' _ "$CHART" "$EXAMPLE"
    t "limits.size and limits.pods exist in values.yaml" test "$(yq '.limits.pods' "$CHART/values.yaml")" = 20 -a "$(yq '.limits.size.cpu.limit' "$CHART/values.yaml")" = 2
    for code in 'limits.size' 'limits.pods' '`namespaces`' '`scopes`' '`render_namespace`' '`grants.writers`' '`grants.modes`' '`crds.install`' '`stamper.enabled`' '`stamper.identity`'; do
        t "the Values added bullet names ${code}" grep -qF -- "${code}" <<<"$(grep '^- Values added:' <<<"$text")"
    done
    t "the keys named in the Values added bullet exist in values.yaml" test "$(yq '[.limits, .namespaces, .scopes, .render_namespace, .grants.writers, .grants.modes, .crds.install, .stamper.enabled, .stamper.identity] | map(. == null) | any' "$CHART/values.yaml")" = false
    t "it says that network_check.enabled was true and is false, outside the Removed bullet" test "$(grep -cF 'Default changed (from `true`): `network_check.enabled` is `false` by default, and the network check needs `operator.enabled`.' <<<"$text")" -eq 1 -a "$(grep '^- Removed:' <<<"$text" | grep -c 'network_check')" -eq 0 -a "$(yq '.network_check.enabled' "$CHART/values.yaml")" = false
    t "it says that the per-namespace policies {namespace}-pod are gone" grep -qF 'The per-namespace pod policies and bindings `{namespace}-pod` of 0.3.0 are gone: one policy `br-sandbox-pod` serves every managed namespace.' <<<"$text"
    t "no render holds a policy named after a namespace" jqt keeper '[.[] | select(.kind == "ValidatingAdmissionPolicy" or .kind == "ValidatingAdmissionPolicyBinding") | .metadata.name | select(endswith("-pod") and . != "br-sandbox-pod")] | length == 0'
    t "it states the pod policy at an update, with the test file" grep -qF 'In 0.4.0 the rules that read the `SandboxPool` judge a creation only (`BR_SANDBOX_IMAGE`, and the bound of the agent size by the pool in `BR_SANDBOX_RESOURCES`' <<<"$text"
    t "it says that BR_SANDBOX_IMMUTABLE compares the images, the init containers, the ephemeral containers and serviceAccountName, and that pods/resize is matched" grep -qF '`BR_SANDBOX_IMMUTABLE` now also compares the images, the init containers, the ephemeral containers and `serviceAccountName`. The binding `br-sandbox-pod-update` matches the update of `pods`, `pods/ephemeralcontainers` and `pods/resize`, and has no `paramRef`.' <<<"$text"
    t "the rendered update binding matches pods, pods/ephemeralcontainers and pods/resize on UPDATE and has no paramRef" jqt keeper '[.[] | select(.kind == "ValidatingAdmissionPolicyBinding" and .metadata.name == "br-sandbox-pod-update")] | length == 1 and (.[0].spec | has("paramRef") | not)'
    t "the rendered policy matches pods/resize and pods/ephemeralcontainers on UPDATE" jqt keeper '[.[] | select(.kind == "ValidatingAdmissionPolicy" and .metadata.name == "br-sandbox-pod") | .spec.matchConstraints.resourceRules[] | select(.operations == ["UPDATE"]) | .resources[]] | contains(["pods/resize", "pods/ephemeralcontainers"])'
    t "the rendered immutable rule compares the init containers, the ephemeral containers and the account name" jqt keeper '[.[] | select(.kind == "ValidatingAdmissionPolicy" and .metadata.name == "br-sandbox-pod") | .spec.validations[] | select(.message | startswith("BR_SANDBOX_IMMUTABLE")) | .expression | (contains("object.spec.initContainers == oldObject.spec.initContainers") and contains("object.spec.ephemeralContainers == oldObject.spec.ephemeralContainers") and contains("object.spec.serviceAccountName == oldObject.spec.serviceAccountName") and contains("object.spec.containers == oldObject.spec.containers"))] == [true]'
    for code in BR_SANDBOX_NOTHING_TO_RENDER 'BR_SANDBOX_OPERATOR_PART_REQUIRED{key=network_check}` and `{key=stamper}' 'BR_SANDBOX_OPERATOR_SETTING_REMOVED{key=pools}' 'BR_SANDBOX_NO_SCOPE{namespace=...}' 'BR_SANDBOX_SCOPES_OVERLAP{scope=...,other=...}' 'BR_SANDBOX_MODE_UNKNOWN{namespace=...,mode=...}' 'BR_SANDBOX_MODE_NOT_STAMPED{namespace=...,mode=...}' BR_SANDBOX_STAMPER_NO_SCOPES 'BR_SANDBOX_STAMPER_NOT_A_WRITER{identity=...}' 'BR_SANDBOX_STAMPER_RULE_NAME_TOO_LONG{rule=...}'; do
        t "the Render faults bullet names ${code}" grep -qF -- "${code}" <<<"$(grep '^- Render faults, new in 0.4.0' <<<"$text")"
        t "the template of the chart raises ${code%%\{*}" grep -rqF -- "\"${code%%\{*}" "$CHART/templates"
    done
    fails_with 'BR_SANDBOX_MODE_UNKNOWN{namespace=sbx-alpha-x,mode=bogus}' -f "$EXAMPLE" --set-json 'namespaces=[{"name":"sbx-alpha-x","mode":"bogus"}]'
    t "the CHANGELOG names the test file of the policy model" grep -qF 'Tested offline with the policy model (`test-sandbox-chart-pod-policy.sh`)' <<<"$text"
}

check_the_readme_lists_the_stamper_values_the_faults_and_the_policies() {
    section "the README values table, error list and render table hold the stamper, its faults and the three policies"
    local readme code name
    readme="$(cat "$CHART/README.md")"
    t "the values table has the row of stamper.enabled and stamper.identity, with the defaults of values.yaml" grep -qF "| \`stamper.{enabled,identity}\` | \`false\`, \`$(yq '.stamper.identity' "$CHART/values.yaml")\` |" <<<"$readme"
    t "the row says that the identity must be in grants.writers and that the stamper needs operator.enabled" grep -qF 'it must be in `grants.writers`, else `BR_SANDBOX_STAMPER_NOT_A_WRITER{identity=...}`; the chart does not add it.' <<<"$readme"
    for code in 'BR_SANDBOX_OPERATOR_PART_REQUIRED{key=network_check}` and `{key=stamper}' BR_SANDBOX_MODE_NOT_STAMPED BR_SANDBOX_STAMPER_NO_SCOPES BR_SANDBOX_STAMPER_NOT_A_WRITER BR_SANDBOX_STAMPER_RULE_NAME_TOO_LONG; do
        t "the render error list holds ${code}" grep -qF -- "${code}" <<<"$(grep '^Render errors:' <<<"$readme")"
    done
    t "every fault raised by a template appears in the README" test -z "$(grep -rhoE 'fail[^}]*"BR_SANDBOX_[A-Z_]+' "$CHART/templates" | grep -oE 'BR_SANDBOX_[A-Z_]+' | sort -u | while read -r code; do grep -qF "$code" "$CHART/README.md" || echo "$code"; done)"
    for name in $(jq -r '[.[] | select(.kind == "ValidatingAdmissionPolicy" or .kind == "ValidatingAdmissionPolicyBinding" or (.kind == "ConfigMap" and .metadata.name == "br-sandbox-grants")) | .metadata.name] | unique[]' "$WORK/keeper.json"); do
        t "the README names the rendered object ${name} in the table of a render" grep -qF "\`${name}\`" <<<"$(sed -n '/^## What a render holds/,/^## Layout/p' "$CHART/README.md")"
    done
    t "the intro of the README names the three policies and the grants" grep -qF 'the admission policies `br-sandbox-pod`, `br-sandbox-namespace` and `br-sandbox-fence` with their bindings, the ConfigMap `br-sandbox-grants`' <<<"$readme"
    t "the README no longer says that the admission policy and its two bindings are the cluster-scoped set" test "$(grep -cF 'the admission policy and its two bindings' <<<"$readme")" -eq 0
    t "the README says that the three policies, their four bindings and the grants are rendered once" grep -qF 'the three admission policies, their four bindings and the ConfigMap `br-sandbox-grants` are rendered once' <<<"$readme"
    t "the render holds three policies and four bindings" jqt keeper '([.[] | select(.kind == "ValidatingAdmissionPolicy")] | length == 3) and ([.[] | select(.kind == "ValidatingAdmissionPolicyBinding")] | length == 4)'
    t "the README says the keeper image is named by the fixtures, not tested with" test "$(grep -cF 'that this chart is tested with is the image' <<<"$readme")" -eq 0 -a "$(grep -cF 'that the fixtures of this chart name is the image' <<<"$readme")" -eq 1
}

check_the_install_names_the_required_fields_of_the_pool() {
    section "INSTALL.md: keeper 0.2.0 requires image, size, minReplicas and maxReplicas in a SandboxPool"
    t "INSTALL.md says that keeper 0.2.0 also requires size, minReplicas and maxReplicas and what happens without them" grep -qF 'but keeper 0.2.0 also requires `size`, `minReplicas` and `maxReplicas` in a `SandboxPool`: a resource without them is `InvalidSpec` and gets no pool' "$CHART/INSTALL.md"
    t "the keeper README lists the four fields as required" grep -qF '`image`, `size`, `minReplicas` and `maxReplicas` are required' "$ROOT/crates/br-sandbox-keeper/README.md"
    t "the SandboxPool example of INSTALL.md holds the four fields" test "$(install_block 4 | yq -o=json -I=0 '.' | jq '.spec | has("image") and has("size") and has("minReplicas") and has("maxReplicas")')" = true
}

check_the_pod_security_labels_are_stated_as_conditional() {
    section "Pod Security labels: rendered on a sandbox namespace only with render_namespace, set by the owner otherwise"
    local namespace grants
    render ps_off -f "$EXAMPLE" -f "$OPERATOR_VALUES" --set render_namespace=false
    t "with render_namespace false no sandbox namespace is rendered, and the system namespace carries the labels" jqt ps_off '([.[] | select(.kind == "Namespace" and .metadata.labels["br.sandbox/managed"] == "true")] | length == 0) and ([.[] | select(.kind == "Namespace" and .metadata.name == "sbx-system") | .metadata.labels["pod-security.kubernetes.io/enforce"]] == ["baseline"])'
    t "with render_namespace true each sandbox namespace carries the three labels" jqt keeper '[.[] | select(.kind == "Namespace" and .metadata.labels["br.sandbox/managed"] == "true") | .metadata.labels | select(.["pod-security.kubernetes.io/enforce"] == "baseline" and .["pod-security.kubernetes.io/warn"] == "restricted" and .["pod-security.kubernetes.io/audit"] == "restricted")] | length == 4'
    t "no admission policy of the chart and no stamper rule reads or sets a Pod Security label" test "$(jq -c '[.[] | select(.kind == "ValidatingAdmissionPolicy" or .kind == "ClusterPolicy")]' "$WORK/keeper.json" "$WORK/stamper.json" | grep -c 'pod-security')" -eq 0
    t "the Layout table says that the sandbox namespace gets the labels only with render_namespace, and the owner sets them otherwise" grep -qF 'only with `render_namespace: true`; otherwise the owner of the namespace sets the labels (no policy, no stamper and no fence part of the chart sets them)' "$CHART/README.md"
    t "Constants, not keys says the same" grep -qF 'with `render_namespace: true`, on the sandbox namespaces; otherwise the owner of the sandbox namespace sets them)' "$CHART/README.md"
    t "INSTALL.md says that the owner sets them when the project makes the Namespace" grep -qF 'The chart renders the Pod Security labels of a sandbox namespace only with `render_namespace: true`. Here the project makes the Namespace, so its owner sets them' "$CHART/INSTALL.md"
    namespace="$(install_block 3 | yq -o=json -I=0 '.')"
    grants="$(grants_of ex)"
    t "the Namespace example of INSTALL.md carries the three labels" test "$(jq -c '.metadata.labels | [.["pod-security.kubernetes.io/enforce"], .["pod-security.kubernetes.io/warn"], .["pod-security.kubernetes.io/audit"]]' <<<"$namespace")" = '["baseline","restricted","restricted"]'
    t "the policy model admits the Namespace without the labels: nothing requires them" test "$(ns_verdict ex CREATE "$(jq -c 'del(.metadata.labels["pod-security.kubernetes.io/enforce"], .metadata.labels["pod-security.kubernetes.io/warn"], .metadata.labels["pod-security.kubernetes.io/audit"])' <<<"$namespace")" null "$PROJECT_USER" "$grants")" = admit
}

check_the_documents_name_rules_by_code_and_keep_no_stale_text() {
    section "the documents name rules by code, and hold no stale key or earlier upgrade path"
    t "no README, POLICIES, LIMITS, STAMPER or INSTALL sentence numbers a rule" test "$(cat "$CHART/README.md" "$CHART/POLICIES.md" "$CHART/LIMITS.md" "$CHART/STAMPER.md" "$CHART/INSTALL.md" | grep -cE '\b[Rr]ules? [0-9]+')" -eq 0
    t "the README names the non-root rule by its code where it describes the probe pod and the check exception" test "$(grep -cF 'the rule `BR_SANDBOX_RUN_AS_NON_ROOT` of the admission policy lets it through' "$CHART/README.md")" -eq 1 -a "$(grep -cF 'so `BR_SANDBOX_RUN_AS_NON_ROOT` admits them by the ServiceAccount' "$CHART/README.md")" -eq 1
    t "the README no longer lists the pool keys among the keys of the keeper document only" test "$(grep -cF 'the pool keys' "$CHART/README.md")" -eq 0
    t "the keeper document holds no pool key" jqt keeper '[.[] | select(.kind == "ConfigMap" and .metadata.name == "br-sandbox-keeper-config") | .data["config.json"] | fromjson | has("pools") or has("floor") or has("max_sandboxes")] == [false]'
}

check_the_documents_state_what_a_cluster_run_showed() {
    section "INSTALL.md, LIMITS.md and POLICIES.md: the order of the fence part, the restored binding, and who refused a project write"
    t "INSTALL.md says that a namespace before its fence part makes the keeper log forbidden errors until the RoleBinding arrives" grep -qF 'the keeper logs forbidden list and watch errors about once a second until the RoleBinding `br-sandbox-keeper` of the fence part arrives, and then it recovers' "$CHART/INSTALL.md"
    t "INSTALL.md and LIMITS.md say that a restored binding needs --force-conflicts at the next upgrade" test "$(grep -cF 'the next `helm upgrade` of the operator release fails on a server-side-apply conflict and needs `--force-conflicts`' "$CHART/INSTALL.md" "$CHART/LIMITS.md" | grep -c ':1$')" -eq 2
    t "INSTALL.md and POLICIES.md say that RBAC refused the project write in the tested install, and that the policy refusal is not proved on a cluster" test "$(grep -cF 'refused by RBAC' "$CHART/INSTALL.md" "$CHART/POLICIES.md" | grep -c ':1$')" -eq 2 -a "$(grep -cF 'not by `BR_SANDBOX_FENCE`' "$CHART/INSTALL.md" "$CHART/POLICIES.md" | grep -c ':1$')" -eq 2
    render route_fence_docs -f "$EXAMPLE" -f "$CHART/ci/fence-values.yaml"
    t "the keeper RoleBinding named in INSTALL.md is rendered by the fence part" jqt route_fence_docs '[.[] | select(.kind == "RoleBinding" and .metadata.name == "br-sandbox-keeper")] | length > 0'
}

check_the_public_documents_name_no_private_material() {
    section "the documents of the chart name no private repository, path or distribution group"
    t "no document of the chart cites a path of a cluster repository, a README of another repository or a file of a cluster install" test "$(all_chart_text | grep -ciE 'clusters/[a-z]+/|\.application\.yaml|of the `[A-Za-z]+` repository|README of `[A-Za-z]+`|comment of `')" -eq 0
    t "no document of the chart names the administrator group of a distribution" test "$(all_chart_text | grep -ciE 'system:masters|administrator is a user')" -eq 0
    t "no file of the chart holds an absolute path of a developer machine" test "$(grep -rlE '/Users/|/home/[a-z]+/' "$CHART" | wc -l | tr -d ' ')" -eq 0
    t "STAMPER.md keeps the facts in generic words: the default network policy, the labelling policy and the rights of the policy engine" test "$(grep -cF 'The cluster default network policy.' "$CHART/STAMPER.md")" -eq 1 -a "$(grep -cF 'The scope labelling policy.' "$CHART/STAMPER.md")" -eq 1 -a "$(grep -cF 'Rights of Kyverno.' "$CHART/STAMPER.md")" -eq 1
}
