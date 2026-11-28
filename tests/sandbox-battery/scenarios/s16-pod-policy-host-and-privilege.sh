#!/usr/bin/env bash
HERE=$(cd "$(dirname "$0")" && pwd)
. "$HERE/common.sh"
scenario_begin s16-pod-policy-host-and-privilege

: "${CODES_HOST_SHARING:=HOST_NAMESPACE|USERNS_REQUIRED}"
: "${CODES_HOST_PORT:=HOST_NAMESPACE}"
: "${CODES_HOST_PATH:=VOLUME_FORBIDDEN}"
: "${CODES_PRIVILEGE:=PRIVILEGE}"
: "${CODES_CAPABILITY:=CAPABILITY}"
: "${CODES_PROBE_HOST:=PROBE_HOST}"
: "${CODES_HOOK_HOST:=PROBE_HOST}"

PROBE_TARGET_HOST=192.0.2.10

cleanup_extra() { policy_pod_security_restore; }

policy_setup "$(ns_of "$CLIENT_A" "$NET_ALLOW")" || scenario_end

host_refs='.spec.hostUsers = true'

policy_case policy-refuses-host-network "pod with hostNetwork true (hostUsers true as well: the API server refuses hostNetwork with hostUsers false, so rule USERNS_REQUIRED refuses it too)" \
  "$CODES_HOST_SHARING" ".spec.hostNetwork = true | $host_refs"
policy_case policy-refuses-host-pid "pod with hostPID true (hostUsers true as well)" \
  "$CODES_HOST_SHARING" ".spec.hostPID = true | $host_refs"
policy_case policy-refuses-host-ipc "pod with hostIPC true (hostUsers true as well)" \
  "$CODES_HOST_SHARING" ".spec.hostIPC = true | $host_refs"

port_bad='.ports = [{containerPort: 18080, hostPort: 18080, protocol: "TCP"}]'
port_ok='.ports = [{containerPort: 18080, protocol: "TCP"}]'
policy_case policy-refuses-host-port-main "agent container with a hostPort" \
  "$CODES_HOST_PORT" "$(on_agent "$port_bad")" "$(on_agent "$port_ok")"
policy_case policy-refuses-host-port-init "init container (restartable) with a hostPort" \
  "$CODES_HOST_PORT" "$(on_init "$port_bad")" "$(on_init "$port_ok")"

policy_case policy-refuses-host-path-volume "pod with a hostPath volume of the node root" \
  "$CODES_HOST_PATH" '.spec.volumes += [{name: "node-root", hostPath: {path: "/", type: "Directory"}}]' \
  '.spec.volumes += [{name: "scratch", emptyDir: {}}]'

priv_bad='.securityContext.privileged = true | .securityContext.allowPrivilegeEscalation = true'
cap_bad='.securityContext.capabilities.add += ["SYS_ADMIN"]'
esc_bad='.securityContext.allowPrivilegeEscalation = true'
policy_case policy-refuses-privileged-main "agent container privileged" "$CODES_PRIVILEGE" "$(on_agent "$priv_bad")"
policy_case policy-refuses-privileged-init "init container privileged" "$CODES_PRIVILEGE" "$(on_init_plain "$priv_bad")"
policy_case policy-refuses-added-capability-main "agent container with the added capability SYS_ADMIN" "$CODES_CAPABILITY" "$(on_agent "$cap_bad")"
policy_case policy-refuses-added-capability-init "init container with the added capability SYS_ADMIN" "$CODES_CAPABILITY" "$(on_init_plain "$cap_bad")"
policy_case policy-refuses-privilege-escalation-main "agent container with allowPrivilegeEscalation true" "$CODES_PRIVILEGE" "$(on_agent "$esc_bad")"
policy_case policy-refuses-privilege-escalation-init "init container with allowPrivilegeEscalation true" "$CODES_PRIVILEGE" "$(on_init_plain "$esc_bad")"

probe_with_host() { printf '%s' "{$1: {httpGet: {host: \"$PROBE_TARGET_HOST\", port: 8080, path: \"/\"}}}"; }
probe_without_host() { printf '%s' "{$1: {httpGet: {port: 8080, path: \"/\"}}}"; }
tcp_with_host() { printf '%s' "{$1: {tcpSocket: {host: \"$PROBE_TARGET_HOST\", port: 8080}}}"; }
tcp_without_host() { printf '%s' "{$1: {tcpSocket: {port: 8080}}}"; }

empty_host_probes() {
  printf '%s' '{livenessProbe: {httpGet: {host: "", port: 8080, path: "/"}}, readinessProbe: {tcpSocket: {host: "", port: 8080}}, startupProbe: {httpGet: {host: "", port: 8080, path: "/"}}, lifecycle: {postStart: {httpGet: {host: "", port: 8080, path: "/"}}, preStop: {tcpSocket: {host: "", port: 8080}}}}'
}

post_start_bad=".lifecycle.postStart = {httpGet: {host: \"$PROBE_TARGET_HOST\", port: 8080, path: \"/\"}}"
post_start_ok='.lifecycle.postStart = {httpGet: {port: 8080, path: "/"}}'
pre_stop_bad=".lifecycle.preStop = {tcpSocket: {host: \"$PROBE_TARGET_HOST\", port: 8080}}"
pre_stop_ok='.lifecycle.preStop = {tcpSocket: {port: 8080}}'

for where in main init; do
  if [ "$where" = main ]; then put=on_agent; else put=on_init; fi
  policy_case "policy-refuses-liveness-http-host-$where" "$where container with a liveness probe httpGet.host" "$CODES_PROBE_HOST" \
    "$($put ". += $(probe_with_host livenessProbe)")" "$($put ". += $(probe_without_host livenessProbe)")"
  policy_case "policy-refuses-readiness-tcp-host-$where" "$where container with a readiness probe tcpSocket.host" "$CODES_PROBE_HOST" \
    "$($put ". += $(tcp_with_host readinessProbe)")" "$($put ". += $(tcp_without_host readinessProbe)")"
  policy_case "policy-refuses-startup-http-host-$where" "$where container with a startup probe httpGet.host" "$CODES_PROBE_HOST" \
    "$($put ". += $(probe_with_host startupProbe)")" "$($put ". += $(probe_without_host startupProbe)")"
  policy_case "policy-refuses-post-start-hook-host-$where" "$where container with a postStart hook httpGet.host" "$CODES_HOOK_HOST" \
    "$($put "$post_start_bad")" "$($put "$post_start_ok")"
  policy_case "policy-refuses-pre-stop-hook-host-$where" "$where container with a preStop hook tcpSocket.host" "$CODES_HOOK_HOST" \
    "$($put "$pre_stop_bad")" "$($put "$pre_stop_ok")"
  policy_accepts "policy-accepts-empty-host-probes-and-hooks-$where" "$where container with a liveness, readiness and startup probe and a postStart and a preStop hook whose host field is empty (the rule refuses a host that is set and not empty)" \
    "$($put ". += $(empty_host_probes)")"
done

scenario_end
