POLICY_NS=
POLICY_TEMPLATE=

first_line() { printf '%s' "$1" | tr '\n' ' ' | cut -c1-260; }

policy_submit() { as_id "$RUNNER_ID" "$POLICY_NS" create --dry-run=server -f - <<< "$1"; }

policy_setup() {
  POLICY_NS=$1
  if ! wait_warm "$POLICY_NS" 1 120; then
    check pod-policy-warm-pod fail "no warm pod in $POLICY_NS"
    return 1
  fi
  POLICY_TEMPLATE=$(pod_template "$POLICY_NS")
  verdict pod-policy-namespace-managed "namespace $POLICY_NS carries $LBL_MANAGED=true, so the pod policy is bound to it" \
    test "$(ns_label "$POLICY_NS" "$LBL_MANAGED")" = true
  verdict pod-policy-no-pod-security-enforce "namespace $POLICY_NS has no pod-security enforce label (a refusal by Pod Security would come before the policy and hide its code)" \
    test -z "$(ns_label "$POLICY_NS" pod-security.kubernetes.io/enforce)"
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

on_agent() { printf '.spec.containers |= map(if .name == $agent then %s else . end)' "$1"; }
on_init() { printf '.spec.initContainers |= map(if .name == "%s" then .restartPolicy = "Always" | %s else . end)' "$INIT_CONTAINER" "$1"; }
on_init_plain() { printf '.spec.initContainers |= map(if .name == "%s" then %s else . end)' "$INIT_CONTAINER" "$1"; }
