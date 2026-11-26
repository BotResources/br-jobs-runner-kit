#!/usr/bin/env bash
HERE=$(cd "$(dirname "$0")" && pwd)
. "$HERE/common.sh"
scenario_begin s10-proxy-canary

ns=$(ns_of "$CLIENT_CANARY" "$NET_ALLOW")
eg=$(egress_ns_of "$CLIENT_CANARY" "$NET_ALLOW")
canary="brcanary-$(openssl rand -hex 16)"
echo_name="canary-echo"
CAPTURE_NAME=
CAPTURE_PID=

cleanup_extra() {
  k -n "$TESTSVC_NS" delete pod/$echo_name svc/$echo_name secret/$echo_name-tls secret/$echo_name-canary configmap/$echo_name-code --ignore-not-found --wait=false > /dev/null 2>&1
  if [ -n "$CAPTURE_NAME" ]; then docker stop "$CAPTURE_NAME" > /dev/null 2>&1; fi
}

start_echo_endpoint() {
  local dir
  dir="$BATTERY_OUT/canary-cert-$(new_id)"
  mkdir -p "$dir"
  openssl req -new -newkey rsa:2048 -nodes -keyout "$dir/tls.key" -subj "/CN=$TEST_HOST_ECHO" -out "$dir/req.csr" 2> /dev/null
  openssl x509 -req -in "$dir/req.csr" -CA "$TESTCA_CERT" -CAkey "$TESTCA_KEY" -set_serial "0x$(openssl rand -hex 8)" -days 2 \
    -extfile <(printf 'subjectAltName=DNS:%s' "$TEST_HOST_ECHO") -out "$dir/tls.crt" 2> /dev/null
  k -n "$TESTSVC_NS" create secret tls $echo_name-tls --cert="$dir/tls.crt" --key="$dir/tls.key" > /dev/null
  rm -f "$dir/tls.key" "$dir/tls.crt" "$dir/req.csr"
  rmdir "$dir"
  set_secret "$TESTSVC_NS" $echo_name-canary value "$canary"
  k -n "$TESTSVC_NS" create configmap $echo_name-code --from-file=canary-echo-server.py="$HERE/canary-echo-server.py" > /dev/null
  k -n "$TESTSVC_NS" apply -f - > /dev/null << YAML
apiVersion: v1
kind: Pod
metadata: {name: $echo_name, labels: {app: $echo_name}}
spec:
  containers:
  - name: echo
    image: $ECHO_IMAGE
    command: [python3, /app/canary-echo-server.py]
    env:
    - name: CANARY
      valueFrom: {secretKeyRef: {name: $echo_name-canary, key: value}}
    volumeMounts:
    - {name: code, mountPath: /app}
    - {name: tls, mountPath: /tls}
  volumes:
  - {name: code, configMap: {name: $echo_name-code}}
  - {name: tls, secret: {secretName: $echo_name-tls}}
---
apiVersion: v1
kind: Service
metadata: {name: $echo_name}
spec:
  selector: {app: $echo_name}
  ports: [{port: 443, targetPort: 8443}]
YAML
  k -n "$TESTSVC_NS" wait --for=condition=Ready pod/$echo_name --timeout=180s > /dev/null
}

capture_start() {
  local sandbox_id pid
  sandbox_id=$(docker exec "$NODE" crictl pods --name "^$2\$" --namespace "$1" -q)
  pid=$(docker exec "$NODE" crictl inspectp "$sandbox_id" | jq -r '.info.pid')
  CAPTURE_NAME="canary-capture-$(new_id)"
  docker run --rm --name "$CAPTURE_NAME" --pid "container:$NODE" --cap-add NET_ADMIN --cap-add SYS_ADMIN --cap-add SYS_PTRACE \
    "$CAPTURE_IMAGE" nsenter -t "$pid" -n tcpdump -U -i any -w - > "$3" 2> /dev/null &
  CAPTURE_PID=$!
  sleep 3
}

capture_stop() {
  sleep 1
  docker stop "$CAPTURE_NAME" > /dev/null 2>&1
  wait "$CAPTURE_PID" 2> /dev/null
  CAPTURE_NAME=
}

calls_done() {
  local count
  count=$(sx "$ns" "$pod" sh -c 'grep -c " [0-9][0-9][0-9]$" /tmp/canary-call.log 2> /dev/null; true')
  echo "${count:-0}"
}

holder_start() {
  local waited=0
  sx "$ns" "$pod" sh -c "CANARY_PLACEHOLDER_ENV_NAME=$CANARY_PLACEHOLDER_ENV nohup $SANDBOX_PYTHON /tmp/canary-call.py $TEST_HOST_ECHO 600 $* > /tmp/canary-call.log 2>&1 & echo \$! > /tmp/canary-call.pid"
  until [ "$(calls_done)" -ge "$#" ] || [ "$waited" -ge 30 ]; do
    sleep 1
    waited=$((waited + 1))
  done
}

