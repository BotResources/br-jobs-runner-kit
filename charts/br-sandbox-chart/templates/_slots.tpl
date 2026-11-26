{{- define "br-sandbox-chart.slot.egress.env" -}}
{{- include "br-sandbox-chart.proxy.sandbox.env" . -}}
{{- end -}}

{{- define "br-sandbox-chart.slot.egress.volumes" -}}
{{- $out := list -}}
{{- if include "br-sandbox-chart.proxy.sandbox.active" . -}}
{{- $out = append $out (dict "name" "egress-ca" "configMap" (dict "name" "br-sandbox-ca")) -}}
{{- end -}}
{{- toYaml $out -}}
{{- end -}}

{{- define "br-sandbox-chart.slot.egress.volume_mounts" -}}
{{- $out := list -}}
{{- if include "br-sandbox-chart.proxy.sandbox.active" . -}}
{{- $out = append $out (dict "name" "egress-ca" "mountPath" .root.Values.egress.ca.mount_path "readOnly" true) -}}
{{- end -}}
{{- toYaml $out -}}
{{- end -}}

{{- define "br-sandbox-chart.slot.egress.objects" -}}
{{- if .root.Values.egress.enabled -}}
{{- include "br-sandbox-chart.proxy.objects" . -}}
{{- end -}}
{{- end -}}

{{- define "br-sandbox-chart.slot.keeper.objects" -}}
{{- end -}}
