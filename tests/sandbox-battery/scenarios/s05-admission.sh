#!/usr/bin/env bash
HERE=$(cd "$(dirname "$0")" && pwd)
. "$HERE/common.sh"
scenario_begin s05-admission

ns=$(ns_of "$CLIENT_A" "$NET_ALLOW")
eg=$(egress_ns_of "$CLIENT_A" "$NET_ALLOW")
NONROOT=RUN_AS_NON_ROOT

as_runner() { as_id "$RUNNER_ID" "$ns" "$@"; }
as_netcheck() { as_id "$NETCHECK_ID" "$ns" "$@"; }
submit() { as_runner create --dry-run=server -f - <<< "$1"; }
submit_netcheck() { as_netcheck create --dry-run=server -f - <<< "$1"; }

root_agent='.spec.containers |= map(if .name == $agent then .securityContext.runAsUser = 0 | .securityContext.runAsGroup = 0 | .securityContext.runAsNonRoot = false else . end)'
ephemeral_patch() {
  jq -n -c --arg seed "$SEED_CONTAINER" --arg image "$(seed_image)" \
    '{spec: {ephemeralContainers: [{name: $seed, image: $image, command: ["/sbx/bin/pause"], securityContext: {runAsUser: 0, readOnlyRootFilesystem: true}}]}}'
}
seed_image() { k -n "$ns" get pod "$warm_name" -o jsonpath="{.spec.containers[?(@.name==\"$SEED_CONTAINER\")].image}"; }

wait_warm "$ns" 1 120 || { check admission-warm-pod fail "no warm pod in $ns"; scenario_end; }
warm_name=$(warm_list "$ns" | head -1 | cut -d' ' -f1)
template=$(pod_template "$ns")

verdict admission-runner-may-create-pods "runner can create pods in $ns" test "$(as_runner auth can-i create pods)" = yes
verdict admission-runner-may-not-create-deployments "runner cannot create deployments" test "$(as_runner auth can-i create deployments)" = no
verdict admission-policy-bound "13 rules: policy and binding $NS_PREFIX-$CLIENT_A-$NET_ALLOW-pod exist" \
  test "$(k get validatingadmissionpolicybinding "$NS_PREFIX-$CLIENT_A-$NET_ALLOW-pod" -o name 2> /dev/null)" != ""

verdict admission-sandbox-pod-passes "legitimate pod: role=sandbox, agent non-root, seed container with the tools image" \
  accepted submit "$(variant "$template" '.metadata.labels[$role] = "sandbox"')"
verdict admission-refuses-agent-root "agent container uid 0: refused with $NONROOT" \
  refused_with $NONROOT submit "$(variant "$template" "$root_agent")"
verdict admission-refuses-agent-no-run-as-non-root "agent container without runAsNonRoot: refused with $NONROOT" \
  refused_with $NONROOT submit "$(variant "$template" '.spec.containers |= map(if .name == $agent then del(.securityContext.runAsNonRoot) else . end)')"
verdict admission-refuses-seed-name-other-image "container named $SEED_CONTAINER with the agent image: refused with $NONROOT" \
  refused_with $NONROOT submit "$(variant "$template" '(.spec.containers | map(select(.name == $agent))[0].image) as $image | .spec.containers |= map(if .name == $seed then .image = $image else . end)')"
verdict admission-refuses-root-container-other-name "root container with the tools image under another name: refused with $NONROOT" \
  refused_with $NONROOT submit "$(variant "$template" '.spec.containers += [(.spec.containers | map(select(.name == $seed))[0]) | .name = "extra"]')"
verdict admission-refuses-init-shaped-like-seed "init container named $SEED_CONTAINER with the tools image, uid 0, read-only root, no port: refused with $NONROOT" \
  refused_with $NONROOT submit "$(variant "$template" '(.spec.containers | map(select(.name == $seed))[0]) as $seed_container | .spec.containers |= map(select(.name != $seed)) | .spec.initContainers += [$seed_container]')"
verdict admission-refuses-shared-process-namespace "shareProcessNamespace true: refused with $NONROOT" \
  refused_with $NONROOT submit "$(variant "$template" '.spec.shareProcessNamespace = true')"
verdict admission-runner-has-no-ephemeral-right "runner patch of pods/ephemeralcontainers is forbidden by RBAC" \
  refused_forbidden as_runner patch pod "$warm_name" --subresource=ephemeralcontainers --type=strategic --dry-run=server -p "$(ephemeral_patch)"
