#!/usr/bin/env bash

adapter_namespaces() {
    jq -r '.[] | select(.kind == "ConfigMap" and .metadata.name == "br-sandbox-config") | .metadata.namespace' "$WORK/$1.json"
}

pod_containers() {
    jq -r --arg ns "$2" '.[] | select(.kind == "ReplicaSet" and .metadata.namespace == $ns) | .spec.template.spec.containers | map(.name) | join(" ")' "$WORK/$1.json"
}

container_names_agree() {
    local source="$1" expression="$2" namespace
    [[ -n "$(adapter_namespaces "$source")" ]] || return 1
    for namespace in $(adapter_namespaces "$source"); do
        [[ "$(adapter_doc "$source" "$namespace" "$expression")" == "$(pod_containers "$source" "$namespace")" ]] || return 1
    done
}

container_names_with_seed='[.agent_container, .seed_container] | join(" ")'
container_names_without_seed='[.agent_container] | join(" ")'

render_copy() {
    local name="$1" dir="$2"
    shift 2
    helm template t "$dir" "$@" 2>"$WORK/$name.err" | yq ea -o=json '[select(. != null)]' - >"$WORK/$name.json"
    [[ "${PIPESTATUS[0]}" -eq 0 ]]
}

mutate_copy() {
    local dir="$1" file="$2" expression="$3"
    cp -R "$CHART" "$dir"
    sed "$expression" "$CHART/$file" >"$dir/$file.mutated"
    mv "$dir/$file.mutated" "$dir/$file"
    ! cmp -s "$dir/$file" "$CHART/$file"
}

changelog_entry() {
    awk '/^## 0.2.4$/ {on=1; next} /^## / {on=0} on' "$CHART/CHANGELOG.md"
}

check_the_container_names_of_the_document_are_the_names_of_the_containers_of_the_pod() {
    section "the adapter document names the agent container and the seed container of the pod, in every namespace"
    render names_plain -f "$EXAMPLE"
    render names_cred -f "$EXAMPLE" -f "$EGRESS_VALUES"
    render names_seeder -f "$EXAMPLE" --set tools.seed_container.name=seeder
    render names_noseed -f "$EXAMPLE" --set tools.seed_container.enabled=false
    t "the example has sandbox namespaces to compare" test "$(adapter_namespaces names_plain | grep -c .)" -ge 3
    t "the example: agent_container and seed_container are the names of the first and second container of the pod, in each namespace" container_names_agree names_plain "$container_names_with_seed"
    t "the example with egress: the same, in each namespace" container_names_agree names_cred "$container_names_with_seed"
    t "a seed container with another name: the same, and the pod holds that name" container_names_agree names_seeder "$container_names_with_seed"
    t "a seed container with another name: the second container of the pod is named seeder" test "$(pod_containers names_seeder sbx-alpha-none)" = "agent seeder"
    t "no seed container: the pod holds the agent container only, and the document names it" container_names_agree names_noseed "$container_names_without_seed"
    t "no seed container: the pod has one container" test "$(pod_containers names_noseed sbx-alpha-none)" = agent
    t "no seed container: the document holds no seed_container and no seed_tools_path, and keeps seed_root and tools_path" test "$(adapter_doc names_noseed sbx-alpha-none '[has("seed_container"), has("seed_tools_path"), has("seed_root"), has("tools_path")] | map(tostring) | join(" ")')" = "false false true true"
    t "no seed container: the other keys are the keys of the document with the seed container" test "$(adapter_doc names_noseed sbx-alpha-none 'keys | join(" ")')" = "$(adapter_doc names_plain sbx-alpha-none 'del(.seed_container, .seed_tools_path) | keys | join(" ")')"
    t "with the seed container the document holds both seed keys" test "$(adapter_doc names_plain sbx-alpha-none '[has("seed_container"), has("seed_tools_path")] | map(tostring) | join(" ")')" = "true true"
    t "mutation: a chart whose document gives the seed container as a literal differs from the chart" mutate_copy "$WORK/chartseedname" templates/_adapter_config.tpl 's|"seed_container" \$values.tools.seed_container.name|"seed_container" "seed"|'
    render_copy names_mut_seed "$WORK/chartseedname" -f "$EXAMPLE" --set tools.seed_container.name=seeder
    t "mutation: with a literal seed container name the document and the pod disagree" test "$(container_names_agree names_mut_seed "$container_names_with_seed" && echo agree || echo differ)" = differ
    t "mutation: a chart whose document gives the agent container of the adapter default differs from the chart" mutate_copy "$WORK/chartagentname" templates/_adapter_config.tpl 's|"agent_container" (include "br-sandbox-chart.agent_container" .root)|"agent_container" "sandbox"|'
    render_copy names_mut_agent "$WORK/chartagentname" -f "$EXAMPLE"
    t "mutation: with the agent container name of the adapter default the document and the pod disagree" test "$(container_names_agree names_mut_agent "$container_names_with_seed" && echo agree || echo differ)" = differ
    t "mutation: a chart that always renders the seed keys differs from the chart" mutate_copy "$WORK/chartalwaysseed" templates/_adapter_config.tpl 's|{{- if \$values.tools.seed_container.enabled -}}|{{- if true -}}|'
    render_copy names_mut_noseed "$WORK/chartalwaysseed" -f "$EXAMPLE" --set tools.seed_container.enabled=false
    t "mutation: with the seed keys always rendered the document of a pod with no seed container names one" test "$(adapter_doc names_mut_noseed sbx-alpha-none 'has("seed_container")')" = true
    t "the README says that the chart names the agent container agent and that the default of the adapter is sandbox" grep -qF 'The chart names the agent container `agent` and gives that name in `agent_container`; the default of the adapter is another name, `sandbox`, so a document without that key would name a container that the pod does not have.' "$README"
    t "the README says what the document holds with no seed container, and what the adapter does" grep -qF 'With `tools.seed_container.enabled` false the pod has no seed container, so the document holds neither `seed_container` nor `seed_tools_path`, and the adapter then uses its defaults `seed` and `/sbx/bin/bsdtar`; the failure of an open with a non-empty seed follows from the adapter code and is not tested here or on a cluster (the exec would name a container that the pod does not have), and the same holds for the statement that an open with an empty seed is not affected.' "$README"
    t "the CHANGELOG 0.2.4 says the same about the seed container" test "$(changelog_entry | grep -cF 'With `tools.seed_container.enabled` false the pod has no seed container, and the document holds neither `seed_container` nor `seed_tools_path`')" -eq 1
    t "the README marks the failure of an open with a non-empty seed as read from the adapter code and not tested" grep -qF 'the failure of an open with a non-empty seed follows from the adapter code and is not tested here or on a cluster' "$README"
    t "the README no longer states that failure as a fact of the chart" test "$(grep -c 'an open with a non-empty seed then fails' "$README")" -eq 0
    t "the CHANGELOG 0.2.4 marks the failure of an open with a non-empty seed as read from the adapter code and not tested" test "$(changelog_entry | grep -cF 'the adapter then uses its defaults `seed` and `/sbx/bin/bsdtar`; the failure of an open with a non-empty seed follows from the adapter code and is not tested here or on a cluster')" -eq 1
    t "the CHANGELOG no longer states that failure as a fact of the chart" test "$(grep -c 'an open with a non-empty seed fails' "$CHART/CHANGELOG.md")" -eq 0
}

