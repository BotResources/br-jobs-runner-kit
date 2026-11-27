{{- define "br-sandbox-chart.fence.rules" -}}
- apiGroups: [""]
  apiVersions: ["v1"]
  operations: ["CREATE", "UPDATE", "DELETE"]
  resources: ["configmaps", "limitranges", "resourcequotas", "serviceaccounts"]
- apiGroups: ["networking.k8s.io"]
  apiVersions: ["v1"]
  operations: ["CREATE", "UPDATE", "DELETE"]
  resources: ["networkpolicies"]
- apiGroups: ["rbac.authorization.k8s.io"]
  apiVersions: ["v1"]
  operations: ["CREATE", "UPDATE", "DELETE"]
  resources: ["rolebindings", "roles"]
{{- end -}}
