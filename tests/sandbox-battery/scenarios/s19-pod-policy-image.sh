#!/usr/bin/env bash
HERE=$(cd "$(dirname "$0")" && pwd)
. "$HERE/common.sh"
scenario_begin s19-pod-policy-image

: "${CODES_IMAGE:=IMAGE}"
FOREIGN_IMAGE=docker.io/library/busybox:1.36
OTHER_TAG=battery-other-tag

cleanup_extra() { policy_pod_security_restore; }

policy_setup "$(ns_of "$CLIENT_A" "$NET_NONE")" || scenario_end

agent_image_is() { printf '.spec.containers |= map(if .name == $agent then .image = "%s" else . end)' "$1"; }
agent_same_repository_other_tag='.spec.containers |= map(if .name == $agent then .image = ((.image | sub("@.*$"; "") | sub(":[^:/]*$"; "")) + ":'"$OTHER_TAG"'") else . end)'
init_image_is() { printf '.spec.initContainers |= map(if .name == "%s" then .image = "%s" else . end)' "$INIT_CONTAINER" "$1"; }

real_create_refused() {
  local doc name out list
  doc=$(variant "$POLICY_TEMPLATE" "$(agent_image_is "$FOREIGN_IMAGE")")
  name=$(jq -r '.metadata.name' <<< "$doc")
  if out=$(as_id "$RUNNER_ID" "$POLICY_NS" create -f - <<< "$doc" 2>&1); then
    track_pod "$POLICY_NS" "$name"
    log "real create with a foreign image was accepted: $(first_line "$out")"
    return 1
  fi
  grep -q -E "BR_SANDBOX_(${CODES_IMAGE})([^A-Z_]|$)" <<< "$out" || return 1
  list=$(k -n "$POLICY_NS" get pods -o name) || return 1
  ! grep -qx "pod/$name" <<< "$list"
}

policy_accepts policy-accepts-pool-own-image "pod made from the pool pod of $POLICY_NS, with the image of the pool, for the runner" '.'
policy_case policy-refuses-agent-foreign-image "agent container with the image $FOREIGN_IMAGE, which is not the image of the pool" "$CODES_IMAGE" \
  "$(agent_image_is "$FOREIGN_IMAGE")"
policy_case policy-refuses-agent-same-repository-other-tag "agent container with the repository of the pool image and the tag $OTHER_TAG" "$CODES_IMAGE" \
  "$agent_same_repository_other_tag"
policy_case policy-refuses-init-foreign-image "init container $INIT_CONTAINER with the image $FOREIGN_IMAGE" "$CODES_IMAGE" \
  "$(init_image_is "$FOREIGN_IMAGE")"
verdict policy-foreign-image-is-not-created "a real create (no dry run) by the runner of a pod with the foreign agent image is refused with the image code and no pod is made" \
  real_create_refused

scenario_end
