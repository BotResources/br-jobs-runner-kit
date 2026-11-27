#!/usr/bin/env bash

NS=sbx-alpha-none
POOLS_NONE='[]'

check_the_pod_policy_takes_the_pool_of_the_namespace_as_its_parameter() {
    section "pod policy: the SandboxPool of the namespace is the parameter; no resource, no new pod"
    t "the policy names the kind SandboxPool of sandbox.botresources.ai/v1alpha1 as its parameter and fails closed" jqt ex "[${VAP} | select(.spec.paramKind == {\"apiVersion\": \"sandbox.botresources.ai/v1alpha1\", \"kind\": \"SandboxPool\"} and .spec.failurePolicy == \"Fail\")] | length == 1"
    t "the binding br-sandbox-pod takes every SandboxPool of the namespace of the pod, with no name and no namespace, and denies when none exists" jqt ex "[${BINDINGS} | select(.metadata.name == \"br-sandbox-pod\") | .spec.paramRef] == [{\"selector\": {}, \"parameterNotFoundAction\": \"Deny\"}]"
    t "the binding br-sandbox-pod matches the creation of pods only" jqt ex "[${BINDINGS} | select(.metadata.name == \"br-sandbox-pod\") | .spec.matchResources.resourceRules[] | [.operations, .resources]] == [[[\"CREATE\"], [\"pods\"]]]"
    t "the binding br-sandbox-pod-update has no parameter and matches the update of pods and of their ephemeral containers" jqt ex "[${BINDINGS} | select(.metadata.name == \"br-sandbox-pod-update\") | select(.spec.policyName == \"br-sandbox-pod\" and (.spec.paramRef | not) and .spec.validationActions == [\"Deny\"] and .spec.matchResources.namespaceSelector.matchLabels == {\"br.sandbox/managed\": \"true\"} and ([.spec.matchResources.resourceRules[] | [.operations, .resources]] == [[[\"UPDATE\"], [\"pods\", \"pods/ephemeralcontainers\"]]]))] | length == 1"
    t "the policy and its bindings name no namespace and no label other than the managed label" jqt ex "[${BINDINGS} | .spec.matchResources.namespaceSelector | keys] | all(. == [\"matchLabels\"])"
    t "only the resources rule and the image rule read the parameter, each behind the guard that skips an update" jqt ex "${QTY}${MODEL}"'[.[] | select(.kind == "ValidatingAdmissionPolicy" and .metadata.name == "br-sandbox-pod") | .spec.validations[] | select(.expression | contains("params"))] | map(.message | split(":")[0]) == ["BR_SANDBOX_RESOURCES", "BR_SANDBOX_IMAGE"]'
    t "no expression reads the parameter before the guard request.operation != CREATE" jqt ex "${QTY}${MODEL}"'[.[] | select(.kind == "ValidatingAdmissionPolicy") | .spec.validations[].expression | select(unguarded_params(.))] | length == 0'
    t "the other containers are bound by the tools image and the check image, never by the image of the pool" jqt ex "[${VAP} | .spec.validations[] | select(.message | startswith(\"BR_SANDBOX_IMAGE:\")) | .expression | select(contains(\"['registry.example.test/tools/br-sandbox-tools', 'registry.example.test/tools/probe'].exists(p, \") and contains(\"c.image == params.spec.image\"))] | length == 1"
    t "the rule on size names the size of the pool and the platform maximum and reads no bound from the pod" jqt ex "[${VAP} | .spec.validations[] | select(.message | startswith(\"BR_SANDBOX_RESOURCES:\")) | .expression | select(contains(\"params.spec.size.cpu.request\") and contains(\"params.spec.size.disk.limit\") and contains(\"quantity('2Gi')\") and (contains(\"object.spec\") | not))] | length == 1"
}

