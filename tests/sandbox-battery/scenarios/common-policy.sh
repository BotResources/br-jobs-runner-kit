POLICY_NS=
POLICY_TEMPLATE=
POLICY_PSA_ORIGINAL=
POLICY_PSA_TOUCHED=

first_line() { printf '%s' "$1" | tr '\n' ' ' | cut -c1-260; }

policy_submit() { as_id "$RUNNER_ID" "$POLICY_NS" create --dry-run=server -f - <<< "$1"; }

policy_ns_label() {
  local json
  json=$(k get ns "$1" -o json) || return 1
  jq -er --arg label "$2" '.metadata.labels[$label] // ""' <<< "$json"
}

policy_setup() {
  POLICY_NS=$1
  if ! wait_warm "$POLICY_NS" 1 120; then
    check pod-policy-warm-pod fail "no warm pod in $POLICY_NS"
    return 1
  fi
  POLICY_TEMPLATE=$(pod_template "$POLICY_NS")
  verdict pod-policy-namespace-managed "namespace $POLICY_NS carries $LBL_MANAGED=true, so the pod policy is bound to it" \
    test "$(ns_label "$POLICY_NS" "$LBL_MANAGED")" = true
  policy_pod_security_remove
}

policy_pod_security_remove() {
  local original
  if ! original=$(policy_ns_label "$POLICY_NS" "$POD_SECURITY_ENFORCE_LABEL"); then
    check pod-policy-set-up fail "the label $POD_SECURITY_ENFORCE_LABEL of namespace $POLICY_NS cannot be read, so nothing was changed and the scenario stops"
    return 1
  fi
  POLICY_PSA_ORIGINAL=$original
  POLICY_PSA_TOUCHED=1
  if [ -z "$POLICY_PSA_ORIGINAL" ]; then
    log "set-up: namespace $POLICY_NS has no $POD_SECURITY_ENFORCE_LABEL label, nothing to remove"
    return 0
  fi
  if ! k label ns "$POLICY_NS" "$POD_SECURITY_ENFORCE_LABEL-" > /dev/null; then
    check pod-policy-set-up fail "the label $POD_SECURITY_ENFORCE_LABEL of namespace $POLICY_NS cannot be removed"
    return 1
  fi
  log "set-up: removed the label $POD_SECURITY_ENFORCE_LABEL=$POLICY_PSA_ORIGINAL from the scratch namespace $POLICY_NS, so Pod Security does not refuse before the pod policy; the scenario puts it back at the end"
}

policy_pod_security_enforce() {
  k label ns "$POLICY_NS" "$POD_SECURITY_ENFORCE_LABEL=$1" --overwrite > /dev/null
}

policy_pod_security_restore() {
  if [ -z "$POLICY_PSA_TOUCHED" ] || [ -z "$POLICY_NS" ]; then return 0; fi
  if [ -n "$POLICY_PSA_ORIGINAL" ]; then
    policy_pod_security_enforce "$POLICY_PSA_ORIGINAL"
  else
    k label ns "$POLICY_NS" "$POD_SECURITY_ENFORCE_LABEL-" > /dev/null 2>&1 || true
  fi
  POLICY_PSA_TOUCHED=
}

policy_case() {
  local name=$1 detail=$2 codes=$3 patch=$4 control=${5:-.} out
  if [ -z "$codes" ]; then
    check "$name" fail "$detail: no expected code is set"
    return 0
  fi
  if ! out=$(policy_submit "$(variant "$POLICY_TEMPLATE" "$control")" 2>&1); then
    check "$name" fail "$detail: the control pod, without the forbidden field, is refused: $(first_line "$out")"
    return 0
  fi
  if out=$(policy_submit "$(variant "$POLICY_TEMPLATE" "$patch")" 2>&1); then
    check "$name" fail "$detail: the pod with the forbidden field was accepted"
    return 0
  fi
  if grep -q -E "BR_SANDBOX_(${codes})([^A-Z_]|$)" <<< "$out"; then
    check "$name" pass "$detail: refused with $(grep -o -E 'BR_SANDBOX_[A-Z_]+' <<< "$out" | head -1) (expected $codes); the control pod is accepted"
  else
    check "$name" fail "$detail: refused with another cause, expected $codes, message: $(first_line "$out")"
  fi
}

policy_accepts() {
  local name=$1 detail=$2 patch=$3 out
  if out=$(policy_submit "$(variant "$POLICY_TEMPLATE" "$patch")" 2>&1); then
    check "$name" pass "$detail: accepted"
  else
    check "$name" fail "$detail: refused: $(first_line "$out")"
  fi
}

on_agent() { printf '.spec.containers |= map(if .name == $agent then %s else . end)' "$1"; }
on_init() { printf '.spec.initContainers |= map(if .name == "%s" then .restartPolicy = "Always" | %s else . end)' "$INIT_CONTAINER" "$1"; }
on_init_plain() { printf '.spec.initContainers |= map(if .name == "%s" then %s else . end)' "$INIT_CONTAINER" "$1"; }
