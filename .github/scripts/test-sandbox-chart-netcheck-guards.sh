#!/usr/bin/env bash

write_connect_retry_mutation() {
    awk '/^connect_state\(\) \{/ { print "connect_state() {\n  once=$(connect_once \"$1\")\n  tries=0\n  while [ \"$once\" = reached ] && [ \"$tries\" -lt 2 ]; do\n    tries=$((tries + 1))\n    sleep 1\n    once=$(connect_once \"$1\")\n  done\n  printf \"%s\" \"$once\"\n}\n"; sub(/connect_state/, "connect_once") } { print }' "$CHART/files/network-check.sh" >"$1"
}

connect_calls() {
    grep -c . "$WORK/netcheckstub/connect.log" || true
}

check_network_check_connect_probes_are_tried_once() {
    section "the network check: a CONNECT probe in mode allow is sent once, whatever the answer"
    mkdir -p "$WORK/netcheckstub"
    stub_tools "$WORK/netcheckbin"
    local out rc probes
    rc=0
    out="$(run_netcheck_egress env NC_MODE=all PROBE_MODE=none CONNECT_MODE=200 2>&1)" || rc=$?
    probes="$(grep -c '"kind":"connect"' <<<"$out")"
    t "a 200 at the CONNECT: there are connect probes" test "$probes" -gt 0
    t "a 200 at the CONNECT: every probe reads reached" test "$(grep -c '"kind":"connect".*"observed":"reached"' <<<"$out")" -eq "$probes"
    t "a 200 at the CONNECT: the Job fails" test "$rc" -ne 0
    t "a 200 at the CONNECT: one CONNECT is sent for each reported probe" test "$(connect_calls)" -eq "$probes"
    t "a 200 at the CONNECT: no wait" test "$(sleeps)" -eq 0
    rc=0
    out="$(run_netcheck_egress env NC_MODE=all PROBE_MODE=none CONNECT_MODE=403then200 2>&1)" || rc=$?
    t "a 403 and then a 200 on a second try: every probe reads refused" test "$(grep -c '"kind":"connect".*"observed":"refused"' <<<"$out")" -eq "$probes"
    t "a 403 and then a 200 on a second try: the Job passes" test "$rc" -eq 0
    t "a 403 and then a 200 on a second try: one CONNECT is sent for each reported probe" test "$(connect_calls)" -eq "$probes"
    write_connect_retry_mutation "$WORK/network-check-connect-retry.sh"
    t "mutation: the script with a retry on a 200 differs from the chart script" test "$(cmp -s "$WORK/network-check-connect-retry.sh" "$CHART/files/network-check.sh" && echo same || echo differ)" = differ
    rc=0
    out="$(NETCHECK_PROBE_SCRIPT="$WORK/network-check-connect-retry.sh" run_netcheck_egress env NC_MODE=all PROBE_MODE=none CONNECT_MODE=200 2>&1)" || rc=$?
    t "mutation: with a retry on a 200 the Job still fails" test "$rc" -ne 0
    t "mutation: with a retry on a 200 the count of CONNECT calls is above the count of probes" test "$(connect_calls)" -gt "$probes"
}

