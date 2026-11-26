{{- define "br-sandbox-chart.proxy.image" -}}
{{- include "br-sandbox-chart.image.mirrored" (dict "root" . "ref" (include "br-sandbox-chart.proxy.image.ref" .)) -}}
{{- end -}}

{{- define "br-sandbox-chart.proxy.image.ref" -}}
{{- printf "%s@%s" .Values.egress.proxy.image.repository .Values.egress.proxy.image.digest -}}
{{- end -}}

{{- define "br-sandbox-chart.proxy.security" -}}
{{- $proxy := .Values.egress.proxy -}}
{{- toJson (dict "allowPrivilegeEscalation" false "privileged" false "readOnlyRootFilesystem" true "runAsNonRoot" true "runAsUser" ($proxy.runAsUser | int64) "runAsGroup" ($proxy.runAsGroup | int64) "capabilities" (dict "drop" (list "ALL")) "seccompProfile" (dict "type" "RuntimeDefault")) -}}
{{- end -}}

{{- define "br-sandbox-chart.proxy.volumes" -}}
{{- $ns := .ns -}}
{{- $volumes := list (dict "name" "config" "configMap" (dict "name" "br-sandbox-proxy-config")) (dict "name" "ca" "configMap" (dict "name" "br-sandbox-proxy-ca")) (dict "name" "ca-key" "secret" (dict "secretName" $ns.proxy.ca.key_secret "defaultMode" 288 "items" (list (dict "key" "ca.key" "path" "ca.key")))) (dict "name" "tmp" "emptyDir" (dict "sizeLimit" "16Mi")) -}}
{{- if $ns.credential_set -}}
{{- $volumes = append $volumes (dict "name" "credentials" "secret" (dict "secretName" $ns.credential_set.secret_ref "defaultMode" 288)) -}}
{{- end -}}
{{- toJson $volumes -}}
{{- end -}}

{{- define "br-sandbox-chart.proxy.mounts" -}}
{{- $ns := .ns -}}
{{- $mounts := list (dict "name" "config" "mountPath" (include "br-sandbox-chart.proxy.mount.config" .) "readOnly" true) (dict "name" "ca" "mountPath" (include "br-sandbox-chart.proxy.mount.ca" .) "readOnly" true) (dict "name" "ca-key" "mountPath" (include "br-sandbox-chart.proxy.mount.ca_key" .) "readOnly" true) (dict "name" "tmp" "mountPath" "/tmp") -}}
{{- if $ns.credential_set -}}
{{- $mounts = append $mounts (dict "name" "credentials" "mountPath" (include "br-sandbox-chart.proxy.mount.credentials" .) "readOnly" true) -}}
{{- end -}}
{{- toJson $mounts -}}
{{- end -}}

{{- define "br-sandbox-chart.proxy.pod" -}}
{{- $root := .root -}}
{{- $proxy := $root.Values.egress.proxy -}}
{{- $ref := include "br-sandbox-chart.proxy.image.ref" $root -}}
{{- $port := $proxy.port | int64 -}}
{{- $probe := dict "tcpSocket" (dict "port" $port) "periodSeconds" 5 -}}
{{- $container := dict "name" "proxy" "image" (include "br-sandbox-chart.proxy.image" $root) "imagePullPolicy" (include "br-sandbox-chart.image.policy" (dict "root" $root "ref" $ref)) "args" (list "-config" (printf "%s/proxy.yaml" (include "br-sandbox-chart.proxy.mount.config" .))) "env" (list (dict "name" "IRON_MANAGEMENT_API_KEY" "valueFrom" (dict "secretKeyRef" (dict "name" $proxy.reload.key_secret "key" "token")))) "ports" (list (dict "name" "tunnel" "containerPort" $port "protocol" "TCP")) "readinessProbe" $probe "livenessProbe" $probe "securityContext" (include "br-sandbox-chart.proxy.security" $root | fromJson) "resources" $proxy.resources "volumeMounts" (include "br-sandbox-chart.proxy.mounts" . | fromJsonArray) -}}
{{- $spec := dict "hostUsers" false "automountServiceAccountToken" false "enableServiceLinks" false "dnsPolicy" "None" "dnsConfig" (dict "nameservers" $root.Values.network.public_resolvers) "securityContext" (dict "seccompProfile" (dict "type" "RuntimeDefault") "runAsNonRoot" true "runAsUser" ($proxy.runAsUser | int64) "runAsGroup" ($proxy.runAsGroup | int64) "fsGroup" ($proxy.runAsGroup | int64)) "containers" (list $container) "volumes" (include "br-sandbox-chart.proxy.volumes" . | fromJsonArray) -}}
{{- $pullSecrets := include "br-sandbox-chart.pull_secrets" $root | fromJsonArray -}}
{{- if $pullSecrets -}}
{{- $_ := set $spec "imagePullSecrets" $pullSecrets -}}
{{- end -}}
{{- toYaml $spec -}}
{{- end -}}
