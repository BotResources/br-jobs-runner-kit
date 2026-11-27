#!/usr/bin/env bash
HERE=$(cd "$(dirname "$0")" && pwd)
. "$HERE/common.sh"
scenario_begin s08-internal-targets

. "$WORK/servers.env"

ICIDRS=$(internal_cidrs_value)
NODE_FIRST=$(k get nodes -o jsonpath='{.items[0].status.addresses[?(@.type=="InternalIP")].address}')
NODE_SECOND=$(docker inspect "$NODE" --format "{{with index .NetworkSettings.Networks \"$SERVERS_NETWORK\"}}{{.IPAddress}}{{end}}")
REC_IP=$(k -n "$TESTSVC_NS" get pod "$REC_POD" -o jsonpath='{.status.podIP}')
EVID_FILE="$EVIDENCE/internal-targets.tsv"

hosts_ports() {
  local host port
  for host in $1; do
    for port in $2; do printf '{"host":"%s","port":%s}\n' "$host" "$port"; done
  done | jq -s -c .
}

direct_targets() {
  { hosts_ports "$PROBE_IP $REC_IP $REC_CLUSTER_IP $NODE_FIRST $NODE_SECOND" "$REC_PORTS"
    hosts_ports "$NODE_FIRST $NODE_SECOND" "10250 6443"; } | jq -s -c 'add'
}

via_targets() {
  local proxy=$1
  jq -n -c --arg p "$proxy" --arg probe "$PROBE_HOST" --arg pod "$POD_TARGET_NAME" --arg svc "$SVC_TARGET_NAME" \
    --arg node "$NODE_TARGET_NAME" --arg first "$NODEFIRST_TARGET_NAME" --arg ip "$PROBE_IP" --arg echo "$ECHO_HOST" --arg second "$NODE_SECOND" '[
    {proxy: $p, target: ($echo + ":443"), tls: true, sni: $echo, class: "control-echo"},
    {proxy: $p, target: ($probe + ":443"), tls: true, sni: $probe, class: "probe-name-tls"},
    {proxy: $p, target: ($probe + ":80"), class: "probe-name-plain"},
    {proxy: $p, target: ($probe + ":8080"), class: "probe-name-plain"},
    {proxy: $p, target: ($ip + ":80"), class: "probe-ip-plain"},
    {proxy: $p, target: ($ip + ":8080"), class: "probe-ip-plain"},
    {proxy: $p, target: ($ip + ":443"), tls: true, class: "probe-ip-tls"},
    {proxy: $p, target: ($probe + ":8080"), absolute: true, class: "probe-absolute"},
    {proxy: $p, target: ($pod + ":443"), tls: true, sni: $pod, class: "pod-name-tls"},
    {proxy: $p, target: ($svc + ":443"), tls: true, sni: $svc, class: "service-name-tls"},
    {proxy: $p, target: ($node + ":443"), tls: true, sni: $node, class: "node-second-name-tls"},
    {proxy: $p, target: ($first + ":443"), tls: true, sni: $first, class: "node-first-name-tls"},
    {proxy: $p, target: ($second + ":10250"), class: "node-second-kubelet-ip"},
    {proxy: $p, target: ($second + ":6443"), class: "node-second-api-ip"},
    {proxy: $p, target: ($node + ":10250"), tls: true, sni: $node, class: "node-second-kubelet-name"},
    {proxy: $p, target: ($node + ":6443"), tls: true, sni: $node, class: "node-second-api-name"}]'
}

recorder_counts() { echo "$(conn_docker "$PROBE_CONTAINER") $(conn_pod "$REC_POD") $(conn_pod "$REC_NODE_POD")"; }

