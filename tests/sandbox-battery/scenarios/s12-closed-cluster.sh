#!/usr/bin/env bash
HERE=$(cd "$(dirname "$0")" && pwd)
. "$HERE/common.sh"
scenario_begin s12-closed-cluster

rendered_file="$BATTERY_OUT/closed-cluster-rendered.yaml"

render_with_mirror() {
  helm template "$HELM_RELEASE" "$CHART_DIR" -n "$HELM_NAMESPACE" -f "$CHART_VALUES_FILE" \
    --set "$VALUES_REGISTRY_MIRROR=$CLOSED_MIRROR_HOST" --set-json "$VALUES_PULL_SECRETS=[\"$CLOSED_PULL_SECRET\"]"
}

pod_template_specs='[.[] | (if .kind == "List" then .items[] else . end)
  | (if .kind == "Pod" then .spec elif .spec.template.spec then .spec.template.spec elif .spec.jobTemplate.spec.template.spec then .spec.jobTemplate.spec.template.spec else empty end)]'

nonempty_output() {
  local out
  out=$("$@") || return 1
  [ -n "$out" ] || return 1
  printf '%s\n' "$out"
}

read_pod_templates() { nonempty_output k create --dry-run=client --validate=false -o json -f - < "$rendered_file"; }
count_pod_templates() { jq -s "$pod_template_specs | length"; }
count_pods_without_pull_secret() { jq -s "$pod_template_specs | map(select((.imagePullSecrets // []) | length == 0)) | length"; }
positive() { case "$1" in '' | *[!0-9]* | 0 | 0[0-9]*) return 1 ;; esac; }
counted_zero() { [ "$1" = 0 ] && positive "$2"; }
fails() { ! "$@"; }

pod_without_secret='{"apiVersion":"v1","kind":"Pod","metadata":{"name":"a"},"spec":{"containers":[{"name":"c","image":"x"}]}}'
pod_with_secret='{"apiVersion":"v1","kind":"Pod","metadata":{"name":"b"},"spec":{"imagePullSecrets":[{"name":"s"}],"containers":[{"name":"c","image":"x"}]}}'
deployment_without_secret='{"apiVersion":"apps/v1","kind":"Deployment","metadata":{"name":"d"},"spec":{"template":{"spec":{"containers":[{"name":"c","image":"x"}]}}}}'
verdict closed-cluster-pull-secret-count-finds-bare-pod "a pod without imagePullSecrets is counted as 1" \
  test "$(printf '%s' "$pod_without_secret" | count_pods_without_pull_secret)" = 1
verdict closed-cluster-pull-secret-count-skips-pod-with-secret "a pod with imagePullSecrets is counted as 0" \
  test "$(printf '%s' "$pod_with_secret" | count_pods_without_pull_secret)" = 0
verdict closed-cluster-pull-secret-count-finds-bare-template "a deployment template without imagePullSecrets is counted as 1" \
  test "$(printf '%s%s' "$deployment_without_secret" "$pod_with_secret" | count_pods_without_pull_secret)" = 1
verdict closed-cluster-template-count-counts-templates "a pod and a deployment are counted as 2 pod templates" \
  test "$(printf '%s%s' "$pod_with_secret" "$deployment_without_secret" | count_pod_templates)" = 2
verdict closed-cluster-template-count-empty-is-not-positive "an empty read holds no pod template and the count is not positive" \
  fails positive "$(printf '' | count_pod_templates)"
verdict closed-cluster-read-fails-on-failed-command "a read whose command fails gives a failure" fails nonempty_output false
verdict closed-cluster-read-fails-on-empty-output "a read whose command prints nothing gives a failure" fails nonempty_output true
verdict closed-cluster-read-keeps-output "a read whose command prints keeps the output" test "$(nonempty_output echo kept)" = kept

if ! render_with_mirror > "$rendered_file"; then check closed-cluster-render fail "helm template failed with the mirror values"; scenario_end; fi
check closed-cluster-render pass "chart renders with registry mirror $CLOSED_MIRROR_HOST and pull secret $CLOSED_PULL_SECRET"

