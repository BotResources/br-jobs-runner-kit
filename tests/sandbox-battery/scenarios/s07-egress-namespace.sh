#!/usr/bin/env bash
HERE=$(cd "$(dirname "$0")" && pwd)
. "$HERE/common.sh"
scenario_begin s07-egress-namespace

ns_a=$(ns_of "$CLIENT_A" "$NET_ALLOW")
eg_a=$(egress_ns_of "$CLIENT_A" "$NET_ALLOW")
ns_b=$(ns_of "$CLIENT_B" "$NET_ALLOW")
eg_b=$(egress_ns_of "$CLIENT_B" "$NET_ALLOW")
sandbox_id="system:serviceaccount:$ns_a:$SANDBOX_SA"
standin_secret="$STANDIN_POD-secret"

cleanup_extra() {
  k -n "$eg_a" delete pod/$STANDIN_POD configmap/$STANDIN_POD-code secret/$standin_secret --ignore-not-found --wait=false > /dev/null 2>&1
}

standin_args() { printf '"%s", ' $STANDIN_PORTS | sed 's/, $//'; }

create_standin() {
  k -n "$eg_a" create configmap $STANDIN_POD-code --from-file=standin-listener.py="$HERE/standin-listener.py" > /dev/null &&
    k -n "$eg_a" create secret generic $standin_secret --from-literal=value=standin > /dev/null &&
    k -n "$eg_a" apply -f - > /dev/null << YAML
apiVersion: v1
kind: Pod
metadata:
  name: $STANDIN_POD
  labels: {$LBL_ROLE: $ROLE_PROXY}
spec:
  containers:
  - name: listener
    image: $ECHO_IMAGE
    imagePullPolicy: IfNotPresent
    command: [python3, /code/standin-listener.py]
    args: [$(standin_args)]
    volumeMounts:
    - {name: code, mountPath: /code}
  volumes:
  - {name: code, configMap: {name: $STANDIN_POD-code}}
YAML
}

ports_json() {
  local port
  for port in $STANDIN_PORTS; do printf '{"host":"%s","port":%s}\n' "$1" "$port"; done | jq -s -c .
}

open_ports() { jq -s -c '[.[] | select(.open) | .port] | sort'; }

identity_checks() {
  local label=$1 who=$2 eg=$3 verb resource sub
  verdict "egress-$label-identity-known" "impersonated user is $who" \
    test "$(k --as="$who" auth whoami -o jsonpath='{.status.userInfo.username}')" = "$who"
  while read -r verb resource sub; do
    verdict "egress-$label-cannot-$verb-$resource${sub:+-$sub}-in-$eg" "kubectl auth can-i $verb $resource ${sub:+subresource $sub }in $eg" \
      test "$(k --as="$who" -n "$eg" auth can-i "$verb" "$resource" ${sub:+--subresource="$sub"})" = no
  done << LIST
create pods exec
create pods
patch pods
delete pods
get pods
list pods
get secrets
list secrets
LIST
}

attempt_checks() {
  local label=$1 who=$2 eg=$3 bare
  bare=$(pod_template "$ns_a")
  verdict "egress-$label-exec-refused" "exec into $STANDIN_POD in $eg" \
    refused_forbidden k --as="$who" -n "$eg" exec "$STANDIN_POD" -- true
  verdict "egress-$label-create-pod-refused" "create of a pod in $eg" \
    refused_forbidden k --as="$who" -n "$eg" create --dry-run=server -f - <<< "$(variant "$bare" '.')"
  verdict "egress-$label-patch-pod-refused" "patch of $STANDIN_POD in $eg" \
    refused_forbidden k --as="$who" -n "$eg" patch pod "$STANDIN_POD" --dry-run=server --type merge -p '{"metadata":{"labels":{"x":"y"}}}'
  verdict "egress-$label-get-secret-refused" "read of $standin_secret in $eg" \
    refused_forbidden k --as="$who" -n "$eg" get secret "$standin_secret"
}

for pair in "$CLIENT_A:$NET_ALLOW:$eg_a:$ns_a" "$CLIENT_B:$NET_ALLOW:$eg_b:$ns_b"; do
  IFS=: read -r _ _ eg sbx <<< "$pair"
  verdict "egress-$eg-labelled" "$eg has $LBL_EGRESS=true" test "$(ns_label "$eg" "$LBL_EGRESS")" = true
  verdict "egress-$eg-not-managed" "$eg has no $LBL_MANAGED label" test -z "$(ns_label "$eg" "$LBL_MANAGED")"
  verdict "egress-$sbx-managed" "$sbx has $LBL_MANAGED=true" test "$(ns_label "$sbx" "$LBL_MANAGED")" = true
  verdict "egress-$eg-no-role-or-binding" "roles and bindings in $eg: none" test -z "$(k -n "$eg" get role,rolebinding -o name)"
  identity_checks "runner-$eg" "$RUNNER_ID" "$eg"
  identity_checks "keeper-$eg" "$KEEPER_ID" "$eg"
  identity_checks "sandbox-$eg" "system:serviceaccount:$sbx:$SANDBOX_SA" "$eg"
