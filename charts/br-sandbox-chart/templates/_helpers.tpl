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
{{- $name := printf "%s-%s-%s" $root.Values.installation.namespace_prefix $client $mode -}}
{{- $egress := "" -}}
{{- if include "br-sandbox-chart.egress.proxied" (dict "mode" $mode "values" $modeValues) -}}
{{- $egress = include "br-sandbox-chart.egress.namespace" $name -}}
{{- end -}}
{{- $item := dict "client" $client "mode" $mode "name" $name "egress" $egress "poolId" (printf "%s-%s" $client $mode) "pool" $pool "runner" $modeValues.runner "allow" ($modeValues.allow | default dict) "credential_set" ($modeValues.credential_set | default dict) "proxy" ($modeValues.proxy | default dict) -}}
{{- $out = append $out $item -}}
{{- end -}}
{{- end -}}
{{- toJson $out -}}
{{- end -}}

{{- define "br-sandbox-chart.egress.namespace" -}}
{{- printf "%s-egress" . -}}
{{- end -}}

{{- define "br-sandbox-chart.egress.proxied" -}}
{{- if or (eq .mode "allow") (and (eq .mode "full") (gt (len (get .values "credential_set" | default dict)) 0)) -}}
true
{{- end -}}
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

{{- define "br-sandbox-chart.qty.factors" -}}
{{- if eq .kind "cpu" -}}
{{- toJson (dict "m" 1.0 "" 1000.0) -}}
{{- else -}}
{{- toJson (dict "" 1.0 "K" 1000.0 "M" 1000000.0 "G" 1000000000.0 "T" 1000000000000.0 "Ki" 1024.0 "Mi" 1048576.0 "Gi" 1073741824.0 "Ti" 1099511627776.0) -}}
{{- end -}}
{{- end -}}

{{- define "br-sandbox-chart.qty.base" -}}
{{- $text := toString .q -}}
{{- $number := mustRegexFind "^[0-9]+(\\.[0-9]+)?" $text -}}
{{- $factors := include "br-sandbox-chart.qty.factors" . | fromJson -}}
{{- $suffix := trimPrefix $number $text -}}
{{- if not (hasKey $factors $suffix) -}}
{{- fail (printf "BR_SANDBOX_QUANTITY_INVALID{value=%s}" $text) -}}
{{- end -}}
{{- printf "%d" (round (mulf (float64 $number) (get $factors $suffix)) 0 | int64) -}}
{{- end -}}

{{- define "br-sandbox-chart.qty.total" -}}
{{- $text := toString .q -}}
{{- $suffix := trimPrefix (mustRegexFind "^[0-9]+(\\.[0-9]+)?" $text) $text -}}
{{- $factors := include "br-sandbox-chart.qty.factors" . | fromJson -}}
{{- $each := include "br-sandbox-chart.qty.base" . | int64 -}}
{{- $seed := include "br-sandbox-chart.qty.base" (dict "kind" .kind "q" .seed) | int64 -}}
{{- $total := add (mul $each (.n | int64)) (mul $seed (.seed_n | int64)) -}}
{{- $unit := get $factors $suffix | int64 -}}
{{- if eq (mod $total $unit) 0 -}}
{{- printf "%d%s" (div $total $unit) $suffix -}}
{{- else if eq .kind "cpu" -}}
{{- printf "%dm" $total -}}
{{- else if eq (mod $total 1073741824) 0 -}}
{{- printf "%dGi" (div $total 1073741824) -}}
{{- else if eq (mod $total 1048576) 0 -}}
{{- printf "%dMi" (div $total 1048576) -}}
{{- else if eq (mod $total 1024) 0 -}}
{{- printf "%dKi" (div $total 1024) -}}
{{- else -}}
{{- printf "%d" $total -}}
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
