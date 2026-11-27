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

probe_rows() {
  local out
  out=$(probe "$@") || return 1
  [ "$(jq 'length' <<< "$4")" -gt 0 ] || return 1
  [ "$(printf '%s\n' "$out" | jq -s 'length')" = "$(jq 'length' <<< "$4")" ] || return 1
  printf '%s\n' "$out"
}

probe_count_to() {
  local var=$1 field=$2 out count
  shift 2
  if out=$(probe_rows "$@") && count=$(jq -s --arg f "$field" '[.[] | select(.[$f])] | length' <<< "$out"); then
    printf -v "$var" '%s' "$count"
  else
    printf -v "$var" '%s' probe-failed
  fi
}

probe_open_to() { probe_count_to "$1" open "${@:2}"; }

probe_control() {
  local ns=$1 pod=$2 tcp_open names_resolved
  probe_open_to tcp_open "$ns" "$pod" self '[{"host":"127.0.0.1"}]'
  probe_count_to names_resolved resolved "$ns" "$pod" names '[{"name":"localhost"}]'
  [ "$tcp_open" = 1 ] && [ "$names_resolved" = 1 ]
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

tree_hash() {
  sx "$1" "$2" sh -c "cd $3 && find . -type f -exec md5sum {} + | sort | md5sum | cut -d' ' -f1"
}
