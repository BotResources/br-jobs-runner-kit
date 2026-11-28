{{- define "br-sandbox-chart.validate.stamper" -}}
{{- if eq (len .Values.scopes) 0 -}}
{{- fail "BR_SANDBOX_STAMPER_NO_SCOPES" -}}
{{- end -}}
{{- if not (has .Values.stamper.identity (.Values.grants.writers | default list)) -}}
{{- fail (printf "BR_SANDBOX_STAMPER_NOT_A_WRITER{identity=%s}" .Values.stamper.identity) -}}
{{- end -}}
{{- range $entry := .Values.namespaces -}}
{{- if include "br-sandbox-chart.egress.proxied" (dict "mode" $entry.mode "values" $entry) -}}
{{- fail (printf "BR_SANDBOX_MODE_NOT_STAMPED{namespace=%s,mode=%s}" $entry.name $entry.mode) -}}
{{- end -}}
{{- end -}}
{{- end -}}
