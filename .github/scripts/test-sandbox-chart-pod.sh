#!/usr/bin/env bash

CAPS='["CHOWN", "DAC_OVERRIDE", "FOWNER", "FSETID", "KILL", "SETGID", "SETUID"]'
POOL_SPEC='.[] | select(.kind == "ReplicaSet" and .metadata.namespace == "sbx-alpha-none") | .spec.template'

check_pod_spec_fixed_fields() {
    section "the fixed fields of the pod"
    t "hostUsers false, no token, no service links, seccomp RuntimeDefault" jqt ex "${SANDBOX_PODS} | all(.spec.hostUsers == false and .spec.automountServiceAccountToken == false and .spec.enableServiceLinks == false and .spec.serviceAccountName == \"sandbox\" and .spec.securityContext.seccompProfile.type == \"RuntimeDefault\")"
    t "every container drops ALL and adds exactly the seven capabilities" jqt ex "${SANDBOX_PODS} | map(.spec.containers[], .spec.initContainers[]) | all(.securityContext.capabilities.drop == [\"ALL\"] and (.securityContext.capabilities.add | sort) == ${CAPS} and .securityContext.allowPrivilegeEscalation == false and .securityContext.privileged == false)"
    t "pause is process 1 of the agent, from the read-only tools mount" jqt ex "${POOL_SPEC} | .spec.containers[0] | .command == [\"/opt/sbx/bin/pause\"] and (.volumeMounts[] | select(.name == \"tools\") | .readOnly == true)"
    t "the init container extracts the tools payload with the tools bsdtar" jqt ex "${POOL_SPEC} | .spec.initContainers[0] | .command[0] == \"/sbx/bin/bsdtar\" and (.volumeMounts[] | select(.name == \"tools\") | has(\"readOnly\") | not)"
    t "the agent runs as a numeric non-root user" jqt ex "${POOL_SPEC} | .spec.containers[0].securityContext | .runAsUser == 10001 and .runAsGroup == 10001"
    t "the writable paths are /work and /home/agent, bounded" jqt ex "${POOL_SPEC} | (.spec.containers[0].volumeMounts | map(select(.readOnly | not) | .mountPath) | sort) == [\"/home/agent\", \"/work\"] and ([.spec.volumes[] | select(.name == \"work\") | .emptyDir.sizeLimit] == [\"3Gi\"])"
    t "the runtime class stays out of the pod when empty" jqt ex "${SANDBOX_PODS} | all(.spec | has(\"runtimeClassName\") | not)"
    render rc -f "$EXAMPLE" --set runtimeClassName=kata-example
    t "a runtime class value reaches the pod and the policy" jqt rc '([.[] | select(.kind == "ReplicaSet") | .spec.template.spec.runtimeClassName] | unique == ["kata-example"]) and ([.[] | select(.kind == "ValidatingAdmissionPolicy") | .spec.validations[].expression | select(test("kata-example"))] | length == 4)'
    render uid -f "$EXAMPLE" --set runAsUser=20000 --set runAsGroup=20001
    t "the user ids are values" jqt uid "${POOL_SPEC} | .spec.containers[0].securityContext | .runAsUser == 20000 and .runAsGroup == 20001"
}

