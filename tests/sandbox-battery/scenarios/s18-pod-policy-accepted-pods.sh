#!/usr/bin/env bash
HERE=$(cd "$(dirname "$0")" && pwd)
. "$HERE/common.sh"
scenario_begin s18-pod-policy-accepted-pods

probe_by_check_identity() { as_id "$NETCHECK_ID" "$1" create --dry-run=server -f - <<< "$(probe_pod_of "$1")"; }
probe_by_runner() { as_id "$RUNNER_ID" "$1" create --dry-run=server -f - <<< "$(probe_pod_of "$1")"; }

probe_pod_of() {
  k -n "$SYSTEM_NS" get cm "$NETCHECK_CM" -o jsonpath="{.data.probe-$1\\.yaml}" \
    | sed "s|name: br-sandbox-netcheck-probe|name: adm-$(new_id)|"
}

for mode in "$NET_NONE" "$NET_ALLOW" "$NET_FULL"; do
  ns=$(ns_of "$CLIENT_A" "$mode")
  if ! wait_warm "$ns" 1 120; then
    check "policy-accepts-pool-pod-mode-$mode" fail "no warm pod in $ns"
    check "policy-accepts-probe-pod-mode-$mode" fail "not run: no warm pod in $ns"
    continue
  fi
  POLICY_NS=$ns
  POLICY_TEMPLATE=$(pod_template "$ns")
  verdict "policy-accepts-pool-pod-mode-$mode" "pod made from the pool pod of $ns (mode $mode) is accepted for the runner" \
    accepted policy_submit "$(variant "$POLICY_TEMPLATE" '.')"
  verdict "policy-accepts-probe-pod-mode-$mode" "probe pod of the network check for $ns, from the chart ConfigMap $NETCHECK_CM, is accepted for $NETCHECK_ID (uid 0 by identity)" \
    accepted probe_by_check_identity "$ns"
done

ns=$(ns_of "$CLIENT_A" "$NET_ALLOW")
if wait_warm "$ns" 1 120; then
  POLICY_NS=$ns
  POLICY_TEMPLATE=$(pod_template "$ns")
  verdict policy-pool-pod-has-seed-container "pool pod of $ns holds a container named $SEED_CONTAINER (uid 0, read-only root)" \
    test "$(jq -r --arg seed "$SEED_CONTAINER" '[.spec.containers[] | select(.name == $seed and .securityContext.runAsUser == 0 and .securityContext.readOnlyRootFilesystem == true)] | length' <<< "$POLICY_TEMPLATE")" = 1
  verdict policy-accepts-pod-with-seed-container "pod with the seed container is accepted for the runner" \
    accepted policy_submit "$(variant "$POLICY_TEMPLATE" '.')"
  verdict policy-refuses-probe-pod-for-runner "probe pod of the network check made by $RUNNER_ID is refused with BR_SANDBOX_RUN_AS_NON_ROOT: only the identity of the check may run it" \
    refused_with RUN_AS_NON_ROOT probe_by_runner "$ns"
else
  check policy-accepts-pod-with-seed-container fail "no warm pod in $ns"
fi

scenario_end