image_fields_of() { grep -E '^[[:space:]]*-?[[:space:]]*image:' "$rendered_file"; }
image_fields=$(image_fields_of | count_lines)
verdict closed-cluster-render-holds-image-fields "$image_fields image fields in the render" positive "$image_fields"
foreign_images=$(image_fields_of | grep -v -F "$CLOSED_MIRROR_HOST/" | count_lines)
verdict closed-cluster-all-images-via-mirror "$foreign_images of $image_fields image fields outside the mirror" counted_zero "$foreign_images" "$image_fields"

templates="not counted"
unpulled="not counted"
if pod_templates_json=$(read_pod_templates); then
  templates=$(count_pod_templates <<< "$pod_templates_json") || templates="not counted"
  unpulled=$(count_pods_without_pull_secret <<< "$pod_templates_json") || unpulled="not counted"
else
  check closed-cluster-pod-templates-read fail "kubectl create --dry-run=client failed or printed nothing for the render"
fi
verdict closed-cluster-render-holds-pod-templates "$templates pod templates in the render" positive "$templates"
verdict closed-cluster-pull-secrets-everywhere "$unpulled of $templates pod templates without imagePullSecrets" counted_zero "$unpulled" "$templates"

vendor_host_start="(^|[^[:alnum:]./-]|://)"
vendor_host_token="([[:alnum:]-]+\\.)*($VENDOR_HOST_REGEX)"
vendor_host_stop="([^[:alnum:].-]|\\.([^[:alnum:]-]|\$)|\$)"
vendor_pattern="$vendor_host_start$vendor_host_token$vendor_host_stop"
vendor_sample_hits() { printf '%s\n' "$1" | grep -c -i -E "$vendor_pattern"; }
vendor_found() { test "$(vendor_sample_hits "$1")" = 1; }
vendor_skipped() { test "$(vendor_sample_hits "$1")" = 0; }
verdict closed-cluster-vendor-search-finds-image-host "an image reference with a vendor host in the host position is found" \
  test "$(vendor_sample_hits 'image: "ghcr.io/example/tool:1"')" = 1
verdict closed-cluster-vendor-search-finds-url-host "a URL with a vendor host is found" \
  test "$(vendor_sample_hits 'url: https://api.iron.sh/v1')" = 1
verdict closed-cluster-vendor-search-finds-bare-registry "a bare vendor host after registry: is found" \
  test "$(vendor_sample_hits 'registry: ghcr.io')" = 1
verdict closed-cluster-vendor-search-finds-quoted-host-with-port "a quoted vendor host with a port after host: is found" \
  test "$(vendor_sample_hits 'host: "ghcr.io:443"')" = 1
verdict closed-cluster-vendor-search-finds-bare-value "a vendor host that is the whole value is found" \
  test "$(vendor_sample_hits 'value: iron.sh')" = 1
verdict closed-cluster-vendor-search-finds-list-item "a vendor host that is a list item is found" \
  test "$(vendor_sample_hits '- quay.io')" = 1
verdict closed-cluster-vendor-search-finds-server-and-url-keys "a vendor host after server: and after url: is found" \
  test "$(vendor_sample_hits 'server: docker.io:5000')$(vendor_sample_hits "url: 'gcr.io'")" = 11
verdict closed-cluster-vendor-search-skips-mirror-path "a vendor host after the mirror host is not found" \
  test "$(vendor_sample_hits "image: \"$CLOSED_MIRROR_HOST/ghcr.io/example/tool:1\"")" = 0
verdict closed-cluster-vendor-search-skips-mirror-path-in-value "a vendor host after the mirror host in a value is not found" \
  test "$(vendor_sample_hits "value: \"$CLOSED_MIRROR_HOST/ghcr.io/example\"")$(vendor_sample_hits "registry: $CLOSED_MIRROR_HOST/ghcr.io")" = 00
verdict closed-cluster-vendor-search-skips-mirror-host "the mirror host alone is not found" \
  test "$(vendor_sample_hits "registry: $CLOSED_MIRROR_HOST")" = 0
