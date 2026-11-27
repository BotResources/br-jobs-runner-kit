#!/usr/bin/env bash

VAP='.[] | select(.kind == "ValidatingAdmissionPolicy")'
CODES='BR_SANDBOX_ROLE BR_SANDBOX_ROLE_IMMUTABLE BR_SANDBOX_USERNS_REQUIRED BR_SANDBOX_HOST_NAMESPACE BR_SANDBOX_VOLUME_FORBIDDEN BR_SANDBOX_CAPABILITY BR_SANDBOX_PRIVILEGE BR_SANDBOX_RESOURCES BR_SANDBOX_IMAGE BR_SANDBOX_TOKEN_OR_RUNTIME BR_SANDBOX_IMMUTABLE BR_SANDBOX_RUN_AS_NON_ROOT BR_SANDBOX_LABELS'

rule() {
    printf '(%s) | .spec.validations[] | select(.message | startswith("%s:")) | .expression' "$1" "$2"
}

has_rule_text() {
    jq -e --arg ns "$1" --arg code "$2" --arg text "$3" '[.[] | select(.kind == "ValidatingAdmissionPolicy" and .metadata.name == ($ns + "-pod")) | .spec.validations[] | select(.message | startswith($code + ":")) | .expression | contains($text)] == [true]' "$WORK/ex.json" >/dev/null
}

check_VAP_every_rule_exists_per_namespace() {
    section "VAP rules 1 to 10, the role rules and the label rules exist for every namespace"
    local code
    t "one policy and one binding per namespace" jqt ex '([.[] | select(.kind == "ValidatingAdmissionPolicy")] | length) == 4 and ([.[] | select(.kind == "ValidatingAdmissionPolicyBinding")] | length) == 4'
    for code in $CODES; do
        t "each policy has the rule ${code}" jqt ex "[${VAP} | [.spec.validations[].message | select(startswith(\"${code}:\"))] | length] | all(. == 1)"
    done
    t "the policy denies and fails closed" jqt ex ". as \$all | ([${VAP} | select(.spec.failurePolicy == \"Fail\")] | length == 4) and ([\$all[] | select(.kind == \"ValidatingAdmissionPolicyBinding\") | select(.spec.validationActions == [\"Deny\"])] | length == 4)"
    t "the policy watches create and update of pods" jqt ex "[${VAP} | .spec.matchConstraints.resourceRules[0] | select(.resources == [\"pods\"] and .operations == [\"CREATE\", \"UPDATE\"])] | length == 4"
    t "rule 1 requires hostUsers false" has_rule_text sbx-alpha-none BR_SANDBOX_USERNS_REQUIRED "object.spec.hostUsers == false"
    t "rule 2 refuses host namespaces and host ports" has_rule_text sbx-alpha-none BR_SANDBOX_HOST_NAMESPACE "hostNetwork"
    t "rule 3 allows emptyDir and configMap volumes, no secret" has_rule_text sbx-alpha-none BR_SANDBOX_VOLUME_FORBIDDEN "has(v.emptyDir) || has(v.configMap)"
    t "rule 3 refuses secretKeyRef and secretRef" has_rule_text sbx-alpha-none BR_SANDBOX_VOLUME_FORBIDDEN "!has(e.valueFrom.secretKeyRef)"
    t "rule 4 lists the seven capabilities" has_rule_text sbx-alpha-none BR_SANDBOX_CAPABILITY "['CHOWN', 'DAC_OVERRIDE', 'FOWNER', 'FSETID', 'SETUID', 'SETGID', 'KILL']"
    t "rule 5 requires seccomp RuntimeDefault and no privilege" has_rule_text sbx-alpha-none BR_SANDBOX_PRIVILEGE "seccompProfile.type == 'RuntimeDefault'"
    t "rule 6 inlines the size of the pool of the namespace" has_rule_text sbx-alpha-none BR_SANDBOX_RESOURCES "quantity('500m')"
    t "rule 6 inlines another size for another pool" has_rule_text sbx-alpha-full BR_SANDBOX_RESOURCES "quantity('10Gi')"
    t "rule 7 inlines the allowed repositories" has_rule_text sbx-alpha-none BR_SANDBOX_IMAGE "'registry.example.test/agents/base'"
    t "rule 8 requires no token and the chart runtime class" has_rule_text sbx-alpha-none BR_SANDBOX_TOKEN_OR_RUNTIME "object.spec.runtimeClassName : '') == \"\""
    t "rule 9 compares security contexts and resources on update" has_rule_text sbx-alpha-none BR_SANDBOX_IMMUTABLE "oldObject.spec.containers.map(c, c.securityContext)"
    t "every isolation rule applies to role=sandbox pods only" jqt ex "[${VAP} | .spec.validations[] | select(.message | test(\"^BR_SANDBOX_(USERNS|HOST|VOLUME|CAPABILITY|PRIVILEGE|RESOURCES|IMAGE|TOKEN|IMMUTABLE|RUN_AS_NON_ROOT)\")) | .expression | startswith(\"variables.role != 'sandbox' || \")] | all"
}