run_row() {
  local client=$1 net=$2 ns rp before after direct_open proxy report refused_bad row
  ns=$(ns_of "$client" "$net")
  row="$client-$net"
  wait_warm "$ns" 1 120 || { check "internal-$row-warm-pod" fail "no warm pod in $ns"; return; }
  root_pod_to rp "$ns" || { check "internal-$row-root-pod" fail "root pod not ready in $ns"; return; }
  verdict "internal-$row-root" "pod $rp in $ns runs as uid $(sx "$ns" "$rp" id -u); internal_cidrs=$ICIDRS" test "$(sx "$ns" "$rp" id -u)" = 0
  before=$(recorder_counts)
  direct_open=$(probe "$ns" "$rp" tcp "$(direct_targets)" | jq -s -c '[.[] | select(.open) | "\(.host):\(.port)"]')
  verdict "internal-$row-direct-refused" "direct tcp to probe, recorders, both node addresses (ports $REC_PORTS 10250 6443): open $direct_open; internal_cidrs=$ICIDRS" test "$direct_open" = "[]"
  proxy=$(proxy_endpoint "$client" "$net")
  if [ -z "$proxy" ] && [ "$net" = "$NET_FULL" ]; then
    verdict "internal-$row-direct-path-control" "mode full without credential set reaches the echo server directly: $(probe "$ns" "$rp" tcp "$(hosts_ports "$ECHO_IP" 443)" | jq -c '.open')" \
      test "$(probe "$ns" "$rp" tcp "$(hosts_ports "$ECHO_IP" 443)" | jq -r '.open')" = true
  fi
  if [ -n "$proxy" ]; then
    report=$(probe "$ns" "$rp" via "$(via_targets "$proxy")" | jq -s -c .)
    sleep 3
    printf '%s\n' "$report" | jq -r --arg row "$row" '.[] | [$row, .class, .target, .status, .inner] | @tsv' >> "$EVID_FILE"
    verdict "internal-$row-positive-control-echo" "through the proxy, echo over TLS by name: $(jq -c '.[] | select(.class == "control-echo") | [.status, .inner]' <<< "$report")" \
      test "$(jq -r '.[] | select(.class == "control-echo") | .inner' <<< "$report")" = "HTTP/1.1 200 OK"
    refused_bad=$(jq -c '[.[] | select(.class != "control-echo") | select(.inner | test("^HTTP/1.[01] 2")) | .class]' <<< "$report")
    verdict "internal-$row-via-proxy-no-success" "no internal target answered 2xx through the proxy (modes: allow lists names, full allows all names): $refused_bad; internal_cidrs=$ICIDRS" test "$refused_bad" = "[]"
    verdict "internal-$row-node-second-address-refused" "node second address $NODE_SECOND ports 10250 and 6443 by IP and by name through the proxy: $(jq -c '[.[] | select(.class | startswith("node-second-")) | [.class, .status, .inner]]' <<< "$report")" \
      test "$(jq '[.[] | select(.class | startswith("node-second-")) | select(.inner | test("^HTTP/1.[01] [2-4]"))] | length' <<< "$report")" = 0
  fi
  after=$(recorder_counts)
  verdict "internal-$row-zero-connections" "connections recorded (probe container, recorder pod, node recorder) before: $before, after: $after" test "$before" = "$after"
}

control_hit() {
  k -n "$NS_TOOLS" exec "$TOOLBOX_POD" -- curl -s -m 3 -o /dev/null "http://$1:$2/" || true
}

before=$(recorder_counts)
control_hit "$PROBE_IP" 80
control_hit "$REC_IP" 80
control_hit "$NODE_FIRST" 80
sleep 2
after=$(recorder_counts)
verdict internal-recorders-count-connections "positive control: three direct hits move the counts from $before to $after" test "$after" != "$before"

: > "$EVID_FILE"
for row in "$CLIENT_A:$NET_NONE" "$CLIENT_A:$NET_ALLOW" "$CLIENT_A:$NET_FULL" "$CLIENT_PIVOT:$NET_FULL"; do
  run_row "${row%%:*}" "${row##*:}"
done

scenario_end