check_create_is_judged_against_the_pool_and_the_platform_maximum() {
    section "pod policy model: a created pod against the SandboxPool and the maximum of the platform"
    local ns pool big
    for ns in sbx-alpha-allow sbx-alpha-full sbx-alpha-none sbx-beta-none; do
        t "the pod that the keeper makes from the template of ${ns} is admitted with its pool" test "$(created ex "$ns")" = admit
    done
    t "the pod of the mirror render is admitted with its pool" test "$(created mirror "$NS")" = admit
    t "the template as rendered, with the tools image as the placeholder of the agent image, is refused by the image rule only" test "$(verdict ex CREATE pods "$(template_pod ex "$NS")" null "[$(pool_of "$NS" | jq -c 'del(.spec.size)')]")" = BR_SANDBOX_IMAGE
    t "an image other than the image of the pool is refused" test "$(created ex "$NS" '(agent).image = "other.example.test/x:1"')" = BR_SANDBOX_IMAGE
    t "the same image under another tag is refused" test "$(created ex "$NS" '(agent).image = "registry.example.test/agents/base:2.0"')" = BR_SANDBOX_IMAGE
    t "the agent container with the tools image is refused" test "$(created ex "$NS" '(agent).image = .spec.containers[1].image')" = BR_SANDBOX_IMAGE
    t "an init container with the image of the pool is refused" test "$(created ex "$NS" '.spec.initContainers[0].image = "registry.example.test/agents/base:1.0"')" = BR_SANDBOX_IMAGE
    t "a pod with no agent container and the image of the pool on another name is refused" test "$(created ex "$NS" '.spec.containers[0].name = "worker"')" = BR_SANDBOX_IMAGE
    t "a pool with no image admits no agent" test "$(created ex "$NS" . "[$(pool_of "$NS" | jq -c 'del(.spec.image)')]")" = BR_SANDBOX_IMAGE
    t "a managed namespace with no SandboxPool admits no pod" test "$(created ex "$NS" . "$POOLS_NONE")" = PARAMETER_NOT_FOUND
    t "a pool with a different image from the second pool of the namespace stops the pod: the pod must pass both" test "$(created ex "$NS" . "[$(pool_of "$NS"), $(pool_of "$NS" | jq -c '.metadata.name = "second" | .spec.image = "registry.example.test/agents/other:1.0"')]")" = BR_SANDBOX_IMAGE
    t "a pod above the size of the pool is refused" test "$(created ex "$NS" '(agent).resources.limits.memory = "2Gi"')" = BR_SANDBOX_RESOURCES
    t "a pod above the cpu request of the pool is refused" test "$(created ex "$NS" '(agent).resources.requests.cpu = "500m"')" = BR_SANDBOX_RESOURCES
    t "a pod above the disk limit of the pool is refused" test "$(created ex "$NS" '(agent).resources.limits["ephemeral-storage"] = "3Gi"')" = BR_SANDBOX_RESOURCES
    big="$(pool_of "$NS" | jq -c '.spec.size.memory.limit = "4Gi"')"
    t "a pod above the maximum of the platform is refused under a pool that allows it" test "$(created ex "$NS" '(agent).resources.limits.memory = "3Gi"' "[${big}]")" = BR_SANDBOX_RESOURCES
    t "a pod at the maximum of the platform is admitted under a pool that has no size" test "$(created ex "$NS" '(agent).resources = {"requests": {"cpu": "500m", "memory": "1Gi", "ephemeral-storage": "2Gi"}, "limits": {"cpu": "2", "memory": "2Gi", "ephemeral-storage": "5Gi"}}' "[$(pool_of "$NS" | jq -c 'del(.spec.size)')]")" = admit
    pool="$(pool_of "$NS" | jq -c '.spec.size = {"cpu": {"limit": "1"}}')"
    t "a pool that sets only the cpu limit bounds only the cpu limit" test "$(created ex "$NS" '(agent).resources.limits.memory = "2Gi" | (agent).resources.requests.cpu = "500m"' "[${pool}]")" = admit
    t "a pool that sets only the cpu limit still bounds the cpu limit" test "$(created ex "$NS" '(agent).resources.limits.cpu = "2"' "[${pool}]")" = BR_SANDBOX_RESOURCES
    t "an init container at the maximum of the platform is not bounded by the pool" test "$(created ex "$NS" '.spec.initContainers[0].resources.limits.memory = "2Gi"')" = admit
    t "an init container above the maximum of the platform is refused" test "$(created ex "$NS" '.spec.initContainers[0].resources.limits.memory = "3Gi"')" = BR_SANDBOX_RESOURCES
    t "a container with no limits is refused" test "$(created ex "$NS" 'del((agent).resources.limits)')" = BR_SANDBOX_RESOURCES
    t "the model refuses an extra capability" test "$(created ex "$NS" '(agent).securityContext.capabilities.add += ["SYS_ADMIN"]')" = BR_SANDBOX_CAPABILITY
    t "the model refuses a secret volume" test "$(created ex "$NS" '.spec.volumes += [{"name": "s", "secret": {"secretName": "x"}}]')" = BR_SANDBOX_VOLUME_FORBIDDEN
    t "the model refuses a secretKeyRef" test "$(created ex "$NS" '(agent).env += [{"name": "A", "valueFrom": {"secretKeyRef": {"name": "x", "key": "y"}}}]')" = BR_SANDBOX_VOLUME_FORBIDDEN
    t "the model refuses seccomp Unconfined on a container" test "$(created ex "$NS" '(agent).securityContext.seccompProfile = {"type": "Unconfined"}')" = BR_SANDBOX_PRIVILEGE
    t "the model refuses hostUsers true" test "$(created ex "$NS" '.spec.hostUsers = true')" = BR_SANDBOX_USERNS_REQUIRED
    t "the model refuses a seed container without the drop list" test "$(created ex "$NS" 'del(.spec.containers[1].securityContext.capabilities.drop)')" = BR_SANDBOX_CAPABILITY
    t "a pod with another role is refused by the role rule" test "$(created ex "$NS" '.metadata.labels["br.sandbox/role"] = "proxy"')" = BR_SANDBOX_ROLE
    t "the probe pod of the check passes with a pool in its namespace" test "$(verdict ex CREATE pods "$(probe_pod ex "$NS" | jq -c '.')" null "[$(pool_of "$NS")]")" = admit
}

