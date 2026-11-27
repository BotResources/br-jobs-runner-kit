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
{{- if hasKey .Values "keeper" -}}
{{- fail "BR_SANDBOX_KEEPER_VALUES_REMOVED{key=keeper}" -}}
{{- end -}}
{{- range $path := list "installation.namespace_prefix" "image" "tools.image.repository" "tools.image.digest" "save.ttl_seconds" "store.endpoint" "store.bucket" "store.region" "store.path_style" "store.credentials_secret" "network.pod_cidr" "network.service_cidr" "network.node_addresses" "network.node_public_addresses" "network.load_balancer_addresses" "clients" -}}
{{- include "br-sandbox-chart.require" (dict "root" $root "path" $path) -}}
{{- end -}}
{{- if le (.Values.save.ttl_seconds | int64) 0 -}}
{{- fail "BR_SANDBOX_SAVE_AGE_INVALID{key=save.ttl_seconds}" -}}
{{- end -}}
{{- $execTimeout := .Values.exec_timeout_seconds -}}
{{- if not (or (kindIs "float64" $execTimeout) (kindIs "int64" $execTimeout) (kindIs "int" $execTimeout)) -}}
{{- fail "BR_SANDBOX_EXEC_TIMEOUT_INVALID{key=exec_timeout_seconds}" -}}
{{- end -}}
{{- if or (lt ($execTimeout | float64) 1.0) (ne ($execTimeout | float64) (floor ($execTimeout | float64))) -}}
{{- fail "BR_SANDBOX_EXEC_TIMEOUT_INVALID{key=exec_timeout_seconds}" -}}
{{- end -}}
{{- if .Values.operator.enabled -}}
{{- range $path := list "operator.image" "network.api_server_addresses" "network.store_cidrs" -}}
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
{{- $egressNames := dict -}}
{{- $ips := dict -}}
{{- $sandboxNames := dict -}}
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
{{- if and $root.Values.egress.enabled (eq $mode "allow") -}}
{{- $listed := get $modeValues "allow" | default dict -}}
{{- if not (or (get $listed "domains" | default list | len) (get $listed "cidrs" | default list | len)) -}}
{{- fail (printf "BR_SANDBOX_ALLOW_EMPTY{client=%s,mode=%s}" $client $mode) -}}
{{- end -}}
{{- end -}}
{{- if not $root.Values.egress.enabled -}}
{{- $allow := get $modeValues "allow" | default dict -}}
{{- if or (get $allow "domains" | default list | len) (get $allow "cidrs" | default list | len) -}}
{{- fail (printf "BR_SANDBOX_EGRESS_DISABLED{client=%s,mode=%s,key=allow}" $client $mode) -}}
{{- end -}}
{{- if get $modeValues "credential_set" | default dict | len -}}
{{- fail (printf "BR_SANDBOX_EGRESS_DISABLED{client=%s,mode=%s,key=credential_set}" $client $mode) -}}
{{- end -}}
{{- end -}}
{{- if and $root.Values.egress.enabled (ne $mode "allow") -}}
{{- $listed := get $modeValues "allow" | default dict -}}
{{- if or (get $listed "domains" | default list | len) (get $listed "cidrs" | default list | len) -}}
{{- fail (printf "BR_SANDBOX_ALLOW_MODE{client=%s,mode=%s}" $client $mode) -}}
{{- end -}}
{{- end -}}
{{- if and $root.Values.egress.enabled (eq $mode "none") (get $modeValues "credential_set" | default dict | len) -}}
{{- fail (printf "BR_SANDBOX_CREDENTIAL_SET_MODE{client=%s,mode=%s}" $client $mode) -}}
{{- end -}}
{{- include "br-sandbox-chart.validate.credentials" (dict "root" $root "client" $client "mode" $mode "values" $modeValues) -}}
{{- $name := printf "%s-%s-%s" $prefix $client $mode -}}
{{- if hasKey $seen $name -}}
{{- fail (printf "BR_SANDBOX_SHARED_NAMESPACE{client_a=%s,client_b=%s,namespace=%s}" (get $seen $name) $client $name) -}}
{{- end -}}
{{- $_ := set $seen $name $client -}}
{{- $_ := set $sandboxNames $name $client -}}
{{- if gt (len $name) 63 -}}
{{- fail (printf "BR_SANDBOX_NAME_TOO_LONG{client=%s,mode=%s,namespace=%s}" $client $mode $name) -}}
{{- end -}}
{{- if include "br-sandbox-chart.egress.proxied" (dict "mode" $mode "values" $modeValues) -}}
{{- $egressName := include "br-sandbox-chart.egress.namespace" $name -}}
{{- if gt (len $egressName) 63 -}}
{{- fail (printf "BR_SANDBOX_NAME_TOO_LONG{client=%s,mode=%s,namespace=%s}" $client $mode $egressName) -}}
{{- end -}}
{{- if hasKey $seen $egressName -}}
{{- fail (printf "BR_SANDBOX_SHARED_NAMESPACE{client_a=%s,client_b=%s,namespace=%s}" (get $seen $egressName) $client $egressName) -}}
{{- end -}}
{{- $_ := set $seen $egressName $client -}}
{{- $_ := set $egressNames $egressName $client -}}
{{- if $root.Values.egress.enabled -}}
{{- include "br-sandbox-chart.validate.proxy" (dict "root" $root "client" $client "mode" $mode "values" $modeValues "ips" $ips) -}}
{{- end -}}
{{- end -}}
{{- $pool := mergeOverwrite (deepCopy $root.Values.pool) (deepCopy ($modeValues.pool | default dict)) -}}
{{- if gt ($pool.floor | int64) ($pool.max_sandboxes | int64) -}}
{{- fail (printf "BR_SANDBOX_POOL_INVALID{client=%s,mode=%s,reason=floor_above_max_sandboxes}" $client $mode) -}}
{{- end -}}
{{- end -}}
{{- end -}}
{{- range $client, $clientValues := .Values.clients -}}
{{- range $mode, $modeValues := $clientValues.modes -}}
{{- if hasKey $egressNames $modeValues.runner.namespace -}}
{{- fail (printf "BR_SANDBOX_SHARED_NAMESPACE{client_a=%s,client_b=runner-of-%s,namespace=%s}" (get $egressNames $modeValues.runner.namespace) $client $modeValues.runner.namespace) -}}
{{- end -}}
{{- if and (eq $modeValues.runner.service_account "sandbox") (hasKey $sandboxNames $modeValues.runner.namespace) -}}
{{- fail (printf "BR_SANDBOX_RUNNER_IS_POD_IDENTITY{client=%s,mode=%s,namespace=%s}" $client $mode $modeValues.runner.namespace) -}}
{{- end -}}
{{- end -}}
{{- end -}}
{{- if gt (len $ips) 0 -}}
{{- include "br-sandbox-chart.validate.proxy_global" $root -}}
{{- end -}}
{{- if $needsResolvers -}}
{{- include "br-sandbox-chart.require" (dict "root" $root "path" "network.public_resolvers") -}}
{{- end -}}
{{- end -}}
