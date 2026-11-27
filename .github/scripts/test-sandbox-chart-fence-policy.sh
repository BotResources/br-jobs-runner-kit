#!/usr/bin/env bash

FENCE_VAP='.[] | select(.kind == "ValidatingAdmissionPolicy" and .metadata.name == "br-sandbox-fence")'
FENCE_BINDING='.[] | select(.kind == "ValidatingAdmissionPolicyBinding" and .metadata.name == "br-sandbox-fence")'
KUBE_USER='system:kube-controller-manager'

check_fence_policy_shape() {
    section "fence policy: a project identity cannot write a fence object of a managed namespace"
    t "the policy br-sandbox-fence takes the grants ConfigMap as parameter and fails closed" jqt ex "[${FENCE_VAP} | select(.spec.paramKind == {\"apiVersion\": \"v1\", \"kind\": \"ConfigMap\"} and .spec.failurePolicy == \"Fail\")] | length == 1"
    t "the policy matches the creation, the update and the deletion of exactly these kinds" jqt ex "[${FENCE_VAP} | .spec.matchConstraints.resourceRules | map(.operations | sort) | unique] == [[[\"CREATE\", \"DELETE\", \"UPDATE\"]]] and ([${FENCE_VAP} | .spec.matchConstraints.resourceRules[] | .resources[]] | sort) == [\"configmaps\", \"limitranges\", \"networkpolicies\", \"resourcequotas\", \"rolebindings\", \"roles\", \"serviceaccounts\"]"
    t "no kind of the policy or of its binding names a subresource, so the status of a quota is not matched" jqt ex "[(${FENCE_VAP} | .spec.matchConstraints.resourceRules[].resources[]), (${FENCE_BINDING} | .spec.matchResources.resourceRules[].resources[])] | all(contains(\"/\") | not)"
    t "the binding has the rules of the policy, selects the managed label in the namespace and denies" jqt ex "[${FENCE_BINDING}] as \$b | [${FENCE_VAP}] as \$v | (\$b | length == 1) and (\$b[0].spec.matchResources.resourceRules == \$v[0].spec.matchConstraints.resourceRules) and (\$b[0].spec.matchResources.namespaceSelector == {\"matchLabels\": {\"br.sandbox/managed\": \"true\"}}) and (\$b[0].spec.validationActions == [\"Deny\"])"
    t "the binding names the grants ConfigMap of the system namespace and denies when it is absent" jqt ex "[${FENCE_BINDING} | .spec.paramRef] == [{\"name\": \"br-sandbox-grants\", \"namespace\": \"sbx-system\", \"parameterNotFoundAction\": \"Deny\"}]"
    t "the policy has one rule and its only exemptions are a writer and a Terminating namespace" jqt ex "[${FENCE_VAP} | .spec.validations | map(.expression)] == [[\"!variables.fenced || variables.writer || (has(namespaceObject.status) && namespaceObject.status.phase == 'Terminating')\"]]"
    t "the ServiceAccount is fenced by the name sandbox and the ConfigMap by the prefix br-sandbox-" jqt ex "[${FENCE_VAP} | .spec.variables[] | select(.name == \"fenced\") | .expression] == [\"request.resource.resource == 'serviceaccounts' ? variables.name == 'sandbox' : (request.resource.resource == 'configmaps' ? variables.name.startsWith('br-sandbox-') : true)\"]"
    t "the fence part renders no fence policy" jqt fence '[.[] | select(.metadata.name == "br-sandbox-fence")] | length == 0'
    t "the three policies of the operator part are the only admission policies and each binding names its policy" jqt ex '([.[] | select(.kind == "ValidatingAdmissionPolicy") | .metadata.name] | sort) == ["br-sandbox-fence", "br-sandbox-namespace", "br-sandbox-pod"] and ([.[] | select(.kind == "ValidatingAdmissionPolicyBinding") | .spec.policyName] | sort) == ["br-sandbox-fence", "br-sandbox-namespace", "br-sandbox-pod", "br-sandbox-pod"]'
}

