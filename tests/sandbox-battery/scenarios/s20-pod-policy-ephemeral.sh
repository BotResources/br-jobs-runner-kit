#!/usr/bin/env bash
HERE=$(cd "$(dirname "$0")" && pwd)
. "$HERE/common.sh"
scenario_begin s20-pod-policy-ephemeral

: "${CODES_EPHEMERAL:=EPHEMERAL_CONTAINERS|IMMUTABLE}"
EPHEMERAL_NAME=battery-ephemeral

cleanup_extra() { policy_pod_security_restore; }

policy_setup "$(ns_of "$CLIENT_A" "$NET_ALLOW")" || scenario_end

warm_name=$(warm_list "$POLICY_NS" | head -1 | cut -d' ' -f1)
seed_image=$(k -n "$POLICY_NS" get pod "$warm_name" -o jsonpath="{.spec.containers[?(@.name==\"$SEED_CONTAINER\")].image}")
if [ -z "$seed_image" ]; then
  check pod-policy-ephemeral-seed-image fail "no container named $SEED_CONTAINER in the pod $warm_name"
  scenario_end
fi
bare=$(k -n "$POLICY_NS" create -o jsonpath='{.metadata.name}' -f - <<< "$(variant "$POLICY_TEMPLATE" '.metadata.labels["br.sandbox/pool"] = "battery" | .spec.containers |= map(select(.name != $seed))')")
if [ -z "$bare" ]; then
  check pod-policy-ephemeral-bare-pod fail "the pod to which the ephemeral containers are added was not made"
  scenario_end
fi
track_pod "$POLICY_NS" "$bare"
if ! k -n "$POLICY_NS" wait --for=condition=Ready "pod/$bare" --timeout=120s > /dev/null; then
  check pod-policy-ephemeral-bare-pod fail "pod $bare is not Ready in 120 s"
  scenario_end
fi

ephemeral_patch() {
  jq -n -c --arg name "$EPHEMERAL_NAME" --arg image "$seed_image" --arg pause "$TOOLS_PAUSE" --argjson context "$1" \
    '{spec: {ephemeralContainers: [{name: $name, image: $image, command: [$pause], securityContext: $context}]}}'
}

ephemeral_submit() {
  k -n "$POLICY_NS" patch pod "$bare" --subresource=ephemeralcontainers --type=strategic --dry-run=server -p "$(ephemeral_patch "$1")"
}

ephemeral_case() {
  local name=$1 detail=$2 out
  if out=$(ephemeral_submit "$3" 2>&1); then
    check "$name" fail "$detail: the ephemeral container was accepted"
  elif grep -q -E "BR_SANDBOX_(${CODES_EPHEMERAL})([^A-Z_]|$)" <<< "$out"; then
    check "$name" pass "$detail: refused with $(grep -o -E 'BR_SANDBOX_[A-Z_]+' <<< "$out" | head -1) (expected $CODES_EPHEMERAL)"
  else
    check "$name" fail "$detail: refused with another cause, expected $CODES_EPHEMERAL, message: $(first_line "$out")"
  fi
}

verdict policy-accepts-update-of-pod-for-ephemeral-checks "pod $bare takes a dry-run annotation update, so the ephemeral checks below are not refused for another reason" \
  accepted k -n "$POLICY_NS" annotate pod "$bare" battery=ephemeral --overwrite --dry-run=server

ephemeral_case policy-refuses-ephemeral-privileged "ephemeral container privileged" '{"privileged": true}'
ephemeral_case policy-refuses-ephemeral-added-capability "ephemeral container with the added capability SYS_ADMIN" '{"capabilities": {"add": ["SYS_ADMIN"]}}'
ephemeral_case policy-refuses-ephemeral-privilege-escalation "ephemeral container with allowPrivilegeEscalation true" '{"allowPrivilegeEscalation": true}'
ephemeral_case policy-refuses-ephemeral-proc-mount-unmasked "ephemeral container with procMount Unmasked" '{"procMount": "Unmasked"}'
ephemeral_case policy-refuses-ephemeral-apparmor-unconfined "ephemeral container with the AppArmor profile Unconfined" '{"appArmorProfile": {"type": "Unconfined"}}'
ephemeral_case policy-refuses-ephemeral-selinux-type "ephemeral container with the SELinux type spc_t" '{"seLinuxOptions": {"type": "spc_t"}}'
ephemeral_case policy-refuses-ephemeral-selinux-user "ephemeral container with an SELinux user" '{"seLinuxOptions": {"user": "system_u"}}'
ephemeral_case policy-refuses-ephemeral-selinux-role "ephemeral container with an SELinux role" '{"seLinuxOptions": {"role": "system_r"}}'
ephemeral_case policy-refuses-ephemeral-seccomp-unconfined "ephemeral container with the seccomp profile Unconfined" '{"seccompProfile": {"type": "Unconfined"}}'

scenario_end