holder_stop() { sx "$ns" "$pod" sh -c 'kill $(cat /tmp/canary-call.pid)'; }

scan_sandbox() {
  sxi "$ns" "$pod" env "SCAN_CANARY=$canary" "SCAN_KEYLINE=PRIVATE KEY-----" "$SANDBOX_PYTHON" - scan < "$HERE/canary-scan.py"
}

scan_pcap() { SCAN_CANARY=$canary SCAN_KEYLINE="PRIVATE KEY-----" python3 "$HERE/canary-scan.py" file "$1"; }

field_sum() { awk -v field="$1" '{for (i = 1; i <= NF; i++) if ($i ~ "^" field "=") {split($i, a, "="); sum += a[2]}} END {print sum + 0}'; }

start_echo_endpoint || { check canary-echo-endpoint fail "echo endpoint did not start"; scenario_end; }
set_secret "$eg" "$CANARY_SECRET_NAME" "$CANARY_SECRET_KEY" "$canary"
need_proxy canary "$CLIENT_CANARY" "$NET_ALLOW" || scenario_end
proxy_restart "$eg" || { check canary-proxy-restart fail "proxy pods of $eg did not come back Ready"; scenario_end; }

wait_warm "$ns" 1 180 || { check canary-warm-pod fail "no warm pod in $ns"; scenario_end; }
claim_to pod "$ns" "canary-$(new_id)" agent-a || { check canary-claim fail "claim refused"; scenario_end; }
sxi "$ns" "$pod" sh -c 'cat > /tmp/canary-call.py' < "$HERE/canary-call.py"

verdict canary-secret-not-in-sandbox-namespace "Secret $CANARY_SECRET_NAME is in $eg and not in $ns" \
  test -z "$(k -n "$ns" get secret "$CANARY_SECRET_NAME" -o name 2> /dev/null)" -a -n "$(k -n "$eg" get secret "$CANARY_SECRET_NAME" -o name)"
verdict canary-sandbox-holds-placeholder-only "env $CANARY_PLACEHOLDER_ENV is not the canary" \
  test "$(sx "$ns" "$pod" printenv "$CANARY_PLACEHOLDER_ENV")" != "$canary"

proxy=$(proxy_endpoint "$CLIENT_CANARY" "$NET_ALLOW")
denied=$(probe "$ns" "$pod" connect "[{\"proxy\":\"$proxy\",\"target\":\"example.com:443\"}]" | jq -r '.status')
verdict canary-proxy-obeys-allow-mode "CONNECT to an unlisted name answers $denied" test "$denied" = 403

capture_start "$ns" "$pod" "$BATTERY_OUT/canary-phase1.pcap"
holder_start /whoami
capture_stop
verdict canary-swap-works "call with the placeholder was accepted by the remote service" \
  sx "$ns" "$pod" grep -q '^/whoami 200$' /tmp/canary-call.log
scan_report=$(scan_sandbox)
printf '%s\n' "$scan_report"
pcap_report=$(scan_pcap "$BATTERY_OUT/canary-phase1.pcap")
phase1_hits=$(( $(printf '%s\n' "$scan_report" "$pcap_report" | field_sum value_hits) ))
phase1_key_hits=$(printf '%s\n' "$scan_report" "$pcap_report" | field_sum key_hits)
record canary-no-echo-hits "$phase1_hits" hits
verdict canary-no-echo-zero-hits "env, process memory, files, network capture: $phase1_hits hits" test "$phase1_hits" = 0
verdict canary-no-ca-private-key "private key lines found: $phase1_key_hits" test "$phase1_key_hits" = 0
holder_stop

capture_start "$ns" "$pod" "$BATTERY_OUT/canary-phase2.pcap"
holder_start /echo /redirect /err
capture_stop
scan_report=$(scan_sandbox)
printf '%s\n' "$scan_report"
pcap_report=$(scan_pcap "$BATTERY_OUT/canary-phase2.pcap")
echo_hits=$(printf '%s\n' "$scan_report" "$pcap_report" | field_sum value_hits)
record canary-echo-hits "$echo_hits" hits
check canary-echo-not-met fail "the proxy has no response scrub; the remote service sends the value back: $echo_hits hits in the sandbox; not met, measured"
holder_stop

sx "$ns" "$pod" sh -c "echo $canary > /tmp/canary-planted"
planted=$(scan_sandbox | field_sum value_hits)
verdict canary-scanner-positive-control "planted canary found: $planted hits" test "$planted" -ge 1

scenario_end
