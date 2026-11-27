{{- define "br-sandbox-chart.fence.service_account" -}}
apiVersion: v1
kind: ServiceAccount
metadata:
  name: sandbox
  namespace: {{ .ns.name }}
  labels:
    {{- include "br-sandbox-chart.labels" .root | nindent 4 }}
automountServiceAccountToken: false
{{- end -}}

{{- define "br-sandbox-chart.fence.quota" -}}
{{- $root := .root -}}
{{- $size := $root.Values.limits.size -}}
{{- $pods := add ($root.Values.limits.pods | int64) ($root.Values.quota.extra_pods | int64) -}}
{{- $seed := $root.Values.tools.seed_container -}}
{{- $seedPods := 0 -}}
{{- if $seed.enabled -}}
{{- $seedPods = $root.Values.limits.pods | int64 -}}
{{- end -}}
apiVersion: v1
kind: ResourceQuota
metadata:
  name: br-sandbox-quota
  namespace: {{ .ns.name }}
  labels:
    {{- include "br-sandbox-chart.labels" $root | nindent 4 }}
spec:
  hard:
    pods: {{ $pods | quote }}
    requests.cpu: {{ include "br-sandbox-chart.qty.total" (dict "kind" "cpu" "q" $size.cpu.request "n" $pods "seed" (dig "resources" "requests" "cpu" "0" $seed) "seed_n" $seedPods) | quote }}
    requests.memory: {{ include "br-sandbox-chart.qty.total" (dict "kind" "bytes" "q" $size.memory.request "n" $pods "seed" (dig "resources" "requests" "memory" "0" $seed) "seed_n" $seedPods) | quote }}
    requests.ephemeral-storage: {{ include "br-sandbox-chart.qty.total" (dict "kind" "bytes" "q" $size.disk.request "n" $pods "seed" (dig "resources" "requests" "ephemeral-storage" "0" $seed) "seed_n" $seedPods) | quote }}
    limits.cpu: {{ include "br-sandbox-chart.qty.total" (dict "kind" "cpu" "q" $size.cpu.limit "n" $pods "seed" (dig "resources" "limits" "cpu" "0" $seed) "seed_n" $seedPods) | quote }}
    limits.memory: {{ include "br-sandbox-chart.qty.total" (dict "kind" "bytes" "q" $size.memory.limit "n" $pods "seed" (dig "resources" "limits" "memory" "0" $seed) "seed_n" $seedPods) | quote }}
    limits.ephemeral-storage: {{ include "br-sandbox-chart.qty.total" (dict "kind" "bytes" "q" $size.disk.limit "n" $pods "seed" (dig "resources" "limits" "ephemeral-storage" "0" $seed) "seed_n" $seedPods) | quote }}
{{- end -}}

{{- define "br-sandbox-chart.fence.limit_range" -}}
{{- $size := .root.Values.limits.size -}}
apiVersion: v1
kind: LimitRange
metadata:
  name: br-sandbox-limits
  namespace: {{ .ns.name }}
  labels:
    {{- include "br-sandbox-chart.labels" .root | nindent 4 }}
spec:
  limits:
    - type: Container
      defaultRequest:
        cpu: {{ $size.cpu.request | toString | quote }}
        memory: {{ $size.memory.request | toString | quote }}
        ephemeral-storage: {{ $size.disk.request | toString | quote }}
      default:
        cpu: {{ $size.cpu.limit | toString | quote }}
        memory: {{ $size.memory.limit | toString | quote }}
        ephemeral-storage: {{ $size.disk.limit | toString | quote }}
      max:
        cpu: {{ $size.cpu.limit | toString | quote }}
        memory: {{ $size.memory.limit | toString | quote }}
        ephemeral-storage: {{ $size.disk.limit | toString | quote }}
{{- end -}}

{{- define "br-sandbox-chart.fence.default_deny" -}}
apiVersion: networking.k8s.io/v1
kind: NetworkPolicy
metadata:
  name: br-sandbox-default-deny
  namespace: {{ .ns.name }}
  labels:
    {{- include "br-sandbox-chart.labels" .root | nindent 4 }}
