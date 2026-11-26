SCEN_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
. "$SCEN_DIR/../lib.sh"
. "$SCEN_DIR/../names.env"
. "$SCEN_DIR/names-chart.env"
if [ -f "$CREDS_ENV" ]; then
  set -a
  . "$CREDS_ENV"
  set +a
fi
: "${CHART_DIR:=$(cd "$SCEN_DIR/../../.." && pwd)/charts/br-sandbox-chart}"
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
now_ms() { perl -MTime::HiRes=time -e 'printf "%d\n", time()*1000'; }
new_id() { openssl rand -hex 4; }
count_lines() { wc -l | tr -d ' '; }

sx() { local ns=$1 pod=$2; shift 2; k -n "$ns" exec "$pod" -c "$SANDBOX_CONTAINER" -- "$@"; }
sxi() { local ns=$1 pod=$2; shift 2; k -n "$ns" exec -i "$pod" -c "$SANDBOX_CONTAINER" -- "$@"; }

warm_list() {
  local sel="$LBL_POOL=warm"
  if [ -n "${2:-}" ]; then sel="$sel,$LBL_POOL_ID=$2"; fi
  k -n "$1" get pods -l "$sel" --field-selector status.phase=Running -o json |
    jq -r '.items[] | select(.metadata.deletionTimestamp == null)
      | select(any(.status.conditions[]?; .type == "Ready" and .status == "True"))
      | "\(.metadata.name) \(.metadata.resourceVersion)"'
}

wait_warm() {
  local ns=$1 want=$2 limit=$3 pool=${4:-} end
  end=$(($(date +%s) + limit))
  while [ "$(warm_list "$ns" "$pool" | count_lines)" -lt "$want" ]; do
    if [ "$(date +%s)" -ge "$end" ]; then return 1; fi
    sleep 1
  done
}

claim_one() {
  k -n "$1" label pod "$2" --resource-version="$3" --overwrite \
    "$LBL_POOL=claimed" "$LBL_THREAD=$4" "$LBL_AGENT=$5" > /dev/null 2>&1
}

claim() {
  local ns=$1 name rv
  while read -r name rv; do
    if claim_one "$ns" "$name" "$rv" "$2" "$3"; then
      echo "$name"
      return 0
    fi
  done < <(warm_list "$ns" "${4:-}")
  return 1
}

claim_to() {
  local var=$1 pod
  shift
  pod=$(claim "$@") || return 1
  TRACKED_PODS+=("$1 $pod")
  printf -v "$var" '%s' "$pod"
}

track_pod() { TRACKED_PODS+=("$1 $2"); }

release_pod() { k -n "$1" delete pod "$2" --wait=false --grace-period=1 > /dev/null 2>&1; }

pod_image() { k -n "$1" get pod "$2" -o jsonpath="{.spec.containers[0].image}"; }

control_pod_to() {
  local var=$1 ns=$2 image=$3 name
  name="ctl-$(new_id)"
  k -n "$ns" run "$name" --image="$image" --restart=Never --command -- sleep 3600 > /dev/null || return 1
  TRACKED_PODS+=("$ns $name")
  k -n "$ns" wait --for=condition=Ready "pod/$name" --timeout=120s > /dev/null || return 1
  printf -v "$var" '%s' "$name"
}

verdict() {
  local name=$1 detail=$2
  shift 2
  if "$@"; then check "$name" pass "$detail"; else check "$name" fail "$detail"; fi
}

with_timeout() {
  local secs=$1 pid guard rc
  shift
  "$@" &
  pid=$!
  (sleep "$secs"; kill "$pid" 2> /dev/null) &
  guard=$!
  wait "$pid"
  rc=$?
  kill "$guard" 2> /dev/null
  wait "$guard" 2> /dev/null
  return $rc
}