bare=$(k -n "$ns" create -o jsonpath='{.metadata.name}' -f - <<< "$(variant "$template" '.metadata.labels["br.sandbox/pool"] = "battery" | .spec.containers |= map(select(.name != $seed))')")
track_pod "$ns" "$bare"
k -n "$ns" wait --for=condition=Ready "pod/$bare" --timeout=120s > /dev/null
verdict admission-refuses-ephemeral-shaped-like-seed "cluster admin adds an ephemeral container shaped like the seed (dry run): refused with $NONROOT" \
  refused_with $NONROOT k -n "$ns" patch pod "$bare" --subresource=ephemeralcontainers --type=strategic --dry-run=server -p "$(ephemeral_patch)"
verdict admission-check-identity-may-run-root "pod with uid 0 created by $NETCHECK_ID is accepted" \
  accepted submit_netcheck "$(variant "$template" "$root_agent")"
verdict admission-check-identity-not-shared-pid "pod with uid 0 and shareProcessNamespace by $NETCHECK_ID is refused" \
  refused_with $NONROOT submit_netcheck "$(variant "$template" "$root_agent | .spec.shareProcessNamespace = true")"

verdict admission-refuses-no-role "pod with no role label is refused" \
  refused_with ROLE submit "$(variant "$template" 'del(.metadata.labels[$role])')"
verdict admission-refuses-proxy-role "pod with role=proxy is refused" \
  refused_with ROLE submit "$(variant "$template" '.metadata.labels[$role] = "proxy"')"
verdict admission-refuses-secret-volume "role=sandbox pod with a Secret volume is refused" \
  refused_with VOLUME_FORBIDDEN submit "$(variant "$template" '.spec.volumes += [{name: "leak", secret: {secretName: "anything"}}]')"
verdict admission-refuses-projected-secret "role=sandbox pod with a projected Secret is refused" \
  refused_with VOLUME_FORBIDDEN submit "$(variant "$template" '.spec.volumes += [{name: "leak", projected: {sources: [{secret: {name: "anything"}}]}}]')"
verdict admission-refuses-secret-key-ref "role=sandbox pod with secretKeyRef is refused" \
  refused_with VOLUME_FORBIDDEN submit "$(variant "$template" '.spec.containers[0].env = ((.spec.containers[0].env // []) + [{name: "LEAK", valueFrom: {secretKeyRef: {name: "anything", key: "k"}}}])')"
verdict admission-refuses-env-from-secret "role=sandbox pod with envFrom secretRef is refused" \
  refused_with VOLUME_FORBIDDEN submit "$(variant "$template" '.spec.containers[0].envFrom = [{secretRef: {name: "anything"}}]')"

claim_to pod "$ns" "adm-$(new_id)" agent-a || { check admission-claim fail "claim refused"; scenario_end; }
verdict admission-runner-may-claim "label of thread on a sandbox pod is accepted" \
  accepted as_runner label pod "$pod" --overwrite --dry-run=server "$LBL_THREAD=changed"
verdict admission-refuses-relabel-role "label role=proxy on a sandbox pod is refused" \
  refused_with "ROLE(_IMMUTABLE)?" as_runner label pod "$pod" --overwrite --dry-run=server "$LBL_ROLE=$ROLE_PROXY"
verdict admission-refuses-role-removal "removal of the role label is refused" \
  refused_with "ROLE(_IMMUTABLE)?" as_runner label pod "$pod" --dry-run=server "$LBL_ROLE-"
verdict admission-refuses-foreign-label "patch of a label outside br.sandbox/ is refused" \
  refused_with LABELS as_runner patch pod "$pod" --dry-run=server --type merge -p '{"metadata":{"labels":{"app":"x"}}}'

verdict admission-no-proxy-pod-in-sandbox-namespace "pods with role=proxy in $ns: none" \
  test -z "$(k -n "$ns" get pods -l "$LBL_ROLE=$ROLE_PROXY" -o name)"
verdict admission-egress-namespace-not-bound "namespace $eg has no $LBL_MANAGED label" \
  test -z "$(ns_label "$eg" "$LBL_MANAGED")"
if need_proxy admission "$CLIENT_A" "$NET_ALLOW"; then
  proxy_pod=$(k -n "$eg" get pods -l "$LBL_ROLE=$ROLE_PROXY" -o jsonpath='{.items[0].metadata.name}')
  replica_set=$(k -n "$eg" get pod "$proxy_pod" -o jsonpath='{.metadata.ownerReferences[0].name}')
  verdict admission-proxy-pods-from-deployment "proxy pod $proxy_pod in $eg owned by ReplicaSet $replica_set of a Deployment" \
    test "$(k -n "$eg" get rs "$replica_set" -o jsonpath='{.metadata.ownerReferences[0].kind}')" = Deployment
fi

scenario_end
