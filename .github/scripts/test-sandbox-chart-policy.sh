#!/usr/bin/env bash

VAP='.[] | select(.kind == "ValidatingAdmissionPolicy" and .metadata.name == "br-sandbox-pod")'
CODES='BR_SANDBOX_ROLE BR_SANDBOX_ROLE_IMMUTABLE BR_SANDBOX_USERNS_REQUIRED BR_SANDBOX_HOST_NAMESPACE BR_SANDBOX_VOLUME_FORBIDDEN BR_SANDBOX_CAPABILITY BR_SANDBOX_PRIVILEGE BR_SANDBOX_RESOURCES BR_SANDBOX_IMAGE BR_SANDBOX_TOKEN_OR_RUNTIME BR_SANDBOX_IMMUTABLE BR_SANDBOX_RUN_AS_NON_ROOT BR_SANDBOX_LABELS BR_SANDBOX_SERVICE_ACCOUNT BR_SANDBOX_EPHEMERAL_CONTAINERS'
BINDINGS='.[] | select(.kind == "ValidatingAdmissionPolicyBinding")'

has_rule_text() {
    jq -e --arg code "$1" --arg text "$2" '[.[] | select(.kind == "ValidatingAdmissionPolicy" and .metadata.name == "br-sandbox-pod") | .spec.validations[] | select(.message | startswith($code + ":")) | .expression | contains($text)] == [true]' "$WORK/ex.json" >/dev/null
}

check_VAP_every_rule_exists() {
    section "VAP: one policy, one binding by the managed label, every rule exists once"
    local code
    t "one pod policy br-sandbox-pod exists and no other pod policy" jqt ex '[.[] | select(.kind == "ValidatingAdmissionPolicy" and (.spec.matchConstraints.resourceRules[0].resources == ["pods"]))] | map(.metadata.name) == ["br-sandbox-pod"]'
    t "the binding br-sandbox-pod exists once, names the policy and selects the managed label and no name" jqt ex '[.[] | select(.kind == "ValidatingAdmissionPolicyBinding" and .metadata.name == "br-sandbox-pod")] | length == 1 and all(.spec.policyName == "br-sandbox-pod" and .spec.matchResources.namespaceSelector == {"matchLabels": {"br.sandbox/managed": "true"}})'
    t "no object of the render is named after a namespace with the suffix -pod" jqt ex '[.[] | select(.kind | startswith("ValidatingAdmission")) | .metadata.name | select(endswith("-pod") and (. != "br-sandbox-pod"))] | length == 0'
    for code in $CODES; do
        t "the policy has the rule ${code}" jqt ex "[${VAP} | [.spec.validations[].message | select(startswith(\"${code}:\"))] | length] == [1]"
    done
    t "the policy denies and fails closed" jqt ex "([${VAP} | select(.spec.failurePolicy == \"Fail\")] | length == 1) and ([${BINDINGS} | select(.spec.policyName == \"br-sandbox-pod\" and .spec.validationActions == [\"Deny\"])] | length == 2)"
    t "the policy watches create and update of pods" jqt ex "[${VAP} | .spec.matchConstraints.resourceRules[0] | select(.resources == [\"pods\"] and .operations == [\"CREATE\", \"UPDATE\"])] | length == 1"
    t "rule 1 requires hostUsers false" has_rule_text BR_SANDBOX_USERNS_REQUIRED "object.spec.hostUsers == false"
    t "rule 2 refuses host namespaces and host ports" has_rule_text BR_SANDBOX_HOST_NAMESPACE "hostNetwork"
    t "rule 3 allows emptyDir and configMap volumes, no secret" has_rule_text BR_SANDBOX_VOLUME_FORBIDDEN "has(v.emptyDir) || has(v.configMap)"
    t "rule 3 refuses secretKeyRef and secretRef" has_rule_text BR_SANDBOX_VOLUME_FORBIDDEN "!has(e.valueFrom.secretKeyRef)"
    t "rule 4 lists the seven capabilities" has_rule_text BR_SANDBOX_CAPABILITY "['CHOWN', 'DAC_OVERRIDE', 'FOWNER', 'FSETID', 'SETUID', 'SETGID', 'KILL']"
    t "rule 5 requires seccomp RuntimeDefault and no privilege" has_rule_text BR_SANDBOX_PRIVILEGE "seccompProfile.type == 'RuntimeDefault'"
    t "rule 6 inlines limits.size as the platform maximum" has_rule_text BR_SANDBOX_RESOURCES "quantity('500m')"
    t "rule 6 bounds the agent container by the size of the pool" has_rule_text BR_SANDBOX_RESOURCES "c.name != 'agent' || ((!has(params.spec.size) || !has(params.spec.size.cpu)"
    t "rule 7 inlines the allowed repositories of the other containers" has_rule_text BR_SANDBOX_IMAGE "'registry.example.test/tools/br-sandbox-tools'"
    t "rule 7 compares the agent image to the image of the pool" has_rule_text BR_SANDBOX_IMAGE "c.name == 'agent' ? (has(params.spec) && has(params.spec.image) && c.image == params.spec.image)"
    t "rule 8 requires no token and the chart runtime class" has_rule_text BR_SANDBOX_TOKEN_OR_RUNTIME "object.spec.runtimeClassName : '') == \"\""
    t "rule 9 compares containers, init containers, ephemeral containers, account and security fields on update" has_rule_text BR_SANDBOX_IMMUTABLE "(has(object.spec.containers) == has(oldObject.spec.containers) && (!has(object.spec.containers) || object.spec.containers == oldObject.spec.containers))"
    t "every isolation rule applies to role=sandbox pods only" jqt ex "[${VAP} | .spec.validations[] | select(.message | test(\"^BR_SANDBOX_(USERNS|HOST|VOLUME|CAPABILITY|PRIVILEGE|RESOURCES|IMAGE|TOKEN|IMMUTABLE|RUN_AS_NON_ROOT|SERVICE_ACCOUNT|EPHEMERAL)\")) | .expression | startswith(\"variables.role != 'sandbox' || \")] | all"
}

