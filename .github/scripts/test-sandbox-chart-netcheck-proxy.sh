#!/usr/bin/env bash

run_netcheck_egress() {
    : >"$WORK/netcheckstub/connect.log"
    NETCHECK_SOURCE=egress_yaml run_netcheck "$@"
}

check_network_check_connect_probes_of_the_proxy_path() {
    section "the network check: pods exclude the proxy pods, CONNECT probes through the proxy of each egress namespace in mode allow"
    mkdir -p "$WORK/netcheckstub"
    stub_tools "$WORK/netcheckbin"
    local out rc
    t "the namespaces file marks the namespace of mode allow that has a proxy and no other" test "$(grep -v '^$' "$(netcheck_dir egress_yaml)/namespaces" | tr '\n' ',')" = "sbx-alpha-allow allow proxy,sbx-alpha-full full,sbx-alpha-none none,sbx-beta-none none,"
    t "the namespaces file of the example has no proxy mark" test -z "$(grep proxy "$(netcheck_dir ex_yaml)/namespaces" || true)"
    t "the off-list host is a value and is probed on port 443" test "$(cat "$(netcheck_dir egress_yaml)/offlist-host")" = "offlist.invalid:443"
    t "the probe pod of allow carries the proxy address and the off-list host" test "$(yq -r '.spec.containers[0].env[] | .name + "=" + .value' "$(netcheck_dir egress_yaml)/probe-sbx-alpha-allow.yaml" | grep -E '^NETCHECK_(PROXY|OFFLIST)=' | sort | tr '\n' ',')" = "NETCHECK_OFFLIST=offlist.invalid:443,NETCHECK_PROXY=10.43.0.50:8888,"
    t "the probe pod of full carries neither the proxy address nor the off-list host" test -z "$(yq -r '.spec.containers[0].env[].name' "$(netcheck_dir egress_yaml)/probe-sbx-alpha-full.yaml" | grep -E '^NETCHECK_(PROXY|OFFLIST)$' || true)"
    t "the probe pod of a mode without a proxy carries neither" test -z "$(yq -r '.spec.containers[0].env[].name' "$(netcheck_dir egress_yaml)/probe-sbx-alpha-none.yaml" | grep -E '^NETCHECK_(PROXY|OFFLIST)$' || true)"
    rc=0
    out="$(run_netcheck_egress env NC_MODE=all PROBE_MODE=none CONNECT_MODE=403 2>&1)" || rc=$?
    if [[ "$rc" -eq 0 ]]; then pass "every CONNECT refused with 403 and a working control exits 0"; else fail "every CONNECT refused with 403 and a working control exits 0" "$out"; fi
    t "a CONNECT probe object exists for each target and the off-list host of the allow namespace" test "$(grep -c '"kind":"connect"' <<<"$out")" -eq 11
    t "a CONNECT object is expected refused and has no control" grep -q '"kind":"connect","expected":"refused","observed":"refused","control":"not_applicable"' <<<"$out"
    t "no CONNECT object is reported in mode full" test "$(grep -c '"mode":"full".*"kind":"connect"' <<<"$out")" -eq 0
    t "the probe CONNECTs to the proxy address and the tunnel port of mode allow" test "$(grep -c '^10\.43\.0\.50:8888 CONNECT ' "$WORK/netcheckstub/connect.log")" -ge 11
    t "no CONNECT goes to another address: only the proxy address of mode allow is used" test "$(cut -d' ' -f1 "$WORK/netcheckstub/connect.log" | sort -u | tr '\n' ',')" = "10.43.0.50:8888,"
    t "no request is sent inside the tunnel" test "$(grep -vc ' CONNECT ' "$WORK/netcheckstub/connect.log")" -eq 0
    t "the allow namespace also CONNECTs to the off-list host" grep -q '^10.43.0.50:8888 CONNECT offlist.invalid:443 ' "$WORK/netcheckstub/connect.log"
    t "the off-list host is reported as a connect object in mode allow" grep -q '"mode":"allow","target":"offlist.invalid:443","kind":"connect"' <<<"$out"
    t "the README says the probe has no positive control and the cluster tests check that a listed host is reached" grep -qF 'The probe has no positive control: a proxy that refuses everything (a broken allow list) also passes the off-list check, so that a listed host is reached is checked by the cluster tests' "$README"
    t "the example render has no CONNECT probe" test "$(run_netcheck env NC_MODE=all PROBE_MODE=none 2>&1 | grep -c '"kind":"connect"')" -eq 0
    t "the pod list excludes the proxy pods by selector" grep -q 'br.sandbox/role!=proxy' "$WORK/netcheckstub/pods.args"
    t "a proxy pod port is not a target when the selector is applied" test "$(run_netcheck_egress env NC_MODE=all PROBE_MODE=none CONNECT_MODE=403 2>&1 | grep -c '"target":"10.42.0.9:8888"')" -eq 0
    sed "s/ -l 'br.sandbox\/role!=proxy'//" "$CHART/files/network-check.sh" >"$WORK/network-check-noselector.sh"
    t "without the selector the same stub lists the proxy pod as a target" test "$(NETCHECK_SCRIPT="$WORK/network-check-noselector.sh" run_netcheck_egress env NC_MODE=all PROBE_MODE=none CONNECT_MODE=403 2>&1 | grep -c '"target":"10.42.0.9:8888"')" -ge 1
}

