#!/usr/bin/env bash

PROJECT_USER='system:serviceaccount:gitops:project'
WRITER_USER='system:serviceaccount:gitops:platform'
NS_VAP='.[] | select(.kind == "ValidatingAdmissionPolicy" and .metadata.name == "br-sandbox-namespace")'
NS_BINDING='.[] | select(.kind == "ValidatingAdmissionPolicyBinding" and .metadata.name == "br-sandbox-namespace")'
GRANTS_CM='.[] | select(.kind == "ConfigMap" and .metadata.name == "br-sandbox-grants")'

check_namespace_policy_shape_and_grants() {
    section "namespace policy: a project cannot give itself a mode; the grants of the platform are a ConfigMap of the system namespace"
    t "the policy br-sandbox-namespace takes a ConfigMap as parameter, fails closed and matches the creation and the update of namespaces" jqt ex "[${NS_VAP} | select(.spec.paramKind == {\"apiVersion\": \"v1\", \"kind\": \"ConfigMap\"} and .spec.failurePolicy == \"Fail\" and .spec.matchConstraints.resourceRules == [{\"apiGroups\": [\"\"], \"apiVersions\": [\"v1\"], \"operations\": [\"CREATE\", \"UPDATE\"], \"resources\": [\"namespaces\"]}])] | length == 1"
    t "the policy has the four rules and no other" jqt ex "[${NS_VAP} | [.spec.validations[].message | split(\":\")[0]]] == [[\"BR_SANDBOX_MODE_UNKNOWN\", \"BR_SANDBOX_MODE_NOT_GRANTED\", \"BR_SANDBOX_LABEL_IMMUTABLE\", \"BR_SANDBOX_SCOPE\"]]"
    t "the binding names the ConfigMap br-sandbox-grants of the system namespace and denies when it is absent" jqt ex "[${NS_BINDING} | .spec.paramRef] == [{\"name\": \"br-sandbox-grants\", \"namespace\": \"sbx-system\", \"parameterNotFoundAction\": \"Deny\"}]"
    t "the binding selects the object by the managed label, so a request that adds or removes the label is matched by the old or the new object" jqt ex "[${NS_BINDING} | .spec.matchResources | select(.objectSelector == {\"matchLabels\": {\"br.sandbox/managed\": \"true\"}} and (has(\"namespaceSelector\") | not))] | length == 1"
    t "the binding matches namespaces, CREATE and UPDATE, and no subresource" jqt ex "[${NS_BINDING} | .spec.matchResources.resourceRules] == [[{\"apiGroups\": [\"\"], \"apiVersions\": [\"v1\"], \"operations\": [\"CREATE\", \"UPDATE\"], \"resources\": [\"namespaces\"]}]]"
    t "the binding denies" jqt ex "[${NS_BINDING} | .spec.validationActions] == [[\"Deny\"]]"
    t "the grants ConfigMap is in the system namespace and is a ConfigMap of the operator part" jqt ex "[${GRANTS_CM} | .metadata.namespace] == [\"sbx-system\"]"
    t "the grants of the default render hold no writer and no mode, only the scopes of the platform" jqt ex "[${GRANTS_CM} | .data] == [{\"writers\": \"\", \"scopes\": \"sbx-alpha,sbx-beta\"}]"
    t "no rule reads a field of the namespace other than its name and its labels" jqt ex "[${NS_VAP} | (.spec.validations[].expression, .spec.variables[].expression) | scan(\"(?:object|oldObject)\\\\.[a-zA-Z.]+\")] | map(select(test(\"^(object|oldObject)\\\\.metadata\\\\.(name|labels)\") | not)) | length == 0"
    t "the fence part renders no grants and no namespace policy" jqt fence '[.[] | select(.metadata.name == "br-sandbox-grants" or .metadata.name == "br-sandbox-namespace")] | length == 0'
    t "the grants render lists exactly the writers and the modes of the values, in order" jqt grants "[${GRANTS_CM} | .data] == [{\"writers\": \"system:serviceaccount:gitops:platform,system:serviceaccount:gitops:second\", \"scopes\": \"sbx-alpha,sbx-beta\", \"mode.sbx-alpha-new\": \"allow,full\", \"mode.sbx-beta-new\": \"allow\"}]"
    fails_with "at '/grants/modes/sbx-alpha-new/0': value must be one of 'allow', 'full'" -f "$EXAMPLE" --set 'grants.modes.sbx-alpha-new={none}'
    fails_with "at '/grants/writers/0': 'a,b' does not match pattern" -f "$EXAMPLE" --set 'grants.writers={a\,b}'
}

