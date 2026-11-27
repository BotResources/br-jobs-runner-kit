#!/usr/bin/env bash

elapsed_grows_by_two() {
    grep '^{' | jq -e -s '[.[] | select(.kind == "tcp" and .net == "sbx-alpha-none") | .refused_after_seconds] | . as $a | ($a | length) > 3 and $a == [range(1; ($a | length) + 1) | . * 2]'
}

check_network_check_elapsed_time_counts_the_wait_of_dropped_probes() {
    section "the network check: the elapsed time is counted from the start of the probe pod and includes the nc wait of the dropped probes"
    stub_tools "$WORK/netcheckbin"
    local out rc=0
    out="$(run_netcheck env NC_MODE=all PROBE_MODE=none NC_DROP_SECONDS=2 2>&1)" || rc=$?
    t "dropped probes: the Job passes, every probe is refused" test "$rc" -eq 0
    t "dropped probes: the k-th target of a pod reports 2 times k seconds" elapsed_grows_by_two <<<"$out"
    t "dropped probes: no wait is spent, each probe is refused at its first try" test "$(sleeps)" -eq 0
    out="$(run_netcheck env NC_MODE=all PROBE_MODE=none 2>&1)" || true
    t "reset probes: every probe of the pod reports 0" test "$(grep -c '"kind":"tcp".*"refused_after_seconds":0,' <<<"$out")" -eq "$(grep -c '"kind":"tcp"' <<<"$out")"
    t "the README says the start is one value for the pod and that the wait of the dropped probes counts in it" grep -qF 'The start is one value for the whole probe pod, not one for each target, and the limit is counted from it too: the limit is one budget for the pod, a later target gets what is left of it, and once it is spent each remaining target is tried once. The `nc -w 2` wait of every probe that is dropped counts in the elapsed time' "$README"
    t "the README says what a REJECT policy and a DROP policy report" grep -qF 'under a policy that resets the connection (REJECT) a probe that is refused at once reports about 0; under a policy that drops the packets (DROP) each probe waits for its `nc -w 2` timeout before it reads as refused, so the k-th target of the pod reports about 2 times k seconds' "$README"
}

precondition_row() {
    grep -qF "{\"net\":\"cluster\",\"mode\":\"all\",\"target\":\"$1\",\"kind\":\"precondition\",\"expected\":\"$2\",\"observed\":\"$3\",\"control\":\"not_applicable\"}"
}