assert_connect_verdict() {
    local stub="$1" verdict="$2" out rc=0
    out="$(run_netcheck_egress env NC_MODE=all PROBE_MODE=none CONNECT_MODE=403 CONNECT_MODE_ALLOW="$stub" 2>&1)" || rc=$?
    t "stub ${stub}: every probe of mode allow is ${verdict}" test "$(grep -c "\"mode\":\"allow\".*\"kind\":\"connect\".*\"observed\":\"${verdict}\"" <<<"$out")" -eq 11
    t "stub ${stub}: the JSON line carries the word ${verdict}" grep -q "\"mode\":\"allow\",\"target\":\"[^\"]*\",\"kind\":\"connect\",\"expected\":\"refused\",\"observed\":\"${verdict}\",\"control\":\"not_applicable\"" <<<"$out"
    if [[ "$verdict" == refused ]]; then
        t "stub ${stub}: the job passes" test "$rc" -eq 0
    else
        t "stub ${stub}: the job fails" test "$rc" -ne 0
    fi
}

check_network_check_connect_verdicts_in_mode_allow() {
    section "the network check: the verdict of a CONNECT probe in mode allow, read at the CONNECT step only"
    mkdir -p "$WORK/netcheckstub"
    stub_tools "$WORK/netcheckbin"
    local stub
    for stub in 403 502 connect403x200; do assert_connect_verdict "$stub" refused; done
    for stub in 200 connect200x403; do assert_connect_verdict "$stub" reached; done
    for stub in 404 garbled nocode silent; do assert_connect_verdict "$stub" no_result; done
    local out rc=0
    out="$(run_netcheck_egress env NC_MODE=all PROBE_MODE=none CONNECT_MODE=offlist 2>&1)" || rc=$?
    t "positive control: a proxy that refuses the off-list host and passes the targets fails the job" test "$rc" -ne 0
    t "positive control: the off-list probe is reported as refused" grep -q '"target":"offlist.invalid:443","kind":"connect","expected":"refused","observed":"refused"' <<<"$out"
    t "positive control: every target probe is reported as reached, so a proxy that passes a target is seen" test "$(grep -c '"kind":"connect","expected":"refused","observed":"reached"' <<<"$out")" -eq 10
    t "the script and the docs name no verdict unanswered and no word refused_or_unanswered" test "$(grep -rcE 'unanswered|tunnel_state|TUNNEL_WAIT' "$CHART/files/network-check.sh" "$README" "$CHART/CHANGELOG.md" | grep -vc ':0$')" -eq 0
    t "the README says the verdict is refused for 403 or 502, reached for 200 and no_result otherwise" grep -qF 'The verdict is `refused` when the answer is 403 or 502, `reached` when it is 200, and `no_result` for any other answer or no answer. The Job fails on any verdict other than `refused`.' "$README"
    t "the CHANGELOG says the same" grep -qF '`refused` for 403 or 502, `reached` for 200, `no_result` for any other answer or none (the Job fails on any verdict other than `refused`)' "$CHART/CHANGELOG.md"
    t "the README says the code is the second field of the status line and a line with no three-digit code is no_result" grep -qF 'The status code is the second field of the status line, compared as a whole field (`HTTP/1.1 200 x 403 y` is read as 200); a status line with no three-digit code is `no_result`.' "$README"
    t "the CHANGELOG says the same about the status code" grep -qF 'The status code is the second field of the status line, compared as a whole field (`HTTP/1.1 200 x 403 y` is read as 200); a status line with no three-digit code is `no_result`.' "$CHART/CHANGELOG.md"
    t "the README says what was captured at the CONNECT in mode allow" grep -qF 'an IP address that is not on the list and an internal IP address both got 403 at the `CONNECT`; a name that is not on the list was not captured at the `CONNECT`' "$README"
    t "the CHANGELOG says what was captured at the CONNECT in mode allow" grep -qF 'an IP address that is not on the list and an internal IP address both got 403 at the `CONNECT`; a name that is not on the list was not captured at the `CONNECT`' "$CHART/CHANGELOG.md"
    t "no text says a 403 at the CONNECT was measured earlier" test "$(grep -c 'measured earlier' "$README" "$CHART/CHANGELOG.md" | grep -vc ':0$')" -eq 0
    t "the README says the Job does not probe the path through the proxy in mode full and why" grep -qF 'The Job does not probe the path through the proxy in mode `full`. In mode `full` with a credential set the proxy answers 200 to every `CONNECT` and refuses only at the first request inside the tunnel, with a 502 that a reached target which does not speak HTTP also gives, so a probe from the sandbox side cannot give an exact verdict.' "$README"
    t "the CHANGELOG says the same" grep -qF 'The Job does not probe the path through the proxy in mode `full`: with a credential set the proxy answers 200 to every `CONNECT` and refuses only at the first request inside the tunnel, with a 502 that a reached target which does not speak HTTP also gives, so a probe from the sandbox side cannot give an exact verdict.' "$CHART/CHANGELOG.md"
    t "the README says the pivot barriers of mode full are proved on a test cluster by a test server that records its connections" grep -qF 'the Job therefore does not prove the pivot barriers of mode `full`: they are proved on a test cluster by a test server at an internal address that records its connections and sees none after a request through the proxy.' "$README"
    t "the CHANGELOG says the same" grep -qF 'the Job therefore does not prove the pivot barriers of mode `full`; they are proved on a test cluster by a test server at an internal address that records its connections and sees none after a request through the proxy.' "$CHART/CHANGELOG.md"
    t "the README says what the Job proves" grep -qF 'What this Job proves is the direct TCP probes of the sandbox pod, in the three modes, and the refusal at the `CONNECT` in mode `allow`.' "$README"
    t "the README gives no refusal-as-control claim and no deploy test" test "$(grep -cE 'its own control|deploy test' "$README")" -eq 0
    t "the README lists the values of kind, control and observed for each kind of line" grep -qF '`kind` is `tcp`, `connect`, `target_class` or `node_list`. By kind: `tcp` carries `observed` `refused`, `reached` or `no_result` and `control` `ok` or `control_unavailable`; `connect` carries `observed` `refused`, `reached` or `no_result` and `control` `not_applicable`; `target_class` carries `observed` `missing` and `control` `ok`; `node_list` carries `observed` `stale` and `control` `ok`.' "$README"
    t "the CHANGELOG lists the same values" grep -qF 'The JSON line carries `kind` `tcp`, `connect`, `target_class` or `node_list`: `tcp` carries `observed` `refused`, `reached` or `no_result` and `control` `ok` or `control_unavailable`; `connect` carries `observed` `refused`, `reached` or `no_result` and `control` `not_applicable`; `target_class` carries `observed` `missing` and `control` `ok`; `node_list` carries `observed` `stale` and `control` `ok`.' "$CHART/CHANGELOG.md"
    t "the README names the expected word of a connect line" grep -qF '`expected` is `refused`.' "$README"
    t "the CHANGELOG names the expected word of a connect line" grep -qF 'The `expected` word of a connect line is `refused`.' "$CHART/CHANGELOG.md"
}
