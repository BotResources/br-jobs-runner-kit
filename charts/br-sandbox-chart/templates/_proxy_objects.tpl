{{- define "br-sandbox-chart.proxy.objects" }}
---
apiVersion: v1
kind: ConfigMap
metadata:
  name: br-sandbox-proxy-config
  namespace: {{ .namespace }}
  labels:
    {{- include "br-sandbox-chart.labels" .root | nindent 4 }}
data:
  proxy.yaml: |
    {{- include "br-sandbox-chart.proxy.config" . | nindent 4 }}
---
apiVersion: v1
kind: ConfigMap
metadata:
  name: br-sandbox-proxy-ca
  namespace: {{ .namespace }}
  labels:
    {{- include "br-sandbox-chart.labels" .root | nindent 4 }}
data:
  ca.crt: |
    {{- .ns.proxy.ca.certificate | trim | nindent 4 }}
---
apiVersion: v1
kind: ConfigMap
metadata:
  name: br-sandbox-ca
  namespace: {{ .ns.name }}
  labels:
    {{- include "br-sandbox-chart.labels" .root | nindent 4 }}
data:
  ca.crt: |
    {{- .ns.proxy.ca.certificate | trim | nindent 4 }}
---
apiVersion: apps/v1
kind: Deployment
metadata:
  name: br-sandbox-proxy
  namespace: {{ .namespace }}
  labels:
    {{- include "br-sandbox-chart.labels" .root | nindent 4 }}
    br.sandbox/role: proxy
spec:
  replicas: {{ .root.Values.egress.proxy.replicas | int64 }}
  selector:
    matchLabels:
      br.sandbox/role: proxy
  template:
    metadata:
      labels:
        br.sandbox/role: proxy
    spec:
      {{- include "br-sandbox-chart.proxy.pod" . | nindent 6 }}
---
apiVersion: v1
kind: Service
metadata:
  name: br-sandbox-proxy
  namespace: {{ .namespace }}
  labels:
    {{- include "br-sandbox-chart.labels" .root | nindent 4 }}
spec:
  type: ClusterIP
  clusterIP: {{ .ns.proxy.cluster_ip | quote }}
  selector:
    br.sandbox/role: proxy
  ports:
    - name: tunnel
      protocol: TCP
      port: {{ .root.Values.egress.proxy.port | int64 }}
      targetPort: tunnel
{{- end }}
