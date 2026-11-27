#!/usr/bin/env bash
HERE=$(cd "$(dirname "$0")" && pwd)
. "$HERE/common.sh"
scenario_begin s03-two-clients

listen_port=9000

start_listener() {
  sx "$1" "$2" sh -c "nohup $SANDBOX_PYTHON -m http.server $listen_port --bind 0.0.0.0 > /dev/null 2>&1 &"
}

listener_control() {
  local local_open
  probe_open_to local_open "$3" "$4" tcp "[{\"host\":\"127.0.0.1\",\"port\":$listen_port}]"
  verdict "$1" "listener answers inside $2: $local_open of 1" test "$local_open" = 1
}

for net in "$NET_NONE" "$NET_ALLOW" "$NET_FULL"; do
  ns_a=$(ns_of "$CLIENT_A" "$net")
  ns_b=$(ns_of "$CLIENT_B" "$net")
  verdict "two-clients-$net-two-namespaces" "$ns_a and $ns_b" test "$ns_a" != "$ns_b"
  wait_warm "$ns_a" 1 120 && wait_warm "$ns_b" 1 120 ||
    { check "two-clients-$net-warm-pods" fail "no warm pod in $ns_a or $ns_b"; continue; }
  claim_to pod_a "$ns_a" "two-clients-$(new_id)" agent-a && claim_to pod_b "$ns_b" "two-clients-$(new_id)" agent-b ||
    { check "two-clients-$net-claim" fail "claim refused in $ns_a or $ns_b"; continue; }
  ip_a=$(k -n "$ns_a" get pod "$pod_a" -o jsonpath='{.status.podIP}')
  ip_b=$(k -n "$ns_b" get pod "$pod_b" -o jsonpath='{.status.podIP}')
  if [ -z "$ip_a" ] || [ -z "$ip_b" ]; then check "two-clients-$net-pod-addresses" fail "pod address of a or b empty"; continue; fi
  proxy_ip_b=$(proxy_pods "$CLIENT_B" "$net" | cut -d' ' -f1)
  if [ "$net" = "$NET_ALLOW" ]; then need_proxy "two-clients-$net" "$CLIENT_B" "$net"; fi

  start_listener "$ns_a" "$pod_a"
  start_listener "$ns_b" "$pod_b"
  sleep 2
  listener_control "two-clients-$net-listener-control-a" a "$ns_a" "$pod_a"
  listener_control "two-clients-$net-listener-control-b" b "$ns_b" "$pod_b"

  targets="[{\"host\":\"$ip_b\",\"port\":$listen_port}]"
  if [ -n "$proxy_ip_b" ]; then
    targets="[{\"host\":\"$ip_b\",\"port\":$listen_port},{\"host\":\"$proxy_ip_b\",\"port\":$PROXY_PORT}]"
  fi
  probe_open_to reached "$ns_a" "$pod_a" tcp "$targets"
  verdict "two-clients-$net-cross-connection-fails" "a to b pod and b proxy in $(egress_ns_of "$CLIENT_B" "$net"): $reached reached" test "$reached" = 0

  probe_open_to reverse "$ns_b" "$pod_b" tcp "[{\"host\":\"$ip_a\",\"port\":$listen_port}]"
  verdict "two-clients-$net-reverse-fails" "b to a pod: $reverse reached" test "$reverse" = 0
done

scenario_end
