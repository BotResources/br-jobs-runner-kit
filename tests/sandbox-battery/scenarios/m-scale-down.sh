#!/usr/bin/env bash
HERE=$(cd "$(dirname "$0")" && pwd)
. "$HERE/common.sh"
scenario_begin m-scale-down

ns=$(ns_of "$CLIENT_A" "$NET_NONE")
pool=$POOL_ID_DEFAULT
turn_seconds=${TURN_SECONDS:-25}
out="$BATTERY_OUT/scale-down-$(new_id)"
original_replicas=

cleanup_extra() {
  if [ -n "$original_replicas" ]; then set_pool_replicas "$ns" "$pool" "$original_replicas"; fi
  keeper_resume
}

run_turn() {
  ( with_timeout 180 sx "$ns" "$1" sh -c "sleep $turn_seconds; echo turn-finished-ok; exit $2" > "$out-$1.txt" 2>&1; echo $? > "$out-$1.rc" ) &
}

keeper_pause
original_replicas=$(pool_replicas "$ns" "$pool")
set_pool_replicas "$ns" "$pool" 3
wait_warm "$ns" 3 180 "$pool" || { check scale-down-warm-pods fail "fewer than three warm pods in $ns"; scenario_end; }
claim_to pod_zero "$ns" "scale-down-$(new_id)" agent-a "$pool"
claim_to pod_five "$ns" "scale-down-$(new_id)" agent-b "$pool"

run_turn "$pod_zero" 0
run_turn "$pod_five" 5
sleep 3
set_pool_replicas "$ns" "$pool" 0
sleep 5

warm_now=$(warm_list "$ns" "$pool" | count_lines)
verdict scale-down-warm-pods-removed "warm pods after the scale to 0: $warm_now" test "$warm_now" = 0
for pod in "$pod_zero" "$pod_five"; do
  verdict "scale-down-claimed-pod-running-$pod" "phase during the scale-down" \
    test "$(k -n "$ns" get pod "$pod" -o jsonpath='{.status.phase}')" = Running
done
wait
verdict scale-down-turn-output-reached "end marker printed by the turn that exits 0" grep -q turn-finished-ok "$out-$pod_zero.txt"
verdict scale-down-true-exit-code-zero "exec exit code $(cat "$out-$pod_zero.rc"), the command chose 0" test "$(cat "$out-$pod_zero.rc")" = 0
verdict scale-down-true-exit-code-five "exec exit code $(cat "$out-$pod_five.rc"), the command chose 5" test "$(cat "$out-$pod_five.rc")" = 5

scenario_end
