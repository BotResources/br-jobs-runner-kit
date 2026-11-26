#!/usr/bin/env bash
HERE=$(cd "$(dirname "$0")" && pwd)
. "$HERE/common.sh"
scenario_begin m-netpol-delay

reps=${REPS:-10}
gate_ns=$GATE_NS
target=np-target
client=np-client
stream="$BATTERY_OUT/netpol-stream.txt"

cleanup_extra() { k -n "$gate_ns" delete pod/$target pod/$client networkpolicy/np-gate --ignore-not-found --wait=false > /dev/null 2>&1; }

start_gate_objects() {
  k -n "$gate_ns" apply -f - > /dev/null << YAML
apiVersion: v1
kind: Pod
metadata: {name: $target, labels: {app: $target}}
spec:
  containers:
  - name: web
    image: $ECHO_IMAGE
    command: [python3, -m, http.server, "8080"]
---
apiVersion: v1
kind: Pod
metadata: {name: $client, labels: {app: $client}}
spec:
  containers:
  - name: probe
    image: $ECHO_IMAGE
    command: [sleep, "3600"]
---
apiVersion: networking.k8s.io/v1
kind: NetworkPolicy
metadata: {name: np-gate}
spec:
  podSelector: {matchLabels: {app: $target}}
  policyTypes: [Ingress]
  ingress:
  - from:
    - podSelector: {matchLabels: {gate: open}}
YAML
  k -n "$gate_ns" wait --for=condition=Ready pod/$target pod/$client --timeout=180s > /dev/null
}

stream_to_file() {
  k -n "$1" exec -i "$2" -- "$SANDBOX_PYTHON" - "$3" "$4" "$5" < "$HERE/netpol-probe.py" | python3 -u "$HERE/stamp.py" > "$stream" &
  STREAM_PID=$!
}

first_after() { awk -v t="$1" -v want="$2" '$1 >= t && $2 == want {print $1 - t; exit}' "$stream"; }

gate_delay() {
  local direction=$1 want=$2 command_started command_returned
  stream_to_file "$gate_ns" $client "$target_ip" 8080 7
  sleep 2
  command_started=$(now_ms)
  if [ "$direction" = open ]; then k -n "$gate_ns" label pod $client gate=open --overwrite > /dev/null; else k -n "$gate_ns" label pod $client gate- > /dev/null; fi
  command_returned=$(now_ms)
  wait "$STREAM_PID" 2> /dev/null
  echo "$(first_after "$command_started" "$want" | grep . || echo na) $(first_after "$command_returned" "$want" | grep . || echo na)"
}

k create namespace "$gate_ns" > /dev/null 2>&1
start_gate_objects || { check m-netpol-objects fail "test pods not ready in $gate_ns"; scenario_end; }
target_ip=$(k -n "$gate_ns" get pod $target -o jsonpath='{.status.podIP}')
lock_timed

open_from_start=
open_from_return=
close_from_start=
close_from_return=
for _ in $(seq 1 "$reps"); do
  read -r a b <<< "$(gate_delay open 1)"
  open_from_start="$open_from_start$a"$'\n'
  open_from_return="$open_from_return$b"$'\n'
  read -r a b <<< "$(gate_delay close 0)"
  close_from_start="$close_from_start$a"$'\n'
  close_from_return="$close_from_return$b"$'\n'
done
for name in open_from_start open_from_return close_from_start close_from_return; do
  values=$(printf '%s' "${!name}" | grep -E '^-?[0-9]+$')
  record "m-netpol-$name-median" "$(printf '%s\n' "$values" | median_of)" ms
  record "m-netpol-$name-max" "$(printf '%s\n' "$values" | max_of)" ms
done

ns=$(ns_of "$CLIENT_A" "$NET_ALLOW")
wait_warm "$ns" "$reps" 180 "$POOL_ID_DEFAULT" || check m-netpol-claim-warm fail "fewer than $reps warm pods in $ns"
failures=0
while read -r name rv && [ "$reps" -gt 0 ]; do
  proxy=$(proxy_hostport "$ns" "$name")
  stream_to_file "$ns" "$name" "${proxy%:*}" "${proxy#*:}" 5
  sleep 2
  claim_one "$ns" "$name" "$rv" "gap-$(new_id)" agent-a
  wait "$STREAM_PID" 2> /dev/null
  failures=$((failures + $(awk '$2 == 0' "$stream" | count_lines)))
  track_pod "$ns" "$name"
  reps=$((reps - 1))
done < <(warm_list "$ns" "$POOL_ID_DEFAULT")
record m-netpol-claim-proxy-probe-failures "$failures" "failed probes at 20 ms across the claims"
check m-netpol-delay-ran pass "gate delays and claim gap recorded"

scenario_end
