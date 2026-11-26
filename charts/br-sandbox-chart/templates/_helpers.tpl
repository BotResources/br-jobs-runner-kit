{{- define "br-sandbox-chart.system_namespace" -}}
{{- include "br-sandbox-chart.validate" . -}}
{{- default (printf "%s-system" .Values.installation.namespace_prefix) .Values.installation.system_namespace -}}
{{- end -}}

{{- define "br-sandbox-chart.labels" -}}
app.kubernetes.io/part-of: br-sandbox
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- end -}}

{{- define "br-sandbox-chart.namespaces" -}}
{{- include "br-sandbox-chart.validate" . -}}
{{- $root := . -}}
{{- $out := list -}}
{{- range $client, $clientValues := .Values.clients -}}
{{- range $mode, $modeValues := $clientValues.modes -}}
{{- $pool := mergeOverwrite (deepCopy $root.Values.pool) (deepCopy ($modeValues.pool | default dict)) -}}
{{- $item := dict "client" $client "mode" $mode "name" (printf "%s-%s-%s" $root.Values.installation.namespace_prefix $client $mode) "poolId" (printf "%s-%s" $client $mode) "pool" $pool "runner" $modeValues.runner "allow" ($modeValues.allow | default dict) -}}
{{- $out = append $out $item -}}
{{- end -}}
{{- end -}}
{{- toJson $out -}}
{{- end -}}

{{- define "br-sandbox-chart.image.repository" -}}
{{- $ref := . -}}
{{- if contains "@" $ref -}}
{{- $ref = index (splitList "@" $ref) 0 -}}
{{- end -}}
{{- $parts := splitList "/" $ref -}}
{{- $last := last $parts -}}
{{- if contains ":" $last -}}
{{- $last = index (splitList ":" $last) 0 -}}
{{- end -}}
{{- join "/" (append (initial $parts) $last) -}}
{{- end -}}

{{- define "br-sandbox-chart.image.mirrored" -}}
{{- if .root.Values.registry_mirror -}}
{{- printf "%s/%s" (trimSuffix "/" .root.Values.registry_mirror) .ref -}}
{{- else -}}
{{- .ref -}}
{{- end -}}
{{- end -}}

{{- define "br-sandbox-chart.image.project" -}}
{{- include "br-sandbox-chart.image.mirrored" (dict "root" . "ref" .Values.image) -}}
{{- end -}}

{{- define "br-sandbox-chart.image.tools" -}}
{{- include "br-sandbox-chart.image.mirrored" (dict "root" . "ref" (printf "%s@%s" .Values.tools.image.repository .Values.tools.image.digest)) -}}
{{- end -}}

{{- define "br-sandbox-chart.image.policy" -}}
{{- if .root.Values.image_pull_policy -}}
{{- .root.Values.image_pull_policy -}}
{{- else if contains "@" .ref -}}
IfNotPresent
{{- else -}}
Always
{{- end -}}
{{- end -}}

{{- define "br-sandbox-chart.image.allowed_repositories" -}}
{{- $repositories := list (include "br-sandbox-chart.image.repository" .Values.image) .Values.tools.image.repository -}}
{{- if .Values.network_check.enabled -}}
{{- $repositories = append $repositories (include "br-sandbox-chart.image.repository" .Values.network_check.image) -}}
{{- end -}}
{{- $out := list -}}
{{- range $repository := $repositories -}}
{{- $out = append $out (include "br-sandbox-chart.image.mirrored" (dict "root" $ "ref" $repository)) -}}
{{- end -}}
{{- toJson ($out | uniq) -}}
{{- end -}}

{{- define "br-sandbox-chart.pull_secrets" -}}
{{- $out := list -}}
{{- range .Values.pull_secrets -}}
{{- $out = append $out (dict "name" .) -}}
{{- end -}}
{{- toJson $out -}}
{{- end -}}

