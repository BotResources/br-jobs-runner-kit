#!/usr/bin/env bash

BASELINE_CODES='BR_SANDBOX_PROBE_HOST BR_SANDBOX_PROC_MOUNT BR_SANDBOX_APPARMOR BR_SANDBOX_SELINUX BR_SANDBOX_SYSCTLS BR_SANDBOX_WINDOWS_HOST_PROCESS'

without_rule() {
    jq -c --arg code "$1" 'map(if .kind == "ValidatingAdmissionPolicy" and .metadata.name == "br-sandbox-pod" then .spec.validations |= map(select(.message | startswith($code + ":") | not)) else . end)' "$WORK/ex.json" >"$WORK/without_$1.json"
}

namespaces_of() {
    jq -r '.[] | select(.kind == "ConfigMap" and .metadata.name == "br-sandbox-pod-template") | .metadata.namespace' "$WORK/$1.json"
}

check_every_rendered_pod_shape_passes_the_baseline_rules() {
    section "baseline rules: the pod of the template in each mode, the probe pod of the network check and the pod with the seed container are admitted"
    local render ns
    for render in ex noseed mirror keeper nondefault egress_keeper; do
        for ns in $(namespaces_of "$render"); do
            t "render ${render}: the pod that the keeper makes for ${ns} is admitted by all rules" test "$(created "$render" "$ns")" = admit
        done
    done
    for render in ex keeper mirror egress_keeper; do
        for ns in $(namespaces_of "$render"); do
            t "render ${render}: the probe pod of the check for ${ns} is admitted by all rules" test "$(verdict "$render" CREATE pods "$(probe_pod "$render" "$ns" | jq -c .)" null "[$(pool_of "$ns")]")" = admit
        done
    done
    t "the render with the seed container holds a seed container and the render without it holds none" test "$(jq -r '[.[] | select(.kind == "ConfigMap" and .metadata.name == "br-sandbox-pod-template") | .data["pod-template.json"] | fromjson | .spec.containers | map(.name) | index("seed") != null] | unique | tostring' "$WORK/ex.json" "$WORK/noseed.json" | tr -d '\n')" = '[true][false]'
    t "the admitted pods set no probe, no hook, no procMount, no AppArmor, no SELinux option, no sysctl and no Windows option" jqt ex "${SANDBOX_PODS} | all(([.spec.containers[], .spec.initContainers[]] | all((has(\"livenessProbe\") or has(\"readinessProbe\") or has(\"startupProbe\") or has(\"lifecycle\")) | not)) and ([.spec.securityContext, .spec.containers[].securityContext, .spec.initContainers[].securityContext] | all((has(\"procMount\") or has(\"appArmorProfile\") or has(\"seLinuxOptions\") or has(\"sysctls\") or has(\"windowsOptions\")) | not)))"
}

check_probes_and_hooks_with_a_host_are_refused() {
    section "baseline rules: a probe or a lifecycle hook with a host is refused, exec and a request with no host are admitted"
    local field kind host open exec empty
    for field in livenessProbe readinessProbe startupProbe lifecycle.postStart lifecycle.preStop; do
        for kind in httpGet tcpSocket; do
            host="$(printf '(agent).%s.%s = {"host": "169.254.169.254", "port": 80}' "$field" "$kind")"
            open="$(printf '(agent).%s.%s = {"port": 80}' "$field" "$kind")"
            t "${field} with ${kind}.host on the agent is refused by BR_SANDBOX_PROBE_HOST" test "$(created ex "$NS" "$host")" = BR_SANDBOX_PROBE_HOST
            t "${field} with ${kind} and no host on the agent is admitted" test "$(created ex "$NS" "$open")" = admit
        done
        exec="$(printf '(agent).%s.exec = {"command": ["true"]}' "$field")"
        empty="$(printf '(agent).%s.httpGet = {"host": "", "port": 80}' "$field")"
        t "${field} with an exec handler is admitted" test "$(created ex "$NS" "$exec")" = admit
        t "${field} with an empty host is admitted" test "$(created ex "$NS" "$empty")" = admit
    done
    t "a probe with a host on an init container is refused" test "$(created ex "$NS" '.spec.initContainers[0].startupProbe.tcpSocket = {"host": "10.0.0.1", "port": 22}')" = BR_SANDBOX_PROBE_HOST
    t "a hook with a host on the seed container is refused" test "$(created ex "$NS" '.spec.containers[1].lifecycle.postStart.httpGet = {"host": "10.0.0.1", "port": 22}')" = BR_SANDBOX_PROBE_HOST
    t "the update binding refuses a probe with a host on a container: the rule runs on update" test "$(updated ex "$NS" '(agent).livenessProbe.httpGet = {"host": "10.0.0.1", "port": 80}')" = BR_SANDBOX_IMMUTABLE,BR_SANDBOX_PROBE_HOST
    without_rule BR_SANDBOX_PROBE_HOST
    t "the model sees a policy without the probe rule: the same pod is then admitted" test "$(created without_BR_SANDBOX_PROBE_HOST "$NS" '(agent).readinessProbe.httpGet = {"host": "10.0.0.1", "port": 80}')" = admit
}