check_rules_14_and_15() {
    section "pod policy model: the account and the ephemeral containers"
    t "the pod of the chart names the ServiceAccount sandbox and is admitted" test "$(created ex "$NS")" = admit
    t "the account default is refused" test "$(created ex "$NS" '.spec.serviceAccountName = "default"')" = BR_SANDBOX_SERVICE_ACCOUNT
    t "another account name is refused" test "$(created ex "$NS" '.spec.serviceAccountName = "runner"')" = BR_SANDBOX_SERVICE_ACCOUNT
    t "an absent account name is refused" test "$(created ex "$NS" 'del(.spec.serviceAccountName)')" = BR_SANDBOX_SERVICE_ACCOUNT
    t "the rule on the account has no exception for a user" jqt ex "[${VAP} | .spec.validations[] | select(.message | startswith(\"BR_SANDBOX_SERVICE_ACCOUNT:\")) | .expression | select(contains(\"userInfo\"))] | length == 0"
    t "the probe pod of every namespace names the ServiceAccount sandbox" test "$(for n in sbx-alpha-allow sbx-alpha-full sbx-alpha-none sbx-beta-none; do probe_pod ex "$n" | jq -r '.spec.serviceAccountName'; done | sort -u)" = sandbox
    t "a pod with one ephemeral container is refused" test "$(created ex "$NS" '.spec.ephemeralContainers = [{"name": "debug", "image": "x"}]')" = BR_SANDBOX_EPHEMERAL_CONTAINERS
    t "a pod with a non-root ephemeral container is refused" test "$(created ex "$NS" '.spec.ephemeralContainers = [{"name": "debug", "image": "x", "securityContext": {"runAsNonRoot": true, "runAsUser": 20000}}]')" = BR_SANDBOX_EPHEMERAL_CONTAINERS
    t "a pod with an empty list of ephemeral containers is admitted" test "$(created ex "$NS" '.spec.ephemeralContainers = []')" = admit
    t "the rule on ephemeral containers has no exception for a user" jqt ex "[${VAP} | .spec.validations[] | select(.message | startswith(\"BR_SANDBOX_EPHEMERAL_CONTAINERS:\")) | .expression | select(contains(\"userInfo\"))] | length == 0"
    t "only the non-root rule reads the user" jqt ex "[${VAP} | .spec.validations[] | select(.expression | contains(\"userInfo\")) | .message | split(\":\")[0]] == [\"BR_SANDBOX_RUN_AS_NON_ROOT\"]"
}