check_the_documents_say_what_the_chart_holds_for_the_constant_and_the_save_paths() {
    section "the README and the CHANGELOG: label_prefix is a constant, and the adapter checks save.paths"
    t "the README says label_prefix is a constant of both documents and not a chart value" grep -qF 'holds the same constant `label_prefix` (`br.sandbox`, which is not a chart value) and the same chart value for `idle_ttl`' "$README"
    t "the README says the overlay sets the chart values and not the constant" grep -qF 'which sets each of these chart values away from its default (`label_prefix` is a constant, so it is not set)' "$README"
    t "the README no longer calls label_prefix a chart value of both documents" test "$(grep -c 'holds the same chart value for `label_prefix`' "$README")" -eq 0
    t "the CHANGELOG 0.2.4 says label_prefix is a constant, the same in both documents" test "$(changelog_entry | grep -cF '`label_prefix` is a constant (`br.sandbox`), the same in both documents; the store keys')" -eq 1
    t "the CHANGELOG no longer says that label_prefix is one chart value" test "$(grep -c '`label_prefix`, the store keys, the save age, the idle time and the idle threshold are one chart value' "$CHART/CHANGELOG.md")" -eq 0
    t "label_prefix is no key of the values and of the schema" test "$(grep -c 'label_prefix' "$CHART/values.yaml" "$CHART/values.schema.json" | grep -vc ':0$')" -eq 0
    t "the Why row on one value, one source says label_prefix is a constant and not a chart value" grep -qF '| `label_prefix` is a constant (`br.sandbox`), the same in both documents and not a chart value; the store keys, the save age, the idle time and the idle threshold are rendered into both documents from the same chart value, and a test with values that differ from their defaults checks both documents;' "$README"
    t "the Why row no longer groups label_prefix with the values that a test sets away from the default" test "$(grep -c '`label_prefix`, the store keys, the save age, the idle time and the idle threshold are rendered' "$README")" -eq 0
    t "the README says that store.path_style is required with no default in the chart, that true is to be used, that the virtual-host style has a known defect read from the code, and that the chart does not test it" grep -qF '`store.path_style` is required and the chart sets no default; use `true`. The virtual-host style (`false`) has a known defect in the adapter and the keeper 0.1.x, read from their code and not run on a live store, and the chart does not test it: both build the key with an empty path segment, and a fix to one library alone would make them address different keys.' "$README"
    t "the README no longer says that the two libraries address different objects, that saves may not expire, or that path_style must be true" test "$(grep -cE 'address different objects|may then not expire|must be `true`' "$README")" -eq 0
    t "the CHANGELOG no longer says that the two libraries address different objects, that saves may not expire, or that path_style must be true" test "$(grep -cE 'address different objects|may then not expire|must be `true`' "$CHART/CHANGELOG.md")" -eq 0
    t "the README says that the adapter, not the chart, refuses a save.paths list without /work, with a nested path, with the seed root, or with a .. part" grep -qF 'The chart does not check `save.paths` against the rules of the adapter: the adapter refuses, when it starts, a list that does not hold `/work` (`ScopeWithoutWork`), a list in which one path holds or equals another (`ScopeNested`), a list in which a path holds or is held by `/skills`, the seed root (`SeedRootInScope`), and a path with a `..` part (`PathEscapes`), and the chart renders such a list as it is given.' "$README"
    t "the CHANGELOG 0.2.4 says the chart renders save.paths as given" test "$(changelog_entry | grep -cF '`save.paths` is rendered as given.')" -eq 1
    render paths_without_work -f "$EXAMPLE" --set 'save.paths={/home/agent}'
    render paths_nested -f "$EXAMPLE" --set 'save.paths={/work,/work/sub}'
    render paths_seed_root -f "$EXAMPLE" --set 'save.paths={/skills,/work}'
    t "a list without /work is rendered as given" test "$(adapter_doc paths_without_work sbx-alpha-none '.save_scope_paths | join(",")')" = /home/agent
    t "a nested list is rendered as given" test "$(adapter_doc paths_nested sbx-alpha-none '.save_scope_paths | join(",")')" = /work,/work/sub
    t "a list that holds the seed root is rendered as given" test "$(adapter_doc paths_seed_root sbx-alpha-none '.save_scope_paths | join(",")')" = /skills,/work
}