check_network_check_waits_for_each_probe_pod_for_the_wait_plus_the_limit() {
    section "the network check: the Job waits for each probe pod for the wait seconds plus the limit"
    stub_tools "$WORK/netcheckbin"
    render_yaml wait5_yaml -f "$EXAMPLE" --set network_check.enforcement_wait_seconds=5
    local count out
    count="$(namespace_count ex_yaml)"
    t "there is a namespace to probe" test "$count" -gt 0
    run_netcheck env NC_MODE=all PROBE_MODE=none >/dev/null 2>&1 || true
    t "limit 60 and wait 1: each probe pod is waited for 61s" test "$(grep -c -e '--timeout=61s$' "$WORK/netcheckstub/wait.args")" -eq "$count"
    NETCHECK_SOURCE=wait5_yaml run_netcheck env NC_MODE=all PROBE_MODE=none >/dev/null 2>&1 || true
    t "limit 5 and wait 1: each probe pod is waited for 6s" test "$(grep -c -e '--timeout=6s$' "$WORK/netcheckstub/wait.args")" -eq "$count"
    t "the wait is on the probe pod, in the namespace of the probe, until it has succeeded" test "$(grep -c -e '^wait --for=jsonpath={.status.phase}=Succeeded pod/br-sandbox-netcheck-probe -n ' "$WORK/netcheckstub/wait.args")" -eq "$count"
    run_netcheck env NC_MODE=all PROBE_MODE=none NETCHECK_WAIT_SECONDS=180 >/dev/null 2>&1 || true
    t "limit 60 and the default wait 180: each probe pod is waited for 240s" test "$(grep -c -e '--timeout=240s$' "$WORK/netcheckstub/wait.args")" -eq "$count"
    NETCHECK_SOURCE=wait5_yaml run_netcheck env NC_MODE=all PROBE_MODE=none NETCHECK_WAIT_SECONDS=180 >/dev/null 2>&1 || true
    t "limit 5 and the default wait 180: each probe pod is waited for 185s" test "$(grep -c -e '--timeout=185s$' "$WORK/netcheckstub/wait.args")" -eq "$count"
    sed 's/\$((wait_seconds + enforcement_wait))/${wait_seconds}/' "$CHART/files/network-check.sh" >"$WORK/network-check-nolimitwait.sh"
    t "mutation: the script that waits without the limit differs from the chart script" test "$(cmp -s "$WORK/network-check-nolimitwait.sh" "$CHART/files/network-check.sh" && echo same || echo differ)" = differ
    NETCHECK_SCRIPT="$WORK/network-check-nolimitwait.sh" NETCHECK_SOURCE=wait5_yaml run_netcheck env NC_MODE=all PROBE_MODE=none >/dev/null 2>&1 || true
    t "mutation: without the limit in the wait the timeout is not 6s" test "$(grep -c -e '--timeout=6s$' "$WORK/netcheckstub/wait.args")" -eq 0
    out="$(grep -c -e '--timeout=1s$' "$WORK/netcheckstub/wait.args")"
    t "mutation: without the limit in the wait the timeout is the wait alone" test "$out" -eq "$count"
}

check_network_check_limit_reaches_the_cronjob() {
    section "the network check: the limit reaches the container of the CronJob as it reaches the Job"
    render nc_cron -f "$EXAMPLE" --set network_check.enforcement_wait_seconds=7 --set network_check.cron='0 * * * *'
    t "the CronJob container carries the limit" jqt nc_cron '[.[] | select(.kind == "CronJob")][0].spec.jobTemplate.spec.template.spec.containers[0].env[] | select(.name == "NETCHECK_ENFORCEMENT_WAIT") | .value == "7"'
    t "the CronJob container runs the same script command as the Job" jqt nc_cron '([.[] | select(.kind == "CronJob")][0].spec.jobTemplate.spec.template.spec.containers[0].command) == ([.[] | select(.kind == "Job")][0].spec.template.spec.containers[0].command)'
}

check_network_check_a_script_that_never_waits_fails_instead_of_hanging() {
    section "the network check: a script with no wait in its retry loop fails fast in the suite, it does not hang"
    stub_tools "$WORK/netcheckbin"
    render_yaml wait3_guard_yaml -f "$EXAMPLE" --set network_check.enforcement_wait_seconds=3
    sed '/^    sleep 1$/d' "$CHART/files/network-check.sh" >"$WORK/network-check-nosleep.sh"
    t "mutation: the script without its sleep differs from the chart script" test "$(cmp -s "$WORK/network-check-nosleep.sh" "$CHART/files/network-check.sh" && echo same || echo differ)" = differ
    local out rc=0
    out="$(NETCHECK_PROBE_SCRIPT="$WORK/network-check-nosleep.sh" NETCHECK_SOURCE=wait3_guard_yaml run_netcheck env NC_MODE=all PROBE_MODE=none PROBE_LATE_K=100000000 2>&1)" || rc=$?
    t "a script with no sleep ends and the guard of the nc stub says why" grep -q 'more than 2000 calls' "$WORK/netcheckstub/runaway"
    t "a script with no sleep fails the Job" test "$rc" -ne 0
    t "a script with no sleep never waited" test "$(sleeps)" -eq 0
}
