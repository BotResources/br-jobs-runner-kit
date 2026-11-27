#!/usr/bin/env bash
HERE=$(cd "$(dirname "$0")" && pwd)
. "$HERE/common.sh"
scenario_begin s10-proxy-canary

ns=$(ns_of "$CLIENT_CANARY" "$NET_ALLOW")
eg=$(egress_ns_of "$CLIENT_CANARY" "$NET_ALLOW")
canary="brcanary-$(openssl rand -hex 16)"
BATTERY_ROOT=$(cd "$HERE/.." && pwd)

calls_done() {
  local count
  count=$(sx "$ns" "$pod" sh -c 'grep -c " [0-9][0-9][0-9]$" /tmp/canary-call.log 2> /dev/null; true')
  echo "${count:-0}"
}

holder_start() {
  local waited=0
  sx "$ns" "$pod" sh -c "CANARY_PLACEHOLDER_ENV_NAME=$CANARY_PLACEHOLDER_ENV nohup $SANDBOX_PYTHON -u /tmp/canary-call.py $TEST_HOST_CANARY 600 $* > /tmp/canary-call.log 2>&1 & echo \$! > /tmp/canary-call.pid"
  until [ "$(calls_done)" -ge "$#" ] || [ "$waited" -ge 40 ]; do
    sleep 1
    waited=$((waited + 1))
  done
  sx "$ns" "$pod" cat /tmp/canary-call.log | tail -8 | sed 's/^/call-log: /'
}

holder_stop() { sx "$ns" "$pod" sh -c 'kill $(cat /tmp/canary-call.pid)'; }

scan_sandbox() {
  sxi "$ns" "$pod" env "SCAN_CANARY=$canary" "SCAN_KEYLINE=PRIVATE KEY-----" "$SANDBOX_PYTHON" - scan < "$HERE/canary-scan.py"
}

field_sum() { awk -v field="$1" '{for (i = 1; i <= NF; i++) if ($i ~ "^" field "=") {split($i, a, "="); sum += a[2]}} END {print sum + 0}'; }

bash "$BATTERY_ROOT/extras.sh" canary "$canary" > /dev/null 2>&1 || { check canary-echo-endpoint fail "canary endpoint container did not start"; scenario_end; }
set_secret "$eg" "$CANARY_SECRET_NAME" "$CANARY_SECRET_KEY" "$canary"
need_proxy canary "$CLIENT_CANARY" "$NET_ALLOW" || scenario_end
proxy_restart "$eg" || { check canary-proxy-restart fail "proxy pods of $eg did not come back Ready"; scenario_end; }

wait_warm "$ns" 1 180 || { check canary-warm-pod fail "no warm pod in $ns"; scenario_end; }
claim_to pod "$ns" "canary-$(new_id)" agent-a || { check canary-claim fail "claim refused"; scenario_end; }
sxi "$ns" "$pod" sh -c 'cat > /tmp/canary-call.py' < "$HERE/canary-call.py"

verdict canary-secret-not-in-sandbox-namespace "Secret $CANARY_SECRET_NAME is in $eg and not in $ns" \
  test -z "$(k -n "$ns" get secret "$CANARY_SECRET_NAME" -o name 2> /dev/null)" -a -n "$(k -n "$eg" get secret "$CANARY_SECRET_NAME" -o name)"
verdict canary-sandbox-holds-placeholder-only "env $CANARY_PLACEHOLDER_ENV is $(sx "$ns" "$pod" printenv "$CANARY_PLACEHOLDER_ENV") and not the canary" \
  test "$(sx "$ns" "$pod" printenv "$CANARY_PLACEHOLDER_ENV")" != "$canary" -a -n "$(sx "$ns" "$pod" printenv "$CANARY_PLACEHOLDER_ENV")"

proxy=$(proxy_endpoint "$CLIENT_CANARY" "$NET_ALLOW")
denied_request="[{\"proxy\":\"$proxy\",\"target\":\"example.com:443\"}]"
denied=0
tries=0
until [ "$denied" != 0 ] || [ "$tries" -ge 15 ]; do
  denied=$(probe "$ns" "$pod" connect "$denied_request" | jq -r '.status')
  tries=$((tries + 1))
  [ "$denied" != 0 ] || sleep 2
done
verdict canary-proxy-obeys-allow-mode "CONNECT to an unlisted name answers $denied after $tries tries (the tunnel port may refuse for a few seconds after a proxy restart)" test "$denied" = 403

holder_start /whoami
verdict canary-swap-works "call with the placeholder was accepted by the remote service (200 only when the real value arrives)" \
  sx "$ns" "$pod" grep -q '^/whoami 200$' /tmp/canary-call.log
scan_report=$(scan_sandbox)
printf '%s\n' "$scan_report"
phase1_hits=$(printf '%s\n' "$scan_report" | field_sum value_hits)
phase1_key_hits=$(printf '%s\n' "$scan_report" | field_sum key_hits)
record canary-no-echo-hits "$phase1_hits" hits
verdict canary-no-echo-zero-hits "service does not echo; env, process memory and files of the sandbox: $phase1_hits hits (through the proxy, TLS)" test "$phase1_hits" = 0
verdict canary-no-ca-private-key "private key lines found in the sandbox: $phase1_key_hits" test "$phase1_key_hits" = 0
holder_stop

holder_start /echo /redirect /err
scan_report=$(scan_sandbox)
printf '%s\n' "$scan_report"
echo_hits=$(printf '%s\n' "$scan_report" | field_sum value_hits)
record canary-echo-hits "$echo_hits" hits
check canary-echo-not-met fail "the proxy has no response scrub; the remote service sends the value back: $echo_hits hits in the sandbox; not met, measured"
holder_stop

sx "$ns" "$pod" sh -c "echo $canary > /tmp/canary-planted"
planted=$(scan_sandbox | field_sum value_hits)
verdict canary-scanner-positive-control "planted canary found: $planted hits" test "$planted" -ge 1

scenario_end
