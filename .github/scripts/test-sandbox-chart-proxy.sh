#!/usr/bin/env bash

PROXY_ALLOW_NS='sbx-alpha-allow-egress'
PROXY_FULL_NS='sbx-alpha-full-egress'
PROXY_DIGEST='sha256:973b7adf44a12ded3b58cf67af35a80a8304d82bef0fb943e6467fe4f4020573'
OTHER_DIGEST='sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd'

proxy_pod() {
    printf '.[] | select(.kind == "Deployment" and .metadata.name == "br-sandbox-proxy" and .metadata.namespace == "%s") | .spec.template.spec' "$1"
}

render_proxy_fixtures() {
    render egressmirror -f "$EXAMPLE" -f "$EGRESS_VALUES" -f "$MIRROR_VALUES"
    render_yaml egressmirror_yaml -f "$EXAMPLE" -f "$EGRESS_VALUES" -f "$MIRROR_VALUES"
    render egresssized -f "$EXAMPLE" -f "$EGRESS_VALUES" --set egress.proxy.replicas=2 --set egress.proxy.resources.requests.cpu=250m --set egress.proxy.port=9999
    render egressother -f "$EXAMPLE" -f "$EGRESS_VALUES" --set egress.proxy.image.repository=registry.example.test/other/proxy --set egress.proxy.image.digest="$OTHER_DIGEST"
    render nocred -f "$(valsx nocred 'del((.namespaces[] | select(.name == "sbx-alpha-full")).credential_set) | del((.namespaces[] | select(.name == "sbx-alpha-full")).proxy)')"
    render_yaml egress_yaml -f "$EXAMPLE" -f "$EGRESS_VALUES"
}

