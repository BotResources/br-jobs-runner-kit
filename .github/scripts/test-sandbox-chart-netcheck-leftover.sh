#!/usr/bin/env bash

event_line() {
    grep -nxF -- "$1" "$WORK/netcheckstub/events.log" | head -1 | cut -d: -f1
}

clear_command() {
    printf 'delete pod/br-sandbox-netcheck-probe -n %s --ignore-not-found --wait=true --timeout=60s' "$1"
}

cleared_before_created() {
    local namespace="$1" cleared created
    cleared="$(event_line "$(clear_command "$namespace")")"
    created="$(event_line "create ${namespace}")"
    [[ -n "$cleared" && -n "$created" && "$cleared" -lt "$created" ]]
}

all_cleared_before_created() {
    local namespace
    for namespace in $1; do
        cleared_before_created "$namespace" || return 1
    done
}

created_count() {
    grep -c '^create ' "$WORK/netcheckstub/events.log" || true
}

probe_pod_rows() {
    grep -c '"kind":"probe_pod"' <<<"$1" || true
}

check_network_check_clears_a_leftover_probe_pod_before_it_creates_one() {
    section "the network check: a probe pod that an earlier Job left is deleted, and waited for, before the probe pod is created"
    stub_tools "$WORK/netcheckbin"
    local all first count out rc
    all="$(awk 'NF {print $1}' "$(netcheck_dir ex_yaml)/namespaces" | tr '\n' ' ')"
    first="$(awk 'NF {print $1; exit}' "$(netcheck_dir ex_yaml)/namespaces")"
    count="$(namespace_count ex_yaml)"
    t "there are namespaces to probe" test "$count" -gt 1
    rc=0
    out="$(run_netcheck env NC_MODE=all PROBE_MODE=none 2>&1)" || rc=$?
    t "no leftover: the Job passes and makes one probe pod in each namespace" test "$rc" -eq 0 -a "$(created_count)" -eq "$count"
    t "no leftover: the clearing delete, with --wait=true and a timeout, comes before the create in each namespace" all_cleared_before_created "$all"
    t "no leftover: the probe pod is still deleted at the end, with no wait, in each namespace" test "$(grep -c -e '^delete pod/br-sandbox-netcheck-probe -n .* --wait=false$' "$WORK/netcheckstub/events.log")" -eq "$count"
    t "no leftover: no probe_pod object is reported" test "$(probe_pod_rows "$out")" -eq 0
    rc=0
    out="$(run_netcheck env NC_MODE=all PROBE_MODE=none LEFTOVER_NS="$all" 2>&1)" || rc=$?
    t "a leftover in each namespace: the Job passes" test "$rc" -eq 0
    t "a leftover in each namespace: it is deleted and waited for before the create, in each namespace" all_cleared_before_created "$all"
    t "a leftover in each namespace: the probe runs in each namespace" test "$(created_count)" -eq "$count" -a "$(grep -c '"kind":"tcp"' <<<"$out")" -ge "$count"
    t "a leftover in each namespace: no AlreadyExists reaches the output" test "$(grep -c 'AlreadyExists' <<<"$out")" -eq 0
    awk '/^    clear_leftover_probe_pod "\$ns" "\$mode" \|\| continue$/ {next} {print}' "$CHART/files/network-check.sh" >"$WORK/network-check-noclear.sh"
    t "mutation: the script with no clearing delete differs from the chart script" test "$(cmp -s "$WORK/network-check-noclear.sh" "$CHART/files/network-check.sh" && echo same || echo differ)" = differ
    rc=0
    out="$(NETCHECK_SCRIPT="$WORK/network-check-noclear.sh" run_netcheck env NC_MODE=all PROBE_MODE=none LEFTOVER_NS="$all" 2>&1)" || rc=$?
    t "mutation: with no clearing delete a leftover fails the Job on AlreadyExists" test "$rc" -ne 0 -a "$(grep -c 'AlreadyExists' <<<"$out")" -gt 0
    sed 's/--ignore-not-found --wait=true --timeout=60s/--ignore-not-found --wait=false --timeout=60s/' "$CHART/files/network-check.sh" >"$WORK/network-check-nowaitclear.sh"
    t "mutation: the script whose clearing delete does not wait differs from the chart script" test "$(cmp -s "$WORK/network-check-nowaitclear.sh" "$CHART/files/network-check.sh" && echo same || echo differ)" = differ
    rc=0
    out="$(NETCHECK_SCRIPT="$WORK/network-check-nowaitclear.sh" run_netcheck env NC_MODE=all PROBE_MODE=none LEFTOVER_NS="$all" 2>&1)" || rc=$?
    t "mutation: a clearing delete that does not wait leaves the leftover, and the Job fails" test "$rc" -ne 0 -a "$(grep -c 'AlreadyExists' <<<"$out")" -gt 0
}

