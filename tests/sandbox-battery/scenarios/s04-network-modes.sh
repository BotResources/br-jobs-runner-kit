#!/usr/bin/env bash
HERE=$(cd "$(dirname "$0")" && pwd)
. "$HERE/common.sh"
scenario_begin s04-network-modes

targets_json() {
  local skip
  skip=$(chart_namespaces)
  {
    k get nodes -o json | jq -c '.items[].status.addresses[] | select(.type == "InternalIP") | .address as $a | (6443, 10250) | {label: "node", host: $a, port: .}'
    k get svc -A -o json | jq -c --argjson skip "$skip" '.items[]
      | select(.metadata.namespace as $n | $skip | index($n) | not)
      | select(.spec.clusterIP != null and .spec.clusterIP != "None")
      | .metadata as $m | .spec.clusterIP as $ip
      | .spec.ports[]? | select(.protocol == "TCP")
      | {label: "svc \($m.namespace)/\($m.name)", host: $ip, port: .port}'
    k get pods -A -o json | jq -c --argjson skip "$skip" --arg role "$LBL_ROLE" --arg proxy "$ROLE_PROXY" '.items[]
      | select(.metadata.namespace as $n | $skip | index($n) | not)
      | select(((.metadata.labels // {})[$role]) != $proxy)
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

tunnel_targets() {
  local ip
  for ip in $1; do printf '{"host":"%s","port":%s}\n' "$ip" "$PROXY_PORT"; done | jq -s -c .
}

tunnel_json() {
  {
    k get pods -A -l "$LBL_ROLE=$ROLE_PROXY" -o json | jq -c --arg port "$PROXY_PORT" \
      '.items[] | select(.status.podIP != null) | {net: (.metadata.namespace | sub("-egress$"; "")), target: "\(.status.podIP):\($port)"}'
    k get svc -A -l "app.kubernetes.io/part-of=br-sandbox" -o json | jq -c --arg name "$PROXY_SERVICE" \
      '.items[] | select(.metadata.name == $name) | {net: (.metadata.namespace | sub("-egress$"; "")), target: "\(.spec.clusterIP):\(.spec.ports[0].port)"}'
  } | jq -s -c .
}

default_ns=$(ns_of "$CLIENT_A" "$NET_NONE")
wait_warm "$default_ns" 1 120 || { check network-modes-warm-pod fail "no warm pod in $default_ns"; scenario_end; }
warm_name=$(warm_list "$default_ns" | head -1 | cut -d' ' -f1)
control_pod_to control "$TESTSVC_NS" "$(pod_image "$default_ns" "$warm_name")" || { check network-modes-control-pod fail "control pod not ready"; scenario_end; }

targets=$(targets_json)
names=$(names_json)
control_reached=$(probe_ctl "$TESTSVC_NS" "$control" tcp "$targets" | jq -c -s '[.[] | select(.open)]')
reached_total=$(jq 'length' <<< "$control_reached")
record network-modes-control-targets-reached "$reached_total" targets
verdict network-modes-control-reaches-targets "control pod outside the sandbox namespaces reaches $reached_total of $(jq 'length' <<< "$targets")" \
  test "$reached_total" -ge "$NETWORK_MIN_CONTROL_REACHED"

have_proxy=0
if need_proxy network-modes "$CLIENT_A" "$NET_ALLOW"; then have_proxy=1; fi
proxy_ips=$(proxy_pods "$CLIENT_A" "$NET_ALLOW")
tunnel=$(tunnel_targets "$proxy_ips")

for net in "$NET_NONE" "$NET_ALLOW" "$NET_FULL"; do
  ns=$(ns_of "$CLIENT_A" "$net")
  wait_warm "$ns" 1 120 || { check "network-modes-$net-warm-pod" fail "no warm pod in $ns"; continue; }
  claim_to pod "$ns" "network-modes-$(new_id)" agent-a
  record "network-modes-$net-agent-uid" "$(sx "$ns" "$pod" id -u)" uid
  opened=$(probe "$ns" "$pod" tcp "$targets" | open_count)
  verdict "network-modes-$net-no-internal-target" "$opened of $(jq 'length' <<< "$targets") internal targets reached" test "$opened" = 0
  resolved=$(probe "$ns" "$pod" names "$names" | jq -s '[.[] | select(.resolved)] | length')
  verdict "network-modes-$net-no-svc-name" "$resolved cluster names resolved" test "$resolved" = 0
  if [ "$have_proxy" = 1 ] && [ "$net" != "$NET_ALLOW" ]; then
    tunnel_open=$(probe "$ns" "$pod" tcp "$tunnel" | open_count)
    verdict "network-modes-$net-tunnel-of-allow-closed" "$tunnel_open proxy tunnel ports of $(egress_ns_of "$CLIENT_A" "$NET_ALLOW") reached" test "$tunnel_open" = 0
  fi
done

if [ "$have_proxy" = 1 ]; then
  allow_ns=$(ns_of "$CLIENT_A" "$NET_ALLOW")
  claim_to allow_pod "$allow_ns" "network-modes-$(new_id)" agent-a
  tunnel_open=$(probe "$allow_ns" "$allow_pod" tcp "$tunnel" | open_count)
  verdict network-modes-allow-tunnel-port-answers "$tunnel_open of $(jq 'length' <<< "$tunnel") proxy tunnel ports answer from the sandbox" \
    test "$tunnel_open" = "$(jq 'length' <<< "$tunnel")"
  cluster_tunnel=$(tunnel_targets "$(proxy_cluster_ip "$CLIENT_A" "$NET_ALLOW")")
  verdict network-modes-allow-cluster-ip-tunnel-answers "tunnel port at the ClusterIP in HTTPS_PROXY answers: $(probe "$allow_ns" "$allow_pod" tcp "$cluster_tunnel" | open_count)" \
    test "$(probe "$allow_ns" "$allow_pod" tcp "$cluster_tunnel" | open_count)" = 1
  others=$(for ip in $proxy_ips; do for port in $PROXY_LOOPBACK_PORTS 80 443; do printf '{"host":"%s","port":%s}\n' "$ip" "$port"; done; done | jq -s -c .)
  other_open=$(probe "$allow_ns" "$allow_pod" tcp "$others" | open_count)
  verdict network-modes-allow-other-proxy-ports-closed "$other_open of $(jq 'length' <<< "$others") other proxy ports answer" test "$other_open" = 0

  proxy=$(proxy_endpoint "$CLIENT_A" "$NET_ALLOW")
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
  verdict network-modes-allow-only-listed-site "listed site 200, every other target 403 or 502; wrong answers: $wrong; internal_cidrs=$(internal_cidrs_value)" test "$wrong" = 0
fi

job_succeeded() { k -n "$SYSTEM_NS" get job "$1" -o jsonpath='{.status.succeeded}' 2> /dev/null; }
job_failed() { k -n "$SYSTEM_NS" get job "$1" -o jsonpath='{.status.failed}' 2> /dev/null; }

release_claimed() {
  local entry end
  for entry in "${TRACKED_PODS[@]:-}"; do
    if [ -n "$entry" ]; then release_pod ${entry}; fi
  done
  end=$(($(date +%s) + 120))
  for entry in "${TRACKED_PODS[@]:-}"; do
    if [ -z "$entry" ]; then continue; fi
    until ! k -n "${entry% *}" get pod "${entry#* }" > /dev/null 2>&1; do
      if [ "$(date +%s)" -ge "$end" ]; then return 1; fi
      sleep 1
    done
  done
  TRACKED_PODS=()
}

run_check_job() {
  local run end
  release_claimed || return 1
  run=$(date +%s)
  CHECK_JOB="$NETCHECK_JOB_PREFIX$run"
  helm_ctx upgrade "$HELM_RELEASE" "$CHART_DIR" -n "$HELM_NAMESPACE" --reuse-values --set-string "$VALUES_NETWORK_CHECK_RUNID=$run" > /dev/null || return 1
  end=$(($(date +%s) + 600))
  until [ -n "$(job_succeeded "$CHECK_JOB")$(job_failed "$CHECK_JOB")" ]; do
    if [ "$(date +%s)" -ge "$end" ]; then return 1; fi
    sleep 3
  done
  [ "$(job_succeeded "$CHECK_JOB")" = 1 ]
}

if run_check_job; then
  report=$(k -n "$SYSTEM_NS" logs -l "job-name=$CHECK_JOB" --tail=-1 | jq -R -c 'fromjson? | select(.target != null)' | jq -s -c .)
  tunnel_path=$(tunnel_json)
  allowed='(. as $r | $tunnel | any(.net == $r.net and .target == $r.target and $r.observed == "reached"))'
  mismatched=$(jq --argjson tunnel "$tunnel_path" "[.[] | select(.observed != .expected) | select($allowed | not)] | length" <<< "$report")
  tunnel_seen=$(jq --argjson tunnel "$tunnel_path" "[.[] | select(.observed != .expected) | select($allowed)] | length" <<< "$report")
  unavailable=$(jq '[.[] | select(.control == "control_unavailable")] | length' <<< "$report")
  record network-modes-job-probes "$(jq 'length' <<< "$report")" probes
  record network-modes-job-tunnel-probes-reached "$tunnel_seen" probes
  probe_count=$(jq 'length' <<< "$report")
  detail=$(jq -c '[.[] | select(.observed != .expected or .control == "control_unavailable") | "\(.net) \(.kind) \(.target) \(.observed) \(.control)"] | .[0:12]' <<< "$report")
  verdict network-modes-job-all-probes-as-expected "mismatched $mismatched (the tunnel port of an own proxy is not counted), control_unavailable $unavailable, tunnel probes reached $tunnel_seen; first rows: $detail; internal_cidrs=$(internal_cidrs_value)" \
    test "$mismatched" = 0 -a "$unavailable" = 0 -a "$probe_count" -gt 0
else
  check network-modes-job-all-probes-as-expected fail "check job $CHECK_JOB did not succeed: succeeded=$(job_succeeded "$CHECK_JOB") failed=$(job_failed "$CHECK_JOB")"
fi

scenario_end
