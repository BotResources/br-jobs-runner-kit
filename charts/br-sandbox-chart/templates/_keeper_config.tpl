{{- define "br-sandbox-chart.keeper.document" -}}
{{- $values := .Values -}}
{{- $prefix := include "br-sandbox-chart.label_prefix" . -}}
{{- $document := dict
  "label_prefix" $prefix
  "managed_namespace_selector" (printf "%s/managed=true" $prefix)
  "store_bucket" $values.store.bucket
  "store_endpoint" $values.store.endpoint
  "store_region" $values.store.region
  "store_path_style" $values.store.path_style
  "listen_address" (printf "0.0.0.0:%d" ($values.operator.port | int64))
  "save_ttl_seconds" ($values.save.ttl_seconds | int64)
  "tombstone_ttl_seconds" (mul ($values.save.tombstone_ttl_days | int64) 86400)
  "warm_max_age_seconds" ($values.warm_max_age_seconds | int64)
  "idle_ttl_seconds" ($values.idle_ttl | int64)
  "idle_keep_min_state_bytes" ($values.idle_keep_min_state_bytes | int64)
  "agent_container" (include "br-sandbox-chart.agent_container" .)
  "lease_duration_seconds" ($values.operator.lease.duration_seconds | int64)
  "renew_deadline_seconds" ($values.operator.lease.renew_deadline_seconds | int64)
  "retry_period_seconds" ($values.operator.lease.retry_period_seconds | int64) -}}
{{- range $key, $setting := $values.operator.settings -}}
{{- if eq $key "pools" -}}
{{- fail "BR_SANDBOX_OPERATOR_SETTING_REMOVED{key=pools}" -}}
{{- end -}}
{{- if hasKey $document $key -}}
{{- fail (printf "BR_SANDBOX_KEEPER_SETTING_COMPUTED{key=%s}" $key) -}}
{{- end -}}
{{- $_ := set $document $key $setting -}}
{{- end -}}
{{- toJson $document -}}
{{- end -}}