check_namespace_policy_create() {
    section "namespace policy model: creating a managed namespace"
    local grants none obj
    grants="$(grants_of grants)"
    none="$(grants_of ex)"
    t "a project creates a managed namespace in mode none under its scope and it is admitted" test "$(ns_verdict ex CREATE "$(namespace_object sbx-alpha-new true none)" null "$PROJECT_USER" "$none")" = admit
    t "a project asks the mode allow with no grant and is refused" test "$(ns_verdict ex CREATE "$(namespace_object sbx-alpha-new true allow)" null "$PROJECT_USER" "$none")" = BR_SANDBOX_MODE_NOT_GRANTED
    t "a project asks the mode full with no grant and is refused" test "$(ns_verdict ex CREATE "$(namespace_object sbx-alpha-new true full)" null "$PROJECT_USER" "$none")" = BR_SANDBOX_MODE_NOT_GRANTED
    t "a mode that is not none, allow or full is refused as unknown and as not granted" test "$(ns_verdict ex CREATE "$(namespace_object sbx-alpha-new true open)" null "$PROJECT_USER" "$none")" = BR_SANDBOX_MODE_NOT_GRANTED,BR_SANDBOX_MODE_UNKNOWN
    t "a managed namespace with no mode label is refused" test "$(ns_verdict ex CREATE "$(namespace_object sbx-alpha-new true '')" null "$PROJECT_USER" "$none")" = BR_SANDBOX_MODE_NOT_GRANTED,BR_SANDBOX_MODE_UNKNOWN
    t "the mode allow is admitted after the grant of the name" test "$(ns_verdict grants CREATE "$(namespace_object sbx-alpha-new true allow)" null "$PROJECT_USER" "$grants")" = admit
    t "the mode full is admitted after a grant that lists allow and full" test "$(ns_verdict grants CREATE "$(namespace_object sbx-alpha-new true full)" null "$PROJECT_USER" "$grants")" = admit
    t "the mode full is refused after a grant that lists allow only" test "$(ns_verdict grants CREATE "$(namespace_object sbx-beta-new true full)" null "$PROJECT_USER" "$grants")" = BR_SANDBOX_MODE_NOT_GRANTED
    t "the grant of a name is not the grant of another name" test "$(ns_verdict grants CREATE "$(namespace_object sbx-alpha-other true allow)" null "$PROJECT_USER" "$grants")" = BR_SANDBOX_MODE_NOT_GRANTED
    t "a user that is not in the writers of the grants is not a writer" test "$(ns_verdict grants CREATE "$(namespace_object sbx-alpha-new true full)" null "$WRITER_USER" "$none")" = BR_SANDBOX_MODE_NOT_GRANTED
    t "a writer of the grants creates a managed namespace in mode full" test "$(ns_verdict grants CREATE "$(namespace_object sbx-alpha-other true full)" null "$WRITER_USER" "$grants")" = admit
    t "the name of a writer is read in the whole list" test "$(ns_verdict grants CREATE "$(namespace_object sbx-alpha-other true full)" null 'system:serviceaccount:gitops:second' "$grants")" = admit
    t "a user whose name only starts with the name of a writer is not a writer" test "$(ns_verdict grants CREATE "$(namespace_object sbx-alpha-other true full)" null 'system:serviceaccount:gitops:platform2' "$grants")" = BR_SANDBOX_MODE_NOT_GRANTED
    t "a namespace that is not managed is not judged" test "$(ns_verdict ex CREATE "$(namespace_object other-thing '' full)" null "$PROJECT_USER" "$none")" = admit
    t "a namespace with the managed label false is not judged" test "$(ns_verdict ex CREATE "$(namespace_object other-thing false full)" null "$PROJECT_USER" "$none")" = admit
    t "with no grants ConfigMap every managed creation is refused" test "$(ns_verdict ex CREATE "$(namespace_object sbx-alpha-new true none)" null "$PROJECT_USER" null)" = PARAMETER_NOT_FOUND
    t "a name equal to a scope is admitted" test "$(ns_verdict ex CREATE "$(namespace_object sbx-alpha true none)" null "$PROJECT_USER" "$none")" = admit
    t "a name under a scope and a dash is admitted" test "$(ns_verdict ex CREATE "$(namespace_object sbx-beta-a-b true none)" null "$PROJECT_USER" "$none")" = admit
    t "a name outside every scope is refused" test "$(ns_verdict ex CREATE "$(namespace_object other-thing true none)" null "$PROJECT_USER" "$none")" = BR_SANDBOX_SCOPE
    t "a name that starts with a scope and no dash is refused" test "$(ns_verdict ex CREATE "$(namespace_object sbx-alphabet true none)" null "$PROJECT_USER" "$none")" = BR_SANDBOX_SCOPE
    t "a writer is refused for a name outside every scope" test "$(ns_verdict grants CREATE "$(namespace_object other-thing true full)" null "$WRITER_USER" "$grants")" = BR_SANDBOX_SCOPE
    obj="$(namespace_object sbx-alpha-new true none)"
    t "a render with no scope refuses every managed creation by the scope rule" test "$(ns_verdict ex CREATE "$obj" null "$PROJECT_USER" "$(jq -c '.data.scopes = ""' <<<"$none")")" = BR_SANDBOX_SCOPE
}

