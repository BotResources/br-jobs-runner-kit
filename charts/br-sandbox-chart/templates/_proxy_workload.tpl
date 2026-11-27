{{- define "br-sandbox-chart.proxy.image" -}}
{{- include "br-sandbox-chart.image.mirrored" (dict "root" . "ref" (include "br-sandbox-chart.proxy.image.ref" .)) -}}
{{- end -}}

{{- define "br-sandbox-chart.proxy.image.ref" -}}
{{- printf "%s@%s" .Values.egress.proxy.image.repository .Values.egress.proxy.image.digest -}}
{{- end -}}

{{- define "br-sandbox-chart.proxy.security" -}}
{{- $id := include "br-sandbox-chart.proxy.run_as" . | int64 -}}
{{- toJson (dict "allowPrivilegeEscalation" false "privileged" false "readOnlyRootFilesystem" true "runAsNonRoot" true "runAsUser" $id "runAsGroup" $id "capabilities" (dict "drop" (list "ALL")) "seccompProfile" (dict "type" "RuntimeDefault")) -}}
{{- end -}}

{{- define "br-sandbox-chart.proxy.trust.file" -}}
{{- $trust := .root.Values.egress.proxy.upstream_ca | default dict -}}
{{- if $trust -}}
{{- printf "%s/%s" (include "br-sandbox-chart.proxy.mount.trust" .) ($trust.key | default "ca.crt") -}}
{{- end -}}
{{- end -}}

{{- define "br-sandbox-chart.proxy.env" -}}
{{- $proxy := .root.Values.egress.proxy -}}
{{- $out := list (dict "name" "IRON_MANAGEMENT_API_KEY" "valueFrom" (dict "secretKeyRef" (dict "name" $proxy.reload.key_secret "key" "token"))) -}}
{{- $file := include "br-sandbox-chart.proxy.trust.file" . -}}
{{- if $file -}}
{{- $out = append $out (dict "name" "SSL_CERT_FILE" "value" $file) -}}
{{- end -}}
{{- toJson $out -}}
{{- end -}}

{{- define "br-sandbox-chart.proxy.volumes" -}}
{{- $ns := .ns -}}
{{- $volumes := list (dict "name" "config" "configMap" (dict "name" "br-sandbox-proxy-config")) (dict "name" "ca" "configMap" (dict "name" "br-sandbox-proxy-ca")) (dict "name" "ca-key" "secret" (dict "secretName" $ns.proxy.ca.key_secret "defaultMode" 288 "items" (list (dict "key" "ca.key" "path" "ca.key")))) (dict "name" "tmp" "emptyDir" (dict "sizeLimit" "16Mi")) -}}
{{- if $ns.credential_set -}}
{{- $volumes = append $volumes (dict "name" "credentials" "secret" (dict "secretName" $ns.credential_set.secret_ref "defaultMode" 288)) -}}
{{- end -}}
{{- $trust := .root.Values.egress.proxy.upstream_ca | default dict -}}
{{- if $trust -}}
{{- $key := $trust.key | default "ca.crt" -}}
{{- $volumes = append $volumes (dict "name" "upstream-ca" "configMap" (dict "name" $trust.config_map "items" (list (dict "key" $key "path" $key)))) -}}
{{- end -}}
{{- toJson $volumes -}}
{{- end -}}

{{- define "br-sandbox-chart.proxy.mounts" -}}
{{- $ns := .ns -}}
{{- $mounts := list (dict "name" "config" "mountPath" (include "br-sandbox-chart.proxy.mount.config" .) "readOnly" true) (dict "name" "ca" "mountPath" (include "br-sandbox-chart.proxy.mount.ca" .) "readOnly" true) (dict "name" "ca-key" "mountPath" (include "br-sandbox-chart.proxy.mount.ca_key" .) "readOnly" true) (dict "name" "tmp" "mountPath" "/tmp") -}}
{{- if $ns.credential_set -}}
{{- $mounts = append $mounts (dict "name" "credentials" "mountPath" (include "br-sandbox-chart.proxy.mount.credentials" .) "readOnly" true) -}}
{{- end -}}
{{- if .root.Values.egress.proxy.upstream_ca -}}
{{- $mounts = append $mounts (dict "name" "upstream-ca" "mountPath" (include "br-sandbox-chart.proxy.mount.trust" .) "readOnly" true) -}}
{{- end -}}
{{- toJson $mounts -}}
{{- end -}}

{{- define "br-sandbox-chart.proxy.pod" -}}
{{- $root := .root -}}
{{- $proxy := $root.Values.egress.proxy -}}
{{- $ref := include "br-sandbox-chart.proxy.image.ref" $root -}}
{{- $port := $proxy.port | int64 -}}
{{- $id := include "br-sandbox-chart.proxy.run_as" . | int64 -}}
{{- $probe := dict "tcpSocket" (dict "port" $port) "periodSeconds" 5 -}}
{{- $container := dict "name" "proxy" "image" (include "br-sandbox-chart.proxy.image" $root) "imagePullPolicy" (include "br-sandbox-chart.image.policy" (dict "root" $root "ref" $ref)) "args" (list "-config" (printf "%s/proxy.yaml" (include "br-sandbox-chart.proxy.mount.config" .))) "env" (include "br-sandbox-chart.proxy.env" . | fromJsonArray) "ports" (list (dict "name" "tunnel" "containerPort" $port "protocol" "TCP")) "readinessProbe" $probe "livenessProbe" $probe "securityContext" (include "br-sandbox-chart.proxy.security" $root | fromJson) "resources" $proxy.resources "volumeMounts" (include "br-sandbox-chart.proxy.mounts" . | fromJsonArray) -}}
{{- $spec := dict "hostUsers" false "automountServiceAccountToken" false "enableServiceLinks" false "dnsPolicy" "None" "dnsConfig" (dict "nameservers" $root.Values.network.public_resolvers) "securityContext" (dict "seccompProfile" (dict "type" "RuntimeDefault") "runAsNonRoot" true "runAsUser" $id "runAsGroup" $id "fsGroup" $id) "containers" (list $container) "volumes" (include "br-sandbox-chart.proxy.volumes" . | fromJsonArray) -}}
{{- $pullSecrets := include "br-sandbox-chart.pull_secrets" $root | fromJsonArray -}}
{{- if $pullSecrets -}}
{{- $_ := set $spec "imagePullSecrets" $pullSecrets -}}
{{- end -}}
{{- toYaml $spec -}}
{{- end -}}
