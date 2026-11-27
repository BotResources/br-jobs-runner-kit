#!/usr/bin/env bash
HERE=$(cd "$(dirname "$0")" && pwd)
. "$HERE/common.sh"
scenario_begin m-restore-stalls

state_mb=${STATE_MB:-30}
runs=${RUNS:-50}
stall_ms=${STALL_MS:-3000}
ns=$(ns_of "$CLIENT_A" "$NET_NONE")
state_file="$BATTERY_OUT/restore-state-${state_mb}mb.tar"
table="$BATTERY_OUT/restore-stalls-$(date +%Y%m%d-%H%M%S).tsv"
context_log="$BATTERY_OUT/restore-stalls-context.log"

make_state() {
  python3 - "$1" "$2" << 'PY'
import io
import os
import sys
import tarfile

target, megabytes = sys.argv[1], int(sys.argv[2])
with tarfile.open(target, "w") as archive:
    for index in range(50):
        content = os.urandom(1024)
        entry = tarfile.TarInfo("state/small-%d.bin" % index)
        entry.size = len(content)
        archive.addfile(entry, io.BytesIO(content))
    content = os.urandom(megabytes * 1024 * 1024)
    entry = tarfile.TarInfo("state/large.bin")
    entry.size = len(content)
    archive.addfile(entry, io.BytesIO(content))
PY
}

stall_context() {
  {
    echo "run $1 total_ms $2 at $(date +%T)"
    docker exec "$NODE" sh -c 'cat /proc/loadavg; cat /proc/pressure/cpu /proc/pressure/io /proc/pressure/memory 2> /dev/null'
    docker stats --no-stream --format '{{.Name}} cpu={{.CPUPerc}} mem={{.MemUsage}}' "$NODE"
    k -n "$ns" get events --sort-by=.lastTimestamp 2> /dev/null | tail -5
  } >> "$context_log"
}

[ -f "$state_file" ] || make_state "$state_file" "$state_mb"
lock_timed
claim_failures=0
printf 'run\tclaim_ms\ttrue_ms\tstream_ms\ttotal_ms\trc\tload\n' > "$table"

for run in $(seq 1 "$runs"); do
  wait_warm "$ns" 1 120 || { check m-restore-stalls-warm fail "no warm pod before run $run"; break; }
  started=$(now_ms)
  claim_to pod "$ns" "stall-$(new_id)" agent-a || { claim_failures=$((claim_failures + 1)); continue; }
  claimed=$(now_ms)
  sx "$ns" "$pod" true
  true_rc=$?
  first_exec=$(now_ms)
  with_timeout 180 sxi "$ns" "$pod" "$AGENT_BSDTAR" -x -p --numeric-owner -f - -C /work < "$state_file" > /dev/null 2>&1
  rc=$?
  if [ "$true_rc" -ne 0 ]; then rc=$true_rc; fi
  finished=$(now_ms)
  total=$((finished - started))
  printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$run" "$((claimed - started))" "$((first_exec - claimed))" "$((finished - first_exec))" "$total" "$rc" "$(load_now)" >> "$table"
  if [ "$total" -gt "$stall_ms" ]; then stall_context "$run" "$total"; fi
  release_pod "$ns" "$pod"
done

ok_samples=$(awk -F'\t' 'NR > 1 && $6 == 0' "$table" | count_lines)
record m-restore-ok-samples "$ok_samples" "runs with exit code 0 of $runs"
if [ "$ok_samples" -gt 0 ]; then
  stalls=$(awk -F'\t' -v limit="$stall_ms" 'NR > 1 && $6 == 0 && $5 > limit' "$table" | count_lines)
  record m-restore-stall-count "$stalls" "stalls of $ok_samples ok runs over ${stall_ms} ms at ${state_mb} MB"
  record m-restore-stall-longest "$(awk -F'\t' 'NR > 1 && $6 == 0 {print $5}' "$table" | max_of)" ms
  record m-restore-total-median "$(awk -F'\t' 'NR > 1 && $6 == 0 {print $5}' "$table" | median_of)" ms
  for phase in 2:claim 3:first-exec 4:stream; do
    column=${phase%%:*}
    count=$(awk -F'\t' -v c="$column" -v limit="$stall_ms" 'NR > 1 && $6 == 0 && $5 > limit && $c >= $5 / 2' "$table" | count_lines)
    record "m-restore-stalls-dominated-by-${phase##*:}" "$count" stalls
  done
fi
verdict m-restore-stalls-ran "$ok_samples ok runs of $runs, $claim_failures claims refused, table $table, context $context_log" \
  test "$ok_samples" = "$runs"

scenario_end