check_namespace_policy_update() {
    section "namespace policy model: updating a namespace"
    local grants none old
    grants="$(grants_of grants)"
    none="$(grants_of ex)"
    old="$(namespace_object sbx-alpha-new true none)"
    t "a project adds the label full to a managed namespace of mode none and is refused" test "$(ns_verdict ex UPDATE "$(namespace_object sbx-alpha-new true full)" "$old" "$PROJECT_USER" "$none")" = BR_SANDBOX_LABEL_IMMUTABLE
    t "a project cannot change the mode even to a mode that is granted" test "$(ns_verdict grants UPDATE "$(namespace_object sbx-alpha-new true allow)" "$old" "$PROJECT_USER" "$grants")" = BR_SANDBOX_LABEL_IMMUTABLE
    t "a project cannot remove the mode label" test "$(ns_verdict ex UPDATE "$(namespace_object sbx-alpha-new true '')" "$old" "$PROJECT_USER" "$none")" = BR_SANDBOX_LABEL_IMMUTABLE
    t "a project cannot remove the managed label, which would unbind the pod policy" test "$(ns_verdict ex UPDATE "$(namespace_object sbx-alpha-new '' none)" "$old" "$PROJECT_USER" "$none")" = BR_SANDBOX_LABEL_IMMUTABLE
    t "a project cannot set the managed label to another value" test "$(ns_verdict ex UPDATE "$(namespace_object sbx-alpha-new false none)" "$old" "$PROJECT_USER" "$none")" = BR_SANDBOX_LABEL_IMMUTABLE
    t "a project cannot add the managed label with the mode full to a namespace that has none" test "$(ns_verdict ex UPDATE "$(namespace_object sbx-alpha-new true full)" "$(namespace_object sbx-alpha-new '' full)" "$PROJECT_USER" "$none")" = BR_SANDBOX_LABEL_IMMUTABLE
    t "a project cannot add the managed label to a namespace that has none, whatever the mode" test "$(ns_verdict ex UPDATE "$old" "$(namespace_object sbx-alpha-new '' '')" "$PROJECT_USER" "$none")" = BR_SANDBOX_LABEL_IMMUTABLE
    t "a writer changes the mode of a managed namespace" test "$(ns_verdict grants UPDATE "$(namespace_object sbx-alpha-new true full)" "$old" "$WRITER_USER" "$grants")" = admit
    t "a project changes another label of a managed namespace and is admitted" test "$(ns_verdict ex UPDATE "$(jq -c '.metadata.labels.team = "x"' <<<"$old")" "$old" "$PROJECT_USER" "$none")" = admit
    t "an update that leaves both labels as they are is admitted although the mode was never granted" test "$(ns_verdict ex UPDATE "$(namespace_object sbx-alpha-new true full)" "$(namespace_object sbx-alpha-new true full)" "$PROJECT_USER" "$none")" = admit
    t "an update of a namespace that was never managed and stays so is not judged" test "$(ns_verdict ex UPDATE "$(namespace_object other-thing '' full)" "$(namespace_object other-thing '' none)" "$PROJECT_USER" "$none")" = admit
    t "with no grants ConfigMap every managed update is refused" test "$(ns_verdict ex UPDATE "$old" "$old" "$PROJECT_USER" null)" = PARAMETER_NOT_FOUND
}

