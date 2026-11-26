#!/usr/bin/env bash
HERE=$(cd "$(dirname "$0")" && pwd)
. "$HERE/common.sh"
scenario_begin s05-admission

ns=$(ns_of "$CLIENT_A" "$NET_ALLOW")
runner="system:serviceaccount:$ns:$RUNNER_SA"

as_runner() { k --as="$runner" -n "$ns" "$@"; }

refused_by_policy() {
  local out
  out=$("$@" 2>&1) && return 1
  printf '%s\n' "$out" | grep -q -i -E 'ValidatingAdmissionPolicy|admission webhook|denied request'
}

accepted() { "$@" > /dev/null 2>&1; }

variant() {
  jq -c --arg role "$LBL_ROLE" --arg name "adm-$(new_id)" "$2 | .metadata.name = \$name" <<< "$1"
}

submit() { as_runner create --dry-run=server -f - <<< "$1"; }

wait_warm "$ns" 1 120 "$POOL_ID_DEFAULT" || { check admission-warm-pod fail "no warm pod in $ns"; scenario_end; }
warm_name=$(warm_list "$ns" "$POOL_ID_DEFAULT" | head -1 | cut -d' ' -f1)
template=$(k -n "$ns" get pod "$warm_name" -o json | jq -c --arg pool "$LBL_POOL" --arg role "$LBL_ROLE" --arg sandbox "$ROLE_SANDBOX" '
  del(.status, .metadata.uid, .metadata.resourceVersion, .metadata.creationTimestamp, .metadata.ownerReferences,
      .metadata.generateName, .metadata.managedFields, .spec.nodeName)
  | .metadata.labels[$pool] = "claimed" | .metadata.labels[$role] = $sandbox')

verdict admission-runner-may-create-pods "runner can create pods" test "$(as_runner auth can-i create pods)" = yes
verdict admission-runner-may-not-create-deployments "runner cannot create deployments" test "$(as_runner auth can-i create deployments)" = no

verdict admission-sandbox-pod-passes "role=sandbox, no Secret volume, no secretKeyRef" \
  accepted submit "$(variant "$template" '.metadata.labels[$role] = "sandbox"')"
verdict admission-refuses-no-role "pod with no role label is refused" \
  refused_by_policy submit "$(variant "$template" 'del(.metadata.labels[$role])')"
verdict admission-refuses-proxy-role "pod with role=proxy is refused" \
  refused_by_policy submit "$(variant "$template" '.metadata.labels[$role] = "proxy"')"
verdict admission-refuses-secret-volume "role=sandbox pod with a Secret volume is refused" \
  refused_by_policy submit "$(variant "$template" '.spec.volumes += [{name: "leak", secret: {secretName: "anything"}}]')"
verdict admission-refuses-projected-secret "role=sandbox pod with a projected Secret is refused" \
  refused_by_policy submit "$(variant "$template" '.spec.volumes += [{name: "leak", projected: {sources: [{secret: {name: "anything"}}]}}]')"
verdict admission-refuses-secret-key-ref "role=sandbox pod with secretKeyRef is refused" \
  refused_by_policy submit "$(variant "$template" '.spec.containers[0].env = ((.spec.containers[0].env // []) + [{name: "LEAK", valueFrom: {secretKeyRef: {name: "anything", key: "k"}}}])')"
verdict admission-refuses-env-from-secret "role=sandbox pod with envFrom secretRef is refused" \
  refused_by_policy submit "$(variant "$template" '.spec.containers[0].envFrom = [{secretRef: {name: "anything"}}]')"

claim_to pod "$ns" "adm-$(new_id)" agent-a "$POOL_ID_DEFAULT" || { check admission-claim fail "claim refused"; scenario_end; }
verdict admission-runner-may-claim "label of thread on a sandbox pod is accepted" \
  accepted as_runner label pod "$pod" --overwrite --dry-run=server "$LBL_THREAD=changed"
verdict admission-refuses-relabel-role "label role=proxy on a sandbox pod is refused" \
  refused_by_policy as_runner label pod "$pod" --overwrite --dry-run=server "$LBL_ROLE=$ROLE_PROXY"
verdict admission-refuses-role-removal "removal of the role label is refused" \
  refused_by_policy as_runner label pod "$pod" --dry-run=server "$LBL_ROLE-"
replacement=$(k -n "$ns" get pod "$pod" -o json | jq -c --arg role "$LBL_ROLE" '.metadata.labels[$role] = "proxy"')
verdict admission-refuses-replace-role "replace of the pod with role=proxy is refused" \
  refused_by_policy as_runner replace --dry-run=server -f - <<< "$replacement"

proxy_pod=$(k -n "$ns" get pods -l "$LBL_ROLE=$ROLE_PROXY" -o jsonpath='{.items[0].metadata.name}')
replica_set=$(k -n "$ns" get pod "$proxy_pod" -o jsonpath='{.metadata.ownerReferences[0].name}')
verdict admission-proxy-pods-from-deployment "proxy pod $proxy_pod owned by ReplicaSet $replica_set of a Deployment" \
  test "$(k -n "$ns" get rs "$replica_set" -o jsonpath='{.metadata.ownerReferences[0].kind}')" = Deployment

scenario_end
