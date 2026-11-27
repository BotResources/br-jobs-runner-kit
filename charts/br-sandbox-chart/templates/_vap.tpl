{{- define "br-sandbox-chart.vap.quantity" -}}
{{- $out := list -}}
{{- range $key, $bounds := dict "cpu" (dict "requests" .size.cpu.request "limits" .size.cpu.limit) "memory" (dict "requests" .size.memory.request "limits" .size.memory.limit) "ephemeral-storage" (dict "requests" .size.disk.request "limits" .size.disk.limit) -}}
{{- range $kind := list "requests" "limits" -}}
{{- $out = append $out (printf "'%s' in c.resources.%s && !quantity(c.resources.%s['%s']).isGreaterThan(quantity('%s'))" $key $kind $kind $key (toString (get $bounds $kind))) -}}
{{- end -}}
{{- end -}}
{{- join " && " $out -}}
{{- end -}}

{{- define "br-sandbox-chart.vap.non_root" -}}
{{- $values := .Values -}}
{{- $nonRoot := "(has(c.securityContext) && has(c.securityContext.runAsNonRoot) && c.securityContext.runAsNonRoot == true && has(c.securityContext.runAsUser) && c.securityContext.runAsUser > 0)" -}}
{{- $regular := $nonRoot -}}
{{- if $values.tools.seed_container.enabled -}}
{{- $regular = printf "%s || (c.name == %s && c.image == %s && has(c.securityContext) && has(c.securityContext.runAsUser) && c.securityContext.runAsUser == 0 && has(c.securityContext.readOnlyRootFilesystem) && c.securityContext.readOnlyRootFilesystem == true && !has(c.ports))" $nonRoot ($values.tools.seed_container.name | toJson) (include "br-sandbox-chart.image.tools" . | toJson) -}}
{{- end -}}
{{- $lists := printf "object.spec.containers.all(c, %s) && (!has(object.spec.initContainers) || object.spec.initContainers.all(c, %s)) && (!has(object.spec.ephemeralContainers) || object.spec.ephemeralContainers.all(c, %s))" $regular $nonRoot $nonRoot -}}
{{- if $values.network_check.enabled -}}
{{- $user := printf "system:serviceaccount:%s:br-sandbox-netcheck" (include "br-sandbox-chart.system_namespace" .) -}}
{{- $lists = printf "request.userInfo.username == %s || (%s)" ($user | toJson) $lists -}}
{{- end -}}
{{- printf "(!(has(object.spec.shareProcessNamespace) && object.spec.shareProcessNamespace) && (%s))" $lists -}}
{{- end -}}