pod_specs='.[] | (if .kind == "Pod" then .spec elif .kind == "CronJob" then .spec.jobTemplate.spec.template.spec elif (.kind | IN("ReplicaSet", "Deployment", "StatefulSet", "DaemonSet", "Job")) then .spec.template.spec else empty end)'

config_maps_of_pods() {
    jq -r "[${pod_specs} | .. | objects | (.configMap, .configMapRef, .configMapKeyRef) | objects | .name] | unique | join(\" \")" "$WORK/$1.json"
}

no_pod_refers_to_the_adapter_config() {
    local source
    for source in "$@"; do
        [[ " $(config_maps_of_pods "$source") " != *" br-sandbox-config "* ]] || return 1
    done
}

check_no_pod_refers_to_the_adapter_config_map() {
    section "no Pod, ReplicaSet, Job or CronJob of the render mounts or reads the ConfigMap br-sandbox-config"
    render refs_keeper -f "$EXAMPLE" -f "$KEEPER_VALUES" --set network_check.cron='0 * * * *'
    t "the query finds the ConfigMaps that the pods of the render use, so the check is not empty" test " $(config_maps_of_pods refs_keeper) " = " br-sandbox-keeper-config br-sandbox-netcheck "
    t "the document ConfigMap exists in the render, so a pod could have named it" test "$(adapter_namespaces refs_keeper | grep -c .)" -ge 3
    t "no workload of the renders of the example, the keeper, the egress and the mirror refers to br-sandbox-config" no_pod_refers_to_the_adapter_config refs_keeper cred ex mirror egress_keeper keeper nondefault
    t "mutation: a chart with a volume of that ConfigMap in the pod template differs from the chart" mutate_copy "$WORK/chartmountconfig" templates/_pod.tpl 's|(dict "name" "home" "emptyDir"|(dict "name" "adapter" "configMap" (dict "name" "br-sandbox-config")) (dict "name" "home" "emptyDir"|'
    render_copy refs_mut "$WORK/chartmountconfig" -f "$EXAMPLE"
    t "mutation: the query finds the volume of that ConfigMap" test "$(config_maps_of_pods refs_mut)" != "$(config_maps_of_pods refs_keeper)" -a "$(config_maps_of_pods refs_mut | grep -c 'br-sandbox-config')" -eq 1
    t "mutation: the check refuses that render" test "$(no_pod_refers_to_the_adapter_config refs_mut && echo none || echo found)" = found
}
