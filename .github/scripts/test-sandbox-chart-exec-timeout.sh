#!/usr/bin/env bash

EXEC_TIMEOUT_VALUES="$CHART/ci/exec-timeout-values.yaml"

exec_timeouts() {
    jq -r '[.[] | select(.kind == "ConfigMap" and .metadata.name == "br-sandbox-config") | .data["config.json"] | fromjson | .exec_timeout_seconds | tostring] | unique | join(",")' "$WORK/$1.json"
}

adapter_documents() {
    jq -r '[.[] | select(.kind == "ConfigMap" and .metadata.name == "br-sandbox-config")] | length' "$WORK/$1.json"
}

fails_on_the_schema() {
    local out
    if out="$(helm template t "$CHART" -f "$EXAMPLE" "$@" 2>&1)"; then
        fail "render passed, wanted a schema refusal"
    elif grep -qF 'specifications of the schema' <<<"$out" && grep -qF 'exec_timeout_seconds' <<<"$out" && ! grep -qF 'BR_SANDBOX_EXEC_TIMEOUT_INVALID' <<<"$out"; then
        pass "the schema refuses ${*}"
    else
        fail "render failed without the schema refusal of exec_timeout_seconds" "$out"
    fi
}

check_the_exec_timeout_is_in_every_adapter_document() {
    section "exec_timeout_seconds: the default and the key of every fixture"
    t "the default of the values file is 600" test "$(yq '.exec_timeout_seconds' "$CHART/values.yaml")" = 600
    t "the default renders 600 in the adapter document of every namespace" test "$(exec_timeouts ex)" = 600 -a "$(adapter_documents ex)" -ge 4
    local overlay name
    for overlay in "$CHART"/ci/*-values.yaml; do
        name="$(basename "$overlay" -values.yaml)"
        [[ "$name" == example ]] && continue
        render "exec_fixture_${name}" -f "$EXAMPLE" -f "$overlay"
        t "the fixture ${name} renders the key as an integer in the document of every namespace" test "$(jq -r '[.[] | select(.kind == "ConfigMap" and .metadata.name == "br-sandbox-config") | .data["config.json"] | fromjson | .exec_timeout_seconds | select(type == "number" and . == floor and . >= 1)] | length' "$WORK/exec_fixture_${name}.json")" = "$(adapter_documents "exec_fixture_${name}")" -a "$(adapter_documents "exec_fixture_${name}")" -ge 4
    done
    t "the fixture with no override keeps 600" test "$(exec_timeouts exec_fixture_mirror)" = 600
}

check_the_exec_timeout_follows_the_value() {
    section "exec_timeout_seconds: a set value renders as set"
    render exec_set -f "$EXAMPLE" --set exec_timeout_seconds=45
    t "a value set on the command line renders as set in every namespace" test "$(exec_timeouts exec_set)" = 45 -a "$(adapter_documents exec_set)" -ge 4
    render exec_overlay -f "$EXAMPLE" -f "$EXEC_TIMEOUT_VALUES"
    t "the overlay value 1800 renders as set in every namespace" test "$(exec_timeouts exec_overlay)" = 1800 -a "$(adapter_documents exec_overlay)" -ge 4
    t "the overlay value is not the default" test "$(yq '.exec_timeout_seconds' "$EXEC_TIMEOUT_VALUES")" != "$(yq '.exec_timeout_seconds' "$CHART/values.yaml")"
    t "the value sits at the top level of the values, beside restore_timeout_seconds" test "$(yq '[has("exec_timeout_seconds"), has("restore_timeout_seconds")] | all' "$CHART/values.yaml")" = true
    t "the value reaches the key and no other key of the document changes" test "$(jq -S '[.[] | select(.kind == "ConfigMap" and .metadata.name == "br-sandbox-config") | .data["config.json"] | fromjson | del(.exec_timeout_seconds)]' "$WORK/exec_set.json")" = "$(jq -S '[.[] | select(.kind == "ConfigMap" and .metadata.name == "br-sandbox-config") | .data["config.json"] | fromjson | del(.exec_timeout_seconds)]' "$WORK/ex.json")"
}

check_the_exec_timeout_refuses_what_is_not_an_integer_above_zero() {
    section "exec_timeout_seconds: a value that is not an integer above zero is refused"
    fails_on_the_schema --set exec_timeout_seconds=0
    fails_on_the_schema --set exec_timeout_seconds=-1
    fails_on_the_schema --set exec_timeout_seconds=1.5
    fails_on_the_schema --set-string exec_timeout_seconds=600
    fails_on_the_schema --set exec_timeout_seconds=true
    fails_on_the_schema -f "$(vals exec_zero_file '.exec_timeout_seconds = 0')"
    fails_with "BR_SANDBOX_EXEC_TIMEOUT_INVALID{key=exec_timeout_seconds}" -f "$EXAMPLE" --set exec_timeout_seconds=null
    fails_with "BR_SANDBOX_EXEC_TIMEOUT_INVALID{key=exec_timeout_seconds}" -f "$(vals exec_null_file '.exec_timeout_seconds = null')"
    t "the value 1 is accepted" render exec_one -f "$EXAMPLE" --set exec_timeout_seconds=1
    t "the value 1 renders 1" test "$(exec_timeouts exec_one)" = 1
}