lock_timed() {
  if [ -n "${TIMING_LOCK_HELD:-}" ]; then return 0; fi
  mkdir -p "$(dirname "$TIMING_LOCK")"
  until mkdir "$TIMING_LOCK" 2> /dev/null; do sleep 2; done
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
  if [ -z "${PREFLIGHT_DONE:-}" ]; then preflight; fi
  mkdir -p "$BATTERY_OUT"
  route_kubectl
  trap scenario_cleanup EXIT
}

scenario_end() { [ "$SCENARIO_FAILS" -eq 0 ]; exit $?; }

set_secret() {
  printf '%s' "$4" | k -n "$1" create secret generic "$2" --from-file="$3=/dev/stdin" --dry-run=client -o yaml | k apply -f - > /dev/null
}

proxy_restart() {
  k -n "$1" rollout restart "deploy/${PROXY_DEPLOY_PREFIX}$2" > /dev/null &&
    k -n "$1" rollout status "deploy/${PROXY_DEPLOY_PREFIX}$2" --timeout=180s > /dev/null
}

probe() {
  local ns=$1 pod=$2 mode=$3 input=$4
  sxi "$ns" "$pod" env "PROBE_INPUT=$input" "$SANDBOX_PYTHON" - "$mode" < "$SCEN_DIR/probe.py"
}

probe_ctl() {
  local ns=$1 pod=$2 mode=$3 input=$4
  k -n "$ns" exec -i "$pod" -- env "PROBE_INPUT=$input" "$SANDBOX_PYTHON" - "$mode" < "$SCEN_DIR/probe.py"
}

proxy_hostport() { sx "$1" "$2" printenv HTTPS_PROXY | sed -e 's|^[a-z]*://||' -e 's|/$||'; }

s3_via_toolbox() {
  k -n "$NS_TOOLS" exec -i "$TOOLBOX_POD" -- curl -sS --fail --aws-sigv4 "aws:amz:${S3_REGION:-us-east-1}:s3" \
    --user "$S3_ACCESS_KEY:$S3_SECRET_KEY" -H "x-amz-content-sha256: UNSIGNED-PAYLOAD" "$@"
}
s3_put() { s3_via_toolbox -T - "$S3_ENDPOINT_IN_CLUSTER/$S3_BUCKET/$1" < "$2"; }
s3_get() { s3_via_toolbox "$S3_ENDPOINT_IN_CLUSTER/$S3_BUCKET/$1" > "$2"; }
s3_delete() { s3_via_toolbox -X DELETE "$S3_ENDPOINT_IN_CLUSTER/$S3_BUCKET/$1"; }

median_of() { sort -n | awk '{a[NR]=$1} END {if (NR) print a[int((NR+1)/2)]}'; }
percentile_of() { sort -n | awk -v p="$1" '{a[NR]=$1} END {if (NR) print a[int((NR*p+99)/100)]}'; }
max_of() { sort -n | tail -1; }

keeper_pause() {
  KEEPER_REPLICAS=$(k -n "$KEEPER_NS" get deploy "$KEEPER_DEPLOY" -o jsonpath='{.spec.replicas}' 2> /dev/null)
  if [ -n "$KEEPER_REPLICAS" ]; then k -n "$KEEPER_NS" scale deploy "$KEEPER_DEPLOY" --replicas=0 > /dev/null; fi
}

keeper_resume() {
  if [ -n "${KEEPER_REPLICAS:-}" ]; then k -n "$KEEPER_NS" scale deploy "$KEEPER_DEPLOY" --replicas="$KEEPER_REPLICAS" > /dev/null; fi
}

pool_rs() { printf '%s%s\n' "$RS_PREFIX" "$1"; }

set_pool_replicas() { k -n "$1" scale rs "$(pool_rs "$2")" --replicas="$3" > /dev/null; }

pool_replicas() { k -n "$1" get rs "$(pool_rs "$2")" -o jsonpath='{.spec.replicas}'; }

tree_hash() {
  sx "$1" "$2" sh -c "cd $3 && find . -type f -exec md5sum {} + | sort | md5sum | cut -d' ' -f1"
}
