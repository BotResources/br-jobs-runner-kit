{{- define "br-sandbox-chart.cidr.normalize" -}}
{{- if contains "/" . -}}
{{- . -}}
{{- else if contains ":" . -}}
{{- printf "%s/128" . -}}
{{- else -}}
{{- printf "%s/32" . -}}
{{- end -}}
{{- end -}}

{{- define "br-sandbox-chart.cidr.except" -}}
{{- $network := .root.Values.network -}}
{{- $all := concat $network.internal_cidrs (list $network.pod_cidr $network.service_cidr) $network.node_addresses $network.node_public_addresses $network.load_balancer_addresses -}}
{{- if eq .family "6" -}}
{{- $all = concat $all (list "fc00::/7" "fe80::/10") -}}
{{- end -}}
{{- $out := list -}}
{{- range $entry := $all -}}
{{- $cidr := include "br-sandbox-chart.cidr.normalize" $entry -}}
{{- if eq (contains ":" $cidr) (eq $.family "6") -}}
{{- $out = append $out $cidr -}}
{{- end -}}
{{- end -}}
{{- toJson ($out | uniq) -}}
{{- end -}}
