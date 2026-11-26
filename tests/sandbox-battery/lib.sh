#!/usr/bin/env bash
BATTERY_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$BATTERY_DIR/names.env"
EVIDENCE="${EVIDENCE:-$HOME/sbx-build-work/battery/evidence}"
WORK="${WORK:-$HOME/sbx-build-work/battery}"
mkdir -p "$EVIDENCE" "$WORK"

log() { printf '[%s] %s\n' "$(date -u +%H:%M:%S)" "$*" >&2; }

die() { log "ERROR: $*"; exit 1; }

load_now() {
  local raw
  raw="$(sysctl -n vm.loadavg)"
  raw="${raw#\{ }"
  raw="${raw% \}}"
  echo "$raw"
}

free_gib() {
  df -k / | awk 'NR==2 {printf "%.1f", $4/1048576}'
}

preflight() {
  local free
  free="$(free_gib)"
  log "preflight: free disk ${free} GiB, load $(load_now)"
  if awk -v f="$free" -v m="$MIN_FREE_GIB" 'BEGIN{exit !(f<m)}'; then
    log "preflight: free disk ${free} GiB is under ${MIN_FREE_GIB} GiB, load $(load_now): stop"
    exit 3
  fi
}

record() {
  local name="$1" value="$2" unit="$3" load l1 l5
  load="$(load_now)"
  l1="${load%% *}"
  l5="$(echo "$load" | awk '{print $2}')"
  printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$name" "$value" "$unit" "$l1" "$l5" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" >>"$EVIDENCE/measurements.tsv"
}

check() {
  local name="$1" verdict="$2" detail="${3:-}"
  case "$verdict" in pass | fail) ;; *) die "check verdict must be pass or fail" ;; esac
  printf '%s\t%s\t%s\t%s\n' "$name" "$verdict" "$detail" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" >>"$EVIDENCE/checks.tsv"
  log "check $name: $verdict $detail"
}
