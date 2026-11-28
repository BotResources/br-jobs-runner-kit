SCEN_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
. "$SCEN_DIR/../lib.sh"
. "$SCEN_DIR/../names.env"
. "$SCEN_DIR/names-chart.env"
if [ -f "$CREDS_ENV" ]; then
  set -a
  . "$CREDS_ENV"
  set +a
fi
map_creds() {
  GIT_TOKEN=${GITEA_TOKEN:-${GIT_TOKEN:-}}
  PKG_TOKEN=${GITEA_TOKEN:-${PKG_TOKEN:-}}
}
map_creds

SCENARIO_FAILS=0
TRACKED_PODS=()
CTX=$CTX_TEST
NODE=$NODE_TEST
KC_VIA_NODE=0

if declare -f check >/dev/null; then
  eval "lib_check() $(declare -f check | tail -n +2)"
  check() {
    lib_check "$@"
    if [ "$2" = fail ]; then SCENARIO_FAILS=$((SCENARIO_FAILS + 1)); fi
    if [ -n "${SCENARIO_TALLY:-}" ]; then printf '%s\t%s\n' "$1" "$2" >> "$SCENARIO_TALLY"; fi
  }
fi

route_kubectl() {
  if kubectl --context "$CTX" --request-timeout=5s get --raw /readyz > /dev/null 2>&1; then KC_VIA_NODE=0; else KC_VIA_NODE=1; fi
}

k() {
  if [ "$KC_VIA_NODE" = 1 ]; then docker exec -i "$NODE" kubectl "$@"; else kubectl --context "$CTX" "$@"; fi
}

use_closed() {
  CTX=$CTX_CLOSED
  NODE=$NODE_CLOSED
  route_kubectl
  if [ -f "$CLOSED_CREDS_ENV" ]; then
    set -a
    . "$CLOSED_CREDS_ENV"
    set +a
  fi
  map_creds
}

helm_ctx() { helm --kube-context "$CTX" "$@"; }
ns_of() { printf '%s-%s-%s\n' "$NS_PREFIX" "$1" "$2"; }
egress_ns_of() { printf '%s%s\n' "$(ns_of "$1" "$2")" "$EGRESS_SUFFIX"; }
chart_at_least() { [ "$(printf '%s\n%s\n' "$1" "$CHART_VERSION" | sort -V | head -1)" = "$1" ]; }
pool_id_strip() {
  if [ "${POOL_ID_STRIP+set}" = set ]; then
    printf '%s' "$POOL_ID_STRIP"
  elif chart_at_least 0.4.0; then
    printf ''
  else
    printf '%s-' "$NS_PREFIX"
  fi
}
pool_of() { printf '%s\n' "${1#"$(pool_id_strip)"}"; }
now_ms() { perl -MTime::HiRes=time -e 'printf "%d\n", time()*1000'; }
new_id() { openssl rand -hex 4; }
count_lines() { wc -l | tr -d ' '; }

sx() { local ns=$1 pod=$2; shift 2; k -n "$ns" exec "$pod" -c "$SANDBOX_CONTAINER" -- "$@"; }
sxi() { local ns=$1 pod=$2; shift 2; k -n "$ns" exec -i "$pod" -c "$SANDBOX_CONTAINER" -- "$@"; }

. "$SCEN_DIR/common-pool.sh"
. "$SCEN_DIR/common-run.sh"
. "$SCEN_DIR/common-probe.sh"
. "$SCEN_DIR/common-admission.sh"
. "$SCEN_DIR/common-policy.sh"
. "$SCEN_DIR/common-isolated.sh"
