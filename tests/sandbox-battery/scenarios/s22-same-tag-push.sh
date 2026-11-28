#!/usr/bin/env bash
HERE=$(cd "$(dirname "$0")" && pwd)
. "$HERE/common.sh"
scenario_begin s22-same-tag-push

: "${MOVING_MARGIN_SECONDS:=120}"
: "${MOVING_AGE_LIMIT_SECONDS:=300}"
VERSION_FILE=/etc/image-version

SCRATCH_PREFIX=bt22
cleanup_extra() { scratch_pool_remove; }

preconditions() {
  local name missing=0
  for name in MOVING_IMAGE MOVING_PUSH BASE_IMAGE; do
    if [ -z "${!name:-}" ]; then
      check "same-tag-push-variable-$name" fail "$name is not set: MOVING_IMAGE is the image as the pool pulls it, MOVING_PUSH the same image as the host pushes it, BASE_IMAGE the image to build from"
      missing=1
    fi
  done
  if ! chart_at_least 0.4.0; then
    check same-tag-push-chart-version fail "this script needs chart 0.4.0 or later and CHART_VERSION is $CHART_VERSION"
    missing=1
  fi
  if ! command -v docker > /dev/null 2>&1; then
    check same-tag-push-docker fail "no docker command: the script builds and pushes the image itself"
    missing=1
  fi
  return "$missing"
}

keeper_warm_max_age() {
  k -n "$KEEPER_NS" get cm "$KEEPER_CONFIG_MAP" -o json |
    jq -r --arg key "$KEEPER_CONFIG_KEY" --arg age "$WARM_MAX_AGE_VALUE_KEY" --argjson default "$KEEPER_DEFAULT_WARM_MAX_AGE" \
      '(.data[$key] // "{}") | fromjson | .[$age] // $default'
}

build_and_push() {
  local out
  if ! out=$(printf 'FROM %s\nRUN echo %s > %s\n' "$BASE_IMAGE" "$1" "$VERSION_FILE" | docker build --no-cache -q -t "$MOVING_PUSH" - 2>&1); then
    check "same-tag-push-build-$1" fail "the image of content $1 was not built: $(first_line "$out")"
    return 1
  fi
  if ! out=$(docker push -q "$MOVING_PUSH" 2>&1); then
    check "same-tag-push-push-$1" fail "$MOVING_PUSH of content $1 was not pushed (the docker of this host needs the login for the registry): $(first_line "$out")"
    return 1
  fi
}

pool_spec() { k -n "$SCRATCH_NS" get "$SANDBOX_POOL_KIND" "$SANDBOX_POOL_NAME" -o json | jq -cS '.spec'; }

pod_content() { sx "$SCRATCH_NS" "$1" cat "$VERSION_FILE" 2> /dev/null | tr -d '\r\n'; }

pod_policy() {
  k -n "$SCRATCH_NS" get pod "$1" -o json | jq -r --arg c "$SANDBOX_CONTAINER" '[.spec.containers[] | select(.name == $c) | .imagePullPolicy] | first // ""'
}

first_new_pod() {
  local old=$1
  warm_image_ids "$SCRATCH_NS" | awk -v old="$old" '$2 != "" && $2 != old {print; exit}'
}

preconditions || scenario_end

age=$(keeper_warm_max_age)
record same_tag_push_warm_max_age_seconds "$age" s
if ! [[ "$age" =~ ^[0-9]+$ ]] || [ "$age" -gt "$MOVING_AGE_LIMIT_SECONDS" ]; then
  check same-tag-push-warm-max-age-short fail "warm_max_age_seconds is $age in the keeper document of the release: it must be a whole number of $MOVING_AGE_LIMIT_SECONDS or less so that the check ends in minutes; set it in the keeper settings of the operator release"
  scenario_end
fi

build_and_push 1 || scenario_end
scratch_pool_make "$MOVING_IMAGE" || scenario_end
scratch_pool_accepted || scenario_end
if ! wait_warm "$SCRATCH_NS" 1 300; then
  check same-tag-push-warm-pod fail "no warm pod Ready in $SCRATCH_NS in 300 s"
  scenario_end
fi

first=$(warm_image_ids "$SCRATCH_NS" | head -1)
old_pod=${first%% *}
old_id=${first#* }
spec_before=$(pool_spec)
verdict same-tag-push-agent-pull-policy-always "container $SANDBOX_CONTAINER of the warm pod $old_pod has imagePullPolicy Always, so a new pod resolves the tag again" \
  test "$(pod_policy "$old_pod")" = Always
verdict same-tag-push-first-content "the warm pod $old_pod (image id $old_id) holds the content 1" \
  test "$(pod_content "$old_pod")" = 1

build_and_push 2 || scenario_end
pushed=$(date +%s)
bound=$((age + MOVING_MARGIN_SECONDS))
found=
delay=0
while [ "$delay" -le "$bound" ]; do
  found=$(first_new_pod "$old_id")
  delay=$(($(date +%s) - pushed))
  if [ -n "$found" ]; then break; fi
  sleep 5
done
record same_tag_push_margin_seconds "$MOVING_MARGIN_SECONDS" s
values="warm_max_age_seconds of the keeper document is $age s, the margin for the start of the pod is $MOVING_MARGIN_SECONDS s, the limit is $bound s"
if [ -z "$found" ]; then
  record same_tag_push_delay_to_new_warm_pod never s
  check same-tag-push-new-warm-pod-within-age-plus-margin fail "no Ready warm pod with another image id than $old_id $delay s after the push of the content 2 under the same tag; $values"
else
  record same_tag_push_delay_to_new_warm_pod "$delay" s
  verdict same-tag-push-new-warm-pod-within-age-plus-margin "the warm pod ${found%% *} has the image id ${found#* } (not $old_id) $delay s after the push; $values" \
    test "$delay" -le "$bound"
  verdict same-tag-push-new-pod-holds-new-content "the new warm pod ${found%% *} holds the content 2" \
    test "$(pod_content "${found%% *}")" = 2
fi
verdict same-tag-push-pool-spec-unchanged "the SandboxPool $SANDBOX_POOL_NAME in $SCRATCH_NS has the same spec before the push and after the new pod" \
  test "$(pool_spec)" = "$spec_before"

scenario_end
