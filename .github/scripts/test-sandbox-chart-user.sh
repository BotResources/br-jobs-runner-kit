#!/usr/bin/env bash

RUNNER_USER='system:serviceaccount:runners:runner'
CHECK_USER='system:serviceaccount:sbx-system:br-sandbox-netcheck'

RUNAS='
def rule($vap): [$vap.spec.validations[] | select(.message | startswith("BR_SANDBOX_RUN_AS_NON_ROOT:")) | .expression] | .[0];
def has_clause($e; $text): $e | contains($text);
def runas($vap; $pod; $user):
  rule($vap) as $e
  | "(has(c.securityContext) && has(c.securityContext.runAsNonRoot) && c.securityContext.runAsNonRoot == true && has(c.securityContext.runAsUser) && c.securityContext.runAsUser > 0)" as $plain
  | ($e | [scan("c\\.name == \"([^\"]+)\" && c\\.image == \"([^\"]+)\"")] | .[0] // [null, null]) as $seed
  | ($e | [scan("request\\.userInfo\\.username == \"([^\"]+)\"")] | .[0][0]) as $check
  | has_clause($e; "!(has(object.spec.shareProcessNamespace) && object.spec.shareProcessNamespace)") as $shared_clause
  | has_clause($e; "c.securityContext.runAsNonRoot == true && has(c.securityContext.runAsUser) && c.securityContext.runAsUser > 0") as $user_clause
  | has_clause($e; "object.spec.ephemeralContainers.all(c,") as $ephemeral_clause
  | has_clause($e; "object.spec.initContainers.all(c, " + $plain + ")") as $init_plain
  | has_clause($e; "object.spec.ephemeralContainers.all(c, " + $plain + ")") as $ephemeral_plain
  | has_clause($e; "c.securityContext.runAsUser == 0") as $root_clause
  | has_clause($e; "c.securityContext.readOnlyRootFilesystem == true") as $readonly_clause
  | has_clause($e; "!has(c.ports)") as $ports_clause
  | def plain_ok: ($user_clause | not) or (.securityContext.runAsNonRoot == true and (.securityContext.runAsUser // 0) > 0);
    def seed_ok:
      .name == $seed[0] and .image == $seed[1]
      and (.securityContext.runAsUser == 0 or ($root_clause | not))
      and (.securityContext.readOnlyRootFilesystem == true or ($readonly_clause | not))
      and ((has("ports") | not) or ($ports_clause | not));
    def passes($plain_only): plain_ok or (($plain_only | not) and seed_ok);
    ((($pod.spec.shareProcessNamespace // false) | not) or ($shared_clause | not))
    and (($check != null and $user == $check)
      or (($pod.spec.containers | all(passes(false)))
        and (($pod.spec.initContainers // []) | all(passes($init_plain)))
        and ((if $ephemeral_clause then ($pod.spec.ephemeralContainers // []) else [] end) | all(passes($ephemeral_plain)))));
'

pool_pod() {
    jq -c --arg ns "$2" '.[] | select(.kind == "ConfigMap" and .metadata.name == "br-sandbox-pod-template" and .metadata.namespace == $ns) | (.data["pod-template.json"] | fromjson)' "$WORK/$1.json"
}

probe_pod() {
    yq -o=json -I0 "select(.kind == \"ConfigMap\" and .metadata.name == \"br-sandbox-netcheck\" and .metadata.namespace == \"sbx-system\") | .data[\"probe-$2.yaml\"] | from_yaml" "$WORK/$1_yaml.yaml"
}

runas_result() {
    local render="$1" pod="$2" user="$3" mutation="$4"
    jq -r --argjson pod "$pod" --arg user "$user" "${RUNAS}"'
      (.[] | select(.kind == "ValidatingAdmissionPolicy" and .metadata.name == "br-sandbox-pod")) as $vap
      | runas($vap; $pod | '"${mutation}"'; $user)' "$WORK/${render}.json"
}

pool_runas() {
    runas_result "$1" "$(pool_pod "$1" "$2")" "$RUNNER_USER" "$3"
}

probe_runas() {
    runas_result "$1" "$(probe_pod "$1" "$2")" "$3" "$4"
}

policy_expression() {
    jq -r '.[] | select(.kind == "ValidatingAdmissionPolicy" and .metadata.name == "br-sandbox-pod") | .spec.validations[] | select(.message | startswith("BR_SANDBOX_RUN_AS_NON_ROOT:")) | .expression' "$WORK/$1.json"
}

expression_holds() {
    [[ "$(policy_expression "$1")" == *"$2"* ]]
}

expression_holds_not() {
    [[ "$(policy_expression "$1")" != *"$2"* ]]
}

check_non_root_rule_accepts_the_pods_of_the_chart() {
    section "non-root rule: every pod of the chart passes"
    local ns
    for ns in sbx-alpha-allow sbx-alpha-full sbx-alpha-none sbx-beta-none; do
        t "the pool pod of ${ns} (agent, seed container, init container) passes" test "$(pool_runas ex "$ns" '.')" = true
        t "the probe pod of ${ns} passes for the identity of the network check" test "$(probe_runas ex "$ns" "$CHECK_USER" '.')" = true
    done
    t "the agent container alone passes" test "$(pool_runas ex sbx-alpha-none '.spec.containers = [.spec.containers[0]] | del(.spec.initContainers)')" = true
    t "the seed container alone passes" test "$(pool_runas ex sbx-alpha-none '.spec.containers = [.spec.containers[1]] | del(.spec.initContainers)')" = true
    t "the init container alone passes" test "$(pool_runas ex sbx-alpha-none '.spec.containers = [.spec.containers[0]] | .spec.initContainers = [.spec.initContainers[0]]')" = true
    t "the pool pod passes with a mirror, the seed image is the mirrored tools image" test "$(pool_runas mirror sbx-alpha-none '.')" = true
    t "the probe pod passes with a mirror" test "$(probe_runas mirror sbx-alpha-none "$CHECK_USER" '.')" = true
    t "the real seed container of the regular list passes next to a non-root init container" test "$(pool_runas ex sbx-alpha-none '.')" = true
    t "a process namespace set to false passes" test "$(pool_runas ex sbx-alpha-none '.spec.shareProcessNamespace = false')" = true
}

check_non_root_rule_refuses() {
    section "non-root rule: refusals"
    t "the agent container as uid 0 is refused" test "$(pool_runas ex sbx-alpha-none '.spec.containers[0].securityContext.runAsUser = 0')" = false
    t "the agent container without runAsNonRoot is refused" test "$(pool_runas ex sbx-alpha-none 'del(.spec.containers[0].securityContext.runAsNonRoot)')" = false
    t "the agent container with runAsNonRoot false is refused" test "$(pool_runas ex sbx-alpha-none '.spec.containers[0].securityContext.runAsNonRoot = false')" = false
    t "the agent container without runAsUser is refused" test "$(pool_runas ex sbx-alpha-none 'del(.spec.containers[0].securityContext.runAsUser)')" = false
    t "the agent container without a security context is refused" test "$(pool_runas ex sbx-alpha-none 'del(.spec.containers[0].securityContext)')" = false
    t "the init container as uid 0 is refused" test "$(pool_runas ex sbx-alpha-none '.spec.initContainers[0].securityContext.runAsUser = 0')" = false
    t "the init container without runAsNonRoot is refused" test "$(pool_runas ex sbx-alpha-none 'del(.spec.initContainers[0].securityContext.runAsNonRoot)')" = false
    t "the seed name with another image is refused" test "$(pool_runas ex sbx-alpha-none '.spec.containers[1].image = "registry.example.test/agents/base:1.0"')" = false
    t "the seed name with an image of another repository is refused" test "$(pool_runas ex sbx-alpha-none '.spec.containers[1].image = "other.example.test/x:1"')" = false
    t "the seed name with the tools repository and another digest is refused" test "$(pool_runas ex sbx-alpha-none '.spec.containers[1].image = "registry.example.test/tools/br-sandbox-tools@sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"')" = false
    t "the tools image under another name is refused" test "$(pool_runas ex sbx-alpha-none '.spec.containers[1].name = "writer"')" = false
    t "an init container with the seed name and the tools image that is not root is refused" test "$(pool_runas ex sbx-alpha-none '.spec.initContainers[0].name = "seed" | del(.spec.initContainers[0].securityContext.runAsNonRoot) | .spec.initContainers[0].securityContext.runAsUser = 5')" = false
    t "an init container with the seed name, the tools image, root, a read-only root file system and no port is refused" test "$(pool_runas ex sbx-alpha-none '.spec.initContainers += [.spec.containers[1]]')" = false
    t "the seed container with a writable root file system is refused" test "$(pool_runas ex sbx-alpha-none 'del(.spec.containers[1].securityContext.readOnlyRootFilesystem)')" = false
    t "the seed container with a port is refused" test "$(pool_runas ex sbx-alpha-none '.spec.containers[1].ports = [{"containerPort": 8080}]')" = false
    t "the seed container without runAsUser 0 and without runAsNonRoot is refused" test "$(pool_runas ex sbx-alpha-none 'del(.spec.containers[1].securityContext.runAsUser)')" = false
    t "a shared process namespace is refused" test "$(pool_runas ex sbx-alpha-none '.spec.shareProcessNamespace = true')" = false
    t "a probe pod made by the runner identity is refused" test "$(probe_runas ex sbx-alpha-none "$RUNNER_USER" '.')" = false
    t "a probe pod made by a ServiceAccount of the same name in another namespace is refused" test "$(probe_runas ex sbx-alpha-none 'system:serviceaccount:runners:br-sandbox-netcheck' '.')" = false
    t "a probe pod with a shared process namespace is refused for the network check identity" test "$(probe_runas ex sbx-alpha-none "$CHECK_USER" '.spec.shareProcessNamespace = true')" = false
    t "a probe pod without the identity of the check is refused although it carries the role label" test "$(probe_runas ex sbx-alpha-none '' '.')" = false
}

check_non_root_rule_text() {
    section "non-root rule: the text of the expression"
    local check_sa check_ns
    check_sa="$(jqv ex '.[] | select(.kind == "Job") | .spec.template.spec.serviceAccountName')"
    check_ns="$(jqv ex '.[] | select(.kind == "Job") | .metadata.namespace')"
    t "the exception names the ServiceAccount of the check Job in its namespace" expression_holds ex "request.userInfo.username == \"system:serviceaccount:${check_ns}:${check_sa}\" ||"
    t "the policy reads the user in no other rule" jqt ex '[.[] | select(.kind == "ValidatingAdmissionPolicy" and .metadata.name == "br-sandbox-pod") | .spec.validations[] | select(.message | startswith("BR_SANDBOX_RUN_AS_NON_ROOT:") | not) | .expression | select(test("userInfo"))] | length == 0'
    t "the rule reads the user once" test "$(policy_expression ex | grep -o 'userInfo' | wc -l | tr -d ' ')" = 1
    t "the seed exception image is the tools image of the seed container" expression_holds ex "c.image == \"$(jqv ex '.[] | select(.kind == "ConfigMap" and .metadata.name == "br-sandbox-pod-template" and .metadata.namespace == "sbx-alpha-none") | (.data["pod-template.json"] | fromjson).spec.containers[1].image')\""
    t "the seed exception image is the mirrored tools image with a mirror" expression_holds mirror "c.image == \"$(jqv mirror '.[] | select(.kind == "ConfigMap" and .metadata.name == "br-sandbox-pod-template" and .metadata.namespace == "sbx-alpha-none") | (.data["pod-template.json"] | fromjson).spec.containers[1].image')\""
    t "the mirrored tools image starts with the mirror" expression_holds mirror 'c.image == "mirror.example.test/'
    t "the seed exception pins root, the read-only root file system and no port" expression_holds ex 'c.securityContext.runAsUser == 0 && has(c.securityContext.readOnlyRootFilesystem) && c.securityContext.readOnlyRootFilesystem == true && !has(c.ports)'
    t "the rule refuses a shared process namespace" expression_holds ex '!(has(object.spec.shareProcessNamespace) && object.spec.shareProcessNamespace)'
    t "the seed exception applies to the regular container list only" expression_holds ex 'object.spec.containers.all(c, (has(c.securityContext) && has(c.securityContext.runAsNonRoot) && c.securityContext.runAsNonRoot == true && has(c.securityContext.runAsUser) && c.securityContext.runAsUser > 0) || (c.name == "seed"'
    t "the init list holds the plain non-root condition only" test "$(policy_expression ex | grep -o 'c.name ==' | wc -l | tr -d ' ')" = 1
    t "the rule reads the two container lists and not the ephemeral containers" expression_holds_not ex 'ephemeralContainers'
    t "the pods of the chart set runAsNonRoot true on the agent and the init container, not on the root containers" jqt ex "${SANDBOX_PODS} | all(.spec | (.containers[0].securityContext.runAsNonRoot == true) and (.initContainers[0].securityContext.runAsNonRoot == true) and (.containers[1:] | all(.securityContext.runAsUser == 0 and (.securityContext | has(\"runAsNonRoot\") | not))))"
    t "the probe pods run as uid 0, which is why the check identity is an exception" test "$(probe_pod ex sbx-alpha-none | jq -c '.spec.containers[0].securityContext | [.runAsUser, has("runAsNonRoot")]')" = '[0,false]'
    t "the policy watches the ephemeral containers and resize subresources on update" jqt ex '[.[] | select(.kind == "ValidatingAdmissionPolicy") | .spec.matchConstraints.resourceRules[1] | select(.resources == ["pods/ephemeralcontainers", "pods/resize"] and .operations == ["UPDATE"])] | length == 1'
    render nocheck -f "$EXAMPLE" --set network_check.enabled=false
    t "without the network check the rule has no user exception" expression_holds_not nocheck 'userInfo'
    t "without the network check the pods of the chart still pass" test "$(pool_runas nocheck sbx-alpha-none '.')" = true
    render noseed -f "$EXAMPLE" --set tools.seed_container.enabled=false
    t "without the seed container the rule has no seed exception" expression_holds_not noseed 'c.name =='
    t "without the seed container the pool pod passes" test "$(pool_runas noseed sbx-alpha-none '.')" = true
    t "without the seed container a seed named container is refused" test "$(pool_runas noseed sbx-alpha-none '.spec.containers += [.spec.initContainers[0] | .name = "seed" | .securityContext.runAsUser = 0 | .securityContext.readOnlyRootFilesystem = true | del(.securityContext.runAsNonRoot)]')" = false
    render custom -f "$EXAMPLE" --set tools.seed_container.name=skills-writer --set installation.system_namespace=custom-system
    t "the seed name and the system namespace are values of the exception" expression_holds custom 'c.name == "skills-writer" && c.image =='
    t "the user of the exception follows the system namespace" expression_holds custom 'request.userInfo.username == "system:serviceaccount:custom-system:br-sandbox-netcheck"'
    t "the pool pod passes with the custom seed name" test "$(pool_runas custom sbx-alpha-none '.')" = true
}

check_user_zero_render_fails() {
    section "a render with runAsUser 0 fails"
    fails_with "/runAsUser': minimum: got 0, want 1" -f "$EXAMPLE" --set runAsUser=0
    fails_with "/runAsGroup': minimum: got 0, want 1" -f "$EXAMPLE" --set runAsGroup=0
    t "the schema holds the minimum 1 for runAsUser" jq -e '.properties.runAsUser.minimum == 1' "$CHART/values.schema.json"
}

check_the_non_root_rule_gives_the_same_verdicts_without_its_ephemeral_branch() {
    section "non-root rule: the ephemeral branch changes no verdict of the policy"
    local branch='(!has(object.spec.ephemeralContainers) || object.spec.ephemeralContainers.all(c, (has(c.securityContext) && has(c.securityContext.runAsNonRoot) && c.securityContext.runAsNonRoot == true && has(c.securityContext.runAsUser) && c.securityContext.runAsUser > 0)))'
    local init='object.spec.initContainers.all(c, (has(c.securityContext) && has(c.securityContext.runAsNonRoot) && c.securityContext.runAsNonRoot == true && has(c.securityContext.runAsUser) && c.securityContext.runAsUser > 0))'
    jq --arg init "$init" --arg branch "$branch" '
      map(if .kind == "ValidatingAdmissionPolicy" then .spec.validations |= map(if (.message | startswith("BR_SANDBOX_RUN_AS_NON_ROOT:")) then .expression |= (split($init + ")") | join($init + ") && " + $branch)) else . end) else . end)' "$WORK/ex.json" > "$WORK/with_branch.json"
    t "the render with the branch put back holds the branch and the render of the chart holds none" test "$(jq -r '[.[] | select(.kind == "ValidatingAdmissionPolicy") | .spec.validations[] | select(.message | startswith("BR_SANDBOX_RUN_AS_NON_ROOT:")) | .expression | contains("ephemeralContainers")] | .[0]' "$WORK/with_branch.json")/$(jq -r '[.[] | select(.kind == "ValidatingAdmissionPolicy") | .spec.validations[] | select(.message | startswith("BR_SANDBOX_RUN_AS_NON_ROOT:")) | .expression | contains("ephemeralContainers")] | .[0]' "$WORK/ex.json")" = true/false
    local mutation refused_with refused_without
    for mutation in \
        '.spec.ephemeralContainers = [{"name": "debug", "image": "x"}]' \
        '.spec.ephemeralContainers = [{"name": "debug", "image": "x", "securityContext": {"runAsNonRoot": true, "runAsUser": 20000}}]' \
        '.spec.ephemeralContainers = [{"name": "debug", "image": "x", "securityContext": {"runAsNonRoot": true, "runAsUser": 0}}]' \
        '.spec.ephemeralContainers = [.spec.containers[1]]' \
        '.spec.ephemeralContainers = []' \
        'del(.spec.ephemeralContainers)'; do
        refused_with="$(refused_with_branch "$mutation")"
        refused_without="$(refused_without_branch "$mutation")"
        t "the verdict of the whole policy is the same with and without the ephemeral branch: ${mutation}" test "${refused_with}" = "${refused_without}"
    done
    t "every pod with one ephemeral container is refused with and without the branch" test "$(refused_without_branch '.spec.ephemeralContainers = [{"name": "debug", "image": "x", "securityContext": {"runAsNonRoot": true, "runAsUser": 20000}}]')/$(refused_with_branch '.spec.ephemeralContainers = [{"name": "debug", "image": "x", "securityContext": {"runAsNonRoot": true, "runAsUser": 20000}}]')" = true/true
}

refused_without_branch() {
    local pod verdict
    pod="$(pool_pod ex sbx-alpha-none | jq -c "$1")"
    verdict="$(created ex sbx-alpha-none "$1")"
    if [[ "$verdict" != admit || "$(runas_result ex "$pod" "$RUNNER_USER" '.')" == false ]]; then echo true; else echo false; fi
}

refused_with_branch() {
    local pod verdict
    pod="$(pool_pod ex sbx-alpha-none | jq -c "$1")"
    verdict="$(created ex sbx-alpha-none "$1")"
    if [[ "$verdict" != admit || "$(runas_result with_branch "$pod" "$RUNNER_USER" '.')" == false ]]; then echo true; else echo false; fi
}