verdict closed-cluster-vendor-search-finds-comma-list "vendor hosts in a comma list are found" vendor_found 'value: ghcr.io,quay.io'
verdict closed-cluster-vendor-search-finds-flow-list "a vendor host in a flow list is found" vendor_found '[ghcr.io]'
verdict closed-cluster-vendor-search-finds-host-alone "a vendor host alone on a line is found" vendor_found 'ghcr.io'
verdict closed-cluster-vendor-search-finds-trailing-semicolon "a vendor host followed by a semicolon is found" vendor_found 'ghcr.io;'
verdict closed-cluster-vendor-search-finds-quoted-port "a quoted vendor host with a port is found" vendor_found '"ghcr.io:443"'
verdict closed-cluster-vendor-search-finds-dash-item "a vendor host after a dash is found" vendor_found '- ghcr.io'
verdict closed-cluster-vendor-search-finds-image-with-tag "an image whose first segment is a vendor host is found" vendor_found 'image: ghcr.io/org/img:1'
verdict closed-cluster-vendor-search-finds-url-with-path "a URL with a vendor host and a path is found" vendor_found 'https://ghcr.io/v2/'
verdict closed-cluster-vendor-search-skips-mirror-path-of-plain-host "a vendor host after a plain mirror host is not found" vendor_skipped 'mirror.example/quay.io/x:1'
verdict closed-cluster-vendor-search-skips-mirror-path-with-port-and-digest "a vendor host after a mirror host with a port is not found" vendor_skipped 'mirror.example:5000/ghcr.io/x@sha256:abc'
vendor_hits=$(grep -c -i -E "$vendor_pattern" "$rendered_file")
record closed-cluster-vendor-host-hits "$vendor_hits" hits
verdict closed-cluster-no-vendor-host-rendered "$vendor_hits vendor or upstream registry hosts in the rendered objects" test "$vendor_hits" = 0
token_hits=$(grep -c -E "$VENDOR_TOKEN_REGEX" "$rendered_file")
verdict closed-cluster-no-control-plane-token "$token_hits mentions of the vendor control-plane token" test "$token_hits" = 0

use_closed
cleanup_extra() { keeper_resume; set_pool_replicas "$ns" "$original_replicas" 2> /dev/null; }

k get nodes > /dev/null 2>&1 || { check closed-cluster-closed-cluster fail "cluster $CTX_CLOSED not reachable"; scenario_end; }
install_on_closed() {
  if [ "$KC_VIA_NODE" = 1 ]; then
    helm template "$HELM_RELEASE" "$CHART_DIR" -n "$HELM_NAMESPACE" -f "$CLOSED_BASE_VALUES_FILE" -f "$CLOSED_VALUES_FILE" \
      --set "$VALUES_REGISTRY_MIRROR=$CLOSED_MIRROR_HOST" --set-json "$VALUES_PULL_SECRETS=[\"$CLOSED_PULL_SECRET\"]" | k apply -f - > /dev/null
  else
    helm_ctx upgrade --install "$HELM_RELEASE" "$CHART_DIR" -n "$HELM_NAMESPACE" -f "$CLOSED_BASE_VALUES_FILE" -f "$CLOSED_VALUES_FILE" \
      --set "$VALUES_REGISTRY_MIRROR=$CLOSED_MIRROR_HOST" --set-json "$VALUES_PULL_SECRETS=[\"$CLOSED_PULL_SECRET\"]" --wait --timeout 600s > /dev/null
  fi
}

install_on_closed || { check closed-cluster-closed-install fail "chart install on $CTX_CLOSED failed"; scenario_end; }
check closed-cluster-closed-install pass "chart installed on $CTX_CLOSED through the mirror"

ns=$(ns_of "$CLIENT_A" "$NET_ALLOW")
full_ns=$(ns_of "$CLIENT_A" "$NET_FULL")
original_replicas=$(pool_replicas "$ns")
wait_warm "$ns" 1 300 || { check closed-cluster-warm-pod fail "no warm pod in $ns"; scenario_end; }
wait_warm "$full_ns" 1 300 || { check closed-cluster-warm-pod-full fail "no warm pod in $full_ns"; scenario_end; }

