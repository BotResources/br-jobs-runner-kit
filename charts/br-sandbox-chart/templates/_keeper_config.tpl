{{- define "br-sandbox-chart.keeper.pool" -}}
{{- $ns := .ns -}}
{{- $values := .root.Values -}}
{{- toJson (dict
  "id" $ns.poolId
  "namespace" $ns.name
  "replicaset" (include "br-sandbox-chart.replicaset_name" $ns)
  "min_replicas" ($ns.pool.floor | int64)
  "max_replicas" ($ns.pool.max_sandboxes | int64)
  "refill_seconds" ($ns.pool.refill_seconds | int64)
  "margin" ($ns.pool.margin | int64)
  "idle_ttl_seconds" ($values.idle_ttl | int64)
  "idle_keep_min_state_bytes" ($values.idle_keep_min_state_bytes | int64)) -}}
{{- end -}}

{{- define "br-sandbox-chart.keeper.document" -}}
{{- $values := .Values -}}
{{- $prefix := include "br-sandbox-chart.label_prefix" . -}}
{{- $pools := list -}}
{{- range $ns := include "br-sandbox-chart.namespaces" . | fromJsonArray -}}
{{- $pools = append $pools (include "br-sandbox-chart.keeper.pool" (dict "root" $ "ns" $ns) | fromJson) -}}
{{- end -}}
{{- $document := dict
  "label_prefix" $prefix
  "managed_namespace_selector" (printf "%s/managed=true" $prefix)
  "store_bucket" $values.store.bucket
  "store_endpoint" $values.store.endpoint
  "store_region" $values.store.region
  "store_path_style" $values.store.path_style
  "listen_address" (printf "0.0.0.0:%d" ($values.keeper.port | int64))
  "save_ttl_seconds" ($values.save.ttl_seconds | int64)
  "tombstone_ttl_seconds" (mul ($values.save.tombstone_ttl_days | int64) 86400)
  "warm_max_age_seconds" ($values.warm_max_age_seconds | int64)
  "lease_duration_seconds" ($values.keeper.lease.duration_seconds | int64)
  "renew_deadline_seconds" ($values.keeper.lease.renew_deadline_seconds | int64)
  "retry_period_seconds" ($values.keeper.lease.retry_period_seconds | int64)
  "pools" $pools -}}
{{- range $key, $setting := $values.keeper.settings -}}
{{- if hasKey $document $key -}}
{{- fail (printf "BR_SANDBOX_KEEPER_SETTING_COMPUTED{key=%s}" $key) -}}
{{- end -}}
{{- $_ := set $document $key $setting -}}
{{- end -}}
{{- toJson $document -}}
{{- end -}}
