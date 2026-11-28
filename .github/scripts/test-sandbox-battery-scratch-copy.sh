#!/usr/bin/env bash
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
scenarios="$here/../../tests/sandbox-battery/scenarios"
failures=0

pass() { echo "ok   $1"; }
fail() {
  echo "FAIL $1" >&2
  failures=$((failures + 1))
}

FENCE_TEMPLATE_MAP=br-sandbox-pod-template
export LBL_POOL_ID=br.sandbox/pool-id
export FENCE_WRITER=writer
. "$scenarios/common-scratch.sh"
first_line() { printf '%s' "$1" | tr '\n' ' ' | cut -c1-260; }

SCRATCH_NS=sbx-pool-a-bt99-abc
export SCRATCH_REF_NS=sbx-pool-a
export SCRATCH_PREFIX=bt99

TEMPLATE='{"metadata":{"labels":{"br.sandbox/pool":"warm","br.sandbox/pool-id":"sbx-pool-a","br.sandbox/role":"sandbox"}},"spec":{"automountServiceAccountToken":false,"containers":[{"name":"agent","image":"registry.example/tools@sha256:0123","imagePullPolicy":"Always"}],"serviceAccountName":"sandbox"}}'

config_map() {
  jq -c -n --arg name "$1" --argjson data "$2" '
    {apiVersion: "v1", kind: "ConfigMap",
     metadata: {name: $name, namespace: "sbx-pool-a", uid: "u-1", resourceVersion: "7", creationTimestamp: "2026-11-27T21:11:06Z",
                labels: {"app.kubernetes.io/managed-by": "Helm"}, annotations: {"meta.helm.sh/release-name": "fence"}},
     data: $data}'
}

template_map() { config_map "$FENCE_TEMPLATE_MAP" "$(jq -c -n --arg t "$1" '{"pod-template.json": $t}')"; }

copied=$(scratch_copy_doc "$(template_map "$TEMPLATE")")
label=$(jq -r '.data["pod-template.json"] | fromjson | .metadata.labels["br.sandbox/pool-id"]' <<< "$copied")
if [ "$label" = "$SCRATCH_NS" ]; then pass "the pool id label of the copied template is the scratch namespace"; else fail "the label is '$label'"; fi

restored=$(jq -c '.data["pod-template.json"] | fromjson | .metadata.labels["br.sandbox/pool-id"] = "sbx-pool-a"' <<< "$copied")
if [ "$(jq -c . <<< "$TEMPLATE")" = "$restored" ]; then pass "the rest of the template is unchanged"; else fail "the template differs beyond the label: $restored"; fi

meta=$(jq -c '[(.metadata | keys), .metadata.namespace, (.data | keys)]' <<< "$copied")
if [ "$meta" = '[["name","namespace"],"sbx-pool-a-bt99-abc",["pod-template.json"]]' ]; then pass "the copied map keeps its name and key and takes the scratch namespace"; else fail "the metadata is $meta"; fi

without_label='{"metadata":{"labels":{"br.sandbox/pool":"warm"}},"spec":{}}'
without_labels='{"metadata":{},"spec":{}}'
without_key=$(config_map "$FENCE_TEMPLATE_MAP" '{"other.json":"{}"}')
without_data=$(jq -c 'del(.data)' <<< "$(template_map "$TEMPLATE")")
not_json=$(template_map 'not json')
refused() {
  local label=$1 doc=$2 out
  if out=$(scratch_copy_doc "$doc" 2>&1); then
    fail "$label: the copy was made: $out"
  elif [ -n "$out" ]; then
    pass "$label: refused with a reason"
  else
    fail "$label: refused with no reason"
  fi
}
refused "a template with no pool id label" "$(template_map "$without_label")"
refused "a template with no labels" "$(template_map "$without_labels")"
refused "a map with no pod-template.json key" "$without_key"
refused "a map with no data" "$without_data"
refused "a template that is not JSON" "$not_json"

other=$(config_map other-map "$(jq -c -n --arg t "$TEMPLATE" '{"pod-template.json": $t}')")
other_copy=$(scratch_copy_doc "$other")
if [ "$(jq -c '.data' <<< "$other_copy")" = "$(jq -c '.data' <<< "$other")" ]; then pass "another ConfigMap keeps its data"; else fail "another ConfigMap was rewritten"; fi

quota=$(jq -c -n '{apiVersion: "v1", kind: "ResourceQuota", metadata: {name: "br-sandbox-quota", namespace: "sbx-pool-a", uid: "u-2", resourceVersion: "9", labels: {a: "b"}}, spec: {hard: {pods: "24"}}, status: {used: {pods: "2"}}}')
quota_copy=$(scratch_copy_doc "$quota")
if [ "$quota_copy" = '{"apiVersion":"v1","kind":"ResourceQuota","metadata":{"name":"br-sandbox-quota","namespace":"sbx-pool-a-bt99-abc"},"spec":{"hard":{"pods":"24"}}}' ]; then pass "a ResourceQuota is copied unchanged apart from the namespace"; else fail "the quota copy is $quota_copy"; fi

binding=$(jq -c -n '{apiVersion: "rbac.authorization.k8s.io/v1", kind: "RoleBinding", metadata: {name: "br-sandbox-keeper", namespace: "sbx-pool-a"}, roleRef: {apiGroup: "rbac.authorization.k8s.io", kind: "ClusterRole", name: "c"}, subjects: [{kind: "ServiceAccount", name: "k", namespace: "sbx-system"}]}')
binding_copy=$(scratch_copy_doc "$binding")
if [ "$(jq -c '[.subjects, .roleRef]' <<< "$binding_copy")" = "$(jq -c '[.subjects, .roleRef]' <<< "$binding")" ]; then pass "a RoleBinding keeps its subjects and role"; else fail "the binding copy is $binding_copy"; fi

CHECKS=""
check() { CHECKS="$CHECKS $1=$2:$3"; }
k() {
  case " $* " in
    *" -n $SCRATCH_NS get "*) return 1 ;;
    *" -n $SCRATCH_REF_NS get "*) printf '%s' "$READ_DOC" ;;
  esac
}
fence_k() { jq -r '"applied " + (.data["pod-template.json"] | fromjson | .metadata.labels["br.sandbox/pool-id"])'; return 1; }

READ_DOC=$(template_map "$without_label")
rc=0
scratch_copy "configmap/$FENCE_TEMPLATE_MAP" || rc=$?
if [ "$rc" -ne 0 ] && [[ "$CHECKS" == *"bt99-fence-template=fail"* ]] && [[ "$CHECKS" != *"fence-copy"* ]]; then pass "a template that cannot be rewritten is not applied and fails a check"; else fail "rc=$rc checks='$CHECKS'"; fi

CHECKS=""
READ_DOC=$(template_map "$TEMPLATE")
rc=0
scratch_copy "configmap/$FENCE_TEMPLATE_MAP" || rc=$?
if [[ "$CHECKS" == *"bt99-fence-copy=fail"*"applied $SCRATCH_NS"* ]] && [[ "$CHECKS" != *"fence-template"* ]]; then pass "a template that can be rewritten is applied with the scratch namespace as pool id"; else fail "rc=$rc checks='$CHECKS'"; fi

if [ "$failures" -gt 0 ]; then
  echo "$failures case(s) failed" >&2
  exit 1
fi
echo "all cases passed"
