#!/usr/bin/env bash
HERE=$(cd "$(dirname "$0")" && pwd)
. "$HERE/common.sh"
scenario_begin s15-keeper-ready

LOG_FILE="$BATTERY_OUT/s15-keeper-logs.txt"
CONFIG_MOUNT=/etc/br-sandbox/$KEEPER_CONFIG_KEY

keeper_pod() {
  k -n "$KEEPER_NS" get pods -l "$KEEPER_LABEL" -o json |
    jq -r '[.items[] | select(.metadata.deletionTimestamp == null)] | sort_by(.metadata.creationTimestamp) | last | .metadata.name // empty'
}

pod_field() { k -n "$KEEPER_NS" get pod "$pod" -o json | jq -r "$1"; }

in_list() {
  local item=$1 entry
  shift
  for entry in "$@"; do
    if [ "$entry" = "$item" ]; then return 0; fi
  done
  return 1
}

redact() {
  awk -v a="${S3_ACCESS_KEY:-}" -v s="${S3_SECRET_KEY:-}" '{
    if (a != "") while ((i = index($0, a)) > 0) $0 = substr($0, 1, i - 1) "REDACTED" substr($0, i + length(a))
    if (s != "") while ((i = index($0, s)) > 0) $0 = substr($0, 1, i - 1) "REDACTED" substr($0, i + length(s))
    print
  }'
}

ready_seconds() {
  pod_field '(.metadata.creationTimestamp | fromdateiso8601) as $created
    | [.status.conditions[] | select(.type == "Ready" and .status == "True")][0].lastTransitionTime
    | fromdateiso8601 - $created'
}

document_problems() {
  local document=$1 key problems=0
  if [ "$(jq -s 'length' <<< "$document")" != 1 ] || [ "$(jq -r 'type' <<< "$document")" != object ]; then return 1; fi
  for key in $(jq -r 'keys[]' <<< "$document"); do
    if ! in_list "$key" $KEEPER_DOCUMENT_KEYS; then echo "unknown key $key"; problems=1; fi
  done
  for key in $(jq -r '.pools[] | keys[]' <<< "$document" | sort -u); do
    if ! in_list "$key" $KEEPER_POOL_KEYS; then echo "unknown pool key $key"; problems=1; fi
  done
  [ "$problems" -eq 0 ]
}

pod=$(keeper_pod)
if [ -z "$pod" ]; then
  check keeper-ready-pod-present fail "no pod with $KEEPER_LABEL in $KEEPER_NS"
  scenario_end
fi
check keeper-ready-pod-present pass "pod $pod in $KEEPER_NS"

verdict keeper-ready-no-stand-in "image $(pod_field '.spec.containers[0].image'), command $(pod_field '.spec.containers[0].command') args $(pod_field '.spec.containers[0].args')" \
  test "$(pod_field '.spec.containers[0].image')" = "$KEEPER_IMAGE" -a "$(pod_field '.spec.containers[0].command')" = null -a "$(pod_field '.spec.containers[0].args')" = null

verdict keeper-ready-within-limit "pod Ready condition within ${KEEPER_READY_SECONDS}s" \
  k -n "$KEEPER_NS" wait --for=condition=Ready "pod/$pod" --timeout="${KEEPER_READY_SECONDS}s"
seconds=$(ready_seconds)
record keeper-ready-seconds "$seconds" s
verdict keeper-ready-seconds-under-limit "creation to Ready took ${seconds}s, limit ${KEEPER_READY_SECONDS}s" \
  test "${seconds:-999999}" -le "$KEEPER_READY_SECONDS"

