#!/usr/bin/env bash
HERE=$(cd "$(dirname "$0")" && pwd)
. "$HERE/common.sh"
scenario_begin s04-network-modes

managed_namespaces() { k get ns -l "$LBL_MANAGED=true" -o json | jq -c '[.items[].metadata.name]'; }

targets_json() {
  local skip
  skip=$(managed_namespaces)
  {
    k get nodes -o json | jq -c '.items[].status.addresses[] | select(.type == "InternalIP") | .address as $a | (6443, 10250) | {label: "node", host: $a, port: .}'
    k get svc -A -o json | jq -c --argjson skip "$skip" '.items[]
      | select(.metadata.namespace as $n | $skip | index($n) | not)
      | select(.spec.clusterIP != null and .spec.clusterIP != "None")
      | .metadata as $m | .spec.clusterIP as $ip
      | .spec.ports[]? | select(.protocol == "TCP")
      | {label: "svc \($m.namespace)/\($m.name)", host: $ip, port: .port}'
    k get pods -A -o json | jq -c --argjson skip "$skip" '.items[]
      | select(.metadata.namespace as $n | $skip | index($n) | not)
      | select(.status.podIP != null)
      | .metadata as $m | .status.podIP as $ip
      | .spec.containers[].ports[]?
      | {label: "pod \($m.namespace)/\($m.name)", host: $ip, port: .containerPort}'
  } | jq -s -c '.[0:90]'
}

names_json() {
  k get svc -A -o json | jq -c '[.items[0:6][] | {name: "\(.metadata.name).\(.metadata.namespace).svc.cluster.local"}] + [{name: "kubernetes.default.svc.cluster.local"}]'
}

open_count() { jq -s '[.[] | select(.open)] | length'; }

default_ns=$(ns_of "$CLIENT_A" "$NET_NONE")
wait_warm "$default_ns" 1 120 "$POOL_ID_DEFAULT" || { check network-modes-warm-pod fail "no warm pod in $default_ns"; scenario_end; }
warm_name=$(warm_list "$default_ns" "$POOL_ID_DEFAULT" | head -1 | cut -d' ' -f1)
control_pod_to control "$TESTSVC_NS" "$(pod_image "$default_ns" "$warm_name")" || { check network-modes-control-pod fail "control pod not ready"; scenario_end; }

targets=$(targets_json)
names=$(names_json)
control_reached=$(probe_ctl "$TESTSVC_NS" "$control" tcp "$targets" | jq -c -s '[.[] | select(.open)]')
reached_total=$(jq 'length' <<< "$control_reached")
record network-modes-control-targets-reached "$reached_total" targets
verdict network-modes-control-reaches-targets "control pod outside the sandbox namespaces reaches $reached_total of $(jq 'length' <<< "$targets")" \
  test "$reached_total" -ge "$NETWORK_MIN_CONTROL_REACHED"

for net in "$NET_NONE" "$NET_ALLOW" "$NET_FULL"; do
  ns=$(ns_of "$CLIENT_A" "$net")
  wait_warm "$ns" 1 120 "$POOL_ID_DEFAULT" || { check "network-modes-$net-warm-pod" fail "no warm pod in $ns"; continue; }
  claim_to pod "$ns" "network-modes-$(new_id)" agent-a "$POOL_ID_DEFAULT"
  record "network-modes-$net-agent-uid" "$(sx "$ns" "$pod" id -u)" uid
  opened=$(probe "$ns" "$pod" tcp "$targets" | open_count)
  verdict "network-modes-$net-no-internal-target" "$opened of $(jq 'length' <<< "$targets") internal targets reached" test "$opened" = 0
  resolved=$(probe "$ns" "$pod" names "$names" | jq -s '[.[] | select(.resolved)] | length')
  verdict "network-modes-$net-no-svc-name" "$resolved cluster names resolved" test "$resolved" = 0
done

allow_ns=$(ns_of "$CLIENT_A" "$NET_ALLOW")
claim_to allow_pod "$allow_ns" "network-modes-$(new_id)" agent-a "$POOL_ID_DEFAULT"
proxy=$(proxy_hostport "$allow_ns" "$allow_pod")
api_ip=$(k -n default get svc kubernetes -o jsonpath='{.spec.clusterIP}')
node_ip=$(k get nodes -o jsonpath='{.items[0].status.addresses[?(@.type=="InternalIP")].address}')
connects=$(jq -n -c --arg p "$proxy" --arg git "$TEST_HOST_GIT:443" --arg api "$api_ip:443" --arg node "$node_ip:10250" '[
  {proxy: $p, target: $git, allowed: true},
  {proxy: $p, target: "example.com:443", allowed: false},
  {proxy: $p, target: "1.1.1.1:443", allowed: false},
  {proxy: $p, target: $api, allowed: false},
  {proxy: $p, target: $node, allowed: false},
  {proxy: $p, target: "169.254.169.254:80", allowed: false}]')
connect_report=$(probe "$allow_ns" "$allow_pod" connect "$connects")
wrong=$(jq -s '[.[] | select(if .allowed then .status != 200 else (.status != 403 and .status != 502) end)] | length' <<< "$connect_report")
verdict network-modes-allow-only-listed-site "listed site 200, every other target 403 or 502; wrong answers: $wrong" test "$wrong" = 0

run_check_job() {
  k -n "$HELM_NAMESPACE" delete job -l "$NETWORK_CHECK_JOB_SELECTOR" --wait=true > /dev/null 2>&1
  helm_ctx upgrade "$HELM_RELEASE" "$CHART_DIR" -n "$HELM_NAMESPACE" --reuse-values --set "$VALUES_NETWORK_CHECK_RUNID=$(date +%s)" > /dev/null || return 1
  k -n "$HELM_NAMESPACE" wait --for=condition=complete job -l "$NETWORK_CHECK_JOB_SELECTOR" --timeout=300s > /dev/null
}

if run_check_job; then
  report=$(k -n "$HELM_NAMESPACE" logs -l "$NETWORK_CHECK_JOB_SELECTOR" --tail=-1 | jq -R -c 'fromjson? | select(.target != null)' | jq -s -c .)
  mismatched=$(jq '[.[] | select(.observed != .expected)] | length' <<< "$report")
  unavailable=$(jq '[.[] | select(.observed == "control_unavailable")] | length' <<< "$report")
  record network-modes-job-probes "$(jq 'length' <<< "$report")" probes
  verdict network-modes-job-all-probes-as-expected "mismatched $mismatched, control_unavailable $unavailable" test "$mismatched" = 0 -a "$unavailable" = 0
else
  check network-modes-job-all-probes-as-expected fail "check job did not complete"
fi

scenario_end
