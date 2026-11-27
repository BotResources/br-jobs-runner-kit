#!/usr/bin/env bash

probe_images() {
    local file
    for file in "$(netcheck_dir "$1")"/probe-*.yaml; do
        yq -r '.spec.containers[0].image' "$file"
    done
}

image_values() {
    {
        probe_images "$1"
        jq -r '.[] | select(.kind == "Job") | .spec.template.spec.containers[0].image' "$WORK/$2.json"
        jq -r '.[] | select(.kind == "CronJob") | .spec.jobTemplate.spec.template.spec.containers[0].image' "$WORK/$2.json"
    }
}

distinct_images() {
    image_values "$1" "$2" | sort -u | grep -c .
}

check_network_check_job_and_probe_pods_run_the_same_image() {
    section "the network check: the Job, the CronJob and every probe pod run the same image, with and without the mirror"
    local probes
    render img_plain -f "$EXAMPLE" --set network_check.cron='0 * * * *'
    render_yaml img_plain_yaml -f "$EXAMPLE" --set network_check.cron='0 * * * *'
    render img_mirror -f "$EXAMPLE" -f "$MIRROR_VALUES"
    render_yaml img_mirror_yaml -f "$EXAMPLE" -f "$MIRROR_VALUES"
    probes="$(namespace_count img_plain_yaml)"
    t "there are probe pods to compare" test "$probes" -gt 0
    t "plain: the Job, the CronJob and each probe pod give $((probes + 2)) image values" test "$(image_values img_plain_yaml img_plain | grep -c .)" -eq $((probes + 2))
    t "plain: all of them are one image" test "$(distinct_images img_plain_yaml img_plain)" -eq 1
    t "plain: the image is network_check.image" test "$(image_values img_plain_yaml img_plain | sort -u)" = "$(yq -r '.network_check.image' "$EXAMPLE")"
    t "mirror: the Job, the CronJob and each probe pod give $((probes + 2)) image values" test "$(image_values img_mirror_yaml img_mirror | grep -c .)" -eq $((probes + 2))
    t "mirror: all of them are one image" test "$(distinct_images img_mirror_yaml img_mirror)" -eq 1
    t "mirror: the image goes through the mirror, so the mirror case is not the plain case" test "$(image_values img_mirror_yaml img_mirror | sort -u)" != "$(image_values img_plain_yaml img_plain | sort -u)"
    cp -R "$CHART" "$WORK/chartimgmut"
    sed -i.bak 's|"name" "probe" "image" \$image|"name" "probe" "image" (printf "%s-x" $image)|' "$WORK/chartimgmut/templates/_pod.tpl"
    rm "$WORK/chartimgmut/templates/_pod.tpl.bak"
    t "mutation: the chart with another image in the probe pod differs from the chart" test "$(cmp -s "$WORK/chartimgmut/templates/_pod.tpl" "$CHART/templates/_pod.tpl" && echo same || echo differ)" = differ
    helm template t "$WORK/chartimgmut" -f "$EXAMPLE" --set network_check.cron='0 * * * *' >"$WORK/img_mut_yaml.yaml" 2>"$WORK/img_mut.err"
    yq ea -o=json '[select(. != null)]' "$WORK/img_mut_yaml.yaml" >"$WORK/img_mut.json"
    t "mutation: with another image in the probe pod the images are not one image" test "$(distinct_images img_mut_yaml img_mut)" -eq 2
}

