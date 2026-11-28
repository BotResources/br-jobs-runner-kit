#!/usr/bin/env bash
HERE=$(cd "$(dirname "$0")" && pwd)
. "$HERE/common.sh"
scenario_begin s23-image-fails-at-once

: "${FAILING_WAIT_SECONDS:=300}"
PULL_REASONS="ErrImagePull ImagePullBackOff"

SCRATCH_PREFIX=bt23
cleanup_extra() { scratch_pool_remove; }

absent_image() {
  local last=${MOVING_IMAGE##*/} repository=$MOVING_IMAGE
  case "$last" in *:*) repository=${MOVING_IMAGE%:*} ;; esac
  printf '%s:battery-absent-%s\n' "$repository" "$(new_id)"
}

failing_condition() {
  jq -c --arg type "$POOL_FAILING_CONDITION" '[.status.conditions[]? | select(.type == $type) | {status, reason, message}] | first // {}'
}

rs_condition() { k -n "$SCRATCH_NS" get rs "$(pool_rs "$SCRATCH_NS")" -o json 2> /dev/null | failing_condition; }

pool_condition() { k -n "$SCRATCH_NS" get "$SANDBOX_POOL_KIND" "$SANDBOX_POOL_NAME" -o json 2> /dev/null | failing_condition; }

waiting_reasons() {
  warm_pods_json "$SCRATCH_NS" | jq -r --arg c "$SANDBOX_CONTAINER" '.items[] | .status.containerStatuses[]? | select(.name == $c) | .state.waiting.reason // empty'
}

has_pull_reason() {
  local reason
  for reason in $(waiting_reasons); do
    if [[ " $PULL_REASONS " == *" $reason "* ]]; then return 0; fi
  done
  return 1
}

wait_condition() {
  local end
  end=$(($(date +%s) + FAILING_WAIT_SECONDS))
  while [ "$(date +%s)" -lt "$end" ]; do
    if [ "$(rs_condition | jq -r '.status // ""')" = True ]; then return 0; fi
    sleep 2
  done
  return 1
}

wait_same_condition() {
  local end
  end=$(($(date +%s) + 30))
  while [ "$(date +%s)" -lt "$end" ]; do
    if [ "$(pool_condition)" = "$1" ]; then return 0; fi
    sleep 2
  done
  return 1
}

if [ -z "${ABSENT_IMAGE:-}" ] && [ -z "${MOVING_IMAGE:-}" ]; then
  check image-fails-at-once-variable fail "neither ABSENT_IMAGE nor MOVING_IMAGE is set: the script needs an image reference of the registry of the test cluster with a tag that does not exist (MOVING_IMAGE gives the repository)"
  scenario_end
fi
if ! chart_at_least 0.4.0; then
  check image-fails-at-once-chart-version fail "this script needs chart 0.4.0 or later and CHART_VERSION is $CHART_VERSION"
  scenario_end
fi
ABSENT_IMAGE=${ABSENT_IMAGE:-$(absent_image)}

scratch_pool_make "$ABSENT_IMAGE" || scenario_end
scratch_pool_accepted || scenario_end
started=$(date +%s)
if wait_condition; then
  seconds=$(($(date +%s) - started))
  record image_fails_at_once_condition_seconds "$seconds" s
  condition=$(rs_condition)
  verdict image-fails-at-once-condition-reason "the pool ReplicaSet of $SCRATCH_NS has $POOL_FAILING_CONDITION True with the reason $POOL_FAILING_REASON after $seconds s for the image $ABSENT_IMAGE: $(first_line "$condition")" \
    test "$(jq -r '.reason // ""' <<< "$condition")" = "$POOL_FAILING_REASON"
  verdict image-fails-at-once-pool-status-same-condition "the SandboxPool status carries the same $POOL_FAILING_CONDITION (status, reason, message) as the ReplicaSet" \
    wait_same_condition "$condition"
else
  check image-fails-at-once-condition-reason fail "no $POOL_FAILING_CONDITION True on the pool ReplicaSet of $SCRATCH_NS in $FAILING_WAIT_SECONDS s for the image $ABSENT_IMAGE: $(first_line "$(rs_condition)")"
fi
verdict image-fails-at-once-pod-waiting-reason "a warm pod has the container $SANDBOX_CONTAINER waiting with one of: $PULL_REASONS (read: $(waiting_reasons | sort -u | tr '\n' ' '))" \
  has_pull_reason
verdict image-fails-at-once-no-warm-pod-ready "no warm pod of $SCRATCH_NS is Ready" \
  test "$(warm_image_ids "$SCRATCH_NS" | count_lines)" = 0
verdict image-fails-at-once-no-claimed-pod "no pod of $SCRATCH_NS is claimed" \
  test "$(k -n "$SCRATCH_NS" get pods -l "$LBL_POOL=claimed" -o name | count_lines)" = 0

scenario_end