check_update_is_not_judged_against_the_pool() {
    section "pod policy model: an update is not judged against the pool, and no field judged at creation can change"
    local none='[]' other changed
    other="[$(pool_of "$NS" | jq -c '.spec.image = "registry.example.test/agents/other:2.0" | .spec.size.memory.limit = "256Mi"')]"
    changed="$(pool_of "$NS" | jq -c '.spec.image = "registry.example.test/agents/other:2.0"')"
    t "a label patch is admitted when the pool has the same image" test "$(updated ex "$NS" '.metadata.labels["x"] = "y"')" = admit
    t "a label patch is admitted after the image of the pool changed" test "$(updated ex "$NS" '.metadata.labels["x"] = "y"' "[${changed}]")" = admit
    t "a label patch is admitted after the pool shrank below the size of the pod" test "$(updated ex "$NS" '.metadata.labels["x"] = "y"' "$other")" = admit
    t "an annotation patch is admitted after the pool was deleted" test "$(updated ex "$NS" '.metadata.annotations["x"] = "y"' "$none")" = admit
    t "a patch of the image of the agent is refused with the pool as it was" test "$(updated ex "$NS" '(agent).image = "registry.example.test/agents/base:1.1"')" = BR_SANDBOX_IMMUTABLE
    t "a patch of the image of the agent is refused with a pool that has another image" test "$(updated ex "$NS" '(agent).image = "other.example.test/x:1"' "[${changed}]")" = BR_SANDBOX_IMMUTABLE
    t "a patch of the image of the agent is refused after the pool was deleted" test "$(updated ex "$NS" '(agent).image = "other.example.test/x:1"' "$none")" = BR_SANDBOX_IMMUTABLE
    t "a patch of the image of an init container is refused" test "$(updated ex "$NS" '.spec.initContainers[0].image = "registry.example.test/agents/base:1.0"')" = BR_SANDBOX_IMMUTABLE
    t "a patch of the image of the tools container is refused" test "$(updated ex "$NS" '.spec.containers[1].image = "registry.example.test/tools/probe:9"')" = BR_SANDBOX_IMMUTABLE
    t "a change of the resources of the agent is refused" test "$(updated ex "$NS" '(agent).resources.limits.memory = "2Gi"')" = BR_SANDBOX_IMMUTABLE
    t "a change of the account is refused" test "$(updated ex "$NS" '.spec.serviceAccountName = "default"')" = BR_SANDBOX_IMMUTABLE,BR_SANDBOX_SERVICE_ACCOUNT
    t "a change of the security context of the pod is refused" test "$(updated ex "$NS" '.spec.securityContext.runAsUser = 0')" = BR_SANDBOX_IMMUTABLE
    t "a change of the security context of a container is refused" test "$(updated ex "$NS" '(agent).securityContext.capabilities.drop = ["NET_RAW"]')" = BR_SANDBOX_CAPABILITY,BR_SANDBOX_IMMUTABLE
    t "a change of hostUsers is refused" test "$(updated ex "$NS" '.spec.hostUsers = true')" = BR_SANDBOX_IMMUTABLE,BR_SANDBOX_USERNS_REQUIRED
    t "a new volume is refused" test "$(updated ex "$NS" '.spec.volumes += [{"name": "v", "emptyDir": {}}]')" = BR_SANDBOX_IMMUTABLE
    t "an update through pods/ephemeralcontainers that adds a container is refused" test "$(updated ex "$NS" '.spec.ephemeralContainers = [{"name": "debug", "image": "x"}]' "$none" pods/ephemeralcontainers)" = BR_SANDBOX_EPHEMERAL_CONTAINERS,BR_SANDBOX_IMMUTABLE
    t "an update through pods/ephemeralcontainers that adds a container is refused with a pool present" test "$(updated ex "$NS" '.spec.ephemeralContainers = [{"name": "debug", "image": "x"}]' "[$(pool_of "$NS")]" pods/ephemeralcontainers)" = BR_SANDBOX_EPHEMERAL_CONTAINERS,BR_SANDBOX_IMMUTABLE
    t "the rule on frozen fields names every field judged at creation" jqt ex "[${VAP} | .spec.validations[] | select(.message | startswith(\"BR_SANDBOX_IMMUTABLE:\")) | .expression | select(contains(\"containers\") and contains(\"initContainers\") and contains(\"ephemeralContainers\") and contains(\"serviceAccountName\") and contains(\"securityContext\") and contains(\"hostUsers\") and contains(\"volumes\"))] | length == 1"
}