spec:
  podSelector: {}
  policyTypes:
    - Ingress
    - Egress
{{- end -}}

{{- define "br-sandbox-chart.fence.egress_proxy" -}}
apiVersion: networking.k8s.io/v1
kind: NetworkPolicy
metadata:
  name: br-sandbox-egress-{{ .ns.mode }}
  namespace: {{ .ns.name }}
  labels:
    {{- include "br-sandbox-chart.labels" .root | nindent 4 }}
spec:
  podSelector:
    matchLabels:
      br.sandbox/role: sandbox
  policyTypes:
    - Egress
  egress:
    - to:
        - namespaceSelector:
            matchLabels:
              kubernetes.io/metadata.name: {{ .ns.egress }}
          podSelector:
            matchLabels:
              br.sandbox/role: proxy
      ports:
        - protocol: TCP
          port: {{ .root.Values.egress.proxy.port | int64 }}
{{- end -}}

{{- define "br-sandbox-chart.fence.egress_full" -}}
apiVersion: networking.k8s.io/v1
kind: NetworkPolicy
metadata:
  name: br-sandbox-egress-full
  namespace: {{ .ns.name }}
  labels:
    {{- include "br-sandbox-chart.labels" .root | nindent 4 }}
spec:
  podSelector:
    matchLabels:
      br.sandbox/role: sandbox
  policyTypes:
    - Egress
  egress:
    - to:
        - ipBlock:
            cidr: 0.0.0.0/0
            except:
              {{- include "br-sandbox-chart.cidr.except" (dict "root" .root "family" "4") | fromJsonArray | toYaml | nindent 14 }}
    {{- if eq .root.Values.network.ipv6 "dual_stack" }}
    - to:
        - ipBlock:
            cidr: ::/0
            except:
              {{- include "br-sandbox-chart.cidr.except" (dict "root" .root "family" "6") | fromJsonArray | toYaml | nindent 14 }}
    {{- end }}
{{- end -}}

{{- define "br-sandbox-chart.fence.config" -}}
apiVersion: v1
kind: ConfigMap
metadata:
  name: br-sandbox-config
  namespace: {{ .ns.name }}
  labels:
    {{- include "br-sandbox-chart.labels" .root | nindent 4 }}
data:
  config.json: |
    {{- include "br-sandbox-chart.adapter.document" . | nindent 4 }}
{{- end -}}

{{- define "br-sandbox-chart.fence.pod_template" -}}
apiVersion: v1
kind: ConfigMap
metadata:
  name: br-sandbox-pod-template
  namespace: {{ .ns.name }}
  labels:
    {{- include "br-sandbox-chart.labels" .root | nindent 4 }}
data:
  pod-template.json: |
    {{- include "br-sandbox-chart.pod.template" . | nindent 4 }}
{{- end -}}

{{- define "br-sandbox-chart.fence.runner_binding" -}}
apiVersion: rbac.authorization.k8s.io/v1
kind: RoleBinding
metadata:
  name: br-sandbox-runner
  namespace: {{ .ns.name }}
  labels:
    {{- include "br-sandbox-chart.labels" .root | nindent 4 }}
roleRef:
  apiGroup: rbac.authorization.k8s.io
  kind: ClusterRole
  name: br-sandbox-runner
subjects:
  - kind: ServiceAccount
    name: {{ .ns.runner.service_account }}
    namespace: {{ .ns.runner.namespace }}
{{- end -}}

{{- define "br-sandbox-chart.fence.keeper_binding" -}}
apiVersion: rbac.authorization.k8s.io/v1
kind: RoleBinding
metadata:
  name: br-sandbox-keeper
  namespace: {{ .ns.name }}
  labels:
    {{- include "br-sandbox-chart.labels" .root | nindent 4 }}
roleRef:
  apiGroup: rbac.authorization.k8s.io
  kind: ClusterRole
  name: br-sandbox-controller
subjects:
  - kind: ServiceAccount
    name: br-sandbox-keeper
    namespace: {{ include "br-sandbox-chart.system_namespace" .root }}
{{- end -}}