check_VAP_role_rules() {
    section "VAP role and patch rules: match, role, immutable role, patch rules"
    t "the binding matches the managed label and the namespace name" jqt ex '[.[] | select(.kind == "ValidatingAdmissionPolicyBinding") | .spec.matchResources.namespaceSelector.matchLabels | select(.["br.sandbox/managed"] == "true" and (.["kubernetes.io/metadata.name"] | startswith("sbx-")))] | length == 4'
    t "a pod with a role other than sandbox is refused: the role rule has one case only" has_rule_text sbx-alpha-none BR_SANDBOX_ROLE "variables.role == 'sandbox'"
    t "the role rule is exactly the one case" jqt ex "[${VAP} | .spec.validations[] | select(.message | startswith(\"BR_SANDBOX_ROLE:\")) | .expression] | all(. == \"variables.role == 'sandbox'\")"
    t "the role rule names no proxy case and no creator" jqt ex "[${VAP} | .spec.validations[] | select(.message | startswith(\"BR_SANDBOX_ROLE:\")) | (.expression, .message) | select(test(\"proxy\"))] | length == 0"
    t "the policy has no proxy creator variable and only the non-root rule reads the user" jqt ex '[.[] | select(.kind == "ValidatingAdmissionPolicy") | (.spec.variables[].name | select(. == "proxyCreator")), (.spec.validations[] | select(.message | startswith("BR_SANDBOX_RUN_AS_NON_ROOT:") | not) | .expression | select(test("userInfo|proxyCreator")))] | length == 0'
    t "the proxy controller value no longer exists" test "$(helm template t "$CHART" -f "$EXAMPLE" --set admission.proxy_controller_user=system:serviceaccount:example:other-controller 2>&1 | grep -c 'admission')" -ge 1
    t "the role cannot change on update" has_rule_text sbx-alpha-none BR_SANDBOX_ROLE_IMMUTABLE "request.operation == 'CREATE' || variables.role == variables.oldRole"
    t "role and pool-id are immutable, other labels are frozen" has_rule_text sbx-alpha-none BR_SANDBOX_LABELS "['br.sandbox/role', 'br.sandbox/pool-id'].all(k"
    t "a patch touches only br.sandbox/ keys" has_rule_text sbx-alpha-none BR_SANDBOX_LABELS "k.startsWith('br.sandbox/') || (k in oldObject.metadata.labels"
    t "a claimed pod never leaves claimed" has_rule_text sbx-alpha-none BR_SANDBOX_LABELS "oldObject.metadata.labels['br.sandbox/pool'] == 'claimed'"
    t "annotations outside br.sandbox/ are frozen" has_rule_text sbx-alpha-none BR_SANDBOX_LABELS "oldObject.metadata.annotations.all(k, k.startsWith('br.sandbox/')"
}

