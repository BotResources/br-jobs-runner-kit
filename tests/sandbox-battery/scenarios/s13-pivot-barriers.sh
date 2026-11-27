#!/usr/bin/env bash
HERE=$(cd "$(dirname "$0")" && pwd)
. "$HERE/common.sh"
scenario_begin s13-pivot-barriers
. "$WORK/servers.env"

ns=$(ns_of "$CLIENT_PIVOT" "$NET_ALLOW")
eg=$(egress_ns_of "$CLIENT_PIVOT" "$NET_ALLOW")
SAVED="$BATTERY_OUT/pivot-saved"
EVID_FILE="$EVIDENCE/pivot-barriers.tsv"
ICIDRS=$(internal_cidrs_value)
REC_IP=$(k -n "$TESTSVC_NS" get pod "$REC_POD" -o jsonpath='{.status.podIP}')
NODE_FIRST=$(k get nodes -o jsonpath='{.items[0].status.addresses[?(@.type=="InternalIP")].address}')
NODE_SECOND=$(docker inspect "$NODE" --format "{{with index .NetworkSettings.Networks \"$SERVERS_NETWORK\"}}{{.IPAddress}}{{end}}")
RESTORE_NEEDED=0

strip_meta() { jq 'del(.metadata.resourceVersion, .metadata.uid, .metadata.creationTimestamp, .metadata.managedFields, .metadata.generation, .status)'; }

save_originals() {
  mkdir -p "$SAVED"
  k -n "$eg" get cm "$PROXY_CONFIG_MAP" -o json | strip_meta > "$SAVED/config.json"
  k -n "$eg" get networkpolicy "$NP_EGRESS_PROXY" -o json | strip_meta > "$SAVED/netpol.json"
  RESTORE_NEEDED=1
}

set_config() {
  local allow_on=$1 deny_on=$2 expr='.' yaml
  if [ "$allow_on" = 0 ]; then expr="$expr | .transforms[0].config.domains = [\"*\"] | .transforms[0].config.cidrs = [\"0.0.0.0/0\"]"; fi
  if [ "$deny_on" = 0 ]; then expr="$expr | .proxy.upstream_deny_cidrs = []"; fi
  yaml=$(jq -r '.data["proxy.yaml"]' "$SAVED/config.json" | yq "$expr")
  jq --arg y "$yaml" '.data["proxy.yaml"] = $y' "$SAVED/config.json" | k replace -f - > /dev/null
  proxy_restart "$eg"
}

set_netpol() {
  if [ "$1" = 1 ]; then
    k replace -f "$SAVED/netpol.json" > /dev/null
  else
    k -n "$eg" patch networkpolicy "$NP_EGRESS_PROXY" --type json -p '[{"op":"replace","path":"/spec/egress","value":[{}]}]' > /dev/null
  fi
  sleep 6
}

cleanup_extra() {
  if [ "$RESTORE_NEEDED" = 1 ]; then
    k replace -f "$SAVED/config.json" > /dev/null 2>&1
    k replace -f "$SAVED/netpol.json" > /dev/null 2>&1
    proxy_restart "$eg"
  fi
}

counts() { echo "$(conn_docker "$PROBE_CONTAINER") $(conn_pod "$REC_POD") $(conn_pod "$REC_NODE_POD")"; }

item() {
  jq -n -c --arg p "$1" --arg t "$2" --argjson tls "$3" --arg sni "$4" '{proxy: $p, target: $t, tls: $tls, sni: (if $sni == "" then null else $sni end)}'
}

names_batch() {
  local proxy=$1 name port
  for name in "$PROBE_HOST" "$POD_TARGET_NAME" "$SVC_TARGET_NAME" "$NODE_TARGET_NAME" "$NODEFIRST_TARGET_NAME"; do
    item "$proxy" "$name:80" false ""
    item "$proxy" "$name:8080" false ""
    item "$proxy" "$name:443" true "$name"
  done | jq -s -c .
}

ips_batch() {
  local proxy=$1 host
  for host in "$PROBE_IP" "$REC_IP" "$REC_CLUSTER_IP" "$NODE_SECOND" "$NODE_FIRST"; do
    item "$proxy" "$host:80" false ""
    item "$proxy" "$host:8080" false ""
  done | jq -s -c .
}

run_batch() {
  local kind=$1 allow_on=$2 deny_on=$3 net_on=$4 proxy batch before after expect_blocked verdict_text delta report
  proxy=$(proxy_endpoint "$CLIENT_PIVOT" "$NET_ALLOW")
  if [ "$kind" = names ]; then
    batch=$(names_batch "$proxy")
    expect_blocked=$((deny_on == 1 || net_on == 1 ? 1 : 0))
  else
    batch=$(ips_batch "$proxy")
    expect_blocked=$((allow_on == 1 || deny_on == 1 || net_on == 1 ? 1 : 0))
  fi
  before=$(counts)
  report=$(probe "$ns" "$pod" via "$batch" | jq -s -c .)
  sleep 4
  after=$(counts)
  read -r b1 b2 b3 <<< "$before"
  read -r a1 a2 a3 <<< "$after"
  delta="$((a1 - b1)) $((a2 - b2)) $((a3 - b3))"
  printf '%s\n' "$report" | jq -r --arg c "allowlist=$allow_on deny=$deny_on policy=$net_on" --arg k "$kind" '.[] | [$c, $k, .target, .status, .inner] | @tsv' >> "$EVID_FILE"
  verdict_text="$kind targets, barriers in place allowlist=$allow_on deny_list=$deny_on policy=$net_on, internal_cidrs=$ICIDRS; new connections at probe container, recorder pod, node recorder: $delta"
  if [ "$expect_blocked" = 1 ]; then
    verdict "pivot-$kind-a$allow_on-d$deny_on-n$net_on-zero-connections" "$verdict_text" test "$delta" = "0 0 0"
  else
    verdict "pivot-$kind-a$allow_on-d$deny_on-n$net_on-control-connects" "no barrier acts, every recorder must see a connection: $verdict_text" \
      test "$((a1 - b1))" -gt 0 -a "$((a2 - b2))" -gt 0 -a "$((a3 - b3))" -gt 0
  fi
}

wait_warm "$ns" 1 120 || { check pivot-warm-pod fail "no warm pod in $ns"; scenario_end; }
need_proxy pivot "$CLIENT_PIVOT" "$NET_ALLOW" || scenario_end
claim_to pod "$ns" "pivot-$(new_id)" agent-a || { check pivot-claim fail "claim refused"; scenario_end; }
save_originals
: > "$EVID_FILE"
verdict pivot-deny-list-holds-every-address "deny list holds pod range, service range, both node addresses, k3d range: $(jq -r '.data["proxy.yaml"]' "$SAVED/config.json" | yq -o=json -I=0 '.proxy.upstream_deny_cidrs')" \
  test "$(jq -r '.data["proxy.yaml"]' "$SAVED/config.json" | yq '.proxy.upstream_deny_cidrs | length')" -ge 6

for pair in "1 1" "0 1" "1 0" "0 0"; do
  read -r allow_on deny_on <<< "$pair"
  set_config "$allow_on" "$deny_on" || { check "pivot-config-a$allow_on-d$deny_on" fail "proxy did not come back Ready"; continue; }
  for net_on in 1 0; do
    set_netpol "$net_on"
    run_batch names "$allow_on" "$deny_on" "$net_on"
    run_batch ips "$allow_on" "$deny_on" "$net_on"
  done
done

scenario_end
