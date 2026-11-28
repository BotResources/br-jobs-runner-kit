{{- define "br-sandbox-chart.vap.pool_quantity" -}}
{{- $out := list -}}
{{- range $key, $sizeKey := dict "cpu" "cpu" "memory" "memory" "ephemeral-storage" "disk" -}}
{{- range $kind := list (dict "resource" "requests" "size" "request") (dict "resource" "limits" "size" "limit") -}}
{{- $path := printf "params.spec.size.%s.%s" $sizeKey $kind.size -}}
{{- $out = append $out (printf "(!has(params.spec.size) || !has(params.spec.size.%s) || !has(%s) || !quantity(c.resources.%s['%s']).isGreaterThan(quantity(%s)))" $sizeKey $path $kind.resource $key $path) -}}
{{- end -}}
{{- end -}}
{{- join " && " $out -}}
{{- end -}}

{{- define "br-sandbox-chart.vap.unchanged" -}}
{{- $out := list -}}
{{- range $field := list "containers" "initContainers" "ephemeralContainers" "hostUsers" "hostNetwork" "hostPID" "hostIPC" "shareProcessNamespace" "serviceAccountName" "automountServiceAccountToken" "runtimeClassName" "securityContext" "volumes" -}}
{{- $out = append $out (printf "(has(object.spec.%s) == has(oldObject.spec.%s) && (!has(object.spec.%s) || object.spec.%s == oldObject.spec.%s))" $field $field $field $field $field) -}}
{{- end -}}
{{- join " && " $out -}}
{{- end -}}