check_network_check_fails_on_a_leftover_probe_pod_that_cannot_be_deleted() {
    section "the network check: a leftover probe pod that cannot be deleted fails the Job with a named object, and no probe runs in that namespace"
    stub_tools "$WORK/netcheckbin"
    local all first count out rc
    all="$(awk 'NF {print $1}' "$(netcheck_dir ex_yaml)/namespaces" | tr '\n' ' ')"
    first="$(awk 'NF {print $1; exit}' "$(netcheck_dir ex_yaml)/namespaces")"
    count="$(namespace_count ex_yaml)"
    rc=0
    out="$(run_netcheck env NC_MODE=all PROBE_MODE=none LEFTOVER_NS="$all" STUCK_NS="$first" 2>&1)" || rc=$?
    t "stuck leftover: the Job fails" test "$rc" -ne 0
    t "stuck leftover: one probe_pod object, for that namespace, with the fields of the report" test "$(probe_pod_rows "$out")" -eq 1 -a "$(grep -c "\"net\":\"${first}\",\"mode\":\"[a-z]*\",\"target\":\"br-sandbox-netcheck-probe\",\"kind\":\"probe_pod\",\"expected\":\"absent\",\"observed\":\"not_deleted\",\"control\":\"not_applicable\"}" <<<"$out")" -eq 1
    t "stuck leftover: the object parses as JSON" test "$(grep '"kind":"probe_pod"' <<<"$out" | jq -r '.net')" = "$first"
    t "stuck leftover: no probe pod is created in that namespace" test "$(grep -cxF "create ${first}" "$WORK/netcheckstub/events.log")" -eq 0
    t "stuck leftover: no probe result is reported for that namespace" test "$(grep -cE "\"net\":\"${first}\".*\"kind\":\"(tcp|connect)\"" <<<"$out")" -eq 0
    t "stuck leftover: the other namespaces are probed" test "$(created_count)" -eq $((count - 1))
    t "stuck leftover: the error of kubectl is in the output" test "$(grep -c 'timed out waiting' <<<"$out")" -ge 1
    awk '/^  if ! kubectl delete / {sub(/^  if ! /, "  "); sub(/; then$/, " || true"); print; print "  if false; then"; next} {print}' "$CHART/files/network-check.sh" >"$WORK/network-check-tolerant.sh"
    t "mutation: the script that tolerates a failed clearing delete differs from the chart script" test "$(cmp -s "$WORK/network-check-tolerant.sh" "$CHART/files/network-check.sh" && echo same || echo differ)" = differ
    rc=0
    out="$(NETCHECK_SCRIPT="$WORK/network-check-tolerant.sh" run_netcheck env NC_MODE=all PROBE_MODE=none LEFTOVER_NS="$all" STUCK_NS="$first" 2>&1)" || rc=$?
    t "mutation: a tolerated failure gives no probe_pod object" test "$(probe_pod_rows "$out")" -eq 0
    t "mutation: a tolerated failure does not end as a verdict for the namespace: the Job still fails, with no object that names the cause" test "$rc" -ne 0 -a "$(grep -c "\"net\":\"${first}\".*\"kind\":\"tcp\"" <<<"$out")" -eq 0
    t "the README says the Job clears a leftover probe pod and waits, and what it reports when it cannot" grep -qF 'Before the Job creates the probe pod of a namespace it deletes a pod of the same name, `br-sandbox-netcheck-probe`, that an earlier Job left there (a Job that was killed does not delete its probe pod), and waits for it to be gone, for at most 60 seconds; a leftover pod that the delete does not remove (the delete command fails for any cause, the wait of 60 seconds ending without the pod gone being one) gives one `probe_pod` object for that namespace (`expected` `absent`, `observed` `not_deleted`), the Job makes no probe in it and fails.' "$README"
    t "the README no longer says that only a pod that cannot be deleted in that time gives the probe_pod object" test "$(grep -c 'cannot be deleted in that time' "$DOCS")" -eq 0
    t "the CHANGELOG no longer says that only a pod that cannot be deleted gives the probe_pod object" test "$(grep -c 'A leftover pod that cannot be deleted' "$CHART/CHANGELOG.md")" -eq 0
    t "the README says the Role of the check identity is not changed" grep -qF 'The Role of the check identity is not changed: it already holds `delete` on pods in the sandbox namespaces.' "$README"
    t "the CHANGELOG says the same fix under 0.2.4" test "$(awk '/^## 0.2.4$/ {on=1; next} /^## / {on=0} on' "$CHART/CHANGELOG.md" | grep -cF -e 'A leftover pod that the delete does not remove (the delete command fails for any cause) gives one object with `kind` `probe_pod`')" -eq 1
    t "the Role of the check identity holds delete on pods in each sandbox namespace" jqt keeperegress '[.[] | select(.kind == "Role" and .metadata.name == "br-sandbox-netcheck") | .rules[] | select(.resources == ["pods"]) | .verbs | index("delete")] | all(. != null) and length > 0'
}
