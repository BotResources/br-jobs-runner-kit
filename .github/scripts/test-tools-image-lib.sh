#!/usr/bin/env bash

FAILED_TESTS=0
RESULTS_FILE="${RESULTS_FILE:-/dev/null}"

record() {
  local verdict="$1" id="$2" title="$3" detail="${4:-}"
  local line="RESULT ${id} ${verdict} ${title} | ${detail}"
  echo "$line"
  echo "$line" >>"$RESULTS_FILE"
  if [ "$verdict" = FAIL ]; then
    FAILED_TESTS=$((FAILED_TESTS + 1))
  fi
}

now_ms() {
  python3 -I -c 'import time; print(int(time.time() * 1000))'
}

tools_run() {
  docker run --rm --platform "$PLATFORM" "$@"
}

probe_run() {
  docker run --rm --platform "$PLATFORM" -v "${TOOLS_VOL}:/opt/sbx:ro" "$@"
}

probe_script() {
  local script="$1"
  shift
  probe_run "$@" "$PROBE_IMAGE" bash -c "$script"
}

verdict_of_script() {
  local id="$1" title="$2" output="$3" code="$4"
  local info
  info=$(grep -E '^(INFO|FAIL):' <<<"$output" | tr '\n' ';' | cut -c1-1500)
  if [ "$code" -ne 0 ] || grep -q '^FAIL:' <<<"$output"; then
    record FAIL "$id" "$title" "exit=${code} ${info}"
    echo "$output" | tail -n 25 >&2
  else
    record PASS "$id" "$title" "$info"
  fi
}

run_scripted_test() {
  local id="$1" title="$2" script="$3"
  shift 3
  local output code
  output=$(probe_script "$script" "$@" 2>&1)
  code=$?
  verdict_of_script "$id" "$title" "$output" "$code"
  echo "$output" >"${WORK_DIR}/last_output"
}

wait_for_container_state() {
  local name="$1" wanted="$2" tries=50
  while [ "$tries" -gt 0 ]; do
    if [ "$(docker inspect -f '{{.State.Status}}' "$name" 2>/dev/null)" = "$wanted" ]; then
      return 0
    fi
    sleep 0.2
    tries=$((tries - 1))
  done
  return 1
}
