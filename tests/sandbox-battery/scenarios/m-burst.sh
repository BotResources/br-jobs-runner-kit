#!/usr/bin/env bash
HERE=$(cd "$(dirname "$0")" && pwd)
. "$HERE/common.sh"
scenario_begin m-burst

default_pools="$CLIENT_A:$NET_ALLOW $CLIENT_A:$NET_FULL $CLIENT_CANARY:$NET_ALLOW $CLIENT_B:$NET_ALLOW $CLIENT_PIVOT:$NET_ALLOW $CLIENT_PIVOT:$NET_FULL $CLIENT_PLATFORM:$NET_ALLOW"
NSS=()
for pair in ${BURST_POOLS:-$default_pools}; do NSS+=("$(ns_of "${pair%%:*}" "${pair##*:}")"); done
pool_count=${#NSS[@]}
start_size=${BURST_START:-2}
max_load=${BURST_MAX_LOAD:-14}
min_free_mb=${BURST_MIN_FREE_MB:-8000}
budget=${BURST_BUDGET_SECONDS:-840}
state_file="$BATTERY_OUT/burst-original-$(new_id).txt"
current_thread=
largest_held=0
largest_passing=0
stop_reason=

restore_pools() {
  local pns replicas
  if [ -s "$state_file" ]; then
    while read -r pns replicas; do set_pool_replicas "$pns" "$replicas"; done < "$state_file"
  fi
}

drop_claimed() {
  local pns
  if [ -n "$current_thread" ]; then
    for pns in "${NSS[@]}"; do k -n "$pns" delete pod -l "$LBL_THREAD=$current_thread" --wait=false --grace-period=1 > /dev/null 2>&1; done
  fi
}

cleanup_extra() {
  drop_claimed
  restore_pools
  keeper_resume
}

free_mb() { df -m /System/Volumes/Data | awk 'NR==2 {print $4}'; }
load_one() { load_now | awk '{print $1}'; }
nodes_ready() { [ "$(k get nodes -o json | jq '[.items[] | select(any(.status.conditions[]; .type == "Ready" and .status == "True") | not)] | length')" = 0 ]; }

limit_reached() {
  local load free
  load=$(load_one)
  free=$(free_mb)
  if ! nodes_ready; then echo "node not Ready"; return 0; fi
  if awk -v l="$load" -v m="$max_load" 'BEGIN {exit !(l >= m)}'; then echo "load average $load at or over $max_load"; return 0; fi
  if [ "$free" -lt "$min_free_mb" ]; then echo "free disk $free MB under $min_free_mb MB"; return 0; fi
  if [ "$(date +%s)" -ge "$deadline" ]; then echo "time budget of $budget s used"; return 0; fi
  return 1
}

share() { echo $(($1 / pool_count + ($2 < $1 % pool_count ? 1 : 0))); }

one_claim() {
  local pns=$1 started rc
  started=$(now_ms)
  if claim_one "$pns" "$2" "$3" "$4" "agent-$5" && sx "$pns" "$2" true > /dev/null 2>&1; then rc=0; else rc=1; fi
  echo "$5 $(($(now_ms) - started)) $rc" >> "$6"
}

burst() {
  local size=$1 thread=$2 report=$3 pns name rv index=0 position want
  : > "$report"
  position=0
  for pns in "${NSS[@]}"; do
    want=$(share "$size" "$position")
    position=$((position + 1))
    if [ "$want" -gt 0 ]; then
      while read -r name rv; do
        index=$((index + 1))
        one_claim "$pns" "$name" "$rv" "$thread" "$index" "$report" &
      done < <(warm_list "$pns" | head -n "$want")
    fi
  done
  wait
}

hold_size() {
  local size=$1 pns want position=0
  for pns in "${NSS[@]}"; do
    want=$(share "$size" "$position")
    position=$((position + 1))
    if [ "$want" -gt 0 ]; then set_pool_replicas "$pns" "$want"; fi
  done
  position=0
  for pns in "${NSS[@]}"; do
    want=$(share "$size" "$position")
    position=$((position + 1))
    if [ "$want" -gt 0 ] && ! wait_warm "$pns" "$want" 240; then
      held_failure="fewer than $want warm pods in $pns after 240 s"
      return 1
    fi
  done
}

lock_timed || scenario_end
keeper_pause
for pns in "${NSS[@]}"; do echo "$pns $(pool_replicas "$pns")" >> "$state_file"; done
deadline=$(($(date +%s) + budget))
record m-burst-pools "$pool_count" "namespaces: ${NSS[*]}"

size=$start_size
while true; do
  if stop_reason=$(limit_reached); then break; fi
  if ! hold_size "$size"; then
    stop_reason="not held: $held_failure"
    record "m-burst-$size-held" 0 "$stop_reason"
    break
  fi
  largest_held=$size
  thread="burst-$(new_id)"
  current_thread=$thread
  report="$BATTERY_OUT/burst-$size-$thread.txt"
  burst "$size" "$thread" "$report"
  answered=$(awk '$3 == 0' "$report" | count_lines)
  within=$(awk '$3 == 0 && $2 <= 500' "$report" | count_lines)
  record "m-burst-$size-answered" "$answered" "claims of $size"
  record "m-burst-$size-within-500ms" "$within" "claims of $size, kubectl client time"
  record "m-burst-$size-median" "$(awk '{print $2}' "$report" | median_of)" ms
  record "m-burst-$size-p95" "$(awk '{print $2}' "$report" | percentile_of 95)" ms
  record "m-burst-$size-slowest" "$(awk '{print $2}' "$report" | max_of)" ms
  if [ "$answered" = "$size" ] && [ $((within * 100)) -ge $((size * 95)) ]; then
    largest_passing=$size
    record "m-burst-$size-passed" 1 "$within of $size within 500 ms"
  else
    record "m-burst-$size-passed" 0 "$within of $size within 500 ms, $answered answered"
  fi
  drop_claimed
  current_thread=
  size=$((size * 2))
done

record m-burst-stopped-by "$stop_reason" "first limit reached after the burst of $largest_held"
record m-burst-largest-held "$largest_held" "warm pods held at once on this Mac"
record m-burst-largest-passing "$largest_passing" "claims, all answered, 95 percent within 500 ms"
verdict m-burst-some-size-passes "largest passing burst: $largest_passing; stopped by: $stop_reason" test "$largest_passing" -gt 0

scenario_end