check_the_proxy_objects_exist_in_each_egress_namespace_only() {
    section "proxy objects: Deployment, Service and two ConfigMaps in each egress namespace, nowhere else"
    local namespace
    for namespace in "$PROXY_ALLOW_NS" "$PROXY_FULL_NS"; do
        t "${namespace}: the Deployment carries the proxy role on the object, the selector and the pod template" jqt cred ".[] | select(.kind == \"Deployment\" and .metadata.namespace == \"${namespace}\") | .metadata.labels[\"br.sandbox/role\"] == \"proxy\" and .spec.selector.matchLabels == {\"br.sandbox/role\": \"proxy\"} and .spec.template.metadata.labels == {\"br.sandbox/role\": \"proxy\"}"
        t "${namespace}: the Service selects the proxy pods and exposes the tunnel port" jqt cred ".[] | select(.kind == \"Service\" and .metadata.namespace == \"${namespace}\") | .spec.selector == {\"br.sandbox/role\": \"proxy\"} and .spec.ports[0].port == 8888 and .spec.ports[0].targetPort == \"tunnel\" and .spec.type == \"ClusterIP\""
        t "${namespace}: the config and the CA certificate are two ConfigMaps" test "$(jqv cred "[.[] | select(.kind == \"ConfigMap\" and .metadata.namespace == \"${namespace}\") | .metadata.name] | sort | join(\",\")")" = "br-sandbox-proxy-ca,br-sandbox-proxy-config"
    done
    t "each Service has the fixed clusterIP of the proxy.cluster_ip of its mode" test "$(jqv cred '[.[] | select(.kind == "Service") | .metadata.namespace + "=" + .spec.clusterIP] | sort | join(",")')" = "sbx-alpha-allow-egress=10.43.0.50,sbx-alpha-full-egress=10.43.0.51"
    t "a Deployment and a Service exist in egress namespaces only" jqt cred "${EGRESS_NAMESPACES} as \$e | [.[] | select(.kind == \"Deployment\" or .kind == \"Service\") | .metadata.namespace as \$n | \$e | index(\$n) != null] | length == 4 and all"
    t "the example without egress renders no proxy object and no CA ConfigMap" jqt ex '[.[] | select((.metadata.name // "") | test("^br-sandbox-(proxy|ca)"))] | length == 0'
    t "mode full without a credential set has no proxy object" jqt nocred '[.[] | select(.kind == "Deployment" and .metadata.name == "br-sandbox-proxy") | .metadata.namespace] == ["sbx-alpha-allow-egress"]'
    t "the tunnel port is a value in the Deployment and the probes" jqt egresssized "$(proxy_pod "$PROXY_ALLOW_NS") | .containers[0] | .ports[0].containerPort == 9999 and .readinessProbe.tcpSocket.port == 9999 and .livenessProbe.tcpSocket.port == 9999"
    t "the replicas and the requests are values" jqt egresssized ".[] | select(.kind == \"Deployment\" and .metadata.namespace == \"${PROXY_ALLOW_NS}\") | .spec.replicas == 2 and .spec.template.spec.containers[0].resources.requests.cpu == \"250m\""
    t "the rollout strategy is a rolling update with a surge of one pod and no unavailable pod" jqt cred ".[] | select(.kind == \"Deployment\" and .metadata.name == \"br-sandbox-proxy\") | .spec.strategy == {\"type\": \"RollingUpdate\", \"rollingUpdate\": {\"maxSurge\": 1, \"maxUnavailable\": 0}}"
    t "the rollout strategy stays the same with two replicas" jqt egresssized ".[] | select(.kind == \"Deployment\" and .metadata.name == \"br-sandbox-proxy\") | .spec.replicas == 2 and .spec.strategy.rollingUpdate == {\"maxSurge\": 1, \"maxUnavailable\": 0}"
    t "the default is one replica with requests 100m and 128Mi and limits 1 and 512Mi" jqt cred ".[] | select(.kind == \"Deployment\" and .metadata.namespace == \"${PROXY_ALLOW_NS}\") | .spec.replicas == 1 and .spec.template.spec.containers[0].resources == {\"requests\": {\"cpu\": \"100m\", \"memory\": \"128Mi\"}, \"limits\": {\"cpu\": \"1\", \"memory\": \"512Mi\"}}"
}

check_the_proxy_pod_is_hardened_and_has_no_identity() {
    section "proxy pod: hardening, no token, no ServiceAccount, no right"
    local namespace
    for namespace in "$PROXY_ALLOW_NS" "$PROXY_FULL_NS"; do
        t "${namespace}: hostUsers false, no token, no service links, no host namespace, no ServiceAccount" jqt cred "$(proxy_pod "$namespace") | .hostUsers == false and .automountServiceAccountToken == false and .enableServiceLinks == false and (has(\"serviceAccountName\") | not) and (has(\"hostNetwork\") | not) and (has(\"hostPID\") | not) and (has(\"hostIPC\") | not)"
        t "${namespace}: pod seccomp RuntimeDefault and a numeric non-root user" jqt cred "$(proxy_pod "$namespace") | .securityContext.seccompProfile.type == \"RuntimeDefault\" and .securityContext.runAsNonRoot == true and (.securityContext.runAsUser | type) == \"number\" and .securityContext.runAsUser > 0"
        t "${namespace}: container drops ALL, read-only root, no escalation, no privilege, numeric non-root user" jqt cred "$(proxy_pod "$namespace") | .containers[0].securityContext | .capabilities == {\"drop\": [\"ALL\"]} and .readOnlyRootFilesystem == true and .allowPrivilegeEscalation == false and .privileged == false and .runAsNonRoot == true and (.runAsUser | type) == \"number\" and .runAsUser > 0 and .seccompProfile.type == \"RuntimeDefault\""
        t "${namespace}: the resolver is None and the name servers are the public resolvers" jqt cred "$(proxy_pod "$namespace") | .dnsPolicy == \"None\" and .dnsConfig.nameservers == [\"198.51.100.53\", \"198.51.100.54\"]"
        t "${namespace}: the only writable mount is a bounded emptyDir" jqt cred "$(proxy_pod "$namespace") | . as \$p | [\$p.containers[0].volumeMounts[] | select(.readOnly | not) | .name] == [\"tmp\"] and ([\$p.volumes[] | select(.name == \"tmp\") | .emptyDir.sizeLimit] == [\"16Mi\"])"
        t "${namespace}: one container, no init container, no hostPort" jqt cred "$(proxy_pod "$namespace") | (.containers | length) == 1 and (has(\"initContainers\") | not) and ([.containers[].ports[] | has(\"hostPort\")] | any | not)"
    done
    t "the user, the group and the fsGroup of the proxy are the constant 65532" jqt cred "$(proxy_pod "$PROXY_ALLOW_NS") | .containers[0].securityContext.runAsUser == 65532 and .containers[0].securityContext.runAsGroup == 65532 and .securityContext.runAsUser == 65532 and .securityContext.runAsGroup == 65532 and .securityContext.fsGroup == 65532"
    fails_with "values don't meet the specifications" -f "$EXAMPLE" -f "$EGRESS_VALUES" --set egress.proxy.runAsUser=20000
    fails_with "values don't meet the specifications" -f "$EXAMPLE" -f "$EGRESS_VALUES" --set egress.proxy.runAsGroup=20001
    t "no ServiceAccount, Role or RoleBinding is rendered in an egress namespace" jqt cred "${EGRESS_NAMESPACES} as \$e | [.[] | select((.kind == \"ServiceAccount\" or .kind == \"Role\" or .kind == \"RoleBinding\") and (.metadata.namespace as \$n | \$e | index(\$n) != null))] | length == 0"
}

check_the_proxy_image_is_a_digest_with_the_mirror_rule() {
    section "proxy image: by digest, never by tag, mirrored like every image"
    t "the default image is the product repository at the pinned digest" jqt cred "$(proxy_pod "$PROXY_ALLOW_NS") | .containers[0] | .image == \"docker.io/ironsh/iron-proxy@${PROXY_DIGEST}\" and .imagePullPolicy == \"IfNotPresent\""
    t "the repository and the digest are values" jqt egressother "$(proxy_pod "$PROXY_ALLOW_NS") | .containers[0].image == \"registry.example.test/other/proxy@${OTHER_DIGEST}\""
    fails_with "values don't meet the specifications" -f "$EXAMPLE" -f "$EGRESS_VALUES" --set egress.proxy.image.tag=0.61.0
    fails_with "values don't meet the specifications" -f "$EXAMPLE" -f "$EGRESS_VALUES" --set egress.proxy.image.repository=docker.io/ironsh/iron-proxy:0.61.0
    fails_with "values don't meet the specifications" -f "$EXAMPLE" -f "$EGRESS_VALUES" --set egress.proxy.image.digest=0.61.0
    fails_with "values don't meet the specifications" -f "$EXAMPLE" -f "$EGRESS_VALUES" --set egress.proxy.image.digest=sha256:abc
    t "with the mirror the proxy image starts with the mirror prefix" jqt egressmirror "$(proxy_pod "$PROXY_ALLOW_NS") | .containers[0].image == \"mirror.example.test/docker.io/ironsh/iron-proxy@${PROXY_DIGEST}\""
    t "with the mirror every image of an egress render starts with the mirror" test -z "$(grep -E '^\s+image: ' "$WORK/egressmirror_yaml.yaml" | grep -vE 'image: "?mirror\.example\.test/' || true)"
    t "with the mirror the render holds proxy images and sandbox images" test "$(grep -cE 'image: "?mirror\.example\.test/' "$WORK/egressmirror_yaml.yaml")" -ge 4
    t "with the mirror each proxy pod names the pull secret" jqt egressmirror '[.[] | select(.kind == "Deployment" and .metadata.name == "br-sandbox-proxy") | .spec.template.spec.imagePullSecrets] | length == 2 and all(. == [{"name": "mirror-pull"}])'
    t "without a pull secret the proxy pod names none" jqt cred '[.[] | select(.kind == "Deployment" and .metadata.name == "br-sandbox-proxy") | .spec.template.spec | has("imagePullSecrets")] | any | not'
}

check_the_secrets_are_mounted_in_the_proxy_pod_only() {
    section "Secrets: mounted in the proxy pod only, never in a sandbox pod, none rendered"
    t "the chart renders no Secret" jqt cred '[.[] | select(.kind == "Secret")] | length == 0'
    t "the credential Secret is mounted read-only in the proxy pod of mode full, named by the value" jqt cred "$(proxy_pod "$PROXY_FULL_NS") | (.volumes[] | select(.name == \"credentials\") | .secret.secretName) == \"sbx-alpha-full-credentials\" and (.containers[0].volumeMounts[] | select(.name == \"credentials\") | .readOnly == true and .mountPath == \"/etc/iron-proxy/credentials\")"
    t "the CA key Secret is mounted read-only in each proxy pod, named by the value" jqt cred '[.[] | select(.kind == "Deployment" and .metadata.name == "br-sandbox-proxy") | {(.metadata.namespace): (.spec.template.spec.volumes[] | select(.name == "ca-key") | .secret.secretName)}] | add == {"sbx-alpha-allow-egress": "sbx-alpha-allow-ca-key", "sbx-alpha-full-egress": "sbx-alpha-full-ca-key"}'
    t "the reload key reaches the proxy pod only as a secretKeyRef of the proxy container" jqt cred "$(proxy_pod "$PROXY_ALLOW_NS") | .containers[0].env == [{\"name\": \"IRON_MANAGEMENT_API_KEY\", \"valueFrom\": {\"secretKeyRef\": {\"name\": \"proxy-reload\", \"key\": \"token\"}}}]"
    t "the CA key volume of each proxy pod has the file mode 288 (octal 0440) and the single item ca.key" jqt cred '[.[] | select(.kind == "Deployment" and .metadata.name == "br-sandbox-proxy") | .spec.template.spec.volumes[] | select(.name == "ca-key") | .secret | {defaultMode, items}] == [{"defaultMode": 288, "items": [{"key": "ca.key", "path": "ca.key"}]}, {"defaultMode": 288, "items": [{"key": "ca.key", "path": "ca.key"}]}]'
    t "the credential volume has the file mode 288 (octal 0440) and no items list, so each key of the Secret is a file" jqt cred "$(proxy_pod "$PROXY_FULL_NS") | [.volumes[] | select(.name == \"credentials\") | .secret | {defaultMode, has_items: has(\"items\")}] == [{\"defaultMode\": 288, \"has_items\": false}]"
    t "mode allow has no credential volume" jqt cred "$(proxy_pod "$PROXY_ALLOW_NS") | [.volumes[].name] == [\"config\", \"ca\", \"ca-key\", \"tmp\"]"
    t "only proxy Deployments name a Secret in the egress render" jqt cred '[.[] | select(.kind != "ValidatingAdmissionPolicy" and (tostring | test("secretName|secretKeyRef"))) | .kind + "/" + .metadata.name] | unique == ["Deployment/br-sandbox-proxy"]'
    t "no sandbox pod names a Secret, a credential or a CA key" jqt cred "${SANDBOX_PODS} | length == 4 and (tostring | test(\"secret|credentials|ca-key\"; \"i\") | not)"
    t "no ConfigMap holds a Secret name or a private key" test -z "$(jq -r '.[] | select(.kind == "ConfigMap") | .data | tostring' "$WORK/cred.json" | grep -iE 'sbx-alpha-full-credentials|PRIVATE KEY' || true)"
    t "a proxied sandbox pod passes the model of its admission policy" test "$(model_result sbx-alpha-allow '.' cred)" = true -a "$(model_result sbx-alpha-full '.' cred)" = true
    t "the model refuses a secretKeyRef on a proxied sandbox pod" test "$(model_result sbx-alpha-full '.spec.containers[0].env += [{"name": "A", "valueFrom": {"secretKeyRef": {"name": "x", "key": "y"}}}]' cred)" = false
    t "the model refuses a Secret volume on a proxied sandbox pod" test "$(model_result sbx-alpha-full '.spec.volumes += [{"name": "s", "secret": {"secretName": "x"}}]' cred)" = false
}

check_the_proxy_probes_the_tmp_mount_and_the_warm_age_default_are_pinned() {
    section "proxy pod: probe period, /tmp mount; chart default: warm_max_age_seconds"
    local namespace
    for namespace in "$PROXY_ALLOW_NS" "$PROXY_FULL_NS"; do
        t "${namespace}: the readiness and the liveness probe are TCP on the tunnel port every 5 seconds" jqt cred "$(proxy_pod "$namespace") | .containers[0] | .readinessProbe == {\"tcpSocket\": {\"port\": 8888}, \"periodSeconds\": 5} and .livenessProbe == {\"tcpSocket\": {\"port\": 8888}, \"periodSeconds\": 5}"
        t "${namespace}: /tmp is the mount of the volume tmp, and that volume is the 16Mi emptyDir" jqt cred "$(proxy_pod "$namespace") | . as \$p | [\$p.containers[0].volumeMounts[] | select(.mountPath == \"/tmp\") | .name] == [\"tmp\"] and ([\$p.volumes[] | select(.name == \"tmp\")] == [{\"name\": \"tmp\", \"emptyDir\": {\"sizeLimit\": \"16Mi\"}}])"
    done
    t "the probe period stays 5 seconds when the tunnel port is set to another value" jqt egresssized "$(proxy_pod "$PROXY_ALLOW_NS") | .containers[0] | .readinessProbe == {\"tcpSocket\": {\"port\": 9999}, \"periodSeconds\": 5} and .livenessProbe == {\"tcpSocket\": {\"port\": 9999}, \"periodSeconds\": 5}"
    t "the default of warm_max_age_seconds in the chart values is 1800" test "$(yq '.warm_max_age_seconds' "$CHART/values.yaml")" = 1800
}
