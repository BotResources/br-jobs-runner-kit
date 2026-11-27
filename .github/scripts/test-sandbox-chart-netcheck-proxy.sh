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
    rc=0
    out="$(run_netcheck_egress env NC_MODE=all PROBE_MODE=none CONNECT_MODE=502 2>&1)" || rc=$?
    t "a 502 answer at the CONNECT is no_result for every probe and never refused" test "$(grep -c '"kind":"connect".*"observed":"no_result"' <<<"$out")" -eq 21 -a "$(grep -c '"kind":"connect".*"observed":"refused"' <<<"$out")" -eq 0
    t "a 502 answer at the CONNECT fails the job" test "$rc" -ne 0
    rc=0
    out="$(run_netcheck_egress env NC_MODE=all PROBE_MODE=none CONNECT_MODE=tunnel200 2>&1)" || rc=$?
    t "a tunnel opened with 200 whose request inside the tunnel answers 200 fails the job" test "$rc" -ne 0
    t "that answer is reported as reached for every probe" test "$(grep -c '"kind":"connect","expected":"[a-z_]*","observed":"reached"' <<<"$out")" -eq 21
    rc=0
    out="$(run_netcheck_egress env NC_MODE=all PROBE_MODE=none CONNECT_MODE=200 2>&1)" || rc=$?
    t "a CONNECT answered 200 with no answer to the request inside the tunnel fails the job" test "$rc" -ne 0
    t "a CONNECT answered 200 with no answer to the request inside the tunnel is reported as no_result and never as refused" test "$(grep -c '"kind":"connect","expected":"[a-z_]*","observed":"no_result"' <<<"$out")" -eq 21
    rc=0
    out="$(run_netcheck_egress env NC_MODE=all PROBE_MODE=none CONNECT_MODE=garbled 2>&1)" || rc=$?
    t "an answer that is not an HTTP status fails the job" test "$rc" -ne 0
    t "an answer that is not an HTTP status is reported as no_result" grep -q '"kind":"connect","expected":"[a-z_]*","observed":"no_result"' <<<"$out"
    rc=0
    out="$(run_netcheck_egress env NC_MODE=all PROBE_MODE=none CONNECT_MODE=silent 2>&1)" || rc=$?
    t "a proxy that does not answer fails the job" test "$rc" -ne 0
    rc=0
    out="$(run_netcheck_egress env NC_MODE=all PROBE_MODE=none CONNECT_MODE=offlist 2>&1)" || rc=$?
    t "positive control: a proxy that refuses the off-list host and passes the targets fails the job" test "$rc" -ne 0
    t "positive control: the off-list probe is reported as refused" grep -q '"target":"offlist.invalid:443","kind":"connect","expected":"refused","observed":"refused"' <<<"$out"
    t "positive control: every target probe is reported as reached, so a proxy that passes a target is seen" test "$(grep -c '"kind":"connect","expected":"[a-z_]*","observed":"reached"' <<<"$out")" -eq 20
    t "the README says the probe has no positive control and the cluster tests check that a listed host is reached" grep -qF 'The probe has no positive control: a proxy that refuses everything (a broken allow list) also passes the off-list check, so that a listed host is reached is checked by the cluster tests' "$README"
    t "the example render has no CONNECT probe" test "$(run_netcheck env NC_MODE=all PROBE_MODE=none 2>&1 | grep -c '"kind":"connect"')" -eq 0
    t "the pod list excludes the proxy pods by selector" grep -q 'br.sandbox/role!=proxy' "$WORK/netcheckstub/pods.args"
    t "a proxy pod port is not a target when the selector is applied" test "$(run_netcheck_egress env NC_MODE=all PROBE_MODE=none CONNECT_MODE=403 2>&1 | grep -c '"target":"10.42.0.9:8888"')" -eq 0
    sed "s/ -l 'br.sandbox\/role!=proxy'//" "$CHART/files/network-check.sh" >"$WORK/network-check-noselector.sh"
    t "without the selector the same stub lists the proxy pod as a target" test "$(NETCHECK_SCRIPT="$WORK/network-check-noselector.sh" run_netcheck_egress env NC_MODE=all PROBE_MODE=none CONNECT_MODE=403 2>&1 | grep -c '"target":"10.42.0.9:8888"')" -ge 1
}