check_fence_policy_model() {
    section "fence policy model: who may write which object, with the grants of the platform"
    local grants none res
    grants="$(grants_of grants)"
    none="$(grants_of ex)"
    for res in networkpolicies resourcequotas limitranges roles rolebindings; do
        t "a project identity creates ${res} in a managed namespace and is refused" test "$(fence_verdict ex CREATE "$res" anything "$PROJECT_USER" "$grants")" = BR_SANDBOX_FENCE
        t "a project identity changes ${res} in a managed namespace and is refused" test "$(fence_verdict ex UPDATE "$res" anything "$PROJECT_USER" "$grants")" = BR_SANDBOX_FENCE
        t "a project identity deletes ${res} in a managed namespace and is refused" test "$(fence_verdict ex DELETE "$res" anything "$PROJECT_USER" "$grants")" = BR_SANDBOX_FENCE
        t "a writer writes ${res} in a managed namespace" test "$(fence_verdict ex CREATE "$res" anything "$WRITER_USER" "$grants")" = admit
    done
    t "the ServiceAccount sandbox is refused to a project identity" test "$(fence_verdict ex CREATE serviceaccounts sandbox "$PROJECT_USER" "$grants")" = BR_SANDBOX_FENCE
    t "a ServiceAccount of another name is not a fence object" test "$(fence_verdict ex CREATE serviceaccounts builder "$PROJECT_USER" "$grants")" = admit
    t "the ConfigMap br-sandbox-pod-template is refused to a project identity" test "$(fence_verdict ex UPDATE configmaps br-sandbox-pod-template "$PROJECT_USER" "$grants")" = BR_SANDBOX_FENCE
    t "the ConfigMap br-sandbox-config is refused to a project identity on deletion" test "$(fence_verdict ex DELETE configmaps br-sandbox-config "$PROJECT_USER" "$grants")" = BR_SANDBOX_FENCE
    t "a ConfigMap of another name is not a fence object" test "$(fence_verdict ex CREATE configmaps app-config "$PROJECT_USER" "$grants")" = admit
    t "a ConfigMap whose name only contains the prefix is not a fence object" test "$(fence_verdict ex CREATE configmaps my-br-sandbox-x "$PROJECT_USER" "$grants")" = admit
    t "a writer writes the ConfigMap br-sandbox-pod-template" test "$(fence_verdict ex UPDATE configmaps br-sandbox-pod-template "$WRITER_USER" "$grants")" = admit
    t "a user that is not in the writers is not a writer" test "$(fence_verdict ex CREATE networkpolicies x "$WRITER_USER" "$none")" = BR_SANDBOX_FENCE
    t "a Terminating namespace admits the deletion of a fence object by any identity, so the deletion of a namespace completes" test "$(fence_verdict ex DELETE networkpolicies br-sandbox-default-deny "$KUBE_USER" "$grants" Terminating)" = admit
    t "a namespace that is Active gives no exemption" test "$(fence_verdict ex DELETE networkpolicies br-sandbox-default-deny "$KUBE_USER" "$grants" Active)" = BR_SANDBOX_FENCE
    t "a namespace without the managed label is not judged" test "$(fence_verdict ex CREATE networkpolicies x "$PROJECT_USER" "$grants" Active '{"team":"x"}')" = admit
    t "the used count of a quota moves: the update of resourcequotas/status by the controller is not matched" test "$(fence_verdict ex UPDATE resourcequotas/status br-sandbox-quota "$KUBE_USER" "$grants")" = admit
    t "the same controller user is refused on the quota itself" test "$(fence_verdict ex UPDATE resourcequotas br-sandbox-quota "$KUBE_USER" "$grants")" = BR_SANDBOX_FENCE
    t "with no grants ConfigMap every write of a fence object is refused" test "$(fence_verdict ex CREATE networkpolicies x "$WRITER_USER" null)" = PARAMETER_NOT_FOUND
    t "with no grants ConfigMap a kind that is not a fence kind is not judged" test "$(fence_verdict ex CREATE deployments x "$PROJECT_USER" null)" = admit
}

check_fence_policy_model_reads_the_text() {
    section "fence policy model: a mutated render changes the verdict, so the model reads the render"
    local grants
    grants="$(grants_of grants)"
    jq 'map(if .kind == "ValidatingAdmissionPolicy" and .metadata.name == "br-sandbox-fence" then .spec.matchConstraints.resourceRules[0].resources += ["resourcequotas/status"] else . end) | map(if .kind == "ValidatingAdmissionPolicyBinding" and .metadata.name == "br-sandbox-fence" then .spec.matchResources.resourceRules[0].resources += ["resourcequotas/status"] else . end)' "$WORK/ex.json" > "$WORK/fence_status.json"
    jq 'map(if .kind == "ValidatingAdmissionPolicy" and .metadata.name == "br-sandbox-fence" then .spec.validations |= map(.expression |= sub("variables.writer \\|\\| "; "")) else . end)' "$WORK/ex.json" > "$WORK/fence_nowriter.json"
    jq 'map(if .kind == "ValidatingAdmissionPolicy" and .metadata.name == "br-sandbox-fence" then .spec.variables |= map(.expression |= sub("variables.name.startsWith\\(.br-sandbox-.\\)"; "true")) else . end)' "$WORK/ex.json" > "$WORK/fence_allcm.json"
    t "a policy that lists the status subresource of quotas refuses the controller on the used count" test "$(fence_verdict fence_status UPDATE resourcequotas/status br-sandbox-quota "$KUBE_USER" "$grants")" = BR_SANDBOX_FENCE
    t "a policy with no writer clause refuses a writer" test "$(fence_verdict fence_nowriter CREATE networkpolicies x "$WRITER_USER" "$grants")" = BR_SANDBOX_FENCE
    t "a policy that fences every ConfigMap refuses a ConfigMap of another name" test "$(fence_verdict fence_allcm CREATE configmaps app-config "$PROJECT_USER" "$grants")" = BR_SANDBOX_FENCE
}