{{- define "br-sandbox-chart.vap.rules" -}}
{{- $values := .root.Values -}}
{{- $pool := .ns.pool -}}
{{- $capabilities := "['CHOWN', 'DAC_OVERRIDE', 'FOWNER', 'FSETID', 'SETUID', 'SETGID', 'KILL']" -}}
{{- $prefixes := include "br-sandbox-chart.image.allowed_repositories" .root | fromJsonArray -}}
{{- $quoted := list -}}
{{- range $prefix := $prefixes -}}
{{- $quoted = append $quoted (printf "'%s'" $prefix) -}}
{{- end -}}
{{- $prefixList := printf "[%s]" (join ", " $quoted) -}}
{{- $sandbox := "variables.role != 'sandbox' || " -}}
{{- $rules := list
  (dict "code" "BR_SANDBOX_ROLE" "message" "a pod of a sandbox namespace carries br.sandbox/role=sandbox; any other role, or none, is refused" "expression" "variables.role == 'sandbox'")
  (dict "code" "BR_SANDBOX_ROLE_IMMUTABLE" "message" "the label br.sandbox/role cannot change" "expression" "request.operation == 'CREATE' || variables.role == variables.oldRole")
  (dict "code" "BR_SANDBOX_USERNS_REQUIRED" "message" "spec.hostUsers must be false" "expression" (print $sandbox "(has(object.spec.hostUsers) && object.spec.hostUsers == false)"))
  (dict "code" "BR_SANDBOX_HOST_NAMESPACE" "message" "hostNetwork, hostPID, hostIPC and hostPort are refused" "expression" (print $sandbox "(!(has(object.spec.hostNetwork) && object.spec.hostNetwork) && !(has(object.spec.hostPID) && object.spec.hostPID) && !(has(object.spec.hostIPC) && object.spec.hostIPC) && variables.containers.all(c, !has(c.ports) || c.ports.all(p, !has(p.hostPort) || p.hostPort == 0)))"))
  (dict "code" "BR_SANDBOX_VOLUME_FORBIDDEN" "message" "only emptyDir and configMap volumes, no secret volume, no secretKeyRef, no secretRef" "expression" (print $sandbox "((!has(object.spec.volumes) || object.spec.volumes.all(v, has(v.emptyDir) || has(v.configMap))) && variables.containers.all(c, (!has(c.env) || c.env.all(e, !has(e.valueFrom) || !has(e.valueFrom.secretKeyRef))) && (!has(c.envFrom) || c.envFrom.all(f, !has(f.secretRef)))))"))
  (dict "code" "BR_SANDBOX_CAPABILITY" "message" "every container drops ALL and adds only the seven capabilities" "expression" (print $sandbox "variables.containers.all(c, has(c.securityContext) && has(c.securityContext.capabilities) && has(c.securityContext.capabilities.drop) && 'ALL' in c.securityContext.capabilities.drop && (!has(c.securityContext.capabilities.add) || c.securityContext.capabilities.add.all(a, a in " $capabilities ")))"))
  (dict "code" "BR_SANDBOX_PRIVILEGE" "message" "no privileged container, allowPrivilegeEscalation false, seccomp RuntimeDefault on the pod and on every container that sets one" "expression" (print $sandbox "(variables.containers.all(c, has(c.securityContext) && has(c.securityContext.allowPrivilegeEscalation) && c.securityContext.allowPrivilegeEscalation == false && !(has(c.securityContext.privileged) && c.securityContext.privileged) && (!has(c.securityContext.seccompProfile) || c.securityContext.seccompProfile.type == 'RuntimeDefault')) && has(object.spec.securityContext) && has(object.spec.securityContext.seccompProfile) && object.spec.securityContext.seccompProfile.type == 'RuntimeDefault')"))
  (dict "code" "BR_SANDBOX_RESOURCES" "message" "every container sets cpu, memory and ephemeral-storage requests and limits, not above the size of the pool" "expression" (print $sandbox "variables.containers.all(c, has(c.resources) && has(c.resources.requests) && has(c.resources.limits) && " (include "br-sandbox-chart.vap.quantity" (dict "size" $pool.size)) ")"))
  (dict "code" "BR_SANDBOX_IMAGE" "message" "every image comes from an allowed repository, with a tag or a digest" "expression" (print $sandbox "variables.containers.all(c, " $prefixList ".exists(p, c.image.startsWith(p + ':') || c.image.startsWith(p + '@')))"))
  (dict "code" "BR_SANDBOX_TOKEN_OR_RUNTIME" "message" "no service account token; runtimeClassName equals the chart value" "expression" (print $sandbox "(has(object.spec.automountServiceAccountToken) && object.spec.automountServiceAccountToken == false && (has(object.spec.runtimeClassName) ? object.spec.runtimeClassName : '') == " ($values.runtimeClassName | toJson) ")"))
  (dict "code" "BR_SANDBOX_IMMUTABLE" "message" "on update the isolation fields do not change" "expression" (print $sandbox "(request.operation == 'CREATE' || (object.spec.hostUsers == oldObject.spec.hostUsers && object.spec.containers.map(c, c.securityContext) == oldObject.spec.containers.map(c, c.securityContext) && object.spec.containers.map(c, c.resources) == oldObject.spec.containers.map(c, c.resources) && (has(object.spec.volumes) ? object.spec.volumes : []) == (has(oldObject.spec.volumes) ? oldObject.spec.volumes : [])))"))
  (dict "code" "BR_SANDBOX_RUN_AS_NON_ROOT" "message" "every container sets runAsNonRoot true and a runAsUser above 0, except the seed container of the chart and the pods of the network check; no shared process namespace" "expression" (print $sandbox (include "br-sandbox-chart.vap.non_root" .root)))
  (dict "code" "BR_SANDBOX_LABELS" "message" "a patch changes only br.sandbox/ labels and annotations; role and pool-id never change; a claimed pod stays claimed" "expression" (print $sandbox "(request.operation == 'CREATE' || (['br.sandbox/role', 'br.sandbox/pool-id'].all(k, (k in object.metadata.labels) == (k in oldObject.metadata.labels) && (!(k in object.metadata.labels) || object.metadata.labels[k] == oldObject.metadata.labels[k])) && object.metadata.labels.all(k, k.startsWith('br.sandbox/') || (k in oldObject.metadata.labels && oldObject.metadata.labels[k] == object.metadata.labels[k])) && oldObject.metadata.labels.all(k, k.startsWith('br.sandbox/') || k in object.metadata.labels) && !('br.sandbox/pool' in oldObject.metadata.labels && oldObject.metadata.labels['br.sandbox/pool'] == 'claimed' && !('br.sandbox/pool' in object.metadata.labels && object.metadata.labels['br.sandbox/pool'] == 'claimed')) && (!has(object.metadata.annotations) || object.metadata.annotations.all(k, k.startsWith('br.sandbox/') || (has(oldObject.metadata.annotations) && k in oldObject.metadata.annotations && oldObject.metadata.annotations[k] == object.metadata.annotations[k]))) && (!has(oldObject.metadata.annotations) || oldObject.metadata.annotations.all(k, k.startsWith('br.sandbox/') || (has(object.metadata.annotations) && k in object.metadata.annotations)))))"))
-}}
{{- toJson $rules -}}
{{- end -}}
