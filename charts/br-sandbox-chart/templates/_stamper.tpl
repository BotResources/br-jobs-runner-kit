{{- define "br-sandbox-chart.stamper.token" -}}
STAMPTOKEN
{{- end -}}

{{- define "br-sandbox-chart.stamper.variable" -}}
{{ "{{ request.object.metadata.name }}" }}
{{- end -}}

{{- define "br-sandbox-chart.stamper.objects" -}}
{{- $objects := list
  (dict "rule" "service-account" "template" "br-sandbox-chart.fence.service_account")
  (dict "rule" "quota" "template" "br-sandbox-chart.fence.quota")
  (dict "rule" "limit-range" "template" "br-sandbox-chart.fence.limit_range")
  (dict "rule" "default-deny" "template" "br-sandbox-chart.fence.default_deny")
  (dict "rule" "config" "template" "br-sandbox-chart.fence.config")
  (dict "rule" "pod-template" "template" "br-sandbox-chart.fence.pod_template")
  (dict "rule" "runner-binding" "template" "br-sandbox-chart.fence.runner_binding")
  (dict "rule" "keeper-binding" "template" "br-sandbox-chart.fence.keeper_binding") -}}
{{- if eq . "full" -}}
{{- $objects = append $objects (dict "rule" "egress-full" "template" "br-sandbox-chart.fence.egress_full") -}}
{{- end -}}
{{- toJson $objects -}}
{{- end -}}

{{- define "br-sandbox-chart.stamper.entry" -}}
{{- $token := include "br-sandbox-chart.stamper.token" . -}}
{{- $scope := include "br-sandbox-chart.scope.of" (dict "root" .root "name" .scope) | fromJson -}}
{{- toJson (dict "name" $token "mode" .mode "client" $scope.client_id "scope" $scope.name "egress" "" "poolId" $token "runner" $scope.runner "allow" (dict) "credential_set" (dict) "proxy" (dict)) -}}
{{- end -}}

{{- define "br-sandbox-chart.stamper.rule" -}}
{{- $object := .object -}}
{{- $metadata := omit $object.metadata "name" "namespace" -}}
{{- $data := set (omit $object "apiVersion" "kind") "metadata" $metadata -}}
{{- $match := dict "kinds" (list "Namespace") "names" (list .scope (printf "%s-*" .scope)) "selector" (dict "matchLabels" (dict "br.sandbox/managed" "true" "br.sandbox/mode" .mode)) -}}
{{- $generate := dict "synchronize" true "generateExisting" true "apiVersion" $object.apiVersion "kind" $object.kind "name" $object.metadata.name "namespace" (include "br-sandbox-chart.stamper.token" .root) "data" $data -}}
{{- toJson (dict "name" .name "match" (dict "any" (list (dict "resources" $match))) "generate" $generate) -}}
{{- end -}}

{{- define "br-sandbox-chart.stamper.rules" -}}
{{- $root := . -}}
{{- $rules := list -}}
{{- range $scope := keys .Values.scopes | sortAlpha -}}
{{- range $mode := list "none" "full" -}}
{{- $ns := include "br-sandbox-chart.stamper.entry" (dict "root" $root "scope" $scope "mode" $mode) | fromJson -}}
{{- range $item := include "br-sandbox-chart.stamper.objects" $mode | fromJsonArray -}}
{{- $name := printf "br-sandbox-%s-%s-%s" $scope $mode $item.rule -}}
{{- if gt (len $name) 63 -}}
{{- fail (printf "BR_SANDBOX_STAMPER_RULE_NAME_TOO_LONG{rule=%s}" $name) -}}
{{- end -}}
{{- $object := printf "%s\n" (include $item.template (dict "root" $root "ns" $ns)) | fromYaml -}}
{{- $rules = append $rules (include "br-sandbox-chart.stamper.rule" (dict "root" $root "scope" $scope "mode" $mode "name" $name "object" $object) | fromJson) -}}
{{- end -}}
{{- end -}}
{{- end -}}
{{- toJson $rules | replace (include "br-sandbox-chart.stamper.token" .) (include "br-sandbox-chart.stamper.variable" .) -}}
{{- end -}}

{{- define "br-sandbox-chart.stamper.policy" -}}
apiVersion: kyverno.io/v1
kind: ClusterPolicy
metadata:
  name: br-sandbox-stamper
  labels:
    {{- include "br-sandbox-chart.labels" . | nindent 4 }}
  annotations:
    policies.kyverno.io/title: Sandbox fence stamper
    policies.kyverno.io/category: Sandbox
    policies.kyverno.io/severity: medium
    policies.kyverno.io/subject: Namespace
spec:
  rules:
    {{- include "br-sandbox-chart.stamper.rules" . | fromJsonArray | toYaml | nindent 4 }}
{{- end -}}