check_procmount_apparmor_selinux_are_held_at_the_baseline_level() {
    section "baseline rules: procMount, AppArmor and SELinux options are held at the Baseline level"
    t "procMount Unmasked on the agent is refused by BR_SANDBOX_PROC_MOUNT" test "$(created ex "$NS" '(agent).securityContext.procMount = "Unmasked"')" = BR_SANDBOX_PROC_MOUNT
    t "procMount Unmasked on an init container is refused" test "$(created ex "$NS" '.spec.initContainers[0].securityContext.procMount = "Unmasked"')" = BR_SANDBOX_PROC_MOUNT
    t "procMount Default is admitted" test "$(created ex "$NS" '(agent).securityContext.procMount = "Default"')" = admit
    t "the update binding refuses procMount Unmasked" test "$(updated ex "$NS" '(agent).securityContext.procMount = "Unmasked"')" = BR_SANDBOX_IMMUTABLE,BR_SANDBOX_PROC_MOUNT
    t "an AppArmor profile Unconfined on the pod is refused by BR_SANDBOX_APPARMOR" test "$(created ex "$NS" '.spec.securityContext.appArmorProfile = {"type": "Unconfined"}')" = BR_SANDBOX_APPARMOR
    t "an AppArmor profile Unconfined on a container is refused" test "$(created ex "$NS" '(agent).securityContext.appArmorProfile = {"type": "Unconfined"}')" = BR_SANDBOX_APPARMOR
    t "an AppArmor profile Unconfined on an init container is refused" test "$(created ex "$NS" '.spec.initContainers[0].securityContext.appArmorProfile = {"type": "Unconfined"}')" = BR_SANDBOX_APPARMOR
    t "an AppArmor profile RuntimeDefault on the pod and Localhost on a container are admitted" test "$(created ex "$NS" '.spec.securityContext.appArmorProfile = {"type": "RuntimeDefault"} | (agent).securityContext.appArmorProfile = {"type": "Localhost", "localhostProfile": "p"}')" = admit
    t "the AppArmor annotation unconfined is refused" test "$(created ex "$NS" '.metadata.annotations["container.apparmor.security.beta.kubernetes.io/agent"] = "unconfined"')" = BR_SANDBOX_APPARMOR
    t "the AppArmor annotation with a made-up value is refused" test "$(created ex "$NS" '.metadata.annotations["container.apparmor.security.beta.kubernetes.io/agent"] = "x"')" = BR_SANDBOX_APPARMOR
    t "the AppArmor annotations runtime/default and localhost/profile are admitted: the API server may set them from the field" test "$(created ex "$NS" '.metadata.annotations["container.apparmor.security.beta.kubernetes.io/agent"] = "runtime/default" | .metadata.annotations["container.apparmor.security.beta.kubernetes.io/seed"] = "localhost/profile"')" = admit
    t "an annotation of another key with the value unconfined is admitted" test "$(created ex "$NS" '.metadata.annotations["example.test/profile"] = "unconfined"')" = admit
    t "the update binding refuses the AppArmor annotation unconfined" test "$(updated ex "$NS" '.metadata.annotations["container.apparmor.security.beta.kubernetes.io/agent"] = "unconfined"')" = BR_SANDBOX_APPARMOR
    t "a SELinux type container_t on the pod and container_engine_t on a container are admitted" test "$(created ex "$NS" '.spec.securityContext.seLinuxOptions = {"type": "container_t", "level": "s0:c1,c2"} | (agent).securityContext.seLinuxOptions = {"type": "container_engine_t"}')" = admit
    t "a SELinux type outside the Baseline list on the pod is refused by BR_SANDBOX_SELINUX" test "$(created ex "$NS" '.spec.securityContext.seLinuxOptions = {"type": "spc_t"}')" = BR_SANDBOX_SELINUX
    t "a SELinux type outside the Baseline list on a container is refused" test "$(created ex "$NS" '(agent).securityContext.seLinuxOptions = {"type": "unconfined_t"}')" = BR_SANDBOX_SELINUX
    t "a SELinux user is refused" test "$(created ex "$NS" '(agent).securityContext.seLinuxOptions = {"user": "system_u"}')" = BR_SANDBOX_SELINUX
    t "a SELinux role on the pod is refused" test "$(created ex "$NS" '.spec.securityContext.seLinuxOptions = {"role": "system_r"}')" = BR_SANDBOX_SELINUX
    t "a SELinux option on an init container is refused" test "$(created ex "$NS" '.spec.initContainers[0].securityContext.seLinuxOptions = {"type": "spc_t"}')" = BR_SANDBOX_SELINUX
    t "the update binding refuses a SELinux type outside the list" test "$(updated ex "$NS" '.spec.securityContext.seLinuxOptions = {"type": "spc_t"}')" = BR_SANDBOX_IMMUTABLE,BR_SANDBOX_SELINUX
}

