{{- define "br-sandbox-chart.proxy.url" -}}
{{- printf "http://%s:%d" .ns.proxy.cluster_ip (.root.Values.egress.proxy.port | int64) -}}
{{- end -}}

{{- define "br-sandbox-chart.proxy.sandbox.active" -}}
{{- if and .root.Values.egress.enabled .ns.egress -}}
true
{{- end -}}
{{- end -}}

{{- define "br-sandbox-chart.proxy.probe.active" -}}
{{- if and (include "br-sandbox-chart.proxy.sandbox.active" .) (eq .ns.mode "allow") -}}
true
{{- end -}}
{{- end -}}

{{- define "br-sandbox-chart.proxy.sandbox.env" -}}
{{- $out := list -}}
{{- if include "br-sandbox-chart.proxy.sandbox.active" . -}}
{{- $url := include "br-sandbox-chart.proxy.url" . -}}
{{- $bundle := printf "%s/ca.crt" (include "br-sandbox-chart.proxy.mount.sandbox_ca" .) -}}
{{- range $name := list "HTTPS_PROXY" "HTTP_PROXY" "https_proxy" "http_proxy" -}}
{{- $out = append $out (dict "name" $name "value" $url) -}}
{{- end -}}
{{- range $name := list "NO_PROXY" "no_proxy" -}}
{{- $out = append $out (dict "name" $name "value" "") -}}
{{- end -}}
{{- range $name := .root.Values.egress.ca.env_names -}}
{{- $out = append $out (dict "name" $name "value" $bundle) -}}
{{- end -}}
{{- range $item := .ns.credential_set.secrets | default list -}}
{{- $out = append $out (dict "name" $item.env_name "value" $item.placeholder) -}}
{{- end -}}
{{- end -}}
{{- toYaml $out -}}
{{- end -}}

{{- define "br-sandbox-chart.proxy.probe.env" -}}
{{- $out := list (dict "name" "NETCHECK_TARGETS" "value" "__TARGETS__") -}}
{{- if include "br-sandbox-chart.proxy.probe.active" . -}}
{{- $out = append $out (dict "name" "NETCHECK_PROXY" "value" (printf "%s:%d" .ns.proxy.cluster_ip (.root.Values.egress.proxy.port | int64))) -}}
{{- $out = append $out (dict "name" "NETCHECK_OFFLIST" "value" (printf "%s:443" .root.Values.network_check.offlist_host)) -}}
{{- end -}}
{{- toYaml $out -}}
{{- end -}}
