#!/usr/bin/env bash
HERE=$(cd "$(dirname "$0")" && pwd)
. "$HERE/common.sh"
scenario_begin s21-pod-policy-isolated-rules

: "${ISOLATED_HOST_RULE:=HOST_NAMESPACE}"
: "${ISOLATED_EPHEMERAL_RULE:=EPHEMERAL_CONTAINERS}"
: "${ISOLATED_WINDOWS_RULE:=WINDOWS_HOST_PROCESS}"

RUN_TAG=$(new_id)
cleanup_extra() { iso_remove; }

host_users='.spec.hostUsers = true'
windows_user='{hostProcess: true, runAsUserName: "NT AUTHORITY\\SYSTEM"}'

host_rule() {
  local name=battery-iso-$RUN_TAG-host
  iso_prepare "$name" "$ISOLATED_HOST_RULE" || return 0
  iso_activate "$name" || return 0
  if ! iso_wait_refusing "$ISOLATED_HOST_RULE" iso_probe "$host_users | .spec.hostPID = true"; then
    check "$name-active" fail "the copy of the host rule did not refuse a pod with hostPID within 90 s"
  fi
  iso_case isolated-host-network "rule $ISOLATED_HOST_RULE alone: pod with hostNetwork true" "$ISOLATED_HOST_RULE" "$host_users | .spec.hostNetwork = true"
  iso_case isolated-host-pid "rule $ISOLATED_HOST_RULE alone: pod with hostPID true" "$ISOLATED_HOST_RULE" "$host_users | .spec.hostPID = true"
  iso_case isolated-host-ipc "rule $ISOLATED_HOST_RULE alone: pod with hostIPC true" "$ISOLATED_HOST_RULE" "$host_users | .spec.hostIPC = true"
  iso_case isolated-host-port "rule $ISOLATED_HOST_RULE alone: container with a hostPort" "$ISOLATED_HOST_RULE" \
    '.spec.containers[0].ports = [{containerPort: 18080, hostPort: 18080, protocol: "TCP"}]' \
    '.spec.containers[0].ports = [{containerPort: 18080, protocol: "TCP"}]'
}

windows_rule() {
  local name=battery-iso-$RUN_TAG-windows host_network="$host_users | .spec.hostNetwork = true"
  iso_prepare "$name" "$ISOLATED_WINDOWS_RULE" || return 0
  iso_activate "$name" || return 0
  if ! iso_wait_refusing "$ISOLATED_WINDOWS_RULE" iso_probe "$host_network | .spec.securityContext.windowsOptions = $windows_user"; then
    check "$name-active" fail "the copy of the Windows host process rule did not refuse a host process pod within 90 s"
  fi
  iso_case isolated-windows-host-process-container "rule $ISOLATED_WINDOWS_RULE alone: container with windowsOptions.hostProcess true (hostNetwork true, as the API server demands)" "$ISOLATED_WINDOWS_RULE" \
    "$host_network | .spec.containers[0].securityContext.windowsOptions = $windows_user" "$host_network"
  iso_case isolated-windows-host-process-pod "rule $ISOLATED_WINDOWS_RULE alone: pod with windowsOptions.hostProcess true (hostNetwork true)" "$ISOLATED_WINDOWS_RULE" \
    "$host_network | .spec.securityContext.windowsOptions = $windows_user" "$host_network"
}

ephemeral_rule() {
  local name=battery-iso-$RUN_TAG-ephemeral warm_ns warm_pod seed_image base out
  warm_ns=$(ns_of "$CLIENT_A" "$NET_ALLOW")
  if ! wait_warm "$warm_ns" 1 120; then
    check "$name-set-up" fail "no warm pod in $warm_ns, so the image of the seed container is not known"
    return 0
  fi
  warm_pod=$(warm_list "$warm_ns" | head -1 | cut -d' ' -f1)
  seed_image=$(k -n "$warm_ns" get pod "$warm_pod" -o jsonpath="{.spec.containers[?(@.name==\"$SEED_CONTAINER\")].image}")
  if [ -z "$seed_image" ]; then
    check "$name-set-up" fail "no container named $SEED_CONTAINER in the pod $warm_pod"
    return 0
  fi
  iso_prepare "$name" "$ISOLATED_EPHEMERAL_RULE" || return 0
  base=$(iso_variant ".spec.containers[0].image = \"$seed_image\" | .spec.containers[0].command = [\"$TOOLS_PAUSE\"]")
  if ! out=$(k -n "$ISO_NS" create -o jsonpath='{.metadata.name}' -f - <<< "$base" 2>&1); then
    check "$name-set-up" fail "the pod that takes the ephemeral container was not made: $(first_line "$out")"
    return 0
  fi
  base=$out
  if ! k -n "$ISO_NS" wait --for=condition=Ready "pod/$base" --timeout=120s > /dev/null; then
    check "$name-set-up" fail "pod $base is not Ready in 120 s"
    return 0
  fi
  iso_activate "$name" || return 0
  local patch
  patch=$(jq -n -c --arg image "$seed_image" --arg pause "$TOOLS_PAUSE" '{spec: {ephemeralContainers: [{name: "battery-ephemeral", image: $image, command: [$pause]}]}}')
  ephemeral_probe() { k -n "$ISO_NS" patch pod "$base" --subresource=ephemeralcontainers --type=strategic --dry-run=server -p "$patch"; }
  if ! iso_wait_refusing "$ISOLATED_EPHEMERAL_RULE" ephemeral_probe; then
    check "$name-active" fail "the copy of the ephemeral container rule did not refuse an ephemeral container within 90 s"
  fi
  verdict isolated-ephemeral-control "rule $ISOLATED_EPHEMERAL_RULE alone: a dry-run annotation update of the pod is accepted" \
    accepted k -n "$ISO_NS" annotate pod "$base" battery=isolated --overwrite --dry-run=server
  if out=$(ephemeral_probe 2>&1); then
    check isolated-ephemeral-container fail "rule $ISOLATED_EPHEMERAL_RULE alone: the ephemeral container was accepted"
  elif grep -q -E "BR_SANDBOX_${ISOLATED_EPHEMERAL_RULE}([^A-Z_]|$)" <<< "$out"; then
    check isolated-ephemeral-container pass "rule $ISOLATED_EPHEMERAL_RULE alone: an ephemeral container is refused with BR_SANDBOX_$ISOLATED_EPHEMERAL_RULE by the copy of that one rule"
  else
    check isolated-ephemeral-container fail "rule $ISOLATED_EPHEMERAL_RULE alone: refused with another cause: $(first_line "$out")"
  fi
}

host_rule
windows_rule
ephemeral_rule

scenario_end