{{- define "br-sandbox-chart.qty.mul" -}}
{{- $parts := mustRegexFind "^[0-9]+(\\.[0-9]+)?" (toString .q) -}}
{{- $suffix := trimPrefix $parts (toString .q) -}}
{{- $n := mulf (float64 $parts) (float64 .n) -}}
{{- if eq (floor $n) $n -}}
{{- printf "%d%s" (int64 $n) $suffix -}}
{{- else -}}
{{- printf "%v%s" $n $suffix -}}
{{- end -}}
{{- end -}}

{{- define "br-sandbox-chart.resources" -}}
{{- $size := . -}}
{{- toJson (dict "requests" (dict "cpu" (toString $size.cpu.request) "memory" (toString $size.memory.request) "ephemeral-storage" (toString $size.disk.request)) "limits" (dict "cpu" (toString $size.cpu.limit) "memory" (toString $size.memory.limit) "ephemeral-storage" (toString $size.disk.limit))) -}}
{{- end -}}

{{- define "br-sandbox-chart.json_string" -}}
{{- . | toJson | replace "\\u0026" "&" | replace "\\u003c" "<" | replace "\\u003e" ">" -}}
{{- end -}}

{{- define "br-sandbox-chart.config.common" -}}
{{- $values := .Values -}}
{{- toYaml (dict
  "label_prefix" "br.sandbox"
  "tools" (dict "mount_path" $values.tools.mount_path "bsdtar_path" (printf "%s/bin/bsdtar" $values.tools.mount_path) "seed_container" (dict "enabled" $values.tools.seed_container.enabled "name" $values.tools.seed_container.name) "seed_root" "/skills" "seed_container_bsdtar_path" "/sbx/bin/bsdtar")
  "restore_timeout_seconds" $values.restore_timeout_seconds
  "idle_ttl" $values.idle_ttl
  "idle_keep_min_state_bytes" $values.idle_keep_min_state_bytes
  "warm_max_age_seconds" $values.warm_max_age_seconds
  "save" $values.save
  "store" $values.store
  "work_size_limit" (toString $values.work.size_limit)
  "home_size_limit" (toString $values.home.size_limit)) -}}
{{- end -}}

{{- define "br-sandbox-chart.config.pool" -}}
{{- $ns := .ns -}}
{{- toYaml (dict
  "client" $ns.client
  "mode" $ns.mode
  "namespace" $ns.name
  "pool_id" $ns.poolId
  "replicaset" (printf "warm-%s-%s" $ns.client $ns.mode)
  "floor" $ns.pool.floor
  "max_sandboxes" $ns.pool.max_sandboxes
  "refill_seconds" $ns.pool.refill_seconds
  "margin" $ns.pool.margin
  "size" $ns.pool.size) -}}
{{- end -}}

{{- define "br-sandbox-chart.config.system" -}}
{{- $config := include "br-sandbox-chart.config.common" . | fromYaml -}}
{{- $pools := list -}}
{{- range $ns := include "br-sandbox-chart.namespaces" . | fromJsonArray -}}
{{- $pools = append $pools (include "br-sandbox-chart.config.pool" (dict "ns" $ns) | fromYaml) -}}
{{- end -}}
{{- $_ := set $config "system_namespace" (include "br-sandbox-chart.system_namespace" .) -}}
{{- $_ := set $config "lease" .Values.keeper.lease -}}
{{- $_ := set $config "settings" .Values.keeper.settings -}}
{{- $_ := set $config "pools" $pools -}}
{{- toYaml $config -}}
{{- end -}}

{{- define "br-sandbox-chart.config.namespace" -}}
{{- $config := include "br-sandbox-chart.config.common" .root | fromYaml -}}
{{- $_ := set $config "pool" (include "br-sandbox-chart.config.pool" (dict "ns" .ns) | fromYaml) -}}
{{- toYaml $config -}}
{{- end -}}

{{- define "br-sandbox-chart.system.security" -}}
{{- toJson (dict "allowPrivilegeEscalation" false "readOnlyRootFilesystem" true "runAsNonRoot" true "runAsUser" 10001 "runAsGroup" 10001 "capabilities" (dict "drop" (list "ALL"))) -}}
{{- end -}}