check_seed_container() {
    section "the seed container (second root container, read-only for the agent)"
    t "the seed container is named seed, runs pause of the tools image, read-only root" jqt ex "${POOL_SPEC} | .spec.containers[1] | .name == \"seed\" and .command == [\"/sbx/bin/pause\"] and .securityContext.readOnlyRootFilesystem == true and .securityContext.runAsUser == 0 and (.securityContext.capabilities.add | sort) == ${CAPS} and (has(\"ports\") | not)"
    t "the seed container image is the tools image" jqt ex "${POOL_SPEC} | .spec.containers[1].image == .spec.initContainers[0].image"
    t "the seed container writes /skills, the agent mounts it read-only" jqt ex "${POOL_SPEC} | (.spec.containers[1].volumeMounts == [{\"name\": \"skills\", \"mountPath\": \"/skills\"}]) and (.spec.containers[0].volumeMounts[] | select(.name == \"skills\") | .readOnly == true)"
    t "no pod shares its process namespace" jqt ex "${SANDBOX_PODS} | all(.spec | has(\"shareProcessNamespace\") | not)"
    t "the seed resources are small keys apart from the pool size" jqt ex "${POOL_SPEC} | .spec.containers[1].resources | .requests == {\"cpu\": \"10m\", \"memory\": \"8Mi\", \"ephemeral-storage\": \"8Mi\"}"
    render bigseed -f "$EXAMPLE" --set pool.size.cpu.limit=3
    t "a bigger pool size leaves the seed resources alone" test "$(jq -c '[.[] | select(.kind == "ReplicaSet") | .spec.template.spec.containers[1].resources]' "$WORK/bigseed.json")" = "$(jq -c '[.[] | select(.kind == "ReplicaSet") | .spec.template.spec.containers[1].resources]' "$WORK/ex.json")"
    render noseed -f "$EXAMPLE" --set tools.seed_container.enabled=false
    t "without the seed container the pod has one container" jqt noseed "${SANDBOX_PODS} | all(.spec.containers | length == 1)"
    render seedname -f "$EXAMPLE" --set tools.seed_container.name=skills-writer
    t "the seed container name is a value" jqt seedname "${POOL_SPEC} | .spec.containers[1].name == \"skills-writer\""
}

check_no_secret_in_a_sandbox_pod() {
    section "no Secret reaches a sandbox pod; the mirror and pull secrets"
    t "sandbox pods hold no secret volume, secretKeyRef or secretRef" jqt keeper "${SANDBOX_PODS} | tostring | test(\"secret\"; \"i\") | not"
    t "the probe pods hold no secret either" test "$(yq 'select(.metadata.name == "br-sandbox-netcheck" and .metadata.namespace == "sbx-system") | .data | to_entries | map(select(.key | test("^probe-"))) | .[].value' "$WORK/ex_yaml.yaml" | grep -ci secret || true)" = 0
    t "the keeper reads the store credential from the named Secret by key" jqt keeper '.[] | select(.kind == "Deployment") | .spec.template.spec.containers[0].env | map(select(.valueFrom.secretKeyRef.name == "store-credentials") | .name) | sort == ["AWS_ACCESS_KEY_ID", "AWS_SECRET_ACCESS_KEY"]'
    t "the chart renders no Secret" jqt mirror '[.[] | select(.kind == "Secret")] | length == 0'
    t "with a mirror every image starts with the mirror" test -z "$(grep -E '^\s+image: ' "$WORK/mirror_yaml.yaml" | grep -vE 'image: mirror\.example\.test/' || true)"
    t "every image of the probe pods starts with the mirror" test -z "$(grep -E '^\s+image: ' "$WORK/mirror_yaml.yaml" | sed -E 's/^ +//' | grep -v '^image: mirror.example.test/' || true)"
    t "every pod spec of the render names the pull secret" jqt mirror '[.[] | select(.kind == "ReplicaSet") | .spec.template.spec] + [.[] | select(.kind == "Deployment") | .spec.template.spec] + [.[] | select(.kind == "Job") | .spec.template.spec] + [.[] | select(.kind == "CronJob") | .spec.jobTemplate.spec.template.spec] | length == 7 and all(.imagePullSecrets == [{"name": "mirror-pull"}])'
    t "the probe pod manifests name the pull secret" test "$(yq 'select(.metadata.name == "br-sandbox-netcheck" and .metadata.namespace == "sbx-system") | .data | to_entries | map(select(.key | test("^probe-")) | .value | from_yaml | .spec.imagePullSecrets[0].name) | unique | .[]' "$WORK/mirror_yaml.yaml")" = mirror-pull
    t "the policy accepts the mirrored repositories only" jqt mirror '[.[] | select(.kind == "ValidatingAdmissionPolicy") | .spec.validations[] | select(.message | startswith("BR_SANDBOX_IMAGE")) | .expression | test("\\[(.mirror.example.test/[^,\\]]+(, )?)+\\]")] | all'
    t "the render names no vendor token" bash -c '! grep -q IRON_PROXY_TOKEN "$1"' _ "$WORK/mirror_yaml.yaml"
}