assert_connect_verdict() {
    local proxy_mode="$1" stub="$2" verdict="$3" other="$4" out rc=0 want
    local allow_stub="$other" full_stub="$other"
    want=10
    if [[ "$proxy_mode" == allow ]]; then allow_stub="$stub"; want=11; else full_stub="$stub"; fi
    out="$(run_netcheck_egress env NC_MODE=all PROBE_MODE=none CONNECT_MODE=403 CONNECT_MODE_ALLOW="$allow_stub" CONNECT_MODE_FULL="$full_stub" 2>&1)" || rc=$?
    t "mode ${proxy_mode}, stub ${stub}: every probe of the mode is ${verdict}" test "$(grep -c "\"mode\":\"${proxy_mode}\".*\"kind\":\"connect\".*\"observed\":\"${verdict}\"" <<<"$out")" -eq "$want"
    t "mode ${proxy_mode}, stub ${stub}: the JSON line carries the word ${verdict}" grep -q "\"mode\":\"${proxy_mode}\",\"target\":\"[^\"]*\",\"kind\":\"connect\",\"expected\":\"[a-z_]*\",\"observed\":\"${verdict}\",\"control\":\"not_applicable\"" <<<"$out"
    if [[ "$verdict" == refused || ( "$verdict" == unanswered && "$proxy_mode" == full ) ]]; then
        t "mode ${proxy_mode}, stub ${stub}: the job passes" test "$rc" -eq 0
    else
        t "mode ${proxy_mode}, stub ${stub}: the job fails" test "$rc" -ne 0
    fi
}

