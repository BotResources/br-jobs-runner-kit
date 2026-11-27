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