check_network_check_any_non_zero_exit_of_nc_is_refused() {
    section "the network check: nc exits with any non-zero status and the probe reads refused"
    stub_tools "$WORK/netcheckbin"
    render_yaml refused_yaml -f "$EXAMPLE" --set network_check.enforcement_wait_seconds=3
    awk '/^    if ! nc -w 2 / {print "    rc=0"; print "    nc -w 2 \"${1%:*}\" \"${1##*:}\" </dev/null >/dev/null 2>&1 || rc=$?"; print "    if [ \"$rc\" -eq 1 ]; then"; next} {print}' "$CHART/files/network-check.sh" >"$WORK/network-check-status-one.sh"
    t "mutation: the script that counts only exit status 1 as refused differs from the chart script" test "$(cmp -s "$WORK/network-check-status-one.sh" "$CHART/files/network-check.sh" && echo same || echo differ)" = differ
    local status out rc total
    for status in 1 2 127 255; do
        rc=0
        out="$(NETCHECK_SOURCE=refused_yaml run_netcheck env NC_MODE=all PROBE_MODE=none NC_REFUSE_STATUS="$status" 2>&1)" || rc=$?
        total="$(grep -c '"kind":"tcp"' <<<"$out")"
        t "nc status ${status}: there are probes" test "$total" -gt 0
        t "nc status ${status}: the Job passes" test "$rc" -eq 0
        t "nc status ${status}: every probe reads refused at once" test "$(grep -c '"kind":"tcp".*"observed":"refused".*"refused_after_seconds":0,"limit_seconds":3' <<<"$out")" -eq "$total"
        t "nc status ${status}: no wait" test "$(sleeps)" -eq 0
    done
    rc=0
    out="$(NETCHECK_PROBE_SCRIPT="$WORK/network-check-status-one.sh" NETCHECK_SOURCE=refused_yaml run_netcheck env NC_MODE=all PROBE_MODE=none NC_REFUSE_STATUS=2 2>&1)" || rc=$?
    t "mutation: with only status 1 as refused, a status 2 fails the Job" test "$rc" -ne 0
    t "mutation: with only status 1 as refused, a status 2 reads reached" grep -q '"kind":"tcp".*"observed":"reached"' <<<"$out"
    rc=0
    out="$(NETCHECK_PROBE_SCRIPT="$WORK/network-check-status-one.sh" NETCHECK_SOURCE=refused_yaml run_netcheck env NC_MODE=all PROBE_MODE=none NC_REFUSE_STATUS=1 2>&1)" || rc=$?
    t "mutation: with only status 1 as refused, a status 1 still passes" test "$rc" -eq 0
}

check_network_check_one_budget_for_the_whole_probe_pod() {
    section "the network check: the limit is one budget for the probe pod, a later target gets what is left, and each target is tried at least once"
    stub_tools "$WORK/netcheckbin"
    render_yaml budget_yaml -f "$EXAMPLE" --set network_check.enforcement_wait_seconds=3
    local count first second out
    count="$(namespace_count ex_yaml)"
    first="$(sed -n 1p "$(netcheck_dir ex_yaml)/extra-targets")"
    second="$(sed -n 2p "$(netcheck_dir ex_yaml)/extra-targets")"
    t "there are two listed targets to compare" test -n "$first" -a -n "$second" -a "$first" != "$second"
    NETCHECK_SOURCE=budget_yaml run_netcheck env NC_MODE=all PROBE_MODE=none PROBE_LATE_K=100000000 >/dev/null 2>&1 || true
    t "never applied: the first target of each pod is tried at 0, 1, 2 and 3 seconds, and once by the control" test "$(grep -cxF "$first" "$WORK/netcheckstub/nc.calls")" -eq $((count * 4 + 1))
    t "never applied: the second target of each pod gets what is left, which is one try, and once by the control" test "$(grep -cxF "$second" "$WORK/netcheckstub/nc.calls")" -eq $((count + 1))
    t "never applied: the pod waited the limit once" test "$(sleeps)" -eq $((count * 3))
    awk '{print} /^until_refused\(\) \{$/ {print "  started=$(date +%s)"}' "$CHART/files/network-check.sh" >"$WORK/network-check-budget-each.sh"
    t "mutation: the script with a budget for each target differs from the chart script" test "$(cmp -s "$WORK/network-check-budget-each.sh" "$CHART/files/network-check.sh" && echo same || echo differ)" = differ
    out="$(NETCHECK_PROBE_SCRIPT="$WORK/network-check-budget-each.sh" NETCHECK_SOURCE=budget_yaml run_netcheck env NC_MODE=all PROBE_MODE=none PROBE_LATE_K=100000000 2>&1 || true)"
    t "mutation: with a budget for each target the second target is tried as often as the first" test "$(grep -cxF "$second" "$WORK/netcheckstub/nc.calls")" -eq $((count * 4 + 1))
    t "the README says the limit is one budget for the pod, and what a later target gets" grep -qF 'the limit is one budget for the pod, a later target gets what is left of it, and once it is spent each remaining target is tried once' "$README"
    t "the README values table says the same, one budget for all the probes of the pod" grep -qF 'is one time budget for all the direct TCP probes of a probe pod, counted from the start of the probe script in that pod: a later target gets what is left of it, and once it is spent each remaining target is tried once' "$README"
}

