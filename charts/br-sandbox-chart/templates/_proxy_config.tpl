{{- define "br-sandbox-chart.proxy.mount.ca" -}}
/etc/iron-proxy/ca
{{- end -}}

{{- define "br-sandbox-chart.proxy.mount.ca_key" -}}
/etc/iron-proxy/ca-key
{{- end -}}

{{- define "br-sandbox-chart.proxy.mount.credentials" -}}
/etc/iron-proxy/credentials
{{- end -}}

{{- define "br-sandbox-chart.proxy.mount.config" -}}
/etc/iron-proxy/config
{{- end -}}

{{- define "br-sandbox-chart.proxy.deny_cidrs" -}}
{{- $out := include "br-sandbox-chart.cidr.except" (dict "root" .root "family" "4") | fromJsonArray -}}
{{- if eq .root.Values.network.ipv6 "dual_stack" -}}
{{- $out = concat $out (include "br-sandbox-chart.cidr.except" (dict "root" .root "family" "6") | fromJsonArray) -}}
{{- end -}}
{{- toJson (concat $out (list "127.0.0.0/8" "::1/128") | uniq) -}}
{{- end -}}

{{- define "br-sandbox-chart.proxy.allowlist" -}}
{{- $config := dict -}}
{{- if eq .mode "full" -}}
{{- $_ := set $config "domains" (list "*") -}}
{{- else -}}
{{- $_ := set $config "domains" (.allow.domains | default list) -}}
{{- $_ := set $config "cidrs" (.allow.cidrs | default list) -}}
{{- end -}}
{{- toJson (dict "name" "allowlist" "config" $config) -}}
{{- end -}}

{{- define "br-sandbox-chart.proxy.secrets" -}}
{{- $out := list -}}
{{- $mount := include "br-sandbox-chart.proxy.mount.credentials" . -}}
{{- range $item := .secrets -}}
{{- $tls := true -}}
{{- if hasKey $item "require_tls" -}}
{{- $tls = $item.require_tls -}}
{{- end -}}
{{- $rules := list -}}
{{- range $host := $item.hosts -}}
{{- $rule := dict "require_tls" $tls -}}
{{- if contains "/" $host -}}
{{- $_ := set $rule "cidr" $host -}}
{{- else -}}
{{- $_ := set $rule "host" $host -}}
{{- end -}}
{{- $rules = append $rules $rule -}}
{{- end -}}
{{- $replace := dict "proxy_value" $item.placeholder "match_headers" ($item.match_headers | default (list "Authorization")) "match_query" ($item.match_query | default false) "match_path" ($item.match_path | default false) "match_body" ($item.match_body | default false) "require" ($item.require | default false) -}}
{{- $out = append $out (dict "source" (dict "type" "file" "path" (printf "%s/%s" $mount $item.key)) "replace" $replace "rules" $rules) -}}
{{- end -}}
{{- toJson $out -}}
{{- end -}}

{{- define "br-sandbox-chart.proxy.config" -}}
{{- $root := .root -}}
{{- $ns := .ns -}}
{{- $proxy := $root.Values.egress.proxy -}}
{{- $transforms := list (include "br-sandbox-chart.proxy.allowlist" (dict "mode" $ns.mode "allow" $ns.allow) | fromJson) -}}
{{- if $ns.credential_set -}}
{{- $transforms = append $transforms (dict "name" "secrets" "config" (dict "secrets" (include "br-sandbox-chart.proxy.secrets" (dict "secrets" $ns.credential_set.secrets) | fromJsonArray))) -}}
{{- end -}}
{{- $config := dict
  "dns" (dict "enabled" false)
  "proxy" (dict "http_listen" "127.0.0.1:8080" "https_listen" "127.0.0.1:8443" "tunnel_listen" (printf "0.0.0.0:%d" ($proxy.port | int64)) "upstream_deny_cidrs" (include "br-sandbox-chart.proxy.deny_cidrs" (dict "root" $root) | fromJsonArray))
  "tls" (dict "mode" "mitm" "ca_cert" (printf "%s/ca.crt" (include "br-sandbox-chart.proxy.mount.ca" .)) "ca_key" (printf "%s/ca.key" (include "br-sandbox-chart.proxy.mount.ca_key" .)))
  "transforms" $transforms
  "management" (dict "listen" $proxy.reload.listen "api_key_env" "IRON_MANAGEMENT_API_KEY")
  "metrics" (dict "listen" "127.0.0.1:9090")
  "log" (dict "level" "info") -}}
{{- toYaml $config -}}
{{- end -}}
