#!/usr/bin/env bash

job_env() {
    jq -r --arg n "$2" '[.[] | select(.kind == "Job")][0].spec.template.spec.containers[0].env[] | select(.name == $n) | .value' "$WORK/$1.json"
}

probe_env() {
    yq -r ".spec.containers[0].env[] | select(.name == \"NETCHECK_ENFORCEMENT_WAIT\") | .value" "$(netcheck_dir "$1")/probe-$2.yaml"
}

namespace_count() {
    grep -c . "$(netcheck_dir "$1")/namespaces"
}

check_network_check_enforcement_wait_reaches_the_job_the_probe_pods_and_the_deadline() {
    section "the network check: enforcement_wait_seconds reaches the Job, the probe pods and the Job deadline"
    local count
    count="$(namespace_count ex_yaml)"
    render nc_wait -f "$EXAMPLE" --set network_check.enforcement_wait_seconds=7 --set network_check.cron='0 * * * *'
    render_yaml nc_wait_yaml -f "$EXAMPLE" --set network_check.enforcement_wait_seconds=7
    render nc_default -f "$EXAMPLE" --set network_check.cron='0 * * * *'
    t "the default is 60 in the Job environment" test "$(job_env nc_default NETCHECK_ENFORCEMENT_WAIT)" = 60
    t "the default is 60 in the probe pod of each namespace" test "$(probe_env ex_yaml sbx-alpha-allow)" = 60 -a "$(probe_env ex_yaml sbx-beta-none)" = 60
    t "the Job deadline is 1800 plus the default for each namespace" jqt nc_default "[.[] | select(.kind == \"Job\")][0].spec.activeDeadlineSeconds == $((1800 + count * 60))"
    t "the CronJob deadline is the same" jqt nc_default "[.[] | select(.kind == \"CronJob\")][0].spec.jobTemplate.spec.activeDeadlineSeconds == $((1800 + count * 60))"
    t "a set value reaches the Job environment" test "$(job_env nc_wait NETCHECK_ENFORCEMENT_WAIT)" = 7
    t "a set value reaches the probe pod of each namespace" test "$(probe_env nc_wait_yaml sbx-alpha-allow)" = 7 -a "$(probe_env nc_wait_yaml sbx-alpha-none)" = 7
    t "a set value moves the Job deadline" jqt nc_wait "[.[] | select(.kind == \"Job\")][0].spec.activeDeadlineSeconds == $((1800 + count * 7))"
    t "a set value moves the CronJob deadline" jqt nc_wait "[.[] | select(.kind == \"CronJob\")][0].spec.jobTemplate.spec.activeDeadlineSeconds == $((1800 + count * 7))"
    t "the probe environment still starts with the targets" test "$(yq -r '.spec.containers[0].env[0].name' "$(netcheck_dir ex_yaml)/probe-sbx-alpha-none.yaml")" = NETCHECK_TARGETS
    fails_with "/network_check/enforcement_wait_seconds': minimum: got 0, want 1" -f "$EXAMPLE" --set network_check.enforcement_wait_seconds=0
    fails_with "/network_check/enforcement_wait_seconds': minimum: got -3, want 1" -f "$EXAMPLE" --set network_check.enforcement_wait_seconds=-3
    fails_with "/network_check/enforcement_wait_seconds': got string, want integer" -f "$EXAMPLE" --set-string network_check.enforcement_wait_seconds=60
    fails_with "/network_check/enforcement_wait_seconds'" -f "$EXAMPLE" --set network_check.enforcement_wait_seconds=1.5
    t "the README names the value, its default and the deadline rule" grep -qF 'the Job deadline is 1800 seconds plus this value for each sandbox namespace' "$README"
}

wait_run() {
    local late="$1"
    shift
    NETCHECK_SOURCE=wait3_yaml run_netcheck env PROBE_LATE_K="$late" "$@"
}

sleeps() {
    grep -c . "$WORK/netcheckstub/sleep.log" || true
}

