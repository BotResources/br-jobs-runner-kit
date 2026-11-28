verdict() {
  local name=$1 detail=$2
  shift 2
  if "$@"; then check "$name" pass "$detail"; else check "$name" fail "$detail"; fi
}

with_timeout() {
  local secs=$1 pid guard rc
  shift
  exec 4<&0
  "$@" <&4 &
  pid=$!
  exec 4<&-
  (sleep "$secs"; kill "$pid" 2> /dev/null) > /dev/null 2>&1 &
  guard=$!
  wait "$pid"
  rc=$?
  kill "$guard" 2> /dev/null
  wait "$guard" 2> /dev/null
  return $rc
}

validate_timing_lock_wait() {
  TIMING_LOCK_LIMIT=${TIMING_LOCK_WAIT_SECONDS-21600}
  case "$TIMING_LOCK_LIMIT" in
    '' | *[!0-9]* | 0 | 0[0-9]*)
      check timing-lock-wait-valid fail "TIMING_LOCK_WAIT_SECONDS must be a whole number of 1 or more, got: $TIMING_LOCK_LIMIT"
      return 1
      ;;
  esac
}

validate_chart_version() {
  if [[ "$CHART_VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+([-+].*)?$ ]]; then return 0; fi
  check chart-version-valid fail "CHART_VERSION must be three numbers such as 0.4.0 (a suffix after - or + is allowed), got: $CHART_VERSION"
  return 1
}

skip_script() {
  check "$1-chart-version" skip "$1 needs chart 0.4.0 or later and CHART_VERSION is $CHART_VERSION"
}

lock_timed() {
  if [ -n "${TIMING_LOCK_HELD:-}" ]; then return 0; fi
  validate_timing_lock_wait || return 1
  mkdir -p "$(dirname "$TIMING_LOCK")"
  local waited=0 limit=$TIMING_LOCK_LIMIT
  until mkdir "$TIMING_LOCK" 2> /dev/null; do
    if [ "$waited" -ge "$limit" ]; then
      check timing-lock fail "the lock $TIMING_LOCK is still held after $limit s; if no test is running, remove the folder by hand"
      return 1
    fi
    sleep 2
    waited=$((waited + 2))
  done
  TIMING_LOCK_OWNED=1
}

unlock_timed() {
  if [ -n "${TIMING_LOCK_OWNED:-}" ]; then rmdir "$TIMING_LOCK"; TIMING_LOCK_OWNED=; fi
}

scenario_cleanup() {
  local entry
  if declare -f cleanup_extra >/dev/null; then cleanup_extra; fi
  for entry in "${TRACKED_PODS[@]:-}"; do
    if [ -n "$entry" ]; then release_pod ${entry}; fi
  done
  unlock_timed
}

scenario_begin() {
  SCENARIO_NAME=$1
  validate_timing_lock_wait || scenario_end
  validate_chart_version || scenario_end
  if [ -z "${PREFLIGHT_DONE:-}" ]; then preflight; fi
  mkdir -p "$BATTERY_OUT"
  route_kubectl
  trap scenario_cleanup EXIT
}

scenario_end() { [ "$SCENARIO_FAILS" -eq 0 ]; exit $?; }

set_secret() {
  printf '%s' "$4" | k -n "$1" create secret generic "$2" --from-file="$3=/dev/stdin" --dry-run=client -o yaml | k apply -f - > /dev/null
}
median_of() { sort -n | awk '{a[NR]=$1} END {if (NR) print a[int((NR+1)/2)]}'; }
percentile_of() { sort -n | awk -v p="$1" '{a[NR]=$1} END {if (NR) print a[int((NR*p+99)/100)]}'; }
max_of() { sort -n | tail -1; }