check_network_check_preconditions_fail_before_any_probe() {
    section "the network check: a bad limit or a missing nc, sleep or date fails at the start with a distinct row and exit status 2"
    stub_tools "$WORK/netcheckbin"
    local value out rc
    for value in 0 -3 abc 1.5 08 1000000000 99999999999999999999; do
        rc=0
        out="$(run_netcheck env NC_MODE=all PROBE_MODE=none NETCHECK_ENFORCEMENT_WAIT="$value" 2>&1)" || rc=$?
        t "limit ${value}: exit status 2" test "$rc" -eq 2
        t "limit ${value}: the precondition row says invalid" precondition_row NETCHECK_ENFORCEMENT_WAIT "integer from 1 to 999999999" invalid <<<"$out"
        t "limit ${value}: no probe and no wait before the row" test ! -s "$WORK/netcheckstub/nc.calls" -a ! -s "$WORK/netcheckstub/wait.args"
    done
    for value in 1 999999999; do
        rc=0
        out="$(run_netcheck env NC_MODE=all PROBE_MODE=none NETCHECK_ENFORCEMENT_WAIT="$value" 2>&1)" || rc=$?
        t "limit ${value}: accepted, the Job passes" test "$rc" -eq 0
        t "limit ${value}: no precondition row" test "$(grep -c '"kind":"precondition"' <<<"$out")" -eq 0
    done
    : >"$WORK/netcheckstub/nc.calls"
    rc=0
    out="$(NETCHECK_ENFORCEMENT_WAIT=abc NETCHECK_TARGETS=10.0.0.1:80 NC_MODE=none PATH="$WORK/netcheckbin:$PATH" STUB_DIR="$WORK/netcheckstub" sh "$CHART/files/network-check.sh" probe 2>&1)" || rc=$?
    t "a probe pod with a bad limit: exit status 2 and the invalid row, no probe" test "$rc" -eq 2 -a "$(grep -c '"observed":"invalid"' <<<"$out")" -eq 1 -a ! -s "$WORK/netcheckstub/nc.calls"
    local tool other
    for tool in nc sleep date; do
        mkdir -p "$WORK/netcheckno-$tool"
        ln -sf "$(command -v sh)" "$WORK/netcheckno-$tool/sh"
        for other in nc sleep date; do
            [[ "$other" == "$tool" ]] || ln -sf "$WORK/netcheckbin/$other" "$WORK/netcheckno-$tool/$other"
        done
        rc=0
        out="$(run_netcheck env PATH="$WORK/netcheckno-$tool" NC_MODE=all PROBE_MODE=none 2>&1)" || rc=$?
        t "no ${tool}: exit status 2" test "$rc" -eq 2
        t "no ${tool}: the precondition row says ${tool} is missing" precondition_row "$tool" present missing <<<"$out"
        t "no ${tool}: it is the only row" test "$(grep -c '^{' <<<"$out")" -eq 1
        t "no ${tool}: no probe and no wait before the row" test ! -s "$WORK/netcheckstub/nc.calls" -a ! -s "$WORK/netcheckstub/wait.args"
        rc=0
        out="$(NETCHECK_ENFORCEMENT_WAIT=3 NETCHECK_TARGETS=10.0.0.1:80 NC_MODE=none PATH="$WORK/netcheckno-$tool" STUB_DIR="$WORK/netcheckstub" sh "$CHART/files/network-check.sh" probe 2>&1)" || rc=$?
        t "no ${tool}: a probe pod gives the same row and exit status 2" test "$rc" -eq 2 -a "$(grep -c '"kind":"precondition"' <<<"$out")" -eq 1 -a "$(grep -c "\"target\":\"${tool}\"" <<<"$out")" -eq 1
        sed "/command -v ${tool} /d" "$CHART/files/network-check.sh" >"$WORK/network-check-no-${tool}-check.sh"
        t "mutation: the script without the ${tool} check differs from the chart script" test "$(cmp -s "$WORK/network-check-no-${tool}-check.sh" "$CHART/files/network-check.sh" && echo same || echo differ)" = differ
        rc=0
        out="$(NETCHECK_SCRIPT="$WORK/network-check-no-${tool}-check.sh" run_netcheck env PATH="$WORK/netcheckno-$tool" NC_MODE=all PROBE_MODE=none 2>&1)" || rc=$?
        t "mutation: without the check a missing ${tool} gives no precondition row" test "$(grep -c '"kind":"precondition"' <<<"$out")" -eq 0
    done
    rc=0
    out="$(run_netcheck env NC_MODE=all PROBE_MODE=none 2>&1)" || rc=$?
    t "with nc present and a valid limit the run has no precondition row" test "$(grep -c '"kind":"precondition"' <<<"$out")" -eq 0
    fails_with "/network_check/enforcement_wait_seconds': maximum: got" -f "$EXAMPLE" --set network_check.enforcement_wait_seconds=1000000000
}

first_missing_row() {
    local out
    out="$(run_netcheck env PATH="$1" NC_MODE=all PROBE_MODE=none 2>&1)" || true
    grep -o '"kind":"precondition"' <<<"$out" >/dev/null && sed -n 's/.*"target":"\([a-z_]*\)","kind":"precondition".*/\1/p' <<<"$out"
}