check_sysctls_and_windows_host_process_are_held_at_the_baseline_level() {
    section "baseline rules: sysctls and Windows host process are held at the Baseline level"
    t "a sysctl outside the safe list is refused by BR_SANDBOX_SYSCTLS" test "$(created ex "$NS" '.spec.securityContext.sysctls = [{"name": "kernel.msgmax", "value": "65536"}]')" = BR_SANDBOX_SYSCTLS
    t "one unsafe sysctl among safe ones is refused" test "$(created ex "$NS" '.spec.securityContext.sysctls = [{"name": "net.ipv4.tcp_syncookies", "value": "1"}, {"name": "net.core.somaxconn", "value": "1024"}]')" = BR_SANDBOX_SYSCTLS
    t "the safe sysctls are admitted" test "$(created ex "$NS" '.spec.securityContext.sysctls = [{"name": "net.ipv4.tcp_syncookies", "value": "1"}, {"name": "kernel.shm_rmid_forced", "value": "1"}, {"name": "net.ipv4.ip_unprivileged_port_start", "value": "0"}]')" = admit
    t "an empty list of sysctls is admitted" test "$(created ex "$NS" '.spec.securityContext.sysctls = []')" = admit
    t "the update binding refuses an unsafe sysctl" test "$(updated ex "$NS" '.spec.securityContext.sysctls = [{"name": "kernel.msgmax", "value": "1"}]')" = BR_SANDBOX_IMMUTABLE,BR_SANDBOX_SYSCTLS
    t "hostProcess true on the pod is refused by BR_SANDBOX_WINDOWS_HOST_PROCESS" test "$(created ex "$NS" '.spec.securityContext.windowsOptions = {"hostProcess": true, "runAsUserName": "x"}')" = BR_SANDBOX_WINDOWS_HOST_PROCESS
    t "hostProcess true on a container is refused" test "$(created ex "$NS" '(agent).securityContext.windowsOptions = {"hostProcess": true}')" = BR_SANDBOX_WINDOWS_HOST_PROCESS
    t "hostProcess true on an init container is refused" test "$(created ex "$NS" '.spec.initContainers[0].securityContext.windowsOptions = {"hostProcess": true}')" = BR_SANDBOX_WINDOWS_HOST_PROCESS
    t "hostProcess false is admitted" test "$(created ex "$NS" '.spec.securityContext.windowsOptions = {"hostProcess": false} | (agent).securityContext.windowsOptions = {"gmsaCredentialSpecName": "x"}')" = admit
    t "the update binding refuses hostProcess true" test "$(updated ex "$NS" '(agent).securityContext.windowsOptions = {"hostProcess": true}')" = BR_SANDBOX_IMMUTABLE,BR_SANDBOX_WINDOWS_HOST_PROCESS
    local code
    for code in $BASELINE_CODES; do
        without_rule "$code"
        t "the policy without ${code} has twenty rules" jqt "without_${code}" "[${VAP} | .spec.validations | length] == [20]"
    done
    t "the model sees a policy without the sysctl rule: the same pod is then admitted" test "$(created without_BR_SANDBOX_SYSCTLS "$NS" '.spec.securityContext.sysctls = [{"name": "kernel.msgmax", "value": "65536"}]')" = admit
    t "the model sees a policy without the procMount rule: the same pod is then admitted" test "$(created without_BR_SANDBOX_PROC_MOUNT "$NS" '(agent).securityContext.procMount = "Unmasked"')" = admit
    t "the model sees a policy without the AppArmor rule: the same pod is then admitted" test "$(created without_BR_SANDBOX_APPARMOR "$NS" '.spec.securityContext.appArmorProfile = {"type": "Unconfined"}')" = admit
    t "the model sees a policy without the SELinux rule: the same pod is then admitted" test "$(created without_BR_SANDBOX_SELINUX "$NS" '(agent).securityContext.seLinuxOptions = {"user": "system_u"}')" = admit
    t "the model sees a policy without the Windows rule: the same pod is then admitted" test "$(created without_BR_SANDBOX_WINDOWS_HOST_PROCESS "$NS" '(agent).securityContext.windowsOptions = {"hostProcess": true}')" = admit
}

