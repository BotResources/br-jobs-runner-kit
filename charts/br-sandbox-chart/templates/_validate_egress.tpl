{{- define "br-sandbox-chart.validate.credentials" -}}
{{- $where := printf "client=%s,mode=%s" .client .mode -}}
{{- $set := get .values "credential_set" | default dict -}}
{{- $placeholders := dict -}}
{{- $envs := dict -}}
{{- $keys := dict -}}
{{- $reserved := concat (list "HOME" "HTTPS_PROXY" "HTTP_PROXY" "https_proxy" "http_proxy" "NO_PROXY" "no_proxy") .root.Values.egress.ca.env_names -}}
{{- range $index, $item := get $set "secrets" | default list -}}
{{- if eq (len $item.hosts) 0 -}}
{{- fail (printf "BR_SANDBOX_CREDENTIAL_HOSTS_EMPTY{%s,item=%d}" $where $index) -}}
{{- end -}}
{{- range $host := $item.hosts -}}
{{- if or (eq (trim $host) "*") (hasSuffix "/0" $host) -}}
{{- fail (printf "BR_SANDBOX_CREDENTIAL_HOST_WILDCARD{%s,item=%d,host=%s}" $where $index $host) -}}
{{- end -}}
{{- end -}}
{{- if has $item.env_name $reserved -}}
{{- fail (printf "BR_SANDBOX_CREDENTIAL_ENV_RESERVED{%s,item=%d,env_name=%s}" $where $index $item.env_name) -}}
{{- end -}}
{{- if eq $item.placeholder "" -}}
{{- fail (printf "BR_SANDBOX_CREDENTIAL_PLACEHOLDER_EMPTY{%s,item=%d}" $where $index) -}}
{{- end -}}
{{- $tls := true -}}
{{- if hasKey $item "require_tls" -}}
{{- $tls = $item.require_tls -}}
{{- end -}}
{{- if and ($item.require | default false) $tls -}}
{{- fail (printf "BR_SANDBOX_CREDENTIAL_REQUIRE_TLS{%s,item=%d}" $where $index) -}}
{{- end -}}
{{- if hasKey $placeholders $item.placeholder -}}
{{- fail (printf "BR_SANDBOX_CREDENTIAL_PLACEHOLDER_DUPLICATE{%s,item=%d,first=%d}" $where $index (get $placeholders $item.placeholder | int)) -}}
{{- end -}}
{{- if hasKey $envs $item.env_name -}}
{{- fail (printf "BR_SANDBOX_CREDENTIAL_ENV_DUPLICATE{%s,item=%d,env_name=%s}" $where $index $item.env_name) -}}
{{- end -}}
{{- if hasKey $keys $item.key -}}
{{- fail (printf "BR_SANDBOX_CREDENTIAL_KEY_DUPLICATE{%s,item=%d,key=%s}" $where $index $item.key) -}}
{{- end -}}
{{- $_ := set $placeholders $item.placeholder $index -}}
{{- $_ := set $envs $item.env_name $index -}}
{{- $_ := set $keys $item.key $index -}}
{{- end -}}
{{- end -}}

{{- define "br-sandbox-chart.validate.proxy" -}}
{{- $root := .root -}}
{{- $base := printf "clients.%s.modes.%s.proxy" .client .mode -}}
{{- range $path := list "cluster_ip" "ca.certificate" "ca.key_secret" -}}
{{- include "br-sandbox-chart.require" (dict "root" $root "path" (printf "%s.%s" $base $path)) -}}
{{- end -}}
{{- $address := .values.proxy.cluster_ip -}}
{{- if hasKey .ips $address -}}
{{- fail (printf "BR_SANDBOX_PROXY_ADDRESS_DUPLICATE{client_a=%s,client_b=%s,cluster_ip=%s}" (get .ips $address) .client $address) -}}
{{- end -}}
{{- $_ := set .ips $address .client -}}
{{- end -}}

{{- define "br-sandbox-chart.validate.proxy_global" -}}
{{- $proxy := .Values.egress.proxy -}}
{{- include "br-sandbox-chart.require" (dict "root" . "path" "egress.proxy.reload.key_secret") -}}
{{- if has (toString ($proxy.port | int64)) (list "8080" "8443" "9090" "9092") -}}
{{- fail (printf "BR_SANDBOX_PROXY_PORT_CONFLICT{port=%d}" ($proxy.port | int64)) -}}
{{- end -}}
{{- if gt (add ($proxy.replicas | int64) 1) (.Values.egress.quota.pods | int64) -}}
{{- fail (printf "BR_SANDBOX_PROXY_QUOTA{replicas=%d,pods=%d}" ($proxy.replicas | int64) (.Values.egress.quota.pods | int64)) -}}
{{- end -}}
{{- end -}}