outside_mirror=$(k get pods -A -o json | jq -r --arg ns "$SYSTEM_NS" --arg mirror "$CLOSED_MIRROR_HOST/" --argjson managed "$(chart_namespaces)" '
  .items[] | select(.metadata.namespace as $n | $n == $ns or ($managed | index($n)))
  | .spec.containers[], (.spec.initContainers[]?) | .image | select(startswith($mirror) | not)' | count_lines)
verdict closed-cluster-running-images-via-mirror "$outside_mirror running images outside the mirror" test "$outside_mirror" = 0

claim_to internet_pod "$full_ns" "closed-cluster-$(new_id)" agent-a || { check closed-cluster-claim-full fail "claim refused in $full_ns"; scenario_end; }
verdict closed-cluster-probe-control "from the same pod a loopback listener answers and localhost resolves" probe_control "$full_ns" "$internet_pod"
probe_open_to routed "$full_ns" "$internet_pod" tcp '[{"host":"1.1.1.1","port":443},{"host":"8.8.8.8","port":443}]'
verdict closed-cluster-no-route-to-internet "$routed of 2 public targets reached from a mode full sandbox" test "$routed" = 0

thread="closed-cluster-$(new_id)"
started=$(now_ms)
claim_to pod "$ns" "$thread" agent-a || { check closed-cluster-claim fail "claim refused in $ns"; scenario_end; }
sx "$ns" "$pod" true
record closed-cluster-open-ms "$(($(now_ms) - started))" ms
sx "$ns" "$pod" sh -c 'mkdir -p /work/tree && i=0 && while [ $i -lt 200 ]; do echo "data-$i" > "/work/tree/f$i"; i=$((i + 1)); done && head -c 5000000 /dev/urandom > /work/tree/big.bin'
before=$(tree_hash "$ns" "$pod" /work/tree)

state_file="$BATTERY_OUT/closed-cluster-$thread.tar"
sx "$ns" "$pod" "$AGENT_BSDTAR" -c --format pax --numeric-owner -f - -C /work tree > "$state_file"
verdict closed-cluster-save "state tar uploaded to the store" s3_put "closed-cluster/$thread/state.tar" "$state_file"
release_pod "$ns" "$pod"
k -n "$ns" wait --for=delete "pod/$pod" --timeout=60s > /dev/null 2>&1

claim_to restored "$ns" "$thread" agent-a || { check closed-cluster-claim-restore fail "claim refused in $ns"; scenario_end; }
s3_get "closed-cluster/$thread/state.tar" "$state_file"
sxi "$ns" "$restored" "$AGENT_BSDTAR" -x -p --numeric-owner -f - -C /work < "$state_file"
after=$(tree_hash "$ns" "$restored" /work/tree)
verdict closed-cluster-restore "tree hash before $before, after $after" test "$before" = "$after"
s3_delete "closed-cluster/$thread/state.tar" > /dev/null
rm -f "$state_file"

if need_proxy closed-cluster "$CLIENT_A" "$NET_ALLOW"; then
  eg=$(egress_ns_of "$CLIENT_A" "$NET_ALLOW")
  proxy=$(proxy_endpoint "$CLIENT_A" "$NET_ALLOW")
  statuses=$(probe "$ns" "$restored" connect "[{\"proxy\":\"$proxy\",\"target\":\"$TEST_HOST_GIT:443\"},{\"proxy\":\"$proxy\",\"target\":\"example.com:443\"}]" | jq -r '.status' | tr '\n' ' ')
  verdict closed-cluster-allow-mode "listed site and unlisted site answer: $statuses" test "$statuses" = "200 403 "
  vendor_log_hits=$(k -n "$eg" logs -l "$LBL_ROLE=$ROLE_PROXY" --tail=-1 | grep -c -i -E "$VENDOR_HOST_REGEX")
  verdict closed-cluster-proxy-no-vendor-call "$vendor_log_hits vendor hosts in the proxy logs" test "$vendor_log_hits" = 0
  token_env=$(k -n "$eg" get deploy -o json | jq '[.items[].spec.template.spec.containers[].env[]?.name | select(test("IRON_PROXY_TOKEN"))] | length')
  verdict closed-cluster-proxy-token-unset "$token_env proxy Deployments set the control-plane token" test "$token_env" = 0
fi

keeper_pause
set_pool_replicas "$ns" $((original_replicas + 2))
verdict closed-cluster-scale "pool grows by two warm pods" wait_warm "$ns" $((original_replicas + 2)) 180

scenario_end