check_network_check_retries_a_probe_until_it_is_refused() {
    section "the network check: a probe pod that is not yet enforced is tried again until its probes are refused; a policy that never applies fails the Job"
    stub_tools "$WORK/netcheckbin"
    render_yaml wait3_yaml -f "$EXAMPLE" --set network_check.enforcement_wait_seconds=3
    render_yaml wait60_yaml -f "$EXAMPLE"
    local out rc count first
    count="$(namespace_count ex_yaml)"
    first="$(sed -n 1p "$(netcheck_dir ex_yaml)/extra-targets")"
    rc=0
    out="$(NETCHECK_SOURCE=wait60_yaml run_netcheck env NC_MODE=all PROBE_MODE=none PROBE_LATE_K=5 2>&1)" || rc=$?
    t "late policy: the Job passes when the probes are refused after some tries" test "$rc" -eq 0
    t "late policy: each probe is refused" test "$(grep -c '"kind":"tcp".*"observed":"refused"' <<<"$out")" -eq "$(grep -c '"kind":"tcp"' <<<"$out")"
    t "late policy: the first probe of each namespace reports the time to its first refusal, 5 seconds" test "$(grep -c "\"target\":\"${first}\".*\"refused_after_seconds\":5,\"limit_seconds\":60" <<<"$out")" -eq "$count"
    t "late policy: the other probes of the pod report the same time, since the policy applied" test "$(grep -c '"kind":"tcp".*"refused_after_seconds":5,' <<<"$out")" -eq "$(grep -c '"kind":"tcp"' <<<"$out")"
    t "late policy: the pod waited one second per try" test "$(sleeps)" -eq $((count * 5))
    sed 's/-ge "\$enforcement_wait"/-ge 0/' "$CHART/files/network-check.sh" >"$WORK/network-check-noretry.sh"
    t "mutation: the script of the late policy case differs from the chart script" test "$(cmp -s "$WORK/network-check-noretry.sh" "$CHART/files/network-check.sh" && echo same || echo differ)" = differ
    rc=0
    out="$(NETCHECK_PROBE_SCRIPT="$WORK/network-check-noretry.sh" NETCHECK_SOURCE=wait60_yaml run_netcheck env NC_MODE=all PROBE_MODE=none PROBE_LATE_K=5 2>&1)" || rc=$?
    t "mutation: without the retry the late policy case fails the Job" test "$rc" -ne 0
    t "mutation: without the retry the probes read reached" grep -q '"observed":"reached"' <<<"$out"
    rc=0
    out="$(wait_run 100000 env NC_MODE=all PROBE_MODE=none 2>&1)" || rc=$?
    t "never applied: the Job fails" test "$rc" -ne 0
    t "never applied: each probe reports not refused and the limit" test "$(grep -c '"kind":"tcp","expected":"refused","observed":"reached","control":"ok","refused_after_seconds":"not refused","limit_seconds":3' <<<"$out")" -eq "$(grep -c '"kind":"tcp"' <<<"$out")"
    t "never applied: the pod waited the limit once, from the start of the probe script, and not once per probe" test "$(sleeps)" -eq $((count * 3))
    t "never applied: every probe was still tried, none was skipped" test "$(grep -c "\"target\":\"${first}\"" <<<"$out")" -eq "$count"
    rc=0
    out="$(wait_run "" env NC_MODE=all PROBE_MODE=none 2>&1)" || rc=$?
    t "already enforced: the Job passes" test "$rc" -eq 0
    t "already enforced: every probe is refused at 0 seconds" test "$(grep -c '"kind":"tcp".*"observed":"refused".*"refused_after_seconds":0,"limit_seconds":3' <<<"$out")" -eq "$(grep -c '"kind":"tcp"' <<<"$out")"
    t "already enforced: no wait" test "$(sleeps)" -eq 0
    t "already enforced: a probe is tried once in each namespace" test "$(grep -cxF "$first" "$WORK/netcheckstub/nc.calls")" -eq $((count + 1))
    rc=0
    out="$(wait_run "" env NC_MODE=none PROBE_MODE=none 2>&1)" || rc=$?
    t "a positive control is not tried again: no control means a failed Job" test "$rc" -ne 0
    t "a positive control is not tried again: no wait in the Job" test "$(sleeps)" -eq 0
    t "a positive control is not tried again: one call for the control and one for each probe pod" test "$(grep -cxF "$first" "$WORK/netcheckstub/nc.calls")" -eq $((count + 1))
    t "the connect probes are tried once: no wait in mode allow with a proxy" test "$(NETCHECK_SOURCE=egress_yaml run_netcheck env NC_MODE=all PROBE_MODE=none CONNECT_MODE=403 >/dev/null 2>&1; sleeps)" -eq 0
    t "the README says the controls and the CONNECT probes are tried once" grep -qF 'The positive controls (from the Job pod) are tried once and never again, and the `CONNECT` probes in mode `allow` are tried once.' "$README"
    t "the README names the products of the measurement and the numbers" grep -qF 'flannel and the kube-router network policy controller (reported by the operator of a test cluster; no record in this repository): the policy started some seconds after the pod; a probe sent at once reached 2 of 2 targets, the same probe sent 30 seconds later reached 0' "$README"
    t "the README says the window exists and the chart does not close it" grep -qF 'A window exists in which a fresh sandbox pod can reach internal targets, and the chart does not close it' "$README"
    t "the README says what the Job proves" grep -qF 'What the Job proves is the state after enforcement.' "$README"
}

