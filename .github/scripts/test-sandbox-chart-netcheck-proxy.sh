#!/usr/bin/env bash

run_netcheck_egress() {
    : >"$WORK/netcheckstub/connect.log"
    NETCHECK_SOURCE=egress_yaml run_netcheck "$@"
}

check_network_check_connect_probes_of_the_proxy_path() {
    section "the network check: pods exclude the proxy pods, CONNECT probes through the proxy of each egress namespace"
    mkdir -p "$WORK/netcheckstub"
    stub_tools "$WORK/netcheckbin"
    local out rc
    t "the namespaces file marks the namespaces that have a proxy" test "$(grep -v '^$' "$(netcheck_dir egress_yaml)/namespaces" | tr '\n' ',')" = "sbx-alpha-allow allow proxy,sbx-alpha-full full proxy,sbx-alpha-none none,sbx-beta-none none,"
    t "the namespaces file of the example has no proxy mark" test -z "$(grep proxy "$(netcheck_dir ex_yaml)/namespaces" || true)"
    t "the off-list host is a value and is probed on port 443" test "$(cat "$(netcheck_dir egress_yaml)/offlist-host")" = "offlist.invalid:443"
    t "the probe pod of allow carries the proxy address and the off-list host" test "$(yq -r '.spec.containers[0].env[] | .name + "=" + .value' "$(netcheck_dir egress_yaml)/probe-sbx-alpha-allow.yaml" | grep -E '^NETCHECK_(PROXY|OFFLIST)=' | sort | tr '\n' ',')" = "NETCHECK_OFFLIST=offlist.invalid:443,NETCHECK_PROXY=10.43.0.50:8888,"
    t "the probe pod of full carries the proxy address and no off-list host" test "$(yq -r '.spec.containers[0].env[] | .name + "=" + .value' "$(netcheck_dir egress_yaml)/probe-sbx-alpha-full.yaml" | grep -E '^NETCHECK_(PROXY|OFFLIST)=' | sort | tr '\n' ',')" = "NETCHECK_PROXY=10.43.0.51:8888,"
    t "the probe pod of a mode without a proxy carries neither" test -z "$(yq -r '.spec.containers[0].env[].name' "$(netcheck_dir egress_yaml)/probe-sbx-alpha-none.yaml" | grep -E '^NETCHECK_(PROXY|OFFLIST)$' || true)"
    rc=0
    out="$(run_netcheck_egress env NC_MODE=all PROBE_MODE=none CONNECT_MODE=403 2>&1)" || rc=$?
    if [[ "$rc" -eq 0 ]]; then pass "every CONNECT refused with 403 and a working control exits 0"; else fail "every CONNECT refused with 403 and a working control exits 0" "$out"; fi
    t "a CONNECT probe object exists for each target of a proxied namespace" test "$(grep -c '"kind":"connect"' <<<"$out")" -eq 21
    t "a CONNECT object is expected refused and has no control" grep -q '"kind":"connect","expected":"refused","observed":"refused","control":"not_applicable"' <<<"$out"
    t "the probe CONNECTs to the proxy address and the tunnel port of its own mode" test "$(sort -u "$WORK/netcheckstub/connect.log" | grep -cE '^10\.43\.0\.5[01]:8888 CONNECT ')" -ge 2
    t "no CONNECT goes to a proxy address in a namespace without a proxy: only the two proxy addresses are used" test "$(cut -d' ' -f1 "$WORK/netcheckstub/connect.log" | sort -u | tr '\n' ',')" = "10.43.0.50:8888,10.43.0.51:8888,"
    t "the allow namespace also CONNECTs to the off-list host" grep -q '^10.43.0.50:8888 CONNECT offlist.invalid:443 ' "$WORK/netcheckstub/connect.log"
    t "the full namespace does not CONNECT to the off-list host" test "$(grep -c '^10.43.0.51:8888 CONNECT offlist.invalid' "$WORK/netcheckstub/connect.log")" -eq 0
    t "the off-list host is reported as a connect object in mode allow" grep -q '"mode":"allow","target":"offlist.invalid:443","kind":"connect"' <<<"$out"
    t "a 502 answer counts as refused" test "$(run_netcheck_egress env NC_MODE=all PROBE_MODE=none CONNECT_MODE=502 2>&1 | grep -c '"kind":"connect".*"observed":"refused"')" -eq 21
    rc=0
    out="$(run_netcheck_egress env NC_MODE=all PROBE_MODE=none CONNECT_MODE=200 2>&1)" || rc=$?
    t "a 200 answer fails the job" test "$rc" -ne 0
    t "a 200 answer is reported as reached" grep -q '"kind":"connect","expected":"refused","observed":"reached"' <<<"$out"
    rc=0
    out="$(run_netcheck_egress env NC_MODE=all PROBE_MODE=none CONNECT_MODE=garbled 2>&1)" || rc=$?
    t "an answer that is not an HTTP status fails the job" test "$rc" -ne 0
    t "an answer that is not an HTTP status is reported as no_result" grep -q '"kind":"connect","expected":"refused","observed":"no_result"' <<<"$out"
    rc=0
    out="$(run_netcheck_egress env NC_MODE=all PROBE_MODE=none CONNECT_MODE=silent 2>&1)" || rc=$?
    t "a proxy that does not answer fails the job" test "$rc" -ne 0
    rc=0
    out="$(run_netcheck_egress env NC_MODE=all PROBE_MODE=none CONNECT_MODE=offlist 2>&1)" || rc=$?
    t "positive control: a proxy that refuses the off-list host and passes the targets fails the job" test "$rc" -ne 0
    t "positive control: the off-list probe is reported as refused" grep -q '"target":"offlist.invalid:443","kind":"connect","expected":"refused","observed":"refused"' <<<"$out"
    t "positive control: every target probe is reported as reached, so a proxy that passes a target is seen" test "$(grep -c '"kind":"connect","expected":"refused","observed":"reached"' <<<"$out")" -eq 20
    t "the README says the probe has no positive control and the cluster tests check that a listed host is reached" grep -qF 'The probe has no positive control: a proxy that refuses everything (a broken allow list) also passes the off-list check, so that a listed host is reached is checked by the cluster tests' "$README"
    t "the example render has no CONNECT probe" test "$(run_netcheck env NC_MODE=all PROBE_MODE=none 2>&1 | grep -c '"kind":"connect"')" -eq 0
    t "the pod list excludes the proxy pods by selector" grep -q 'br.sandbox/role!=proxy' "$WORK/netcheckstub/pods.args"
    t "a proxy pod port is not a target when the selector is applied" test "$(run_netcheck_egress env NC_MODE=all PROBE_MODE=none CONNECT_MODE=403 2>&1 | grep -c '"target":"10.42.0.9:8888"')" -eq 0
    sed "s/ -l 'br.sandbox\/role!=proxy'//" "$CHART/files/network-check.sh" >"$WORK/network-check-noselector.sh"
    t "without the selector the same stub lists the proxy pod as a target" test "$(NETCHECK_SCRIPT="$WORK/network-check-noselector.sh" run_netcheck_egress env NC_MODE=all PROBE_MODE=none CONNECT_MODE=403 2>&1 | grep -c '"target":"10.42.0.9:8888"')" -ge 1
}
