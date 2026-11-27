{{- define "br-sandbox-chart.vap.namespace_rules" -}}
{{- $rules := list
  (dict "code" "BR_SANDBOX_MODE_UNKNOWN" "message" "the label br.sandbox/mode is none, allow or full" "expression" "request.operation != 'CREATE' || variables.mode in ['none', 'allow', 'full']")
  (dict "code" "BR_SANDBOX_MODE_NOT_GRANTED" "message" "a mode other than none needs a grant of the platform for the name of the namespace" "expression" "request.operation != 'CREATE' || variables.mode == 'none' || variables.writer || variables.mode in variables.granted")
  (dict "code" "BR_SANDBOX_LABEL_IMMUTABLE" "message" "the labels br.sandbox/managed and br.sandbox/mode cannot be added, changed or removed by a project" "expression" "request.operation != 'UPDATE' || variables.writer || (variables.managed == variables.oldManaged && variables.mode == variables.oldMode)")
  (dict "code" "BR_SANDBOX_SCOPE" "message" "the name of a managed namespace is a scope of the platform or starts with a scope and a dash" "expression" "request.operation != 'CREATE' || variables.scopes.exists(s, s != '' && (object.metadata.name == s || object.metadata.name.startsWith(s + '-')))")
-}}
{{- $rules | toJson -}}
{{- end -}}
