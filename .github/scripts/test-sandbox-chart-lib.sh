#!/usr/bin/env bash

for TOOL in helm yq jq; do
    command -v "$TOOL" >/dev/null || { echo "${TOOL} is required" >&2; exit 2; }
done

SCRIPTS="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPTS/../.." && pwd)"
export CHART="$ROOT/charts/br-sandbox-chart"
export EXAMPLE="$CHART/ci/example-values.yaml"
export MIRROR_VALUES="$CHART/ci/mirror-values.yaml"
export OPERATOR_VALUES="$CHART/ci/operator-values.yaml"
export EGRESS_VALUES="$CHART/ci/egress-values.yaml"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/test-sandbox-chart.XXXXXX")"
FAILURES=0
CASES=0
CURRENT=""
LAST_RENDER=""
SUMMARY_DONE=0

finish() {
    local code=$?
    if [[ "$code" -ne 0 && "$SUMMARY_DONE" -eq 0 ]]; then
        printf 'FAIL  the suite stopped with exit %s before its summary, in the section "%s", after the render "%s"\n' "$code" "$CURRENT" "$LAST_RENDER" >&2
        if [[ -n "$LAST_RENDER" && -s "$WORK/$LAST_RENDER.err" ]]; then
            sed 's/^/        | /' "$WORK/$LAST_RENDER.err" | head -12 >&2
        fi
    fi
    rm -rf "$WORK"
}
trap finish EXIT

section() {
    CURRENT="$1"
    printf '%s\n' "$1"
}

pass() {
    CASES=$((CASES + 1))
    printf '  ok  %s\n' "$1"
}

fail() {
    CASES=$((CASES + 1))
    FAILURES=$((FAILURES + 1))
    printf 'FAIL  %s: %s\n' "$CURRENT" "$1"
    if [[ -n "${2:-}" ]]; then
        printf '%s\n' "$2" | sed 's/^/        | /' | head -12
    fi
}

t() {
    local what="$1"
    shift
    if "$@" >/dev/null 2>&1; then pass "$what"; else fail "$what"; fi
}

render() {
    local name="$1"
    shift
    LAST_RENDER="$name"
    helm template t "$CHART" "$@" 2>"$WORK/$name.err" | yq ea -o=json '[select(. != null)]' - >"$WORK/$name.json"
    [[ "${PIPESTATUS[0]}" -eq 0 ]]
}

render_yaml() {
    local name="$1"
    shift
    LAST_RENDER="$name"
    helm template t "$CHART" "$@" >"$WORK/$name.yaml" 2>"$WORK/$name.err"
}

jqt() {
    local name="$1"
    shift
    jq -e "$@" "$WORK/$name.json" >/dev/null
}

jqv() {
    local name="$1"
    shift
    jq -r "$@" "$WORK/$name.json"
}

vals() {
    local name="$1" expr="$2"
    yq "$expr" "$EXAMPLE" >"$WORK/$name.yaml"
    printf '%s' "$WORK/$name.yaml"
}

export CREDENTIAL_SET='{"secret_ref": "set-a-credentials", "secrets": [{"placeholder": "placeholder-a", "env_name": "SET_A_ACCESS", "key": "a", "hosts": ["git.example.test"]}]}'

valsx() {
    local name="$1" expr="$2"
    yq ea 'select(fileIndex == 0) * select(fileIndex == 1) * select(fileIndex == 2)' "$EXAMPLE" "$EGRESS_VALUES" "$CHART/ci/fence-values.yaml" | yq "$expr" - >"$WORK/$name.yaml"
    printf '%s' "$WORK/$name.yaml"
}

fails_with() {
    local want="$1" out
    shift
    if out="$(helm template t "$CHART" "$@" 2>&1)"; then
        fail "render passed, wanted ${want}"
    elif grep -qF -- "$want" <<<"$out"; then
        pass "render fails with ${want}"
    else
        fail "render failed without ${want}" "$out"
    fi
}

hosts_of() {
    grep -ohE '[A-Za-z0-9-]+(\.[A-Za-z0-9-]+)*\.(com|io|ai|net|org|dev|fr|cloud|app|test|local)([^A-Za-z0-9-]|$)' "$@" |
        sed -E 's/[^A-Za-z0-9.-]$//' | sort -u
}

export QTY='def qty: tostring | capture("^(?<n>[0-9.]+)(?<u>m|Ki|Mi|Gi|Ti|K|M|G|T)?$") | (.n | tonumber) * ({"":1,"m":0.001,"Ki":1024,"Mi":1048576,"Gi":1073741824,"Ti":1099511627776,"K":1000,"M":1000000,"G":1000000000,"T":1000000000000}[.u // ""]);'
export SANDBOX_PODS='[.[] | select(.kind == "ConfigMap" and .metadata.name == "br-sandbox-pod-template") | .data["pod-template.json"] | fromjson]'

summary() {
    SUMMARY_DONE=1
    echo
    if [[ "$FAILURES" -gt 0 ]]; then
        echo "${FAILURES}/${CASES} case(s) failed" >&2
        exit 1
    fi
    echo "${CASES} cases passed"
}