check_network_check_the_precondition_row_names_the_first_missing_program_in_order() {
    section "the network check: with two programs missing, the precondition row names the first one in the order nc, sleep, date"
    stub_tools "$WORK/netcheckbin"
    local pair first second kept dir
    for pair in nc:sleep:date nc:date:sleep sleep:date:nc; do
        first="${pair%%:*}"
        second="$(cut -d: -f2 <<<"$pair")"
        kept="${pair##*:}"
        dir="$WORK/netcheckno-${first}-${second}"
        mkdir -p "$dir"
        ln -sf "$(command -v sh)" "$dir/sh"
        ln -sf "$WORK/netcheckbin/$kept" "$dir/$kept"
        t "no ${first} and no ${second}: the row names ${first}" test "$(first_missing_row "$dir")" = "$first"
    done
    awk '/command -v sleep /{held=$0; next} {print} /command -v date /{print held}' "$CHART/files/network-check.sh" >"$WORK/network-check-date-before-sleep.sh"
    awk '/command -v nc /{held=$0; next} {print} /command -v sleep /{print held}' "$CHART/files/network-check.sh" >"$WORK/network-check-sleep-before-nc.sh"
    t "mutation: the script that checks date before sleep differs from the chart script" test "$(cmp -s "$WORK/network-check-date-before-sleep.sh" "$CHART/files/network-check.sh" && echo same || echo differ)" = differ
    t "mutation: the script that checks sleep before nc differs from the chart script" test "$(cmp -s "$WORK/network-check-sleep-before-nc.sh" "$CHART/files/network-check.sh" && echo same || echo differ)" = differ
    t "mutation: with date checked before sleep, no sleep and no date gives the row of date" test "$(NETCHECK_SCRIPT="$WORK/network-check-date-before-sleep.sh" first_missing_row "$WORK/netcheckno-sleep-date")" = date
    t "mutation: with sleep checked before nc, no nc and no sleep gives the row of sleep" test "$(NETCHECK_SCRIPT="$WORK/network-check-sleep-before-nc.sh" first_missing_row "$WORK/netcheckno-nc-sleep")" = sleep
    t "a probe pod with no nc and no sleep gives the row of nc" test "$(NETCHECK_ENFORCEMENT_WAIT=3 NETCHECK_TARGETS=10.0.0.1:80 NC_MODE=none PATH="$WORK/netcheckno-nc-sleep" STUB_DIR="$WORK/netcheckstub" sh "$CHART/files/network-check.sh" probe 2>&1 | grep -c '"target":"nc","kind":"precondition"')" -eq 1
}

check_network_check_readme_says_what_refused_means() {
    section "the README: what refused means, the shared check image, the steady state"
    t "the README says refused is any non-zero exit of nc and a drop is read as refused on purpose" grep -qF 'What `refused` means: `nc` exited with any non-zero status. That covers a reset, the timeout of a dropped probe (a drop is read as a refusal on purpose: the policy stops the traffic), a name that does not resolve and an `nc` that fails for another reason.' "$README"
    t "the README says a missing nc gets its own row and a broken nc is caught by the positive control only" grep -qF 'An `nc` that exists but is broken is caught only by the positive control: the Job pod runs the same image as the probe pods (`network_check.image`) and the same `nc` call' "$README"
    t "the README says the control and the probe pods share the check image" grep -qF 'The control and the probe pods share the check image, and that sharing is what the argument rests on' "$README"
    t "the README words the fail closed sentence for the steady state" grep -qF 'so, once the network plugin enforces the policies, a sandbox has no other path (fail closed in that steady state; a fresh pod is not covered during its first seconds, see The enforcement window).' "$README"
    t "the README says the rows of the modes table hold once the policies are enforced" grep -qF 'They hold for a pod once the network plugin enforces them; they say nothing about the first seconds of a fresh pod' "$README"
    t "the README words each network sentence of fail closed for the steady state" test "$(grep -o 'fail closed in that steady state' "$README" | grep -c .)" -eq 2
}
