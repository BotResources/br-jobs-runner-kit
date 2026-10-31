{{- define "br-runner-toolkit-chart.env" -}}
{{- $runner := required "the runner values are required: declare br-runner-toolkit-chart with alias: runner in the thin chart" .Values.runner -}}
- name: RUNNER_VERSION
  value: {{ required "runner.runner_version is required" $runner.runner_version | quote }}
- name: MAX_CONCURRENT_RUNS
  value: {{ include "br-runner-toolkit-chart.number" (dict "name" "max_concurrent_runs" "value" $runner.max_concurrent_runs) | quote }}
- name: ACK_WAIT_MS
  value: {{ include "br-runner-toolkit-chart.number" (dict "name" "ack_wait" "value" $runner.ack_wait) | quote }}
- name: DRAIN_TIMEOUT_MS
  value: {{ include "br-runner-toolkit-chart.number" (dict "name" "drain_timeout" "value" $runner.drain_timeout) | quote }}
- name: CANCEL_GRACE_MS
  value: {{ include "br-runner-toolkit-chart.number" (dict "name" "cancel_grace" "value" $runner.cancel_grace) | quote }}
- name: CLAIM_WAIT_MS
  value: {{ include "br-runner-toolkit-chart.number" (dict "name" "claim_wait" "value" $runner.claim_wait) | quote }}
- name: OUTCOME_WAIT_MS
  value: {{ include "br-runner-toolkit-chart.number" (dict "name" "outcome_wait" "value" $runner.outcome_wait) | quote }}
- name: RETRY_WAIT_MS
  value: {{ include "br-runner-toolkit-chart.number" (dict "name" "retry_wait" "value" $runner.retry_wait) | quote }}
{{- end -}}

{{- define "br-runner-toolkit-chart.number" -}}
{{- if kindIs "invalid" .value -}}
{{- fail (printf "runner.%s is required" .name) -}}
{{- end -}}
{{- .value | int64 -}}
{{- end -}}