check_network_check_connect_verdicts_of_each_mode() {
    section "the network check: the verdict of a CONNECT probe in mode allow and in mode full, two steps through the tunnel"
    mkdir -p "$WORK/netcheckstub"
    stub_tools "$WORK/netcheckbin"
    local proxy_mode stub
    for proxy_mode in allow full; do
        assert_connect_verdict "$proxy_mode" 403 refused 403
        for stub in tunnel502 tunnel502target connect200x403; do assert_connect_verdict "$proxy_mode" "$stub" unanswered 403; done
        for stub in tunnel200 tunnel404 tunnel502other tunnel200x502; do assert_connect_verdict "$proxy_mode" "$stub" reached 403; done
        for stub in 200 502 tunnelgarbled garbled silent tunnelnocode nocode; do assert_connect_verdict "$proxy_mode" "$stub" no_result 403; done
    done
    local out
    out="$(run_netcheck_egress env NC_MODE=all PROBE_MODE=none CONNECT_MODE=403 CONNECT_MODE_FULL=tunnel502 2>&1)" || true
    t "mode full: a CONNECT answered 200 then a request answered 502 with the body bad gateway is unanswered and never refused" test "$(grep -c '"mode":"full".*"kind":"connect".*"observed":"unanswered"' <<<"$out")" -eq 10 -a "$(grep -c '"mode":"full".*"kind":"connect".*"observed":"refused"' <<<"$out")" -eq 0
    t "mode full: the expected word of a probe is refused_or_unanswered" test "$(grep -c '"mode":"full","target":"[^"]*","kind":"connect","expected":"refused_or_unanswered"' <<<"$out")" -eq 10
    t "mode allow: the expected word of a probe is refused" test "$(grep -c '"mode":"allow","target":"[^"]*","kind":"connect","expected":"refused"' <<<"$out")" -eq 11
    t "mode allow, off-list host: a CONNECT answered 403 is refused" grep -q '"mode":"allow","target":"offlist.invalid:443","kind":"connect","expected":"refused","observed":"refused"' <<<"$out"
    out="$(run_netcheck_egress env NC_MODE=all PROBE_MODE=none CONNECT_MODE=403 CONNECT_MODE_FULL=tunnel502target 2>&1)" || true
    t "documented limit: a reached HTTP target that answers 502 with the body bad gateway is counted unanswered in mode full" test "$(grep -c '"mode":"full".*"kind":"connect".*"observed":"unanswered"' <<<"$out")" -eq 10 -a "$(grep -c '"mode":"full".*"kind":"connect".*"observed":"reached"' <<<"$out")" -eq 0
    out="$(run_netcheck_egress env NC_MODE=all PROBE_MODE=none CONNECT_MODE=403 CONNECT_MODE_FULL=tunnel200x502 2>&1)" || true
    t "a status line HTTP/1.1 200 x 502 y with a bad gateway line inside the tunnel is reached, the code is the second field" test "$(grep -c '"mode":"full".*"kind":"connect".*"observed":"reached"' <<<"$out")" -eq 10 -a "$(grep -c '"mode":"full".*"kind":"connect".*"observed":"unanswered"' <<<"$out")" -eq 0
    out="$(run_netcheck_egress env NC_MODE=all PROBE_MODE=none CONNECT_MODE=403 CONNECT_MODE_ALLOW=connect200x403 CONNECT_MODE_FULL=connect200x403 2>&1)" || true
    t "a CONNECT line HTTP/1.1 200 x 403 y is read as 200 and never as refused" test "$(grep -c '"kind":"connect".*"observed":"refused"' <<<"$out")" -eq 0 -a "$(grep -c '"kind":"connect".*"observed":"unanswered"' <<<"$out")" -eq 21
    out="$(run_netcheck_egress env NC_MODE=all PROBE_MODE=none CONNECT_MODE=403 CONNECT_MODE_FULL=tunnelnocode 2>&1)" || true
    t "a status line with no three-digit code inside the tunnel is no_result" test "$(grep -c '"mode":"full".*"kind":"connect".*"observed":"no_result"' <<<"$out")" -eq 10
    out="$(run_netcheck_egress env NC_MODE=all PROBE_MODE=none CONNECT_MODE=nocode 2>&1)" || true
    t "a CONNECT status line with no three-digit code is no_result for every probe" test "$(grep -c '"kind":"connect".*"observed":"no_result"' <<<"$out")" -eq 21
    t "a request GET / is sent after each CONNECT on the same connection" test "$(grep -c '^10.43.0.5[01]:8888 GET / HTTP/1.1$' "$WORK/netcheckstub/connect.log")" -eq "$(grep -c '^10.43.0.5[01]:8888 CONNECT ' "$WORK/netcheckstub/connect.log")"
    out="$(run_netcheck_egress env NC_MODE=all PROBE_MODE=none CONNECT_MODE=offlist 2>&1)" || true
    t "positive control: a proxy that passes a target with a 200 answer inside the tunnel is seen as reached in mode full" test "$(grep -c '"mode":"full".*"kind":"connect".*"observed":"reached"' <<<"$out")" -eq 10
    t "the README says the CONNECT verdict is read from the request inside the tunnel when the CONNECT answers 200" grep -qF 'sends a plain HTTP request `GET /` inside the tunnel on the same connection' "$README"
    t "the README says unanswered is a 502 with the body bad gateway inside the tunnel and is accepted in mode full only" grep -qF 'is `unanswered` when the request inside the tunnel gets an answer 502 with the body `bad gateway`' "$README"
    t "the README says what unanswered does not prove" grep -qF 'It does not prove that the deny list refused the target' "$README"
    t "the README says the proof that the deny list refused is the audit log of the proxy" grep -qF 'The evidence that the deny list refused an address is the audit log of the proxy: an entry with `action` `error` and the error text `denied by upstream_deny_cidrs`. Nothing in this chart reads that log; an operator reads it.' "$README"
    t "the README says what a pass means in mode full and names the targets it cannot cover" grep -qF 'a pass of the probes through the proxy is no evidence about the deny list of the proxy or about the network policy of the proxy namespace for a target that does not answer plain HTTP. The Job cannot cover that way: the message bus, the database, the DNS service, the kubelet and the cluster API server when they do not answer plain HTTP, and any non-HTTP pod port.' "$README"
    t "the README says what the Job proves" grep -qF 'What this Job proves is the direct TCP probes of the sandbox pod and the network policy of the sandbox namespace.' "$README"
    t "the README gives no refusal-as-control claim and no deploy test" test "$(grep -cE 'its own control|deploy test' "$README")" -eq 0
    t "the README says unanswered has no control of its own and refused keeps its meaning" grep -qF 'A `refused` verdict (403 at the `CONNECT`, mode `allow`) is sent by the proxy only. An `unanswered` verdict has no control of its own.' "$README"
    t "the README says a CONNECT answer other than 200 and 403 is no_result, a 502 included" grep -qF 'a `CONNECT` answer other than 200 and 403 (a 502 at the `CONNECT` included' "$README"
    t "the README names the expected word of a connect line in each mode" grep -qF '`expected` is `refused` in mode `allow`, and `refused_or_unanswered` in mode `full` (the Job accepts `refused` and `unanswered` there)' "$README"
    t "the CHANGELOG says a CONNECT answer other than 200 and 403 is no_result" grep -qF 'a CONNECT answer other than 200 and 403, a 502 included, is `no_result`' "$CHART/CHANGELOG.md"
    t "the CHANGELOG names the targets that the probes through the proxy cannot cover in mode full" grep -qF 'the message bus, the database, the DNS service, the kubelet and the cluster API server when they do not answer plain HTTP, and any non-HTTP pod port' "$CHART/CHANGELOG.md"
    t "the CHANGELOG says an operator reads the audit log" grep -qF 'nothing in this chart reads that log, an operator reads it' "$CHART/CHANGELOG.md"
    t "the CHANGELOG names the expected word of a connect line" grep -qF 'is `refused` in mode `allow` and `refused_or_unanswered` in mode `full`' "$CHART/CHANGELOG.md"
    t "the CHANGELOG names no deploy test" test "$(grep -c 'deploy test' "$CHART/CHANGELOG.md")" -eq 0
    t "the README says that mode full answers 200 at the CONNECT for a deny-listed target" grep -qF 'the proxy answers the `CONNECT` with 200 for any target, a target of the deny list included' "$README"
    t "the README says what unanswered means and names the limit for a target that answers the same bytes" grep -qF '`unanswered` means an answer that has the proxy'"'"'s 502 form: status 502 and the body line `bad gateway`. It does not prove that the deny list refused the target. A target that is reached and does not speak HTTP gets that answer from the proxy. A target that answers HTTP with exactly that form (status 502 and the body line `bad gateway`) gives the same bytes, so it is counted `unanswered` too: that is a limit of the probe, which cannot tell the two apart. A target that answers HTTP in any other form is `reached`.' "$README"
    t "the CHANGELOG says what unanswered means and names the same limit" grep -qF 'a target that answers HTTP with exactly that form gives the same bytes and is counted `unanswered` too: a limit of the probe, which cannot tell them apart. A target that answers HTTP in any other form is `reached`.' "$CHART/CHANGELOG.md"
    t "no text says unanswered proves no HTTP answer or that a target that answers HTTP is still caught" test "$(grep -cE 'proves that the proxy returned no HTTP answer|still fails on a target that answers HTTP|still caught as' "$README" "$CHART/CHANGELOG.md" | grep -vc ':0$')" -eq 0
    t "the README says the code is the second field of the status line and a line with no three-digit code is no_result" grep -qF 'The status code is the second field of the status line, compared as a whole field (`HTTP/1.1 200 x 502 y` is read as 200); a status line with no three-digit code is `no_result`.' "$README"
    t "the CHANGELOG says the same about the status code" grep -qF 'The status code is the second field of the status line, compared as a whole field (`HTTP/1.1 200 x 502 y` is read as 200); a status line with no three-digit code is `no_result`.' "$CHART/CHANGELOG.md"
    t "the README lists the values of kind, control and observed for each kind of line" grep -qF '`kind` is `tcp`, `connect`, `target_class` or `node_list`. By kind: `tcp` carries `observed` `refused`, `reached` or `no_result` and `control` `ok` or `control_unavailable`; `connect` carries `observed` `refused`, `reached`, `unanswered` or `no_result` and `control` `not_applicable`; `target_class` carries `observed` `missing` and `control` `ok`; `node_list` carries `observed` `stale` and `control` `ok`.' "$README"
    t "the CHANGELOG lists the same values" grep -qF 'The JSON line carries `kind` `tcp`, `connect`, `target_class` or `node_list`: `tcp` carries `observed` `refused`, `reached` or `no_result` and `control` `ok` or `control_unavailable`; `connect` carries `observed` `refused`, `reached`, `unanswered` or `no_result` and `control` `not_applicable`; `target_class` carries `observed` `missing` and `control` `ok`; `node_list` carries `observed` `stale` and `control` `ok`.' "$CHART/CHANGELOG.md"
    t "the README no longer says control is only ok or control_unavailable" test "$(grep -cF 'control` is `ok` or `control_unavailable`' "$README")" -eq 0
    t "the CHANGELOG names the verdict unanswered" grep -qF '`unanswered` when the request inside the tunnel gets 502 with the body `bad gateway`' "$CHART/CHANGELOG.md"
    t "the CHANGELOG says the same" grep -qF 'when the CONNECT answers 200 the probe sends a plain HTTP request inside the tunnel' "$CHART/CHANGELOG.md"
}

check_network_check_unanswered_targets_by_port() {
    section "the network check: the plain request inside the tunnel for each port of the probe targets"
    mkdir -p "$WORK/netcheckstub"
    stub_tools "$WORK/netcheckbin"
    local out port rc=0
    out="$(run_netcheck_egress env NC_MODE=all PROBE_MODE=none CONNECT_MODE=403 CONNECT_MODE_FULL=bytarget 2>&1)" || rc=$?
    for port in 443 6443 10250 53; do
        t "mode full, port ${port}: a 502 with the body bad gateway inside the tunnel is unanswered" test "$(grep -c "\"mode\":\"full\",\"target\":\"[^\"]*:${port}\",\"kind\":\"connect\",\"expected\":\"refused_or_unanswered\",\"observed\":\"unanswered\"" <<<"$out")" -ge 1
        t "mode full, port ${port}: the plain GET is sent through the tunnel to the target" grep -q "^10.43.0.51:8888 CONNECT [^ ]*:${port} HTTP/1.1$" "$WORK/netcheckstub/connect.log"
    done
    t "mode full: a target that answers 200 inside the tunnel is reached while the others stay unanswered" grep -q '"mode":"full","target":"10.42.0.5:8080","kind":"connect","expected":"refused_or_unanswered","observed":"reached"' <<<"$out"
    t "the job fails when one target is reached" test "$rc" -ne 0
}