check_namespace_policy_model_reads_the_text() {
    section "namespace policy model: a mutated render changes the verdict, so the model reads the render"
    local none old
    none="$(grants_of ex)"
    old="$(namespace_object sbx-alpha-new true none)"
    jq 'map(if .kind == "ValidatingAdmissionPolicy" and .metadata.name == "br-sandbox-namespace" then .spec.validations |= map(if (.message | startswith("BR_SANDBOX_LABEL_IMMUTABLE:")) then .expression |= sub("variables.writer \\|\\| "; "") else . end) else . end)' "$WORK/ex.json" > "$WORK/ns_nowriter.json"
    jq 'map(if .kind == "ValidatingAdmissionPolicy" and .metadata.name == "br-sandbox-namespace" then .spec.validations |= map(if (.message | startswith("BR_SANDBOX_MODE_NOT_GRANTED:")) then .expression |= sub("variables.mode == .none. \\|\\| "; "") else . end) else . end)' "$WORK/ex.json" > "$WORK/ns_noopen.json"
    jq 'map(if .kind == "ValidatingAdmissionPolicyBinding" and .metadata.name == "br-sandbox-namespace" then .spec.matchResources.resourceRules[0].operations = ["CREATE"] else . end)' "$WORK/ex.json" > "$WORK/ns_createonly.json"
    jq 'map(if .kind == "ValidatingAdmissionPolicyBinding" and .metadata.name == "br-sandbox-namespace" then .spec.paramRef.parameterNotFoundAction = "Allow" else . end)' "$WORK/ex.json" > "$WORK/ns_allow.json"
    t "without the writer clause a writer is refused on a label change" test "$(ns_verdict ns_nowriter UPDATE "$(namespace_object sbx-alpha-new true full)" "$old" "$WRITER_USER" "$(grants_of grants)")" = BR_SANDBOX_LABEL_IMMUTABLE
    t "without the clause for the mode none a project is refused the mode none" test "$(ns_verdict ns_noopen CREATE "$old" null "$PROJECT_USER" "$none")" = BR_SANDBOX_MODE_NOT_GRANTED
    t "a binding that matches the creation only leaves an update to a project" test "$(ns_verdict ns_createonly UPDATE "$(namespace_object sbx-alpha-new true full)" "$old" "$PROJECT_USER" "$none")" = admit
    t "a binding that allows a missing parameter admits every request when the grants are absent" test "$(ns_verdict ns_allow CREATE "$(namespace_object sbx-alpha-new true full)" null "$PROJECT_USER" null)" = admit
}