done
for net in "$NET_NONE" "$NET_FULL"; do
  verdict "egress-none-for-$net" "no egress namespace for $(ns_of "$CLIENT_A" "$net")" \
    test -z "$(k get ns "$(egress_ns_of "$CLIENT_A" "$net")" -o name 2> /dev/null)"
done

verdict egress-runner-positive-control "runner can create pods/exec in $ns_a" \
  test "$(as_id "$RUNNER_ID" "$ns_a" auth can-i create pods --subresource=exec)" = yes
verdict egress-keeper-positive-control "keeper can delete pods in $ns_a" test "$(as_id "$KEEPER_ID" "$ns_a" auth can-i delete pods)" = yes

wait_warm "$ns_a" 1 120 || { check egress-warm-pod fail "no warm pod in $ns_a"; scenario_end; }
verdict egress-namespace-admits-proxy-role-pod "pod $STANDIN_POD with $LBL_ROLE=$ROLE_PROXY is created in $eg_a" create_standin
k -n "$eg_a" wait --for=condition=Ready pod/$STANDIN_POD --timeout=180s > /dev/null || { check egress-standin-ready fail "stand-in not ready in $eg_a"; scenario_end; }
standin_ip=$(k -n "$eg_a" get pod $STANDIN_POD -o jsonpath='{.status.podIP}')
ports=$(ports_json "$standin_ip")
want=$(printf '%s\n' $STANDIN_PORTS | jq -s -c 'sort')
verdict egress-standin-listens-on-every-port "positive control inside the pod: open ports $(probe_ctl "$eg_a" $STANDIN_POD tcp "$(ports_json 127.0.0.1)" | open_ports)" \
  test "$(probe_ctl "$eg_a" $STANDIN_POD tcp "$(ports_json 127.0.0.1)" | open_ports)" = "$want"

attempt_checks runner "$RUNNER_ID" "$eg_a"
attempt_checks keeper "$KEEPER_ID" "$eg_a"
attempt_checks sandbox "$sandbox_id" "$eg_a"

claim_to pod_a "$ns_a" "egress-$(new_id)" agent-a
verdict egress-tunnel-port-only "open ports of the proxy from the sandbox of the same client and mode: $(probe "$ns_a" "$pod_a" tcp "$ports" | open_ports)" \
  test "$(probe "$ns_a" "$pod_a" tcp "$ports" | open_ports)" = "[$PROXY_PORT]"

for other in "$CLIENT_B:$NET_ALLOW" "$CLIENT_A:$NET_NONE" "$CLIENT_A:$NET_FULL"; do
  IFS=: read -r client net <<< "$other"
  other_ns=$(ns_of "$client" "$net")
  wait_warm "$other_ns" 1 120 || { check "egress-$other_ns-warm-pod" fail "no warm pod in $other_ns"; continue; }
  claim_to other_pod "$other_ns" "egress-$(new_id)" agent-b
  verdict "egress-$other_ns-no-port" "open ports of $eg_a from $other_ns: $(probe "$other_ns" "$other_pod" tcp "$ports" | open_ports)" \
    test "$(probe "$other_ns" "$other_pod" tcp "$ports" | open_ports)" = "[]"
done

warm_name=$(warm_list "$ns_a" | head -1 | cut -d' ' -f1)
control_pod_to control "$TESTSVC_NS" "$(pod_image "$ns_a" "$warm_name")" || { check egress-control-pod fail "control pod not ready"; scenario_end; }
verdict egress-no-port-from-outside "open ports of $eg_a from $TESTSVC_NS: $(probe_ctl "$TESTSVC_NS" "$control" tcp "$ports" | open_ports)" \
  test "$(probe_ctl "$TESTSVC_NS" "$control" tcp "$ports" | open_ports)" = "[]"

runner_variant=$(variant "$(pod_template "$ns_a")" '.metadata.labels[$role] = "sandbox"')
verdict egress-sandbox-policy-accepts-sandbox-role "runner pod with role=sandbox in $ns_a is accepted" \
  accepted as_id "$RUNNER_ID" "$ns_a" create --dry-run=server -f - <<< "$runner_variant"
verdict egress-sandbox-policy-refuses-proxy-role "runner pod with role=proxy in $ns_a is refused" \
  refused_with ROLE as_id "$RUNNER_ID" "$ns_a" create --dry-run=server -f - <<< "$(variant "$(pod_template "$ns_a")" '.metadata.labels[$role] = "proxy"')"

scenario_end