check_VAP_role_rules() {
    section "VAP role and patch rules: match, role, immutable role, patch rules"
    t "a pod with a role other than sandbox is refused: the role rule has one case only" has_rule_text BR_SANDBOX_ROLE "variables.role == 'sandbox'"
    t "the role rule is exactly the one case" jqt ex "[${VAP} | .spec.validations[] | select(.message | startswith(\"BR_SANDBOX_ROLE:\")) | .expression] | all(. == \"variables.role == 'sandbox'\")"
    t "the role rule names no proxy case and no creator" jqt ex "[${VAP} | .spec.validations[] | select(.message | startswith(\"BR_SANDBOX_ROLE:\")) | (.expression, .message) | select(test(\"proxy\"))] | length == 0"
    t "the pod policy has no proxy creator variable and only the non-root rule reads the user" jqt ex "[${VAP} | (.spec.variables[].name | select(. == \"proxyCreator\")), (.spec.validations[] | select(.message | startswith(\"BR_SANDBOX_RUN_AS_NON_ROOT:\") | not) | .expression | select(test(\"userInfo|proxyCreator\")))] | length == 0"
    t "the proxy controller value no longer exists" test "$(helm template t "$CHART" -f "$EXAMPLE" --set admission.proxy_controller_user=system:serviceaccount:example:other-controller 2>&1 | grep -c 'admission')" -ge 1
    t "the role cannot change on update" has_rule_text BR_SANDBOX_ROLE_IMMUTABLE "request.operation == 'CREATE' || variables.role == variables.oldRole"
    t "role and pool-id are immutable, other labels are frozen" has_rule_text BR_SANDBOX_LABELS "['br.sandbox/role', 'br.sandbox/pool-id'].all(k"
    t "a patch touches only br.sandbox/ keys" has_rule_text BR_SANDBOX_LABELS "k.startsWith('br.sandbox/') || (k in oldObject.metadata.labels"
    t "a claimed pod never leaves claimed" has_rule_text BR_SANDBOX_LABELS "oldObject.metadata.labels['br.sandbox/pool'] == 'claimed'"
    t "annotations outside br.sandbox/ are frozen" has_rule_text BR_SANDBOX_LABELS "oldObject.metadata.annotations.all(k, k.startsWith('br.sandbox/')"
}

EXPECTED="$SCRIPTS/test-sandbox-chart-policy-expected.json"

expression_of() {
    jq -r --arg code "$1" '.[] | select(.kind == "ValidatingAdmissionPolicy" and .metadata.name == "br-sandbox-pod") | .spec.validations[] | select(.message | startswith($code + ":")) | .expression' "$WORK/ex.json"
}

expected_of() {
    jq -r --arg code "$1" '.["br-sandbox-pod"][$code]' "$EXPECTED"
}

expression_is_exactly() {
    [[ "$(expression_of "$1")" == "$(expected_of "$1")" ]]
}

check_VAP_every_expression_is_the_exact_text() {
    section "VAP expressions: each full CEL text equals the expected text"
    local code
    for code in $CODES; do
        t "the expression of ${code} equals the expected text" expression_is_exactly "$code"
    done
    t "the policy holds exactly the fifteen rules and no other" jqt ex "[${VAP} | [.spec.validations[].message | split(\":\")[0]] | . == (\"${CODES}\" | split(\" \"))] | all"
    t "a pod with a role other than sandbox is refused: the role rule is the one case" expression_is_exactly BR_SANDBOX_ROLE
    t "the user-namespace rule has no alternative before the guard and the clause" expression_is_exactly BR_SANDBOX_USERNS_REQUIRED
    t "no expression holds a literal true or false operand of || or &&" jqt ex "[${VAP} | .spec.validations[].expression | select(test(\"(^|[(]|[|][|] |&& )(true|false) ([|][|]|&&)|([|][|]|&&) (true|false)([) ]|$)\"))] | length == 0"
    t "every isolation rule starts with the guard for role=sandbox pods; any other role is refused by the role rule" jqt ex "[${VAP} | .spec.validations[] | select(.message | test(\"^BR_SANDBOX_(USERNS|HOST|VOLUME|CAPABILITY|PRIVILEGE|RESOURCES|IMAGE|TOKEN|IMMUTABLE|RUN_AS_NON_ROOT|LABELS|SERVICE_ACCOUNT|EPHEMERAL)\")) | .expression | startswith(\"variables.role != 'sandbox' || \") and (. | split(\"variables.role != 'sandbox' || \") | length == 2)] | all"
}