MODEL='
def bounds($e): [$e | scan("quantity\\(c\\.resources\\.(requests|limits)\\[.([a-z-]+).\\]\\)\\.isGreaterThan\\(quantity\\(.([^'"'"']+).\\)\\)")] | map({key: (.[0] + "/" + .[1]), value: (.[2] | qty)}) | from_entries;
def list($e; $re): [$e | capture($re).l | split(", ")[] | gsub("'"'"'"; "")];
def passes($vap; $pod):
  ([$vap.spec.validations[] | select(.message | startswith("BR_SANDBOX_CAPABILITY:")) | .expression] | .[0]) as $capExpr
  | ([$vap.spec.validations[] | select(.message | startswith("BR_SANDBOX_IMAGE:")) | .expression] | .[0]) as $imgExpr
  | ([$vap.spec.validations[] | select(.message | startswith("BR_SANDBOX_RESOURCES:")) | .expression] | .[0] | bounds(.)) as $b
  | list($capExpr; "a in \\[(?<l>[^\\]]*)\\]") as $caps
  | ($imgExpr | capture("c, \\[(?<l>[^\\]]*)\\]\\.exists").l | split(", ") | map(gsub("'"'"'"; ""))) as $repos
  | ($pod.spec.hostUsers == false)
    and ($pod.spec.automountServiceAccountToken == false)
    and ($pod.spec.securityContext.seccompProfile.type == "RuntimeDefault")
    and (($pod.spec.volumes // []) | all(has("emptyDir") or has("configMap")))
    and ((($pod.spec.initContainers // []) + $pod.spec.containers) | all(
      (.securityContext.capabilities.drop | index("ALL") != null)
      and ((.securityContext.capabilities.add // []) - $caps == [])
      and (.securityContext.allowPrivilegeEscalation == false)
      and ((.securityContext.privileged // false) | not)
      and ((.securityContext.seccompProfile.type // "RuntimeDefault") == "RuntimeDefault")
      and (has("ports") | not)
      and ((.env // []) | all(.valueFrom.secretKeyRef == null))
      and (.image as $i | $repos | any(. as $r | ($i | startswith($r + ":")) or ($i | startswith($r + "@"))))
      and (. as $c | ["requests", "limits"] | all(. as $k | ["cpu", "memory", "ephemeral-storage"] | all(. as $r | ($c.resources[$k][$r] != null) and (($c.resources[$k][$r] | qty) <= $b[$k + "/" + $r])))))) ;
'

model_result() {
    local ns="$1" mutation="$2"
    jq -r --arg ns "$ns" "${QTY}${MODEL}"'
      (.[] | select(.kind == "ValidatingAdmissionPolicy" and .metadata.name == ($ns + "-pod"))) as $vap
      | (.[] | select(.kind == "ReplicaSet" and .metadata.namespace == $ns) | .spec.template) as $pod
      | passes($vap; $pod | '"${mutation}"')' "$WORK/ex.json"
}

check_VAP_model_of_the_pods() {
    section "VAP model: the rendered pods pass the constants of their own policy, a broken pod does not"
    local ns
    for ns in sbx-alpha-allow sbx-alpha-full sbx-alpha-none sbx-beta-none; do
        t "the pool pod of ${ns} passes the model of its policy, seed container and init container included" test "$(model_result "$ns" '.')" = true
    done
    t "the model refuses an extra capability" test "$(model_result sbx-alpha-none '.spec.containers[0].securityContext.capabilities.add += ["SYS_ADMIN"]')" = false
    t "the model refuses an image of another repository" test "$(model_result sbx-alpha-none '.spec.containers[0].image = "other.example.test/x:1"')" = false
    t "the model refuses a container without limits" test "$(model_result sbx-alpha-none 'del(.spec.containers[0].resources.limits)')" = false
    t "the model refuses a limit above the size" test "$(model_result sbx-alpha-none '.spec.containers[0].resources.limits.memory = "3Gi"')" = false
    t "the model refuses a secret volume" test "$(model_result sbx-alpha-none '.spec.volumes += [{"name": "s", "secret": {"secretName": "x"}}]')" = false
    t "the model refuses a secretKeyRef" test "$(model_result sbx-alpha-none '.spec.containers[0].env += [{"name": "A", "valueFrom": {"secretKeyRef": {"name": "x", "key": "y"}}}]')" = false
    t "the model refuses a container with seccomp Unconfined" test "$(model_result sbx-alpha-none '.spec.containers[0].securityContext.seccompProfile = {"type": "Unconfined"}')" = false
    t "the model refuses a container with a Localhost seccomp profile" test "$(model_result sbx-alpha-none '.spec.containers[1].securityContext.seccompProfile = {"type": "Localhost", "localhostProfile": "p.json"}')" = false
    t "the model refuses an init container with seccomp Unconfined" test "$(model_result sbx-alpha-none '.spec.initContainers[0].securityContext.seccompProfile = {"type": "Unconfined"}')" = false
    t "the model accepts a container with seccomp RuntimeDefault" test "$(model_result sbx-alpha-none '.spec.containers[0].securityContext.seccompProfile = {"type": "RuntimeDefault"}')" = true
    t "the model refuses hostUsers true" test "$(model_result sbx-alpha-none '.spec.hostUsers = true')" = false
    t "the model refuses a seed container without the drop list" test "$(model_result sbx-alpha-none 'del(.spec.containers[1].securityContext.capabilities.drop)')" = false
}

EXPECTED="$SCRIPTS/test-sandbox-chart-policy-expected.json"

expression_of() {
    jq -r --arg ns "$1" --arg code "$2" '.[] | select(.kind == "ValidatingAdmissionPolicy" and .metadata.name == ($ns + "-pod")) | .spec.validations[] | select(.message | startswith($code + ":")) | .expression' "$WORK/ex.json"
}

expected_of() {
    jq -r --arg ns "$1" --arg code "$2" '.[$ns][$code]' "$EXPECTED"
}

expression_is_exactly() {
    [[ "$(expression_of "$1" "$2")" == "$(expected_of "$1" "$2")" ]]
}

check_VAP_every_expression_is_the_exact_text() {
    section "VAP expressions: each full CEL text equals the expected text"
    local code
    for code in $CODES; do
        t "the expression of ${code} equals the expected text" expression_is_exactly sbx-alpha-none "$code"
    done
    t "the policy holds exactly the thirteen rules and no other" jqt ex "[${VAP} | select(.metadata.name == \"sbx-alpha-none-pod\") | [.spec.validations[].message | split(\":\")[0]] | . == (\"${CODES}\" | split(\" \"))] | all"
    t "the size rule of a pool with another size equals the expected text" expression_is_exactly sbx-alpha-full BR_SANDBOX_RESOURCES
    t "the image rule of the allow namespace equals the expected text" expression_is_exactly sbx-alpha-allow BR_SANDBOX_IMAGE
    t "a pod with a role other than sandbox is refused: the role rule is the one case" expression_is_exactly sbx-alpha-none BR_SANDBOX_ROLE
    t "the user-namespace rule has no alternative before the guard and the clause" expression_is_exactly sbx-alpha-none BR_SANDBOX_USERNS_REQUIRED
    t "no expression holds a literal true or false operand of || or &&" jqt ex "[${VAP} | .spec.validations[].expression | select(test(\"(^|[(]|[|][|] |&& )(true|false) ([|][|]|&&)|([|][|]|&&) (true|false)([) ]|$)\"))] | length == 0"
    t "every isolation rule starts with the guard for role=sandbox pods; any other role is refused by the role rule" jqt ex "[${VAP} | .spec.validations[] | select(.message | test(\"^BR_SANDBOX_(USERNS|HOST|VOLUME|CAPABILITY|PRIVILEGE|RESOURCES|IMAGE|TOKEN|IMMUTABLE|RUN_AS_NON_ROOT|LABELS)\")) | .expression | startswith(\"variables.role != 'sandbox' || \") and (. | split(\"variables.role != 'sandbox' || \") | length == 2)] | all"
}
