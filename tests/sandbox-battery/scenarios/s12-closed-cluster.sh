#!/usr/bin/env bash
HERE=$(cd "$(dirname "$0")" && pwd)
. "$HERE/common.sh"
scenario_begin s12-closed-cluster

rendered_file="$BATTERY_OUT/closed-cluster-rendered.yaml"

render_with_mirror() {
  helm template "$HELM_RELEASE" "$CHART_DIR" -n "$HELM_NAMESPACE" -f "$CHART_VALUES_FILE" \
    --set "$VALUES_REGISTRY_MIRROR=$CLOSED_MIRROR_HOST" --set-json "$VALUES_PULL_SECRETS=[\"$CLOSED_PULL_SECRET\"]"
}

pods_without_pull_secret() {
  k apply --dry-run=client --validate=false -o json -f - < "$rendered_file" | jq -s '[.[] | (if .kind == "List" then .items[] else . end)
    | (if .kind == "Pod" then .spec elif .spec.template.spec then .spec.template.spec elif .spec.jobTemplate.spec.template.spec then .spec.jobTemplate.spec.template.spec else empty end)
    | select((.imagePullSecrets // []) | length == 0)] | length'
}

if ! render_with_mirror > "$rendered_file"; then check closed-cluster-render fail "helm template failed with the mirror values"; scenario_end; fi
check closed-cluster-render pass "chart renders with registry mirror $CLOSED_MIRROR_HOST and pull secret $CLOSED_PULL_SECRET"

foreign_images=$(grep -E '^[[:space:]]*-?[[:space:]]*image:' "$rendered_file" | grep -v -F "$CLOSED_MIRROR_HOST/" | count_lines)
verdict closed-cluster-all-images-via-mirror "$foreign_images image fields outside the mirror" test "$foreign_images" = 0
unpulled=$(pods_without_pull_secret)
verdict closed-cluster-pull-secrets-everywhere "$unpulled pod templates without imagePullSecrets" test "$unpulled" = 0
vendor_hits=$(grep -c -i -E "$VENDOR_HOST_REGEX" "$rendered_file")
record closed-cluster-vendor-host-hits "$vendor_hits" hits
verdict closed-cluster-no-vendor-host-rendered "$vendor_hits vendor or upstream registry hosts in the rendered objects" test "$vendor_hits" = 0
token_hits=$(grep -c -E "$VENDOR_TOKEN_REGEX" "$rendered_file")
verdict closed-cluster-no-control-plane-token "$token_hits mentions of the vendor control-plane token" test "$token_hits" = 0

use_closed
cleanup_extra() { keeper_resume; set_pool_replicas "$ns" "$POOL_ID_DEFAULT" "$original_replicas" 2> /dev/null; }

k get nodes > /dev/null 2>&1 || { check closed-cluster-closed-cluster fail "cluster $CTX_CLOSED not reachable"; scenario_end; }
install_on_closed() {
  k create namespace "$HELM_NAMESPACE" > /dev/null 2>&1
  if [ "$KC_VIA_NODE" = 1 ]; then
    helm template "$HELM_RELEASE" "$CHART_DIR" -n "$HELM_NAMESPACE" -f "$CLOSED_VALUES_FILE" \
      --set "$VALUES_REGISTRY_MIRROR=$CLOSED_MIRROR_HOST" --set-json "$VALUES_PULL_SECRETS=[\"$CLOSED_PULL_SECRET\"]" | k apply -f - > /dev/null
  else
    helm_ctx upgrade --install "$HELM_RELEASE" "$CHART_DIR" -n "$HELM_NAMESPACE" -f "$CLOSED_VALUES_FILE" \
      --set "$VALUES_REGISTRY_MIRROR=$CLOSED_MIRROR_HOST" --set-json "$VALUES_PULL_SECRETS=[\"$CLOSED_PULL_SECRET\"]" --wait --timeout 600s > /dev/null
  fi
}

install_on_closed || { check closed-cluster-closed-install fail "chart install on $CTX_CLOSED failed"; scenario_end; }
check closed-cluster-closed-install pass "chart installed on $CTX_CLOSED through the mirror"

ns=$(ns_of "$CLIENT_A" "$NET_ALLOW")
full_ns=$(ns_of "$CLIENT_A" "$NET_FULL")
original_replicas=$(pool_replicas "$ns" "$POOL_ID_DEFAULT")
wait_warm "$ns" 1 300 "$POOL_ID_DEFAULT" || { check closed-cluster-warm-pod fail "no warm pod in $ns"; scenario_end; }
wait_warm "$full_ns" 1 300 "$POOL_ID_DEFAULT" || { check closed-cluster-warm-pod-full fail "no warm pod in $full_ns"; scenario_end; }

outside_mirror=$(k get pods -A -o json | jq -r --arg ns "$HELM_NAMESPACE" --arg mirror "$CLOSED_MIRROR_HOST/" --argjson managed "$(k get ns -l "$LBL_MANAGED=true" -o json | jq -c '[.items[].metadata.name]')" '
  .items[] | select(.metadata.namespace as $n | $n == $ns or ($managed | index($n)))
  | .spec.containers[], (.spec.initContainers[]?) | .image | select(startswith($mirror) | not)' | count_lines)
verdict closed-cluster-running-images-via-mirror "$outside_mirror running images outside the mirror" test "$outside_mirror" = 0

claim_to internet_pod "$full_ns" "closed-cluster-$(new_id)" agent-a "$POOL_ID_DEFAULT"
routed=$(probe "$full_ns" "$internet_pod" tcp '[{"host":"1.1.1.1","port":443},{"host":"8.8.8.8","port":443}]' | jq -s '[.[] | select(.open)] | length')
verdict closed-cluster-no-route-to-internet "$routed public targets reached from a mode full sandbox" test "$routed" = 0

thread="closed-cluster-$(new_id)"
started=$(now_ms)
claim_to pod "$ns" "$thread" agent-a "$POOL_ID_DEFAULT"
sx "$ns" "$pod" true
record closed-cluster-open-ms "$(($(now_ms) - started))" ms
sx "$ns" "$pod" sh -c 'mkdir -p /work/tree && i=0 && while [ $i -lt 200 ]; do echo "data-$i" > "/work/tree/f$i"; i=$((i + 1)); done && head -c 5000000 /dev/urandom > /work/tree/big.bin'
before=$(tree_hash "$ns" "$pod" /work/tree)

state_file="$BATTERY_OUT/closed-cluster-$thread.tar"
sx "$ns" "$pod" "$AGENT_BSDTAR" -c --format pax --numeric-owner -f - -C /work tree > "$state_file"
verdict closed-cluster-save "state tar uploaded to the store" s3_put "closed-cluster/$thread/state.tar" "$state_file"
release_pod "$ns" "$pod"
k -n "$ns" wait --for=delete "pod/$pod" --timeout=60s > /dev/null 2>&1

claim_to restored "$ns" "$thread" agent-a "$POOL_ID_DEFAULT"
s3_get "closed-cluster/$thread/state.tar" "$state_file"
sxi "$ns" "$restored" "$AGENT_BSDTAR" -x -p --numeric-owner -f - -C /work < "$state_file"
after=$(tree_hash "$ns" "$restored" /work/tree)
verdict closed-cluster-restore "tree hash before $before, after $after" test "$before" = "$after"
s3_delete "closed-cluster/$thread/state.tar" > /dev/null
rm -f "$state_file"

proxy=$(proxy_hostport "$ns" "$restored")
statuses=$(probe "$ns" "$restored" connect "[{\"proxy\":\"$proxy\",\"target\":\"$TEST_HOST_GIT:443\"},{\"proxy\":\"$proxy\",\"target\":\"example.com:443\"}]" | jq -r '.status' | tr '\n' ' ')
verdict closed-cluster-allow-mode "listed site and unlisted site answer: $statuses" test "$statuses" = "200 403 "

vendor_log_hits=$(k -n "$ns" logs -l "$LBL_ROLE=$ROLE_PROXY" --tail=-1 | grep -c -i -E "$VENDOR_HOST_REGEX")
verdict closed-cluster-proxy-no-vendor-call "$vendor_log_hits vendor hosts in the proxy logs" test "$vendor_log_hits" = 0
token_env=$(k -n "$ns" get deploy -o json | jq '[.items[].spec.template.spec.containers[].env[]?.name | select(test("IRON_PROXY_TOKEN"))] | length')
verdict closed-cluster-proxy-token-unset "$token_env proxy Deployments set the control-plane token" test "$token_env" = 0

keeper_pause
set_pool_replicas "$ns" "$POOL_ID_DEFAULT" $((original_replicas + 2))
verdict closed-cluster-scale "pool grows by two warm pods" wait_warm "$ns" $((original_replicas + 2)) 180 "$POOL_ID_DEFAULT"

scenario_end
