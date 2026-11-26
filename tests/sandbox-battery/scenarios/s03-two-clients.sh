#!/usr/bin/env bash
HERE=$(cd "$(dirname "$0")" && pwd)
. "$HERE/common.sh"
scenario_begin s03-two-clients

listen_port=9000
for net in "$NET_NONE" "$NET_ALLOW" "$NET_FULL"; do
  ns_a=$(ns_of "$CLIENT_A" "$net")
  ns_b=$(ns_of "$CLIENT_B" "$net")
  verdict "two-clients-$net-two-namespaces" "$ns_a and $ns_b" test "$ns_a" != "$ns_b"
  wait_warm "$ns_a" 1 120 && wait_warm "$ns_b" 1 120 ||
    { check "two-clients-$net-warm-pods" fail "no warm pod in $ns_a or $ns_b"; continue; }
  claim_to pod_a "$ns_a" "two-clients-$(new_id)" agent-a
  claim_to pod_b "$ns_b" "two-clients-$(new_id)" agent-b
  ip_b=$(k -n "$ns_b" get pod "$pod_b" -o jsonpath='{.status.podIP}')
  proxy_ip_b=$(proxy_pods "$CLIENT_B" "$net" | cut -d' ' -f1)
  if [ "$net" = "$NET_ALLOW" ]; then need_proxy "two-clients-$net" "$CLIENT_B" "$net"; fi

  sx "$ns_b" "$pod_b" sh -c "nohup $SANDBOX_PYTHON -m http.server $listen_port --bind 0.0.0.0 > /dev/null 2>&1 &"
  sleep 2
  local_open=$(probe "$ns_b" "$pod_b" tcp "[{\"host\":\"127.0.0.1\",\"port\":$listen_port}]" | jq -r '.open')
  verdict "two-clients-$net-listener-control" "listener answers inside b: $local_open" test "$local_open" = true

  targets="[{\"host\":\"$ip_b\",\"port\":$listen_port}]"
  if [ -n "$proxy_ip_b" ]; then
    targets="[{\"host\":\"$ip_b\",\"port\":$listen_port},{\"host\":\"$proxy_ip_b\",\"port\":$PROXY_PORT}]"
  fi
  reached=$(probe "$ns_a" "$pod_a" tcp "$targets" | jq -s '[.[] | select(.open)] | length')
  verdict "two-clients-$net-cross-connection-fails" "a to b pod and b proxy in $(egress_ns_of "$CLIENT_B" "$net"): $reached reached" test "$reached" = 0

  reverse=$(probe "$ns_b" "$pod_b" tcp "[{\"host\":\"$(k -n "$ns_a" get pod "$pod_a" -o jsonpath='{.status.podIP}')\",\"port\":$listen_port}]" | jq -s '[.[] | select(.open)] | length')
  verdict "two-clients-$net-reverse-fails" "b to a pod: $reverse reached" test "$reverse" = 0
done

scenario_end
