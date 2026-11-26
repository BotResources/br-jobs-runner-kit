#!/usr/bin/env bash

daemon_runs_platform_natively() {
  local daemon_arch
  daemon_arch=$(docker info --format '{{.Architecture}}')
  case "${daemon_arch}:${PLATFORM}" in
    x86_64:linux/amd64 | aarch64:linux/arm64 | arm64:linux/arm64) return 0 ;;
    *) return 1 ;;
  esac
}

ORPHAN_SCRIPT='(sleep 2 & echo $! > /tmp/orphan_pid); true'

orphan_state() {
  local container="$1"
  docker exec "$container" bash -c 'p=$(cat /tmp/orphan_pid); if [ -e /proc/$p/status ]; then grep -m1 "^State:" /proc/$p/status; else echo gone; fi; echo "zombies=$(grep -l "^State:.*Z" /proc/[0-9]*/status 2>/dev/null | wc -l)"' 2>&1 | tr '\n' ' '
}

test_7_pause_as_pid_1() {
  local name="${RUN_ID}-pause" control="${RUN_ID}-control" state verdict=PASS detail=""
  docker run -d --init=false --name "$control" --platform "$PLATFORM" "$PROBE_IMAGE" sleep infinity >/dev/null
  docker exec "$control" bash -c "$ORPHAN_SCRIPT"
  sleep 4
  state=$(orphan_state "$control")
  docker rm -f "$control" >/dev/null
  detail="negative control (pid 1 = sleep infinity): ${state}; "
  case "$state" in *"State:"*"Z"*) ;; *) detail="${detail}control shows no zombie, the test would not detect one; "; verdict=FAIL ;; esac

  docker run -d --init=false --name "$name" --platform "$PLATFORM" -v "${TOOLS_VOL}:/opt/sbx:ro" $SEVEN_CAPS "$PROBE_IMAGE" /opt/sbx/bin/pause >/dev/null
  wait_for_container_state "$name" running
  docker exec "$name" bash -c "$ORPHAN_SCRIPT"
  sleep 4
  state=$(orphan_state "$name")
  detail="${detail}pause as pid 1: ${state}; "
  case "$state" in "gone zombies=0 ") ;; *) verdict=FAIL ;; esac
  local rss
  rss=$(docker exec "$name" grep -m1 '^VmRSS:' /proc/1/status | tr -s ' ')
  detail="${detail}pid 1 ${rss}; "
  if daemon_runs_platform_natively; then
    case "$rss" in *" "[0-9]" kB"|*" "[0-9][0-9]" kB"|*" "[0-9][0-9][0-9]" kB"|*" "10[01][0-9]" kB"|*" "102[0-3]" kB") ;; *) verdict=FAIL ;; esac
  else
    detail="${detail}(emulated platform: the emulator counts in the resident memory, not judged); "
  fi
  local t0 t1 code
  t0=$(now_ms)
  docker kill --signal TERM "$name" >/dev/null
  code=$(docker wait "$name")
  t1=$(now_ms)
  detail="${detail}SIGTERM: exit=${code} after $((t1 - t0)) ms; "
  [ "$code" = 0 ] && [ $((t1 - t0)) -lt 1000 ] || verdict=FAIL
  docker rm -f "$name" >/dev/null

  docker run -d --init=false --name "$name" --platform "$PLATFORM" -v "${TOOLS_VOL}:/opt/sbx:ro" "$PROBE_IMAGE" /opt/sbx/bin/pause >/dev/null
  wait_for_container_state "$name" running
  t0=$(now_ms)
  docker stop -t 10 "$name" >/dev/null
  t1=$(now_ms)
  detail="${detail}docker stop -t 10: $((t1 - t0)) ms exit=$(docker inspect -f '{{.State.ExitCode}}' "$name"); "
  [ $((t1 - t0)) -lt 2000 ] || verdict=FAIL
  docker rm -f "$name" >/dev/null
  record "$verdict" 7 "pause as pid 1: reaps orphans, stops on SIGTERM, under 1 MiB" "$detail"
}