check_network_check_readme_says_what_the_elapsed_time_measures() {
    section "the README: what refused_after_seconds measures, and which programs have a precondition row"
    t "the README says the value runs from the start of the probe script to the first refusal and is not the length of the window" grep -qF '`refused_after_seconds` is the time from the start of the probe script to the first refusal of that probe, it is not a measure of the length of the window (it carries the `nc -w 2` waits of the probes that were dropped before it), and the report does not tell the length of the window.' "$README"
    t "the README no longer says that the Job reports the length of the window" test "$(grep -c 'reports its length' "$README")" -eq 0
    t "the CHANGELOG no longer says that the Job reports the length of the window" test "$(grep -c 'reports its length' "$CHART/CHANGELOG.md")" -eq 0
    t "the README names the three programs of the precondition rows, in order" grep -qF 'runs `command -v` for `nc`, `sleep` and `date`, in that order, and for the first one that is not found prints one `precondition` object (`target` the name of the program, `observed` `missing`)' "$README"
    t "the CHANGELOG names the three programs of the precondition rows" grep -qF 'a missing `nc`, `sleep` or `date` (the first one missing, in that order; `target` is its name)' "$CHART/CHANGELOG.md"
}

nc_calls_of() {
    awk -v role="$1" -v host="${3%:*}" -v port="${3##*:}" '$1 == role && $(NF - 1) == host && $NF == port { $1 = ""; print }' "$2" | sort -u
}

check_network_check_job_and_probe_pods_make_the_same_nc_call() {
    section "the network check: the control of the Job pod and the probe of a probe pod call nc with the same arguments"
    stub_tools "$WORK/netcheckbin"
    render_yaml samecall_yaml -f "$EXAMPLE" --set network_check.enforcement_wait_seconds=3
    local first control probe args="$WORK/netcheckstub/nc.args"
    first="$(sed -n 1p "$(netcheck_dir samecall_yaml)/extra-targets")"
    NETCHECK_SOURCE=samecall_yaml run_netcheck env NC_MODE=all PROBE_MODE=none >/dev/null 2>&1 || true
    control="$(nc_calls_of control "$args" "$first")"
    probe="$(nc_calls_of probe "$args" "$first")"
    t "the control and the probe each call nc for the first target" test -n "$control" -a -n "$probe"
    t "the control makes one form of call and the probe makes one form of call" test "$(grep -c . <<<"$control")" -eq 1 -a "$(grep -c . <<<"$probe")" -eq 1
    t "the control call is nc -w 2 with the host and the port, and the probe call is the same" test "$control" = " -w 2 ${first%:*} ${first##*:}" -a "$probe" = "$control"
    sed '/^control() {$/,/^}$/s/nc -w 2 /nc -w 7 /' "$CHART/files/network-check.sh" >"$WORK/network-check-control-w7.sh"
    t "mutation: the script with another wait in the control call differs from the chart script" test "$(cmp -s "$WORK/network-check-control-w7.sh" "$CHART/files/network-check.sh" && echo same || echo differ)" = differ
    NETCHECK_SCRIPT="$WORK/network-check-control-w7.sh" NETCHECK_SOURCE=samecall_yaml run_netcheck env NC_MODE=all PROBE_MODE=none >/dev/null 2>&1 || true
    t "mutation: with another wait in the control call the control call is not the probe call" test "$(nc_calls_of control "$args" "$first")" = " -w 7 ${first%:*} ${first##*:}" -a "$(nc_calls_of probe "$args" "$first")" = "$probe"
    sed '/^until_refused() {$/,/^}$/s/nc -w 2 /nc -w 7 /' "$CHART/files/network-check.sh" >"$WORK/network-check-probe-w7.sh"
    t "mutation: the script with another wait in the probe call differs from the chart script" test "$(cmp -s "$WORK/network-check-probe-w7.sh" "$CHART/files/network-check.sh" && echo same || echo differ)" = differ
    NETCHECK_PROBE_SCRIPT="$WORK/network-check-probe-w7.sh" NETCHECK_SOURCE=samecall_yaml run_netcheck env NC_MODE=all PROBE_MODE=none >/dev/null 2>&1 || true
    t "mutation: with another wait in the probe call the probe call is not the control call" test "$(nc_calls_of probe "$args" "$first")" = " -w 7 ${first%:*} ${first##*:}" -a "$(nc_calls_of control "$args" "$first")" = "$control"
}
