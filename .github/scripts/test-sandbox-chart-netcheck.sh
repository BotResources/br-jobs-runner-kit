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
    case "$3" in
        10.43.0.50) mode="${CONNECT_MODE_ALLOW:-$CONNECT_MODE}" ;;
        *) mode="$CONNECT_MODE" ;;
    esac
    case "$mode" in
        200) printf 'HTTP/1.1 200 Connection established\r\n\r\n' ;;
        403then200) if [ "$(grep -c "CONNECT $(printf '%s\n' "$req" | sed -n '1s/^CONNECT \([^ ]*\) .*/\1/p') " "$STUB_DIR/connect.log")" -le 1 ]; then printf 'HTTP/1.1 403 Forbidden\r\n\r\n'; else printf 'HTTP/1.1 200 Connection established\r\n\r\n'; fi ;;
        403) printf 'HTTP/1.1 403 Forbidden\r\n\r\n' ;;
        502) printf 'HTTP/1.1 502 Bad Gateway\r\n\r\n' ;;
        404) printf 'HTTP/1.1 404 Not Found\r\n\r\n' ;;
        garbled) printf 'garbage\r\n' ;;
        connect200x403) printf 'HTTP/1.1 200 x 403 y\r\n\r\n' ;;
        connect403x200) printf 'HTTP/1.1 403 x 200 y\r\n\r\n' ;;
        nocode) printf 'HTTP/1.1 OK\r\n\r\n' ;;
        offlist) case "$req" in *offlist.invalid*) printf 'HTTP/1.1 403 Forbidden\r\n\r\n' ;; *) printf 'HTTP/1.1 200 Connection established\r\n\r\n' ;; esac ;;
    esac
    exit 0
fi
printf '%s\n' "$target" >>"$STUB_DIR/nc.calls"
if [ "$(($(wc -l <"$STUB_DIR/nc.calls")))" -gt 2000 ]; then
    printf 'nc stub: more than 2000 calls, the script does not wait\n' >"$STUB_DIR/runaway"
    kill "$PPID"
    exit 1
fi
refuse() {
    [ -z "${NC_DROP_SECONDS:-}" ] || printf '%s\n' "$(($(cat "$STUB_DIR/clock") + NC_DROP_SECONDS))" >"$STUB_DIR/clock"
    exit "${NC_REFUSE_STATUS:-1}"
}
if [ -n "${NC_LATE_K:-}" ]; then
    printf x >>"$STUB_DIR/nc.late"
    [ "$(($(wc -c <"$STUB_DIR/nc.late")))" -le "$NC_LATE_K" ] && exit 0 || refuse
fi
case "$NC_MODE" in
    all) exit 0 ;;
    none) refuse ;;
    first) [ "$target" = "$NC_FIRST" ] && exit 0 || refuse ;;
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
        printf '1000\n' >"$STUB_DIR/clock"
        : >"$STUB_DIR/nc.late"
        exit 0
        ;;
    wait)
        printf '%s\n' "$*" >>"$STUB_DIR/wait.args"
        exit 0
        ;;
    delete) exit 0 ;;
    logs)
        [ "${NO_LOG:-}" = 1 ] && exit 1
        targets="$(sed -n 's/^ *value: *//p' "$STUB_DIR/pod.yaml" | head -1 | tr -d '"')"
        proxy="$(awk '/name: NETCHECK_PROXY/ {getline; print $2}' "$STUB_DIR/pod.yaml" | tr -d '"')"
        offlist="$(awk '/name: NETCHECK_OFFLIST/ {getline; print $2}' "$STUB_DIR/pod.yaml" | tr -d '"')"
        wait="$(awk '/name: NETCHECK_ENFORCEMENT_WAIT/ {getline; print $2}' "$STUB_DIR/pod.yaml" | tr -d '"')"
        NETCHECK_ENFORCEMENT_WAIT="$wait" NC_LATE_K="$PROBE_LATE_K" NETCHECK_PROXY="$proxy" NETCHECK_OFFLIST="$offlist" NETCHECK_TARGETS="$targets" NC_MODE="$PROBE_MODE" NETCHECK_DIR="$NETCHECK_DIR" sh "${NETCHECK_PROBE_SCRIPT:-$NETCHECK_DIR/network-check.sh}" probe
        exit 0
        ;;
esac
exit 1
STUB
    cat >"$bin/sleep" <<'STUB'
#!/bin/sh
printf '%s\n' "$1" >>"$STUB_DIR/sleep.log"
printf '%s\n' "$(($(cat "$STUB_DIR/clock") + $1))" >"$STUB_DIR/clock"
STUB
    cat >"$bin/date" <<'STUB'
#!/bin/sh
cat "$STUB_DIR/clock"
STUB
    chmod +x "$bin/nc" "$bin/kubectl" "$bin/sleep" "$bin/date"
}

netcheck_dir() {
    local source="${1:-ex_yaml}" dir="$WORK/netcheckdir-${1:-ex_yaml}"
    mkdir -p "$dir"
    yq -r 'select(.kind == "ConfigMap" and .metadata.name == "br-sandbox-netcheck" and .metadata.namespace == "sbx-system") | .data | to_entries | .[] | .key' "$WORK/${source}.yaml" | while read -r key; do
        yq -r "select(.kind == \"ConfigMap\" and .metadata.name == \"br-sandbox-netcheck\" and .metadata.namespace == \"sbx-system\") | .data[\"${key}\"]" "$WORK/${source}.yaml" >"$dir/$key"
    done
    printf '%s' "$dir"
}

reset_netcheck_stub() {
    mkdir -p "$WORK/netcheckstub"
    : >"$WORK/netcheckstub/sleep.log"
    : >"$WORK/netcheckstub/nc.calls"
    : >"$WORK/netcheckstub/wait.args"
    : >"$WORK/netcheckstub/runaway"
    printf '1000\n' >"$WORK/netcheckstub/clock"
}

run_netcheck() {
    reset_netcheck_stub
    NETCHECK_ENFORCEMENT_WAIT="$(yq -r 'select(.kind == "Job") | .spec.template.spec.containers[0].env[] | select(.name == "NETCHECK_ENFORCEMENT_WAIT") | .value' "$WORK/${NETCHECK_SOURCE:-ex_yaml}.yaml")" NETCHECK_DIR="$(netcheck_dir "${NETCHECK_SOURCE:-ex_yaml}")" STUB_DIR="$WORK/netcheckstub" PATH="$WORK/netcheckbin:$PATH" NETCHECK_WAIT_SECONDS=1 "$@" sh "${NETCHECK_SCRIPT:-$CHART/files/network-check.sh}" discover
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
