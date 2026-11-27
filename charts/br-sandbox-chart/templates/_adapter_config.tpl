{{- define "br-sandbox-chart.adapter.allowlist" -}}
{{- $allow := .ns.allow -}}
{{- if eq .ns.mode "allow" -}}
{{- toJson (concat ($allow.domains | default list) ($allow.cidrs | default list)) -}}
{{- else -}}
{{- toJson list -}}
{{- end -}}
{{- end -}}

{{- define "br-sandbox-chart.adapter.document" -}}
{{- $values := .root.Values -}}
{{- $ns := .ns -}}
{{- $document := dict
  "namespace" $ns.name
  "client_id" $ns.client
  "label_prefix" (include "br-sandbox-chart.label_prefix" .root)
  "pool_id" $ns.poolId
  "network_mode" $ns.mode
  "save_ttl_seconds" ($values.save.ttl_seconds | int64)
  "save_scope_paths" $values.save.paths
  "restore_timeout_seconds" ($values.restore_timeout_seconds | int64)
  "idle_ttl_seconds" ($values.idle_ttl | int64)
  "idle_keep_min_state_bytes" ($values.idle_keep_min_state_bytes | int64)
  "agent_container" (include "br-sandbox-chart.agent_container" .root)
  "tools_path" (printf "%s/bin/bsdtar" $values.tools.mount_path)
  "seed_root" (include "br-sandbox-chart.seed_root" .root)
  "store_endpoint" $values.store.endpoint
  "store_bucket" $values.store.bucket
  "store_region" $values.store.region
  "store_path_style" $values.store.path_style -}}
{{- if $values.tools.seed_container.enabled -}}
{{- $_ := set $document "seed_container" $values.tools.seed_container.name -}}
{{- $_ := set $document "seed_tools_path" (include "br-sandbox-chart.seed_tools_path" .root) -}}
{{- end -}}
{{- $allowlist := include "br-sandbox-chart.adapter.allowlist" . | fromJsonArray -}}
{{- if $allowlist -}}
{{- $_ := set $document "allowlist" $allowlist -}}
{{- end -}}
{{- toJson $document -}}
{{- end -}}
