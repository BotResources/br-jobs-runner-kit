#!/usr/bin/env bash

stub_tools() {
    local bin="$1"
    mkdir -p "$bin"
    cat >"$bin/nc" <<'STUB'
#!/bin/sh
target="$3:$4"
if [ "${CONNECT_MODE:-}" ] && [ "$2" = 3 ]; then
    req="$(cat | tr -d '\r')"
    printf '%s %s\n' "$target" "$(printf '%s\n' "$req" | sed -n 1p)" >>"$STUB_DIR/connect.log"
    inner="$(printf '%s\n' "$req" | grep '^GET ' | sed -n 1p)"
    ct="$(printf '%s\n' "$req" | sed -n 1p | cut -d' ' -f2)"
    [ -n "$inner" ] && printf '%s %s\n' "$target" "$inner" >>"$STUB_DIR/connect.log"
    case "$3" in
        10.43.0.51) mode="${CONNECT_MODE_FULL:-$CONNECT_MODE}" ;;
        10.43.0.50) mode="${CONNECT_MODE_ALLOW:-$CONNECT_MODE}" ;;
        *) mode="$CONNECT_MODE" ;;
    esac
    open='HTTP/1.1 200 Connection established\r\n\r\n'
    case "$mode" in
        200) printf "$open" ;;
        403) printf 'HTTP/1.1 403 Forbidden\r\n\r\n' ;;
        502) printf 'HTTP/1.1 502 Bad Gateway\r\n\r\n' ;;
        garbled) printf 'garbage\r\n' ;;
        tunnel502) printf "$open"; [ -n "$inner" ] && printf 'HTTP/1.1 502 Bad Gateway\r\nContent-Length: 11\r\n\r\nbad gateway' ;;
        tunnel502other) printf "$open"; [ -n "$inner" ] && printf 'HTTP/1.1 502 Bad Gateway\r\nContent-Length: 13\r\n\r\nupstream down' ;;
        tunnel404) printf "$open"; [ -n "$inner" ] && printf 'HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n' ;;
        tunnel200) printf "$open"; [ -n "$inner" ] && printf 'HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok' ;;
        tunnelgarbled) printf "$open"; [ -n "$inner" ] && printf 'garbage\r\n' ;;
        bytarget) printf "$open"; if [ -n "$inner" ]; then case "$ct" in *:8080) printf 'HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok' ;; *) printf 'HTTP/1.1 502 Bad Gateway\r\nContent-Length: 11\r\n\r\nbad gateway' ;; esac; fi ;;
        offlist) case "$req" in *offlist.invalid*) printf 'HTTP/1.1 403 Forbidden\r\n\r\n' ;; *) printf "$open"; [ -n "$inner" ] && printf 'HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok' ;; esac ;;
    esac
    exit 0
fi
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
                [ "${NO_NODES:-}" = 1 ] && exit 0
                printf 'InternalIP 10.0.0.11\nExternalIP 203.0.113.11\nHostname node-a\n'
                [ -n "${STALE_NODE:-}" ] && printf 'InternalIP %s\n' "$STALE_NODE"
                exit 0
                ;;
            svc)
                [ "${NO_SVC:-}" = 1 ] && exit 0
                case "$3" in
                    kubernetes) printf '10.43.0.1:443' ;;
                    kube-dns) printf '10.43.0.10:53' ;;
                esac
                exit 0
                ;;
            pods)
                printf '%s\n' "$*" >"$STUB_DIR/pods.args"
                case "$*" in *"br.sandbox/role!=proxy"*) ;; *) printf '10.42.0.9:8888\n' ;; esac
                [ "${NO_POD_PORT:-}" = 1 ] && { printf '10.42.0.5:\n:\n'; exit 0; }
                printf '10.42.0.5:8080\n:\n'
                exit 0
                ;;
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
        proxy="$(awk '/name: NETCHECK_PROXY/ {getline; print $2}' "$STUB_DIR/pod.yaml" | tr -d '"')"
        offlist="$(awk '/name: NETCHECK_OFFLIST/ {getline; print $2}' "$STUB_DIR/pod.yaml" | tr -d '"')"
        NETCHECK_PROXY="$proxy" NETCHECK_OFFLIST="$offlist" NETCHECK_TARGETS="$targets" NC_MODE="$PROBE_MODE" NETCHECK_DIR="$NETCHECK_DIR" sh "$NETCHECK_DIR/network-check.sh" probe
        exit 0
        ;;
esac
exit 1
STUB
    chmod +x "$bin/nc" "$bin/kubectl"
}

netcheck_dir() {
    local source="${1:-ex_yaml}" dir="$WORK/netcheckdir-${1:-ex_yaml}"
    mkdir -p "$dir"
    yq -r 'select(.kind == "ConfigMap" and .metadata.name == "br-sandbox-netcheck" and .metadata.namespace == "sbx-system") | .data | to_entries | .[] | .key' "$WORK/${source}.yaml" | while read -r key; do
        yq -r "select(.kind == \"ConfigMap\" and .metadata.name == \"br-sandbox-netcheck\" and .metadata.namespace == \"sbx-system\") | .data[\"${key}\"]" "$WORK/${source}.yaml" >"$dir/$key"
    done
    printf '%s' "$dir"
}

run_netcheck() {
    NETCHECK_DIR="$(netcheck_dir "${NETCHECK_SOURCE:-ex_yaml}")" STUB_DIR="$WORK/netcheckstub" PATH="$WORK/netcheckbin:$PATH" NETCHECK_WAIT_SECONDS=1 NETCHECK_TUNNEL_WAIT=0 "$@" sh "${NETCHECK_SCRIPT:-$CHART/files/network-check.sh}" discover
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
    local class
    for class in node:NO_NODES=1:node_kubelet pod:NO_POD_PORT=1:pod api:NO_SVC=1:api_service; do
        rc=0
        out="$(run_netcheck env NC_MODE=all PROBE_MODE=none "$(cut -d: -f2 <<<"$class")" 2>&1)" || rc=$?
        t "no ${class%%:*} target fails the job" test "$rc" -ne 0
        t "the missing class $(cut -d: -f3 <<<"$class") is reported" grep -qF "\"target\":\"$(cut -d: -f3 <<<"$class")\",\"kind\":\"target_class\",\"expected\":\"present\",\"observed\":\"missing\"" <<<"$out"
    done
    rc=0
    out="$(run_netcheck env NC_MODE=all PROBE_MODE=none NO_POD_PORT=1 2>&1)" || rc=$?
    t "with no pod port every other class still reports and no pod target is probed" test "$(grep -c '"kind":"tcp"' <<<"$out")" -ge 30 -a "$(grep -c '"target":"10.42.0.5' <<<"$out")" -eq 0
    mkdir -p "$WORK/netchecknotargets"
    cp "$(netcheck_dir)"/* "$WORK/netchecknotargets/"
    : >"$WORK/netchecknotargets/extra-targets"
    rc=0
    out="$(run_netcheck env NC_MODE=all PROBE_MODE=none NETCHECK_DIR="$WORK/netchecknotargets" 2>&1)" || rc=$?
    t "no listed bus or database target fails the job" test "$rc" -ne 0
    t "the missing targets class is reported" grep -qF '"target":"targets","kind":"target_class"' <<<"$out"
    t "a complete target set reports no missing class" test "$(run_netcheck env NC_MODE=all PROBE_MODE=none 2>&1 | grep -c '"observed":"missing"')" -eq 0
    t "the probe substitutes the target list into the pod manifest" grep -q '10.0.0.11:10250' "$WORK/netcheckstub/pod.yaml"
}