image_id=$(pod_field '.status.containerStatuses[0].imageID')
image_digest=${image_id##*@}
verdict keeper-ready-image-digest "imageID $image_id; manifest list $KEEPER_INDEX_DIGEST; platform digests $KEEPER_PLATFORM_DIGESTS" \
  in_list "$image_digest" "$KEEPER_INDEX_DIGEST" $KEEPER_PLATFORM_DIGESTS

document=$(k -n "$KEEPER_NS" get cm "$KEEPER_CONFIG_MAP" -o json | jq -r --arg key "$KEEPER_CONFIG_KEY" '.data[$key] // empty')
data_keys=$(k -n "$KEEPER_NS" get cm "$KEEPER_CONFIG_MAP" -o json | jq -r '.data | keys | join(" ")')
problems=$(document_problems "$document")
document_ok() { [ "$data_keys" = "$KEEPER_CONFIG_KEY" ] && [ -n "$document" ] && document_problems "$document" > /dev/null; }
verdict keeper-ready-document-one-json "ConfigMap $KEEPER_CONFIG_MAP has data keys [$data_keys]; one JSON object with only keeper keys${problems:+; $problems}" \
  document_ok
verdict keeper-ready-document-path "env $KEEPER_CONFIG_ENV is $CONFIG_MOUNT" \
  test "$(pod_field ".spec.containers[0].env[] | select(.name == \"$KEEPER_CONFIG_ENV\") | .value")" = "$CONFIG_MOUNT"
verdict keeper-ready-document-no-credentials "document holds no credential key or value of the store" \
  test "$(jq -r 'keys[]' <<< "$document" | grep -c -i -E 'access|secret|credential|token|password')" = 0

secret_value() { k -n "$KEEPER_NS" get secret "$KEEPER_STORE_CREDENTIALS_SECRET" -o jsonpath="{.data.$1}" | base64 -d; }
verdict keeper-ready-credentials-from-secret "AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY of the pod come from Secret $KEEPER_STORE_CREDENTIALS_SECRET and equal the store identity" \
  test "$(pod_field '[.spec.containers[0].env[] | select(.name == "AWS_ACCESS_KEY_ID" or .name == "AWS_SECRET_ACCESS_KEY") | .valueFrom.secretKeyRef.name] | unique | join(",")')" = "$KEEPER_STORE_CREDENTIALS_SECRET" \
  -a "$(secret_value AWS_ACCESS_KEY_ID)" = "${S3_ACCESS_KEY:-unset}" -a "$(secret_value AWS_SECRET_ACCESS_KEY)" = "${S3_SECRET_KEY:-unset}"

pod_ip=$(pod_field '.status.podIP')
readyz_status() {
  docker exec "$NODE" wget -S -O /dev/null -T 5 "http://$pod_ip:$KEEPER_PORT/readyz" 2>&1 | awk '/HTTP\// {print $2; exit}'
}
status=$(readyz_status)
verdict keeper-ready-readyz-200 "GET /readyz on $pod_ip:$KEEPER_PORT from the node $NODE answers ${status:-no answer}" \
  test "$status" = 200
other=$(k -n "$NS_TOOLS" exec "$TOOLBOX_POD" -- curl -s -m 5 -o /dev/null -w '%{http_code}' "http://$pod_ip:$KEEPER_PORT/readyz" 2> /dev/null || true)
echo "info: GET /readyz from pod $TOOLBOX_POD in $NS_TOOLS: ${other:-no answer} (default-deny of $KEEPER_NS)"

metrics_lines() {
  docker exec "$NODE" wget -q -O - -T 5 "http://$pod_ip:$KEEPER_PORT/metrics" 2> /dev/null | grep -c "^$1" || true
}
verdict keeper-ready-sweep-errors-counter "counter $KEEPER_SWEEP_COUNTER present at GET /metrics: $(metrics_lines "$KEEPER_SWEEP_COUNTER") sample lines" \
  test "$(metrics_lines "$KEEPER_SWEEP_COUNTER")" -gt 0

has_config_error() {
  { k -n "$KEEPER_NS" logs "$pod" -c "$KEEPER_CONTAINER"; k -n "$KEEPER_NS" logs "$pod" -c "$KEEPER_CONTAINER" --previous 2> /dev/null; } |
    grep -c -i -E '"level":"(ERROR|FATAL)"|unknown field|invalid config|panicked|failed to (read|parse|load)' || true
}
verdict keeper-ready-no-config-error "error, fatal, unknown field or parse lines in the log of $pod" \
  test "$(has_config_error)" = 0

sleep "$KEEPER_OBSERVE_SECONDS"
restarts=$(pod_field '.status.containerStatuses[0].restartCount')
record keeper-ready-restarts-after-observe "$restarts" count
verdict keeper-ready-stable "after ${KEEPER_OBSERVE_SECONDS}s: restartCount $restarts, ready $(pod_field '.status.containerStatuses[0].ready'), same pod $(keeper_pod)" \
  test "$restarts" = 0 -a "$(pod_field '.status.containerStatuses[0].ready')" = true -a "$(keeper_pod)" = "$pod"

mkdir -p "$BATTERY_OUT"
k -n "$KEEPER_NS" logs "$pod" -c "$KEEPER_CONTAINER" | head -n "$KEEPER_LOG_LINES" | redact > "$LOG_FILE"
echo "first $KEEPER_LOG_LINES log lines ($(wc -l < "$LOG_FILE" | tr -d ' ') present), credentials removed:"
cat "$LOG_FILE"

scenario_end