check_the_existing_isolation_rules_hold_each_item_of_the_baseline_level() {
    section "baseline level: the items that older rules hold are refused by the model"
    t "hostNetwork is refused by BR_SANDBOX_HOST_NAMESPACE" test "$(created ex "$NS" '.spec.hostNetwork = true')" = BR_SANDBOX_HOST_NAMESPACE
    t "hostPID is refused by BR_SANDBOX_HOST_NAMESPACE" test "$(created ex "$NS" '.spec.hostPID = true')" = BR_SANDBOX_HOST_NAMESPACE
    t "hostIPC is refused by BR_SANDBOX_HOST_NAMESPACE" test "$(created ex "$NS" '.spec.hostIPC = true')" = BR_SANDBOX_HOST_NAMESPACE
    t "the host namespaces set to false are admitted" test "$(created ex "$NS" '.spec.hostNetwork = false | .spec.hostPID = false | .spec.hostIPC = false')" = admit
    t "a host port on the agent is refused" test "$(created ex "$NS" '(agent).ports = [{"containerPort": 8080, "hostPort": 8080}]')" = BR_SANDBOX_HOST_NAMESPACE
    t "a host port on an init container is refused" test "$(created ex "$NS" '.spec.initContainers[0].ports = [{"containerPort": 8080, "hostPort": 8080}]')" = BR_SANDBOX_HOST_NAMESPACE
    t "a container port with no host port, and a host port 0, are admitted" test "$(created ex "$NS" '(agent).ports = [{"containerPort": 8080}, {"containerPort": 8081, "hostPort": 0}]')" = admit
    t "a hostPath volume is refused by BR_SANDBOX_VOLUME_FORBIDDEN" test "$(created ex "$NS" '.spec.volumes += [{"name": "h", "hostPath": {"path": "/"}}]')" = BR_SANDBOX_VOLUME_FORBIDDEN
    t "a privileged agent is refused by BR_SANDBOX_PRIVILEGE" test "$(created ex "$NS" '(agent).securityContext.privileged = true')" = BR_SANDBOX_PRIVILEGE
    t "a privileged init container is refused" test "$(created ex "$NS" '.spec.initContainers[0].securityContext.privileged = true')" = BR_SANDBOX_PRIVILEGE
    t "allowPrivilegeEscalation true is refused by BR_SANDBOX_PRIVILEGE" test "$(created ex "$NS" '(agent).securityContext.allowPrivilegeEscalation = true')" = BR_SANDBOX_PRIVILEGE
    t "an absent allowPrivilegeEscalation is refused" test "$(created ex "$NS" 'del((agent).securityContext.allowPrivilegeEscalation)')" = BR_SANDBOX_PRIVILEGE
    t "an added capability outside the seven on an init container is refused" test "$(created ex "$NS" '.spec.initContainers[0].securityContext.capabilities.add += ["NET_ADMIN"]')" = BR_SANDBOX_CAPABILITY
    t "a capability of the Baseline list that is not one of the seven is refused: the chart is stricter" test "$(created ex "$NS" '(agent).securityContext.capabilities.add += ["NET_BIND_SERVICE"]')" = BR_SANDBOX_CAPABILITY
    t "seccomp Localhost on the pod is refused: the chart is stricter than the Baseline level" test "$(created ex "$NS" '.spec.securityContext.seccompProfile = {"type": "Localhost", "localhostProfile": "p"}')" = BR_SANDBOX_PRIVILEGE
    t "the update binding refuses hostNetwork" test "$(updated ex "$NS" '.spec.hostNetwork = true')" = BR_SANDBOX_HOST_NAMESPACE,BR_SANDBOX_IMMUTABLE
    without_rule BR_SANDBOX_HOST_NAMESPACE
    t "the model sees a policy without the host rule: hostNetwork is then admitted" test "$(created without_BR_SANDBOX_HOST_NAMESPACE "$NS" '.spec.hostNetwork = true')" = admit
    t "the model sees a policy without the host rule: a host port is then admitted" test "$(created without_BR_SANDBOX_HOST_NAMESPACE "$NS" '(agent).ports = [{"containerPort": 8080, "hostPort": 8080}]')" = admit
}
