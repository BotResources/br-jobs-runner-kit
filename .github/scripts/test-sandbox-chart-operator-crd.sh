#!/usr/bin/env bash

CRD='.[] | select(.kind == "CustomResourceDefinition")'

render_crd_fixtures() {
    render crd_on -f "$EXAMPLE" --set crds.install=true
    render crd_only -f "$EXAMPLE" -f "$CHART/ci/crds-values.yaml"
}

check_the_definition_names_the_resource() {
    section "the definition: group, kind, plural, scope, version, the keep annotation"
    t "the definition is named plural.group" jqt crd_on "[${CRD} | .metadata.name] == [\"sandboxpools.sandbox.botresources.ai\"]"
    t "the group is sandbox.botresources.ai" jqt crd_on "[${CRD} | .spec.group] == [\"sandbox.botresources.ai\"]"
    t "the kind is SandboxPool and the plural is sandboxpools" jqt crd_on "[${CRD} | .spec.names | [.kind, .plural]] == [[\"SandboxPool\", \"sandboxpools\"]]"
    t "the scope is Namespaced" jqt crd_on "[${CRD} | .spec.scope] == [\"Namespaced\"]"
    t "the definition has one version, v1alpha1, served and stored" jqt crd_on "[${CRD} | [.spec.versions[] | [.name, .served, .storage]]] == [[[\"v1alpha1\", true, true]]]"
    t "the definition has the annotation helm.sh/resource-policy keep" jqt crd_on "[${CRD} | .metadata.annotations[\"helm.sh/resource-policy\"]] == [\"keep\"]"
    t "the definition carries the labels of the chart" jqt crd_on "[${CRD} | .metadata.labels[\"app.kubernetes.io/part-of\"]] == [\"br-sandbox\"]"
    t "the render of the example with crds.install holds the definition once" jqt crd_on "[${CRD}] | length == 1"
}

check_the_definition_is_optional() {
    section "crds.install false renders no definition; crds.install true alone renders the definition only"
    t "the example holds no definition" jqt ex "[${CRD}] | length == 0"
    t "crds.install defaults to false" test "$(yq '.crds.install' "$CHART/values.yaml")" = false
    t "the fixture crds-values renders the definition and no other object" jqt crd_only "length == 1 and .[0].kind == \"CustomResourceDefinition\""
    t "the chart has no crds folder" test ! -d "$CHART/crds"
    t "the chart holds no SandboxPool in any render" jqt crd_on '[.[] | select(.kind == "SandboxPool")] | length == 0'
}

check_the_definition_validates_and_reports() {
    section "the definition has validation rules, a status subresource and printer columns"
    t "the spec has CEL rules" jqt crd_on "[${CRD} | .spec.versions[0].schema.openAPIV3Schema.properties.spec[\"x-kubernetes-validations\"] | length] | .[0] >= 1"
    t "every CEL rule has a message" jqt crd_on "[${CRD} | .spec.versions[0].schema.openAPIV3Schema.properties.spec[\"x-kubernetes-validations\"][] | has(\"message\") and has(\"rule\")] | all"
    t "the image is required" jqt crd_on "[${CRD} | .spec.versions[0].schema.openAPIV3Schema.properties.spec.required] == [[\"image\"]]"
    t "the size holds cpu, memory and disk, each with request and limit" jqt crd_on "[${CRD} | .spec.versions[0].schema.openAPIV3Schema.properties.spec.properties.size.properties | to_entries | map(.key + \":\" + (.value.properties | keys | join(\"+\")))] == [[\"cpu:limit+request\", \"disk:limit+request\", \"memory:limit+request\"]]"
    t "the status is a subresource" jqt crd_on "[${CRD} | .spec.versions[0].subresources | has(\"status\")] == [true]"
    t "the printer columns are Desired, Ready, Accepted and Age" jqt crd_on "[${CRD} | .spec.versions[0].additionalPrinterColumns | map(.name)] == [[\"Desired\", \"Ready\", \"Accepted\", \"Age\"]]"
}

check_the_definition_file_is_the_one_of_keeper_0_2_0() {
    section "the definition file equals the definition file of keeper 0.2.0"
    local keeper_file="$ROOT/crates/br-sandbox-keeper/definition/sandboxpools.yaml"
    t "the definition file of the chart equals the definition file of the keeper byte for byte" cmp "$CHART/files/sandboxpool-crd.yaml" "$keeper_file"
    t "the README says that the file equals the file of keeper 0.2.0 and that two Rust tests hold it" grep -qF "equals, byte for byte, the file \`crates/br-sandbox-keeper/definition/sandboxpools.yaml\` of keeper 0.2.0, and that file equals what \`br_sandbox_keeper::resource::definition_yaml()\` generates; the Rust test \`crates/br-sandbox-keeper/tests/chart_definition.rs\` holds both" "$CHART/README.md"
    t "the README no longer calls the file a copy of a commit of the keeper" test "$(cat "$CHART/README.md" "$CHART/INSTALL.md" | grep -cE "is a copy of the definition file|the keeper release replaces it")" -eq 0
    t "the Rust test of the definition exists" test -f "$ROOT/crates/br-sandbox-keeper/tests/chart_definition.rs"
    t "the file holds a CustomResourceDefinition of the group sandbox.botresources.ai" test "$(yq '.kind + " " + .spec.group' "$CHART/files/sandboxpool-crd.yaml")" = "CustomResourceDefinition sandbox.botresources.ai"
    yq -o=json -I0 'del(.metadata.annotations, .metadata.labels)' "$CHART/files/sandboxpool-crd.yaml" >"$WORK/crd_file.json"
    t "the rendered definition equals the file but for the annotation and the labels" jq -e --slurpfile file "$WORK/crd_file.json" "[${CRD} | del(.metadata.annotations, .metadata.labels)] == \$file" "$WORK/crd_on.json"
}
