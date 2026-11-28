ISO_POLICY_DOC=
ISO_POLICIES=()
ISO_BINDINGS=()
ISO_NAMESPACES=()
ISO_NS=
ISO_LABEL=battery.isolated-rule
ISO_IMAGE=registry.invalid/battery/isolated:1

iso_pod() {
  jq -n -c --arg role "$LBL_ROLE" --arg name "iso-$(new_id)" --arg image "$ISO_IMAGE" \
    '{apiVersion: "v1", kind: "Pod", metadata: {name: $name, labels: {($role): "sandbox"}}, spec: {containers: [{name: "c", image: $image}]}}'
}

iso_variant() { jq -c "$1" <<< "$(iso_pod)"; }

iso_submit() { k -n "$ISO_NS" create --dry-run=server -f - <<< "$1"; }

iso_policy_doc() {
  jq -c --arg name "$1" --arg code "BR_SANDBOX_$2:" '
    {apiVersion: "admissionregistration.k8s.io/v1", kind: "ValidatingAdmissionPolicy", metadata: {name: $name},
     spec: {failurePolicy: "Fail", matchConstraints: .spec.matchConstraints, variables: .spec.variables,
            validations: [.spec.validations[] | select(.message | startswith($code))]}}' <<< "$3"
}

iso_binding_doc() {
  jq -n -c --arg name "$1" --arg label "$ISO_LABEL" \
    '{apiVersion: "admissionregistration.k8s.io/v1", kind: "ValidatingAdmissionPolicyBinding", metadata: {name: $name},
      spec: {policyName: $name, validationActions: ["Deny"], matchResources: {namespaceSelector: {matchLabels: {($label): $name}}}}}'
}

iso_namespace_doc() {
  jq -n -c --arg name "$1" --arg label "$ISO_LABEL" \
    '{apiVersion: "v1", kind: "Namespace", metadata: {name: $name, labels: {($label): $name}}}'
}

iso_wait_default_account() {
  local end
  end=$(($(date +%s) + 60))
  until k -n "$ISO_NS" get serviceaccount default > /dev/null 2>&1; do
    if [ "$(date +%s)" -ge "$end" ]; then return 1; fi
    sleep 1
  done
}

iso_remove() {
  local name
  for name in "${ISO_BINDINGS[@]:-}"; do
    if [ -n "$name" ]; then k delete validatingadmissionpolicybinding "$name" --ignore-not-found --wait=false > /dev/null 2>&1; fi
  done
  for name in "${ISO_POLICIES[@]:-}"; do
    if [ -n "$name" ]; then k delete validatingadmissionpolicy "$name" --ignore-not-found --wait=false > /dev/null 2>&1; fi
  done
  for name in "${ISO_NAMESPACES[@]:-}"; do
    if [ -n "$name" ]; then k delete namespace "$name" --ignore-not-found --wait=false > /dev/null 2>&1; fi
  done
}

iso_prepare() {
  local name=$1 code=$2 live
  ISO_NS=$name
  if ! live=$(k get validatingadmissionpolicy "$POD_POLICY" -o json); then
    check "$name-policy" fail "the live policy $POD_POLICY cannot be read"
    return 1
  fi
  ISO_POLICY_DOC=$(iso_policy_doc "$name" "$code" "$live")
  if [ "$(jq -r '.spec.validations | length' <<< "$ISO_POLICY_DOC")" != 1 ]; then
    check "$name-policy" fail "the live policy $POD_POLICY holds no single rule with the code BR_SANDBOX_$code"
    return 1
  fi
  ISO_NAMESPACES+=("$name")
  if ! k apply -f - > /dev/null <<< "$(iso_namespace_doc "$name")"; then
    check "$name-set-up" fail "namespace $name was not made"
    return 1
  fi
  if ! iso_wait_default_account; then
    check "$name-set-up" fail "no default ServiceAccount in namespace $name after 60 s"
    return 1
  fi
}

iso_activate() {
  local name=$1
  ISO_POLICIES+=("$name")
  if ! k apply -f - > /dev/null <<< "$ISO_POLICY_DOC"; then
    check "$name-set-up" fail "policy $name was not made"
    return 1
  fi
  ISO_BINDINGS+=("$name")
  if ! k apply -f - > /dev/null <<< "$(iso_binding_doc "$name")"; then
    check "$name-set-up" fail "binding $name was not made"
    return 1
  fi
}

iso_probe() { iso_submit "$(iso_variant "$1")"; }

iso_wait_refusing() {
  local code=$1 end out
  shift
  end=$(($(date +%s) + 90))
  while [ "$(date +%s)" -lt "$end" ]; do
    if out=$("$@" 2>&1); then
      sleep 2
    else
      grep -q "BR_SANDBOX_$code" <<< "$out"
      return
    fi
  done
  return 1
}

iso_case() {
  local name=$1 detail=$2 code=$3 patch=$4 control=${5:-.} out
  if ! out=$(iso_submit "$(iso_variant "$control")" 2>&1); then
    check "$name" fail "$detail: the control pod is refused: $(first_line "$out")"
    return 0
  fi
  if out=$(iso_submit "$(iso_variant "$patch")" 2>&1); then
    check "$name" fail "$detail: the pod with the forbidden field was accepted"
    return 0
  fi
  if grep -q -E "BR_SANDBOX_${code}([^A-Z_]|$)" <<< "$out"; then
    check "$name" pass "$detail: refused with BR_SANDBOX_$code by the copy of that one rule; the control pod is accepted"
  else
    check "$name" fail "$detail: refused with another cause, expected $code, message: $(first_line "$out")"
  fi
}
