{{- define "br-sandbox-chart.require" -}}
{{- include "br-sandbox-chart.require.in" (dict "node" .root.Values "path" .path "label" .path) -}}
{{- end -}}

{{- define "br-sandbox-chart.require.in" -}}
{{- $node := .node -}}
{{- $ok := true -}}
{{- range $segment := splitList "." .path -}}
{{- if and $ok (kindIs "map" $node) (hasKey $node $segment) -}}
{{- $node = index $node $segment -}}
{{- else -}}
{{- $ok = false -}}
{{- end -}}
{{- end -}}
{{- if or (not $ok) (kindIs "invalid" $node) (and (kindIs "string" $node) (eq $node "")) -}}
{{- fail (printf "BR_SANDBOX_MISSING_VALUE{key=%s}" .label) -}}
{{- end -}}
{{- end -}}

{{- define "br-sandbox-chart.validate.scopes" -}}
{{- $root := . -}}
{{- range $scope, $table := .Values.scopes -}}
{{- range $path := list "service_account" "namespace" -}}
{{- include "br-sandbox-chart.require.in" (dict "node" $table "path" (printf "runner.%s" $path) "label" (printf "scopes.%s.runner.%s" $scope $path)) -}}
{{- end -}}
{{- range $other, $_ := $root.Values.scopes -}}
{{- if and (ne $scope $other) (hasPrefix (printf "%s-" $scope) $other) -}}
{{- fail (printf "BR_SANDBOX_SCOPES_OVERLAP{scope=%s,other=%s}" $scope $other) -}}
{{- end -}}
{{- end -}}
{{- end -}}
{{- end -}}

{{- define "br-sandbox-chart.validate" -}}
{{- $root := . -}}
{{- if hasKey .Values "keeper" -}}
{{- fail "BR_SANDBOX_KEEPER_VALUES_REMOVED{key=keeper}" -}}
{{- end -}}
{{- if hasKey .Values "clients" -}}
{{- fail "BR_SANDBOX_CLIENTS_REMOVED{key=clients}" -}}
{{- end -}}
{{- range $path := list "installation.namespace_prefix" "tools.image.repository" "tools.image.digest" "save.ttl_seconds" "store.endpoint" "store.bucket" "store.region" "store.path_style" "store.credentials_secret" "network.pod_cidr" "network.service_cidr" "network.node_addresses" "network.node_public_addresses" "network.load_balancer_addresses" -}}
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
{{- include "br-sandbox-chart.validate.scopes" . -}}
{{- $prefix := .Values.installation.namespace_prefix -}}
{{- $system := default (printf "%s-system" $prefix) .Values.installation.system_namespace -}}
{{- $seen := dict $system "system" -}}
{{- $egressNames := dict -}}
{{- $ips := dict -}}
{{- $sandboxNames := dict -}}
{{- $needsResolvers := false -}}
{{- range $entry := .Values.namespaces -}}
{{- $name := $entry.name -}}
{{- $mode := $entry.mode -}}
{{- if not (has $mode (list "none" "allow" "full")) -}}
{{- fail (printf "BR_SANDBOX_MODE_UNKNOWN{namespace=%s,mode=%s}" $name $mode) -}}
{{- end -}}
{{- if ne $mode "none" -}}
{{- $needsResolvers = true -}}
{{- end -}}
{{- $scope := include "br-sandbox-chart.scope.of" (dict "root" $root "name" $name) | fromJson -}}
{{- if and $root.Values.egress.enabled (eq $mode "allow") -}}
{{- $listed := get $entry "allow" | default dict -}}
{{- if not (or (get $listed "domains" | default list | len) (get $listed "cidrs" | default list | len)) -}}
{{- fail (printf "BR_SANDBOX_ALLOW_EMPTY{namespace=%s,mode=%s}" $name $mode) -}}
{{- end -}}
{{- end -}}
{{- if not $root.Values.egress.enabled -}}
{{- $allow := get $entry "allow" | default dict -}}
{{- if or (get $allow "domains" | default list | len) (get $allow "cidrs" | default list | len) -}}
{{- fail (printf "BR_SANDBOX_EGRESS_DISABLED{namespace=%s,mode=%s,key=allow}" $name $mode) -}}
{{- end -}}
{{- if get $entry "credential_set" | default dict | len -}}
{{- fail (printf "BR_SANDBOX_EGRESS_DISABLED{namespace=%s,mode=%s,key=credential_set}" $name $mode) -}}
{{- end -}}
{{- end -}}
{{- if and $root.Values.egress.enabled (ne $mode "allow") -}}
{{- $listed := get $entry "allow" | default dict -}}
{{- if or (get $listed "domains" | default list | len) (get $listed "cidrs" | default list | len) -}}
{{- fail (printf "BR_SANDBOX_ALLOW_MODE{namespace=%s,mode=%s}" $name $mode) -}}
{{- end -}}
{{- end -}}
{{- if and $root.Values.egress.enabled (eq $mode "none") (get $entry "credential_set" | default dict | len) -}}
{{- fail (printf "BR_SANDBOX_CREDENTIAL_SET_MODE{namespace=%s,mode=%s}" $name $mode) -}}
{{- end -}}
{{- include "br-sandbox-chart.validate.credentials" (dict "root" $root "namespace" $name "mode" $mode "values" $entry) -}}
{{- if hasKey $seen $name -}}
{{- fail (printf "BR_SANDBOX_SHARED_NAMESPACE{first=%s,second=sandbox,namespace=%s}" (get $seen $name) $name) -}}
{{- end -}}
{{- $_ := set $seen $name "sandbox" -}}
{{- $_ := set $sandboxNames $name $scope.name -}}
{{- if include "br-sandbox-chart.egress.proxied" (dict "mode" $mode "values" $entry) -}}
{{- $egressName := include "br-sandbox-chart.egress.namespace" $name -}}
{{- if gt (len $egressName) 63 -}}
{{- fail (printf "BR_SANDBOX_NAME_TOO_LONG{namespace=%s,egress=%s}" $name $egressName) -}}
{{- end -}}
{{- if hasKey $seen $egressName -}}
{{- fail (printf "BR_SANDBOX_SHARED_NAMESPACE{first=%s,second=egress,namespace=%s}" (get $seen $egressName) $egressName) -}}
{{- end -}}
{{- $_ := set $seen $egressName "egress" -}}
{{- $_ := set $egressNames $egressName $name -}}
{{- if $root.Values.egress.enabled -}}
{{- include "br-sandbox-chart.validate.proxy" (dict "root" $root "namespace" $name "mode" $mode "values" $entry "ips" $ips) -}}
{{- end -}}
{{- end -}}
{{- end -}}
{{- range $scope, $table := .Values.scopes -}}
{{- if hasKey $egressNames $table.runner.namespace -}}
{{- fail (printf "BR_SANDBOX_SHARED_NAMESPACE{first=egress,second=runner-of-%s,namespace=%s}" $scope $table.runner.namespace) -}}
{{- end -}}
{{- if and (eq $table.runner.service_account "sandbox") (hasKey $sandboxNames $table.runner.namespace) -}}
{{- fail (printf "BR_SANDBOX_RUNNER_IS_POD_IDENTITY{scope=%s,namespace=%s}" $scope $table.runner.namespace) -}}
{{- end -}}
{{- end -}}
{{- if gt (len $ips) 0 -}}
{{- include "br-sandbox-chart.validate.proxy_global" $root -}}
{{- end -}}
{{- if $needsResolvers -}}
{{- include "br-sandbox-chart.require" (dict "root" $root "path" "network.public_resolvers") -}}
{{- end -}}
{{- end -}}
