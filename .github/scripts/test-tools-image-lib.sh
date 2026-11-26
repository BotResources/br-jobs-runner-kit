#!/usr/bin/env bash

FAILED_TESTS=0
RESULTS_FILE="${RESULTS_FILE:-/dev/null}"

ASSERTIONS='
pass() { echo "PASS: $1"; }
fail() { echo "FAIL: $1"; }
eq() { if [ "$2" = "$3" ]; then pass "$1"; else fail "$1: wanted [$3] got [$2]"; fi; }
ne0() { if [ "$2" -ne 0 ]; then pass "$1"; else fail "$1: exit 0"; fi; }
has() { case "$2" in *"$3"*) pass "$1" ;; *) fail "$1: [$2] lacks [$3]" ;; esac; }
lacks() { case "$2" in *"$3"*) fail "$1: [$2] holds [$3]" ;; *) pass "$1" ;; esac; }
info() { echo "INFO: $*"; }
skip() { echo "SKIP: $*"; }
'
eval "$ASSERTIONS"

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
  docker run --rm --platform "$PLATFORM" -e SAVE_FLAGS -e RESTORE_FLAGS -e INIT_FLAGS -v "${TOOLS_VOL}:/opt/sbx:ro" "$@"
}

probe_script() {
  local script="$1"
  shift
  probe_run "$@" "$PROBE_IMAGE" bash -c "${ASSERTIONS}${script}"$'\n''echo END'
}

probe_checks() {
  local output code
  output=$(probe_script "$@" 2>&1)
  code=$?
  if ! grep -qx END <<<"$output"; then
    echo "FAIL: the script did not reach its end (exit=${code})"
  elif [ "$code" -ne 0 ]; then
    echo "FAIL: the script exited with ${code}"
  fi
  grep -vx END <<<"$output" || true
}

verdict_of_script() {
  local id="$1" title="$2" output="$3"
  local info asserts
  info=$(grep -E '^(INFO|FAIL|SKIP):' <<<"$output" | tr '\n' ';' | cut -c1-1500)
  asserts=$(grep -c '^PASS:' <<<"$output")
  if grep -q '^FAIL:' <<<"$output"; then
    record FAIL "$id" "$title" "${info}"
    grep '^FAIL:' <<<"$output" | tail -n 25 >&2
  elif ! grep -qx END <<<"$output"; then
    record FAIL "$id" "$title" "the test did not reach its end; ${info}"
  elif [ "$asserts" -eq 0 ] && grep -q '^SKIP:' <<<"$output"; then
    record SKIP "$id" "$title" "${info}"
  elif [ "$asserts" -eq 0 ]; then
    record FAIL "$id" "$title" "no assertion ran"
  else
    record PASS "$id" "$title" "assertions=${asserts}; ${info}"
  fi
}

run_host_test() {
  local id="$1" title="$2" body="$3" output
  output=$("$body" 2>&1)
  echo "$output" >"${WORK_DIR}/last_output"
  verdict_of_script "$id" "$title" "${output}"$'\n'END
}

run_scripted_test() {
  local id="$1" title="$2" script="$3"
  shift 3
  local output
  output=$(probe_checks "$script" "$@" 2>&1)
  echo "$output" >"${WORK_DIR}/last_output"
  verdict_of_script "$id" "$title" "${output}"$'\n'END
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

daemon_runs_platform_natively() {
  local daemon_arch
  daemon_arch=$(docker info --format '{{.Architecture}}')
  case "${daemon_arch}:${PLATFORM}" in
    x86_64:linux/amd64 | aarch64:linux/arm64 | arm64:linux/arm64) return 0 ;;
    *) return 1 ;;
  esac
}

readme_flags() {
  local readme="$1" pattern="$2" flags
  flags=$(sed -n "$pattern" "$readme")
  if [ -z "$flags" ]; then
    echo "the README line for ${pattern} was not found" >&2
    return 1
  fi
  echo "$flags"
}
