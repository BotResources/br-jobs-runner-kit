{{- define "br-sandbox-chart.pod.security" -}}
{{- $security := dict "allowPrivilegeEscalation" false "privileged" false "runAsUser" (.uid | int64) "runAsGroup" (.gid | int64) "capabilities" (dict "drop" (list "ALL") "add" (list "CHOWN" "DAC_OVERRIDE" "FOWNER" "FSETID" "SETUID" "SETGID" "KILL")) -}}
{{- if .read_only_root -}}
{{- $_ := set $security "readOnlyRootFilesystem" true -}}
{{- end -}}
{{- toJson $security -}}
{{- end -}}

{{- define "br-sandbox-chart.pod.labels" -}}
br.sandbox/role: sandbox
br.sandbox/pool: {{ .pool | quote }}
br.sandbox/pool-id: {{ .ns.poolId | quote }}
{{- end -}}

{{- define "br-sandbox-chart.pod.spec" -}}
{{- $root := .root -}}
{{- $ns := .ns -}}
{{- $values := $root.Values -}}
{{- $probe := .probe | default false -}}
{{- $resources := include "br-sandbox-chart.resources" $ns.pool.size | fromJson -}}
{{- $uid := $values.runAsUser -}}
{{- $gid := $values.runAsGroup -}}
{{- $mount := $values.tools.mount_path -}}
{{- $seed := $values.tools.seed_container.enabled -}}
{{- $nameservers := list "127.0.0.1" -}}
{{- if and (eq $ns.mode "full") (not $ns.egress) -}}
{{- $nameservers = $values.network.public_resolvers -}}
{{- end -}}
{{- $spec := dict "hostUsers" false "automountServiceAccountToken" false "enableServiceLinks" false "serviceAccountName" "sandbox" "terminationGracePeriodSeconds" ($ns.pool.terminationGracePeriodSeconds | int64) "dnsPolicy" "None" "dnsConfig" (dict "nameservers" $nameservers) "securityContext" (dict "seccompProfile" (dict "type" "RuntimeDefault")) -}}
{{- if $values.runtimeClassName -}}
{{- $_ := set $spec "runtimeClassName" $values.runtimeClassName -}}
{{- end -}}
{{- if $ns.pool.node_selector -}}
{{- $_ := set $spec "nodeSelector" $ns.pool.node_selector -}}
{{- end -}}
{{- $pullSecrets := include "br-sandbox-chart.pull_secrets" $root | fromJsonArray -}}
{{- if $pullSecrets -}}
{{- $_ := set $spec "imagePullSecrets" $pullSecrets -}}
{{- end -}}
{{- if $probe -}}
{{- $image := include "br-sandbox-chart.image.mirrored" (dict "root" $root "ref" $values.network_check.image) -}}
{{- $_ := set $spec "restartPolicy" "Never" -}}
{{- $_ := set $spec "containers" (list (dict "name" "probe" "image" $image "imagePullPolicy" (include "br-sandbox-chart.image.policy" (dict "root" $root "ref" $values.network_check.image)) "command" (list "sh" "/netcheck/network-check.sh" "probe") "env" (list (dict "name" "NETCHECK_TARGETS" "value" "__TARGETS__")) "securityContext" (include "br-sandbox-chart.pod.security" (dict "uid" 0 "gid" 0) | fromJson) "resources" $resources "volumeMounts" (list (dict "name" "netcheck" "mountPath" "/netcheck" "readOnly" true)))) -}}
{{- $_ := set $spec "volumes" (list (dict "name" "netcheck" "configMap" (dict "name" "br-sandbox-netcheck"))) -}}
{{- else -}}
{{- $toolsImage := include "br-sandbox-chart.image.tools" $root -}}
{{- $toolsRef := printf "%s@%s" $values.tools.image.repository $values.tools.image.digest -}}
{{- $toolsPolicy := include "br-sandbox-chart.image.policy" (dict "root" $root "ref" $toolsRef) -}}
{{- $mounts := list (dict "name" "tools" "mountPath" $mount "readOnly" true) (dict "name" "work" "mountPath" "/work") (dict "name" "home" "mountPath" "/home/agent") (dict "name" "skills" "mountPath" "/skills" "readOnly" $seed) -}}
{{- $mounts = concat $mounts (include "br-sandbox-chart.slot.egress.volume_mounts" $root | fromYamlArray) -}}
{{- $env := concat (list (dict "name" "HOME" "value" "/home/agent")) (include "br-sandbox-chart.slot.egress.env" $root | fromYamlArray) -}}
{{- $agent := dict "name" "agent" "image" (include "br-sandbox-chart.image.project" $root) "imagePullPolicy" (include "br-sandbox-chart.image.policy" (dict "root" $root "ref" $values.image)) "command" (list (printf "%s/bin/pause" $mount)) "env" $env "securityContext" (include "br-sandbox-chart.pod.security" (dict "uid" $uid "gid" $gid) | fromJson) "resources" $resources "volumeMounts" $mounts -}}
{{- $containers := list $agent -}}
{{- if $seed -}}
{{- $containers = append $containers (dict "name" $values.tools.seed_container.name "image" $toolsImage "imagePullPolicy" $toolsPolicy "command" (list "/sbx/bin/pause") "securityContext" (include "br-sandbox-chart.pod.security" (dict "uid" 0 "gid" 0 "read_only_root" true) | fromJson) "resources" $values.tools.seed_container.resources "volumeMounts" (list (dict "name" "skills" "mountPath" "/skills"))) -}}
{{- end -}}
{{- $init := dict "name" "tools-init" "image" $toolsImage "imagePullPolicy" $toolsPolicy "command" (list "/sbx/bin/bsdtar" "-x" "-p" "--no-same-owner" "-f" "/sbx/tools.tar" "-C" $mount) "securityContext" (include "br-sandbox-chart.pod.security" (dict "uid" $uid "gid" $gid) | fromJson) "resources" $values.tools.init_resources "volumeMounts" (list (dict "name" "tools" "mountPath" $mount)) -}}
{{- $volumes := list (dict "name" "tools" "emptyDir" (dict "sizeLimit" (toString $values.tools.size_limit))) (dict "name" "work" "emptyDir" (dict "sizeLimit" (toString $values.work.size_limit))) (dict "name" "home" "emptyDir" (dict "sizeLimit" (toString $values.home.size_limit))) (dict "name" "skills" "emptyDir" (dict "sizeLimit" (toString $values.seed.size_limit))) -}}
{{- $volumes = concat $volumes (include "br-sandbox-chart.slot.egress.volumes" $root | fromYamlArray) -}}
{{- $_ := set $spec "initContainers" (list $init) -}}
{{- $_ := set $spec "containers" $containers -}}
{{- $_ := set $spec "volumes" $volumes -}}
{{- end -}}
{{- toYaml $spec -}}
{{- end -}}
