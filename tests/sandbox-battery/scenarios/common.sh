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
pool_of() { printf '%s\n' "${1#"$NS_PREFIX"-}"; }
now_ms() { perl -MTime::HiRes=time -e 'printf "%d\n", time()*1000'; }
new_id() { openssl rand -hex 4; }
count_lines() { wc -l | tr -d ' '; }

sx() { local ns=$1 pod=$2; shift 2; k -n "$ns" exec "$pod" -c "$SANDBOX_CONTAINER" -- "$@"; }
sxi() { local ns=$1 pod=$2; shift 2; k -n "$ns" exec -i "$pod" -c "$SANDBOX_CONTAINER" -- "$@"; }

warm_list() {
  k -n "$1" get pods -l "$LBL_POOL=warm,$LBL_POOL_ID=$(pool_of "$1")" --field-selector status.phase=Running -o json |
    jq -r '.items[] | select(.metadata.deletionTimestamp == null)
      | select(any(.status.conditions[]?; .type == "Ready" and .status == "True"))
      | "\(.metadata.name) \(.metadata.resourceVersion)"'
}

wait_warm() {
  local ns=$1 want=$2 limit=$3 end
  end=$(($(date +%s) + limit))
  while [ "$(warm_list "$ns" | count_lines)" -lt "$want" ]; do
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
  done < <(warm_list "$ns")
  return 1
}

claim_to() {
  local var=$1 claimed_name
  shift
  claimed_name=$(claim "$@") || return 1
  TRACKED_PODS+=("$1 $claimed_name")
  printf -v "$var" '%s' "$claimed_name"
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

proxy_pods() {
  k -n "$(egress_ns_of "$1" "$2")" get pods -l "$LBL_ROLE=$ROLE_PROXY" --field-selector status.phase=Running \
    -o jsonpath='{.items[*].status.podIP}' 2> /dev/null
}

proxy_cluster_ip() { k -n "$(egress_ns_of "$1" "$2")" get svc "$PROXY_SERVICE" -o jsonpath='{.spec.clusterIP}' 2> /dev/null; }

proxy_endpoint() {
  local ip
  ip=$(proxy_cluster_ip "$1" "$2")
  if [ -n "$ip" ]; then printf '%s:%s\n' "$ip" "$PROXY_PORT"; fi
}

internal_cidrs_value() { helm_ctx get values "$HELM_RELEASE" -n "$HELM_NAMESPACE" -o json | jq -r '.network.internal_cidrs | join(",")'; }

conn_docker() { docker logs "$1" 2>&1 | grep -c ' CONN ' || true; }
conn_pod() { k -n "$NS_TESTSVC" logs "$1" 2> /dev/null | grep -c ' CONN ' || true; }

root_pod_to() {
  local var=$1 ns=$2 template created
  template=$(pod_template "$ns") || return 1
  created=$(as_id "$NETCHECK_ID" "$ns" create -o jsonpath='{.metadata.name}' -f - <<< "$(variant "$template" '.metadata.labels["br.sandbox/pool"] = "battery" | .spec.containers |= map(if .name == $agent then .securityContext.runAsUser = 0 | .securityContext.runAsGroup = 0 | .securityContext.runAsNonRoot = false else . end)')") || return 1
  TRACKED_PODS+=("$ns $created")
  k -n "$ns" wait --for=condition=Ready "pod/$created" --timeout=120s > /dev/null || return 1
  printf -v "$var" '%s' "$created"
}

need_proxy() {
  if [ -n "$(proxy_pods "$2" "$3")" ]; then
    check "$1-proxy-present" pass "running pod with $LBL_ROLE=$ROLE_PROXY in $(egress_ns_of "$2" "$3")"
  else
    check "$1-proxy-present" fail "no running pod with $LBL_ROLE=$ROLE_PROXY in $(egress_ns_of "$2" "$3"); the checks that need a proxy are skipped"
    return 1
  fi
}

proxy_restart() {
  local end
  k -n "$1" delete pod -l "$LBL_ROLE=$ROLE_PROXY" --wait=true > /dev/null || return 1
  end=$(($(date +%s) + 180))
  until k -n "$1" wait --for=condition=Ready pod -l "$LBL_ROLE=$ROLE_PROXY" --timeout=5s > /dev/null 2>&1; do
    if [ "$(date +%s)" -ge "$end" ]; then return 1; fi
    sleep 2
  done
  until [ -n "$(k -n "$1" get endpoints "$PROXY_SERVICE" -o jsonpath='{.subsets[*].addresses[*].ip}' 2> /dev/null)" ]; do
    if [ "$(date +%s)" -ge "$end" ]; then return 1; fi
    sleep 1
  done
  sleep "${PROXY_RESTART_SETTLE_SECONDS:-8}"
}

probe() {
  local ns=$1 pod=$2 mode=$3 input=$4
  sxi "$ns" "$pod" env "PROBE_INPUT=$input" "$SANDBOX_PYTHON" - "$mode" < "$SCEN_DIR/probe.py"
}

probe_ctl() {
  local ns=$1 pod=$2 mode=$3 input=$4
  k -n "$ns" exec -i "$pod" -- env "PROBE_INPUT=$input" "$SANDBOX_PYTHON" - "$mode" < "$SCEN_DIR/probe.py"
}

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

pool_rs() { printf '%s%s\n' "$RS_PREFIX" "$(pool_of "$1")"; }

set_pool_replicas() { k -n "$1" scale rs "$(pool_rs "$1")" --replicas="$2" > /dev/null; }

pool_replicas() { k -n "$1" get rs "$(pool_rs "$1")" -o jsonpath='{.spec.replicas}'; }

chart_namespaces() {
  { k get ns -l "$LBL_MANAGED=true" -o json; k get ns -l "$LBL_EGRESS=true" -o json; } | jq -s -c '[.[].items[].metadata.name]'
}

ns_label() { k get ns "$1" -o json | jq -r --arg label "$2" '.metadata.labels[$label] // ""'; }

pod_template() {
  local name
  name=$(warm_list "$1" | head -1 | cut -d' ' -f1)
  k -n "$1" get pod "$name" -o json | jq -c '
    del(.status, .metadata.uid, .metadata.resourceVersion, .metadata.creationTimestamp, .metadata.ownerReferences,
        .metadata.generateName, .metadata.managedFields, .metadata.namespace, .spec.nodeName)'
}

variant() {
  jq -c --arg role "$LBL_ROLE" --arg seed "$SEED_CONTAINER" --arg agent "$SANDBOX_CONTAINER" --arg name "adm-$(new_id)" "$2 | .metadata.name = \$name" <<< "$1"
}

as_id() {
  local who=$1 ns=$2
  shift 2
  k --as="$who" -n "$ns" "$@"
}

accepted() { "$@" > /dev/null 2>&1; }

refused_with() {
  local code=$1 out
  shift
  out=$("$@" 2>&1) && return 1
  printf '%s\n' "$out" | grep -q -E "BR_SANDBOX_${code}([^A-Z_]|$)"
}

refused_forbidden() {
  local out
  out=$("$@" 2>&1) && return 1
  printf '%s\n' "$out" | grep -q -i 'forbidden'
}

tree_hash() {
  sx "$1" "$2" sh -c "cd $3 && find . -type f -exec md5sum {} + | sort | md5sum | cut -d' ' -f1"
}
