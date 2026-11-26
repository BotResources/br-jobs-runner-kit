#!/usr/bin/env bash
HERE=$(cd "$(dirname "$0")" && pwd)
. "$HERE/common.sh"
scenario_begin m-burst

ns=$(ns_of "$CLIENT_A" "$NET_NONE")
pool=$POOL_ID_DEFAULT
sizes=${BURST_SIZES:-"5 10 20 40"}
original_replicas=
largest_held=0
largest_passing=0

cleanup_extra() {
  if [ -n "$original_replicas" ]; then set_pool_replicas "$ns" "$pool" "$original_replicas"; fi
  keeper_resume
}

one_claim() {
  local started
  started=$(now_ms)
  if claim_one "$ns" "$1" "$2" "$3" "agent-$4" && sx "$ns" "$1" true > /dev/null 2>&1; then rc=0; else rc=1; fi
  echo "$4 $(($(now_ms) - started)) $rc" >> "$5"
}

burst() {
  local size=$1 thread=$2 report=$3 name rv index=0
  : > "$report"
  while read -r name rv; do
    index=$((index + 1))
    one_claim "$name" "$rv" "$thread" "$index" "$report" &
  done < <(warm_list "$ns" "$pool" | head -n "$size")
  wait
}

lock_timed
keeper_pause
original_replicas=$(pool_replicas "$ns" "$pool")

for size in $sizes; do
  set_pool_replicas "$ns" "$pool" "$size"
  if ! wait_warm "$ns" "$size" 240 "$pool"; then
    record "m-burst-$size-held" 0 "not held: fewer than $size warm pods after 240 s"
    break
  fi
  largest_held=$size
  thread="burst-$(new_id)"
  report="$BATTERY_OUT/burst-$size-$thread.txt"
  burst "$size" "$thread" "$report"
  answered=$(awk '$3 == 0' "$report" | count_lines)
  within=$(awk '$3 == 0 && $2 <= 500' "$report" | count_lines)
  record "m-burst-$size-answered" "$answered" "claims of $size"
  record "m-burst-$size-within-500ms" "$within" "claims of $size, kubectl client time"
  record "m-burst-$size-median" "$(awk '{print $2}' "$report" | median_of)" ms
  record "m-burst-$size-p95" "$(awk '{print $2}' "$report" | percentile_of 95)" ms
  record "m-burst-$size-slowest" "$(awk '{print $2}' "$report" | max_of)" ms
  refill_started=$(now_ms)
  k -n "$ns" delete pod -l "$LBL_THREAD=$thread" --wait=false --grace-period=1 > /dev/null 2>&1
  if wait_warm "$ns" "$size" 240 "$pool"; then record "m-burst-$size-refill" "$(($(now_ms) - refill_started))" ms; fi
  if [ "$answered" = "$size" ] && [ $((within * 100)) -ge $((size * 95)) ]; then
    largest_passing=$size
    record "m-burst-$size-passed" 1 "$within of $size within 500 ms"
  else
    record "m-burst-$size-passed" 0 "$within of $size within 500 ms, $answered answered"
  fi
done

record m-burst-largest-held "$largest_held" "warm pods held at once on this Mac"
record m-burst-largest-passing "$largest_passing" "claims, all answered, 95 percent within 500 ms"
verdict m-burst-some-size-passes "largest passing burst: $largest_passing" test "$largest_passing" -gt 0

scenario_end
