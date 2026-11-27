#!/usr/bin/env bash
HERE=$(cd "$(dirname "$0")" && pwd)
. "$HERE/common.sh"
scenario_begin s17-pod-policy-baseline

: "${CODES_PROC_MOUNT:=PROC_MOUNT}"
: "${CODES_APPARMOR_FIELD:=APPARMOR}"
: "${CODES_APPARMOR_ANNOTATION:=APPARMOR}"
: "${CODES_SELINUX_CONTAINER:=SELINUX}"
: "${CODES_SELINUX_POD:=SELINUX}"
: "${CODES_SYSCTL:=SYSCTL}"
: "${CODES_WINDOWS_HOST_PROCESS:=WINDOWS_HOST_PROCESS}"
: "${CODES_SECCOMP_CONTAINER:=PRIVILEGE}"
: "${CODES_SECCOMP_POD:=PRIVILEGE}"

policy_setup "$(ns_of "$CLIENT_A" "$NET_ALLOW")" || scenario_end

apparmor_key='("container.apparmor.security.beta.kubernetes.io/" + $agent)'

policy_case policy-refuses-proc-mount-unmasked "agent container with procMount Unmasked" "$CODES_PROC_MOUNT" \
  "$(on_agent '.securityContext.procMount = "Unmasked"')"
policy_case policy-refuses-apparmor-unconfined-field "agent container with the AppArmor profile Unconfined in the field" "$CODES_APPARMOR_FIELD" \
  "$(on_agent '.securityContext.appArmorProfile = {type: "Unconfined"}')" \
  "$(on_agent '.securityContext.appArmorProfile = {type: "RuntimeDefault"}')"
policy_case policy-refuses-apparmor-unconfined-annotation "pod with the AppArmor annotation unconfined for the agent container" "$CODES_APPARMOR_ANNOTATION" \
  ".metadata.annotations = ((.metadata.annotations // {}) + {$apparmor_key: \"unconfined\"})" \
  ".metadata.annotations = ((.metadata.annotations // {}) + {$apparmor_key: \"runtime/default\"})"
policy_case policy-refuses-selinux-type-container "agent container with the SELinux type spc_t" "$CODES_SELINUX_CONTAINER" \
  "$(on_agent '.securityContext.seLinuxOptions = {type: "spc_t"}')" \
  "$(on_agent '.securityContext.seLinuxOptions = {type: "container_t"}')"
policy_case policy-refuses-selinux-type-pod "pod with the SELinux type spc_t" "$CODES_SELINUX_POD" \
  '.spec.securityContext.seLinuxOptions = {type: "spc_t"}' \
  '.spec.securityContext.seLinuxOptions = {type: "container_t"}'
policy_case policy-refuses-unsafe-sysctl "pod with the unsafe sysctl net.core.somaxconn" "$CODES_SYSCTL" \
  '.spec.securityContext.sysctls = [{name: "net.core.somaxconn", value: "1024"}]' \
  '.spec.securityContext.sysctls = [{name: "net.ipv4.ip_local_port_range", value: "32768 60999"}]'

host_process='.securityContext.windowsOptions = {hostProcess: true, runAsUserName: "NT AUTHORITY\\SYSTEM"}'
policy_case policy-refuses-windows-host-process "pod whose containers all run as Windows host process (the API server needs hostNetwork true and hostUsers true for it)" "$CODES_WINDOWS_HOST_PROCESS" \
  ".spec.hostNetwork = true | .spec.hostUsers = true | .spec.containers |= map($host_process) | .spec.initContainers |= map($host_process)"

policy_case policy-refuses-seccomp-unconfined-container "agent container with the seccomp profile Unconfined" "$CODES_SECCOMP_CONTAINER" \
  "$(on_agent '.securityContext.seccompProfile = {type: "Unconfined"}')" \
  "$(on_agent '.securityContext.seccompProfile = {type: "RuntimeDefault"}')"
policy_case policy-refuses-seccomp-unconfined-pod "pod with the seccomp profile Unconfined" "$CODES_SECCOMP_POD" \
  '.spec.securityContext.seccompProfile = {type: "Unconfined"}'

scenario_end