check_PSS_namespace_labels() {
    section "PSS baseline is enforced, restricted warns and audits"
    t "every managed namespace carries the Pod Security labels and the managed label" jqt ex '[.[] | select(.kind == "Namespace" and .metadata.labels["br.sandbox/managed"] == "true") | .metadata.labels | select(.["pod-security.kubernetes.io/enforce"] == "baseline" and .["pod-security.kubernetes.io/warn"] == "restricted" and .["pod-security.kubernetes.io/audit"] == "restricted" and (.["br.sandbox/mode"] | IN("none", "allow", "full")) and .["br.sandbox/client"] != null)] | length == 4'
    t "the system namespace is not managed, so no exclusion applies to it" jqt ex '.[] | select(.kind == "Namespace" and .metadata.name == "sbx-system") | .metadata.labels | has("br.sandbox/managed") | not'
    t "the sandbox service account has no token and no Role" jqt ex '[.[] | select(.kind == "ServiceAccount" and .metadata.name == "sandbox" and .automountServiceAccountToken == false)] | length == 4'
}

check_label_contract() {
    section "label contract of the pool"
    t "selector and template carry role, pool and pool-id" jqt ex "${POOL_SPEC} | .metadata.labels == {\"br.sandbox/role\": \"sandbox\", \"br.sandbox/pool\": \"warm\", \"br.sandbox/pool-id\": \"alpha-none\"}"
    t "the selector holds pool and pool-id, never the role alone" jqt ex '.[] | select(.kind == "ReplicaSet" and .metadata.namespace == "sbx-alpha-none") | .spec.selector.matchLabels == {"br.sandbox/pool": "warm", "br.sandbox/pool-id": "alpha-none"}'
    t "no policy selects a pod by the pool label" jqt ex "[${NP} | .. | objects | select(has(\"matchLabels\")) | .matchLabels | keys[] | select(. == \"br.sandbox/pool\")] | length == 0"
}

check_slots_render_nothing() {
    section "slots render nothing now"
    local slot
    for slot in egress.env egress.volumes egress.volume_mounts egress.objects keeper.objects; do
        t "the slot ${slot} exists" grep -q "define \"br-sandbox-chart.slot.${slot}\"" "$CHART/templates/_slots.tpl"
    done
    t "the render holds only the kinds of the chart" jqt ex '[.[].kind] | unique - ["Namespace", "ServiceAccount", "ResourceQuota", "LimitRange", "ReplicaSet", "NetworkPolicy", "ValidatingAdmissionPolicy", "ValidatingAdmissionPolicyBinding", "Role", "RoleBinding", "ClusterRole", "ClusterRoleBinding", "ConfigMap", "Job", "CronJob", "Deployment"] == []'
    render egon -f "$EXAMPLE" --set egress.enabled=true
    t "egress.enabled true renders no proxy object: the slot objects stay empty" jqt egon '([.[] | select(.kind == "Namespace")] | length) > 0 and ([.[] | select(.kind == "Deployment" or .kind == "Service" or .kind == "Secret")] | length) == 0'
}

check_egress_values_fail_while_egress_is_disabled() {
    section "a credential set or an allow list fails the render while egress is disabled"
    fails_with "BR_SANDBOX_EGRESS_DISABLED{client=alpha,mode=allow,key=allow}" -f "$(vals allowdomains '.clients.alpha.modes.allow.allow.domains = ["git.example.test"]')"
    fails_with "BR_SANDBOX_EGRESS_DISABLED{client=alpha,mode=allow,key=allow}" -f "$(vals allowcidrs '.clients.alpha.modes.allow.allow.cidrs = ["192.0.2.0/24"]')"
    fails_with "BR_SANDBOX_EGRESS_DISABLED{client=beta,mode=none,key=credential_set}" -f "$(vals credset '.clients.beta.modes.none.credential_set = {"git": "set-a"}')"
    t "empty lists and an empty credential set render" helm template t "$CHART" -f "$(vals emptyegress '.clients.alpha.modes.allow.allow = {"domains": [], "cidrs": []} | .clients.beta.modes.none.credential_set = {}')"
}
