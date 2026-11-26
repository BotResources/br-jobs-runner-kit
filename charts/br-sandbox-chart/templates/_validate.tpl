{{- define "br-sandbox-chart.require" -}}
{{- $node := .root.Values -}}
{{- $ok := true -}}
{{- range $segment := splitList "." .path -}}
{{- if and $ok (kindIs "map" $node) (hasKey $node $segment) -}}
{{- $node = index $node $segment -}}
{{- else -}}
{{- $ok = false -}}
{{- end -}}
{{- end -}}
{{- if or (not $ok) (kindIs "invalid" $node) (and (kindIs "string" $node) (eq $node "")) -}}
{{- fail (printf "BR_SANDBOX_MISSING_VALUE{key=%s}" .path) -}}
{{- end -}}
{{- end -}}

{{- define "br-sandbox-chart.validate" -}}
{{- $root := . -}}
{{- range $path := list "installation.namespace_prefix" "image" "tools.image.repository" "tools.image.digest" "save.ttl_seconds" "store.endpoint" "store.bucket" "store.region" "store.path_style" "store.credentials_secret" "network.pod_cidr" "network.service_cidr" "network.node_addresses" "network.node_public_addresses" "network.load_balancer_addresses" "clients" -}}
{{- include "br-sandbox-chart.require" (dict "root" $root "path" $path) -}}
{{- end -}}
{{- if le (.Values.save.ttl_seconds | int64) 0 -}}
{{- fail "BR_SANDBOX_SAVE_AGE_INVALID{key=save.ttl_seconds}" -}}
{{- end -}}
{{- if .Values.egress.enabled -}}
{{- fail "BR_SANDBOX_SLOT_EMPTY{slot=egress}" -}}
{{- end -}}
{{- if .Values.keeper.enabled -}}
{{- range $path := list "keeper.image" "network.api_server_addresses" "network.store_cidrs" -}}
{{- include "br-sandbox-chart.require" (dict "root" $root "path" $path) -}}
{{- end -}}
{{- end -}}
{{- if .Values.network_check.enabled -}}
{{- range $path := list "network_check.image" "network.api_server_addresses" -}}
{{- include "br-sandbox-chart.require" (dict "root" $root "path" $path) -}}
{{- end -}}
{{- end -}}
{{- $prefix := .Values.installation.namespace_prefix -}}
{{- $system := default (printf "%s-system" $prefix) .Values.installation.system_namespace -}}
{{- $seen := dict $system "system" -}}
{{- $needsResolvers := false -}}
{{- range $client, $clientValues := .Values.clients -}}
{{- if not (hasKey $clientValues "modes") -}}
{{- fail (printf "BR_SANDBOX_MISSING_VALUE{key=clients.%s.modes}" $client) -}}
{{- end -}}
{{- range $mode, $modeValues := $clientValues.modes -}}
{{- if not (has $mode (list "none" "allow" "full")) -}}
{{- fail (printf "BR_SANDBOX_MODE_UNKNOWN{client=%s,mode=%s}" $client $mode) -}}
{{- end -}}
{{- if ne $mode "none" -}}
{{- $needsResolvers = true -}}
{{- end -}}
{{- range $path := list "service_account" "namespace" -}}
{{- include "br-sandbox-chart.require" (dict "root" $root "path" (printf "clients.%s.modes.%s.runner.%s" $client $mode $path)) -}}
{{- end -}}
{{- $name := printf "%s-%s-%s" $prefix $client $mode -}}
{{- if hasKey $seen $name -}}
{{- fail (printf "BR_SANDBOX_SHARED_NAMESPACE{client_a=%s,client_b=%s,namespace=%s}" (get $seen $name) $client $name) -}}
{{- end -}}
{{- $_ := set $seen $name $client -}}
{{- $pool := mergeOverwrite (deepCopy $root.Values.pool) (deepCopy ($modeValues.pool | default dict)) -}}
{{- if gt ($pool.floor | int64) ($pool.max_sandboxes | int64) -}}
{{- fail (printf "BR_SANDBOX_POOL_INVALID{client=%s,mode=%s,reason=floor_above_max_sandboxes}" $client $mode) -}}
{{- end -}}
{{- end -}}
{{- end -}}
{{- if $needsResolvers -}}
{{- include "br-sandbox-chart.require" (dict "root" $root "path" "network.public_resolvers") -}}
{{- end -}}
{{- end -}}