first_refusal() {
    local source="$1" late="$2" first
    first="$(sed -n 1p "$(netcheck_dir "$source")/extra-targets")"
    NETCHECK_SOURCE="$source" run_netcheck env NC_MODE=all PROBE_MODE=none PROBE_LATE_K="$late" NC_DROP_SECONDS=2 2>&1 | grep '^{' | jq -r --arg target "$first" 'select(.kind == "tcp" and .target == $target) | .refused_after_seconds' | sort -u
}

check_network_check_a_probe_is_read_at_most_three_seconds_after_the_limit() {
    section "the network check: under a policy that starts to drop after some tries, a probe is read at most 3 seconds after the limit"
    stub_tools "$WORK/netcheckbin"
    render_yaml bound_yaml -f "$EXAMPLE" --set network_check.enforcement_wait_seconds=3
    local late value
    for late in 0 1 2 3; do
        value="$(first_refusal bound_yaml "$late")"
        t "limit 3, reached ${late} times and then dropped (2 seconds each): the first target reads refused after $((late + 2)) seconds" test "$value" = "$((late + 2))"
        t "limit 3, reached ${late} times and then dropped: the time is at most the limit plus 3" test "$value" -le 6
    done
    t "limit 3, reached 3 times and then dropped: the time is above the limit, and the Job reads it as refused" test "$(first_refusal bound_yaml 3)" -gt 3
    t "limit 3, reached 4 times: the probe is not refused when the limit is checked" test "$(first_refusal bound_yaml 4)" = "not refused"
    sed 's/^    sleep 1$/    sleep 5/' "$CHART/files/network-check.sh" >"$WORK/network-check-sleep5.sh"
    t "mutation: the script that sleeps 5 seconds between two tries differs from the chart script" test "$(cmp -s "$WORK/network-check-sleep5.sh" "$CHART/files/network-check.sh" && echo same || echo differ)" = differ
    value="$(NETCHECK_PROBE_SCRIPT="$WORK/network-check-sleep5.sh" first_refusal bound_yaml 1)"
    t "mutation: with a sleep of 5 seconds a probe is read more than 3 seconds after the limit" test "$value" -gt 6
    t "the README says that a printed time is whole seconds" grep -qF 'A printed time is the difference of two readings of `date +%s`, in whole seconds, so it differs from the real elapsed time by less than 1 second.' "$README"
}
