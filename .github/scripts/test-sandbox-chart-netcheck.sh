#!/usr/bin/env bash

stub_tools() {
    local bin="$1"
    mkdir -p "$bin"
    cat >"$bin/nc" <<'STUB'
#!/bin/sh
target="$3:$4"
case "$NC_MODE" in
    all) exit 0 ;;
    none) exit 1 ;;
    first) [ "$target" = "$NC_FIRST" ] && exit 0 || exit 1 ;;
esac
STUB
    cat >"$bin/kubectl" <<'STUB'
#!/bin/sh
case "$1" in
    get)
        case "$2" in
            nodes)
                printf 'InternalIP 10.0.0.11\nExternalIP 203.0.113.11\nHostname node-a\n'
                [ -n "${STALE_NODE:-}" ] && printf 'InternalIP %s\n' "$STALE_NODE"
                exit 0
                ;;
            svc)
                case "$3" in
                    kubernetes) printf '10.43.0.1:443' ;;
                    kube-dns) printf '10.43.0.10:53' ;;
                esac
                exit 0
                ;;
            pods) printf '10.42.0.5:8080\n:\n'; exit 0 ;;
        esac
        ;;
    create)
        cat >"$STUB_DIR/pod.yaml"
        exit 0
        ;;
    wait | delete) exit 0 ;;
    logs)
        [ "${NO_LOG:-}" = 1 ] && exit 1
        targets="$(sed -n 's/^ *value: *//p' "$STUB_DIR/pod.yaml" | head -1 | tr -d '"')"
        NETCHECK_TARGETS="$targets" NC_MODE="$PROBE_MODE" NETCHECK_DIR="$NETCHECK_DIR" sh "$NETCHECK_DIR/network-check.sh" probe
        exit 0
        ;;
esac
exit 1
STUB
    chmod +x "$bin/nc" "$bin/kubectl"
}

netcheck_dir() {
    local dir="$WORK/netcheckdir"
    mkdir -p "$dir"
    yq -r 'select(.kind == "ConfigMap" and .metadata.name == "br-sandbox-netcheck" and .metadata.namespace == "sbx-system") | .data | to_entries | .[] | .key' "$WORK/ex_yaml.yaml" | while read -r key; do
        yq -r "select(.kind == \"ConfigMap\" and .metadata.name == \"br-sandbox-netcheck\" and .metadata.namespace == \"sbx-system\") | .data[\"${key}\"]" "$WORK/ex_yaml.yaml" >"$dir/$key"
    done
    printf '%s' "$dir"
}

run_netcheck() {
    NETCHECK_DIR="$(netcheck_dir)" STUB_DIR="$WORK/netcheckstub" PATH="$WORK/netcheckbin:$PATH" NETCHECK_WAIT_SECONDS=1 "$@" sh "$CHART/files/network-check.sh" discover
}

check_network_check_script_against_stubs() {
    section "the network check script: verdicts against stub kubectl and nc"
    mkdir -p "$WORK/netcheckstub"
    stub_tools "$WORK/netcheckbin"
    local out rc
    rc=0
    out="$(run_netcheck env NC_MODE=all PROBE_MODE=none 2>&1)" || rc=$?
    if [[ "$rc" -eq 0 ]]; then pass "all refused with a working control exits 0"; else fail "all refused with a working control exits 0" "$out"; fi
    t "the report has one JSON object per probe and namespace" test "$(grep -c '"kind":"tcp"' <<<"$out")" -ge 40
    t "every object parses and carries the fields of the report" test "$(grep '^{' <<<"$out" | jq -s 'map(select(.net and .mode and .target and .kind and .expected and .observed and .control)) | length')" = "$(grep -c '^{' <<<"$out")"
    rc=0
    out="$(run_netcheck env NC_MODE=all PROBE_MODE=all 2>&1)" || rc=$?
    t "a reached target fails the job" test "$rc" -ne 0
    t "a reached target is reported as reached" grep -q '"observed":"reached"' <<<"$out"
    rc=0
    out="$(run_netcheck env NC_MODE=none PROBE_MODE=none 2>&1)" || rc=$?
    t "no control means control_unavailable and a failed job" test "$rc" -ne 0
    t "the report says control_unavailable" grep -q '"control":"control_unavailable"' <<<"$out"
    rc=0
    out="$(run_netcheck env NC_MODE=all PROBE_MODE=none STALE_NODE=10.0.0.99 2>&1)" || rc=$?
    t "a node address missing from the lists fails the job" test "$rc" -ne 0
    t "the stale node is reported" grep -q '"target":"10.0.0.99".*"observed":"stale"' <<<"$out"
    rc=0
    out="$(run_netcheck env NC_MODE=all PROBE_MODE=none NO_LOG=1 2>&1)" || rc=$?
    t "a probe pod with no log fails the job" test "$rc" -ne 0
    t "the missing result is reported as no_result" grep -q '"observed":"no_result"' <<<"$out"
    t "the probe substitutes the target list into the pod manifest" grep -q '10.0.0.11:10250' "$WORK/netcheckstub/pod.yaml"
}
