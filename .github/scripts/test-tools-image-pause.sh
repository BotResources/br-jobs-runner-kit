#!/usr/bin/env bash

ORPHAN_SCRIPT='(sleep 2 & echo $! > /tmp/orphan_pid); true'

orphan_state() {
  local container="$1"
  docker exec "$container" bash -c 'p=$(cat /tmp/orphan_pid); if [ -e /proc/$p/status ]; then grep -m1 "^State:" /proc/$p/status; else echo gone; fi; echo "zombies=$(grep -l "^State:.*Z" /proc/[0-9]*/status 2>/dev/null | wc -l)"' 2>&1 | tr '\n' ' '
}

start_pause() {
  local name="$1"
  docker run -d --init=false --name "$name" --platform "$PLATFORM" -v "${TOOLS_VOL}:/opt/sbx:ro" $SEVEN_CAPS "$PROBE_IMAGE" /opt/sbx/bin/pause >/dev/null
  wait_for_container_state "$name" running
}

exit_after_signal() {
  local name="$1" signal="$2" t0 t1
  t0=$(now_ms)
  docker kill --signal "$signal" "$name" >/dev/null
  if wait_for_container_state "$name" exited; then
    t1=$(now_ms)
    echo "$(docker inspect -f '{{.State.ExitCode}}' "$name") $((t1 - t0))"
  else
    echo "still-running -1"
  fi
}

test_7_body() {
  local name="${RUN_ID}-pause" control="${RUN_ID}-control" state rss result
  docker run -d --init=false --name "$control" --platform "$PLATFORM" "$PROBE_IMAGE" sleep infinity >/dev/null
  docker exec "$control" bash -c "$ORPHAN_SCRIPT"
  sleep 4
  state=$(orphan_state "$control")
  docker rm -f "$control" >/dev/null
  if [[ "$state" == *"State:"*"Z (zombie)"* ]]; then
    pass "negative control, pid 1 is sleep infinity: the orphan stays a zombie"
  else
    fail "negative control shows no zombie, the test would not detect one: ${state}"
  fi

  start_pause "$name"
  docker exec "$name" bash -c "$ORPHAN_SCRIPT"
  sleep 4
  state=$(orphan_state "$name")
  eq "pause as pid 1: the orphan is reaped and no zombie remains" "$state" "gone zombies=0 "
  rss=$(docker exec "$name" grep -m1 '^VmRSS:' /proc/1/status | tr -s ' ')
  info "pid 1 ${rss}"
  if daemon_runs_platform_natively; then
    case "$rss" in
      *" "[0-9]" kB" | *" "[0-9][0-9]" kB" | *" "[0-9][0-9][0-9]" kB" | *" "10[01][0-9]" kB" | *" "102[0-3]" kB") pass "resident memory of pid 1 is under 1 MiB" ;;
      *) fail "resident memory of pid 1: ${rss}" ;;
    esac
  else
    skip "emulated platform: the emulator counts in the resident memory of pid 1, the size of pause is not judged"
  fi
  result=$(exit_after_signal "$name" TERM)
  eq "SIGTERM ends pause with exit 0" "${result% *}" 0
  info "SIGTERM: ${result#* } ms"
  docker rm -f "$name" >/dev/null

  start_pause "$name"
  result=$(exit_after_signal "$name" INT)
  eq "SIGINT ends pause with exit 0" "${result% *}" 0
  info "SIGINT: ${result#* } ms"
  docker rm -f "$name" >/dev/null

  start_pause "$name"
  local t0 t1
  t0=$(now_ms)
  docker stop -t 10 "$name" >/dev/null
  t1=$(now_ms)
  eq "docker stop -t 10 ends pause with exit 0, not killed" "$(docker inspect -f '{{.State.ExitCode}}' "$name")" 0
  info "docker stop: $((t1 - t0)) ms"
  docker rm -f "$name" >/dev/null
}

test_7_pause_as_pid_1() {
  run_host_test 7 "pause as pid 1: reaps orphans, ends on SIGTERM and SIGINT with exit 0, under 1 MiB" test_7_body
}

test_7b_body() {
  local name="${RUN_ID}-pause-quiet" output_file="${WORK_DIR}/pause-quiet.out" state result client code
  printf 'bytes on stdin\nmore bytes\n' | docker run -i --init=false --name "$name" --platform "$PLATFORM" -v "${TOOLS_VOL}:/opt/sbx:ro" $SEVEN_CAPS \
    "$PROBE_IMAGE" /opt/sbx/bin/pause --version -h an-argument >"$output_file" 2>&1 &
  client=$!
  wait_for_container_state "$name" running
  sleep 3
  eq "pause with arguments and bytes on stdin still runs after the end of its input" "$(docker inspect -f '{{.State.Status}}' "$name")" running
  docker exec "$name" bash -c "$ORPHAN_SCRIPT"
  sleep 4
  state=$(orphan_state "$name")
  eq "pause with arguments and bytes on stdin reaps the orphan" "$state" "gone zombies=0 "
  result=$(exit_after_signal "$name" TERM)
  eq "SIGTERM ends pause with exit 0" "${result% *}" 0
  wait "$client"
  code=$?
  eq "the client of docker run gets the exit code 0" "$code" 0
  eq "pause printed nothing on stdout and stderr" "$(cat "$output_file")" ""
  docker rm -f "$name" >/dev/null
}

test_7b_pause_is_silent() {
  run_host_test 7b "pause prints nothing and ignores its arguments and its stdin; it still reaps and stops on SIGTERM" test_7b_body
}
