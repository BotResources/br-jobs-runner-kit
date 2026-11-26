#!/usr/bin/env bash
HERE=$(cd "$(dirname "$0")" && pwd)
. "$HERE/common.sh"

UNTIMED="s01-docker-parity s02-two-agents s03-two-clients s04-network-modes s05-admission s06-seed-read-only s10-proxy-canary s11-platform s12-closed-cluster"
TIMED="m-restore-stalls m-netpol-delay m-coldstart m-scale-down m-burst"

mkdir -p "$BATTERY_OUT"
SCENARIO_TALLY="$BATTERY_OUT/tally-$(date +%Y%m%d-%H%M%S).tsv"
export SCENARIO_TALLY
: > "$SCENARIO_TALLY"
export PREFLIGHT_DONE=1

run_one() {
  local name=$1 before rc
  preflight
  echo "== $name, load $(load_now)"
  before=$(count_lines < "$SCENARIO_TALLY")
  bash "$HERE/$name.sh"
  rc=$?
  if [ "$rc" -ne 0 ] && [ "$(count_lines < "$SCENARIO_TALLY")" = "$before" ]; then
    check "$name-script-error" fail "exit code $rc with no check recorded"
  fi
}

run_timed() {
  lock_timed
  export TIMING_LOCK_HELD=1
  run_one "$1"
  unset TIMING_LOCK_HELD
  unlock_timed
}

selected="$*"
for name in $UNTIMED; do
  if [ -z "$selected" ] || [[ " $selected " == *" $name "* ]]; then run_one "$name"; fi
done
for name in $TIMED; do
  if [ -z "$selected" ] || [[ " $selected " == *" $name "* ]]; then run_timed "$name"; fi
done

passed=$(awk -F'\t' '$2 == "pass"' "$SCENARIO_TALLY" | count_lines)
failed=$(awk -F'\t' '$2 == "fail"' "$SCENARIO_TALLY" | count_lines)
echo "== total: $passed pass, $failed fail (tally $SCENARIO_TALLY)"
[ "$failed" -eq 0 ]
