{{- define "br-sandbox-chart.vap.handler_host" -}}
{{- printf "((!has(%s.httpGet) || !has(%s.httpGet.host) || %s.httpGet.host == '') && (!has(%s.tcpSocket) || !has(%s.tcpSocket.host) || %s.tcpSocket.host == ''))" . . . . . . -}}
{{- end -}}

{{- define "br-sandbox-chart.vap.probe_host" -}}
{{- $out := list -}}
{{- range $probe := list "livenessProbe" "readinessProbe" "startupProbe" -}}
{{- $path := printf "c.%s" $probe -}}
{{- $out = append $out (printf "(!has(%s) || %s)" $path (include "br-sandbox-chart.vap.handler_host" $path)) -}}
{{- end -}}
{{- range $hook := list "postStart" "preStop" -}}
{{- $path := printf "c.lifecycle.%s" $hook -}}
{{- $out = append $out (printf "(!has(c.lifecycle) || !has(%s) || %s)" $path (include "br-sandbox-chart.vap.handler_host" $path)) -}}
{{- end -}}
{{- printf "variables.containers.all(c, %s)" (join " && " $out) -}}
{{- end -}}

{{- define "br-sandbox-chart.vap.on_contexts" -}}
{{- printf "((!has(object.spec.securityContext) || %s) && variables.containers.all(c, !has(c.securityContext) || %s))" (replace "@" "object.spec.securityContext" .) (replace "@" "c.securityContext" .) -}}
{{- end -}}

{{- define "br-sandbox-chart.vap.proc_mount" -}}
{{- printf "variables.containers.all(c, !has(c.securityContext) || !has(c.securityContext.procMount) || c.securityContext.procMount == 'Default')" -}}
{{- end -}}

{{- define "br-sandbox-chart.vap.apparmor" -}}
{{- $profile := "(!has(@.appArmorProfile) || @.appArmorProfile.type in ['RuntimeDefault', 'Localhost'])" -}}
{{- $annotation := "(!has(object.metadata.annotations) || object.metadata.annotations.all(k, !k.startsWith('container.apparmor.security.beta.kubernetes.io/') || object.metadata.annotations[k] == 'runtime/default' || object.metadata.annotations[k].startsWith('localhost/')))" -}}
{{- printf "(%s && %s)" (include "br-sandbox-chart.vap.on_contexts" $profile) $annotation -}}
{{- end -}}

{{- define "br-sandbox-chart.vap.selinux" -}}
{{- $options := "(!has(@.seLinuxOptions) || ((!has(@.seLinuxOptions.type) || @.seLinuxOptions.type in ['', 'container_t', 'container_init_t', 'container_kvm_t', 'container_engine_t']) && (!has(@.seLinuxOptions.user) || @.seLinuxOptions.user == '') && (!has(@.seLinuxOptions.role) || @.seLinuxOptions.role == '')))" -}}
{{- include "br-sandbox-chart.vap.on_contexts" $options -}}
{{- end -}}

{{- define "br-sandbox-chart.vap.sysctls" -}}
{{- $safe := "['kernel.shm_rmid_forced', 'net.ipv4.ip_local_port_range', 'net.ipv4.ip_unprivileged_port_start', 'net.ipv4.tcp_syncookies', 'net.ipv4.ping_group_range', 'net.ipv4.ip_local_reserved_ports', 'net.ipv4.tcp_keepalive_time', 'net.ipv4.tcp_fin_timeout', 'net.ipv4.tcp_keepalive_intvl', 'net.ipv4.tcp_keepalive_probes', 'net.ipv4.tcp_rmem', 'net.ipv4.tcp_wmem']" -}}
{{- printf "(!has(object.spec.securityContext) || !has(object.spec.securityContext.sysctls) || object.spec.securityContext.sysctls.all(s, s.name in %s))" $safe -}}
{{- end -}}

{{- define "br-sandbox-chart.vap.host_process" -}}
{{- include "br-sandbox-chart.vap.on_contexts" "(!has(@.windowsOptions) || !has(@.windowsOptions.hostProcess) || !@.windowsOptions.hostProcess)" -}}
{{- end -}}
