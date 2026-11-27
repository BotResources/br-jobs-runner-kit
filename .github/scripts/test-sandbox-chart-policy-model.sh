#!/usr/bin/env bash

POOL_IMAGE='registry.example.test/agents/base:1.0'
POOL_SIZE='{"cpu": {"request": "250m", "limit": "1"}, "memory": {"request": "512Mi", "limit": "1Gi"}, "disk": {"request": "1Gi", "limit": "2Gi"}}'
POOL_RESOURCES='{"requests": {"cpu": "250m", "memory": "512Mi", "ephemeral-storage": "1Gi"}, "limits": {"cpu": "1", "memory": "1Gi", "ephemeral-storage": "2Gi"}}'

MODEL='
def q: [39] | implode;
def txt($vap; $code): [$vap.spec.validations[] | select(.message | startswith($code + ":")) | .expression] | .[0] // "";
def rule_matches($rules; $op; $res): any($rules[]?; ((.operations | index($op)) != null) and ((.resources | index($res)) != null));
def guard: "request.operation != " + q + "CREATE" + q + " ||";
def guarded($e): ($e | contains(guard));
def unguarded_params($e): ($e | contains("params.")) and ((guarded($e) | not) or (($e | index("params.")) < ($e | index(guard))));
def platform_bounds($e): [$e | scan("quantity\\(c\\.resources\\.(requests|limits)\\[" + q + "([a-z-]+)" + q + "\\]\\)\\.isGreaterThan\\(quantity\\(" + q + "([^" + q + "]+)" + q + "\\)\\)")] | map({key: (.[0] + "/" + .[1]), value: (.[2] | qty)}) | from_entries;
def pool_bounds($e): [$e | scan("quantity\\(c\\.resources\\.(requests|limits)\\[" + q + "([a-z-]+)" + q + "\\]\\)\\.isGreaterThan\\(quantity\\(params\\.spec\\.size\\.(cpu|memory|disk)\\.(request|limit)\\)\\)")] | map({kind: .[0], res: .[1], size: .[2], bound: .[3]});
def quoted_list($e; $re): [first($e | capture($re).l) | split(", ")[] | gsub("" + q + ""; "")];
def under($c; $kind; $res; $bound): (($c.resources[$kind][$res] // null) != null) and (($c.resources[$kind][$res] | qty) <= $bound);

def failing($vap; $r; $params):
  $r.pod as $pod
  | (($pod.metadata.labels["br.sandbox/role"] // "") == "sandbox") as $sandbox
  | ((($pod.spec.initContainers // []) + $pod.spec.containers)) as $all
  | txt($vap; "BR_SANDBOX_CAPABILITY") as $capE
  | txt($vap; "BR_SANDBOX_IMAGE") as $imgE
  | txt($vap; "BR_SANDBOX_RESOURCES") as $resE
  | txt($vap; "BR_SANDBOX_SERVICE_ACCOUNT") as $saE
  | txt($vap; "BR_SANDBOX_EPHEMERAL_CONTAINERS") as $ephE
  | txt($vap; "BR_SANDBOX_IMMUTABLE") as $immE
  | quoted_list($capE; "a in \\[(?<l>[^\\]]*)\\]") as $caps
  | quoted_list($imgE; "\\[(?<l>[^\\]]*)\\]\\.exists") as $repos
  | ([$imgE | scan("c\\.name == " + q + "([^" + q + "]+)" + q + " \\?") | .[0]] | .[0]) as $agent
  | ($imgE | contains("c.image == params.spec.image")) as $poolImage
  | platform_bounds($resE) as $pb
  | pool_bounds($resE) as $ob
  | ([$saE | scan("serviceAccountName == " + q + "([^" + q + "]+)" + q + "") | .[0]] | .[0]) as $account
  | [$immE | scan("has\\(object\\.spec\\.([A-Za-z]+)\\) == has\\(oldObject\\.spec\\.[A-Za-z]+\\)") | .[0]] as $frozen
  | ($r.op == "CREATE") as $create
  | [
      (if $sandbox | not then "BR_SANDBOX_ROLE" else empty end),
      (if $sandbox and ($pod.spec.hostUsers != false) then "BR_SANDBOX_USERNS_REQUIRED" else empty end),
      (if $sandbox and ($pod.spec.automountServiceAccountToken != false) then "BR_SANDBOX_TOKEN_OR_RUNTIME" else empty end),
      (if $sandbox and (($pod.spec.securityContext.seccompProfile.type != "RuntimeDefault") or ($all | any((.securityContext.allowPrivilegeEscalation != false) or (.securityContext.privileged // false) or ((.securityContext.seccompProfile.type // "RuntimeDefault") != "RuntimeDefault")))) then "BR_SANDBOX_PRIVILEGE" else empty end),
      (if $sandbox and ((($pod.spec.volumes // []) | any(has("emptyDir") or has("configMap") | not)) or ($all | any((.env // []) | any(.valueFrom.secretKeyRef != null)))) then "BR_SANDBOX_VOLUME_FORBIDDEN" else empty end),
      (if $sandbox and ($all | any(((.securityContext.capabilities.drop // []) | index("ALL")) == null or (((.securityContext.capabilities.add // []) - $caps) != []))) then "BR_SANDBOX_CAPABILITY" else empty end),
      (if $sandbox and ($all | any(has("ports"))) then "BR_SANDBOX_HOST_NAMESPACE" else empty end),
      (if $sandbox and (($create | not) and unguarded_params($imgE)) then "BR_SANDBOX_IMAGE"
       elif $sandbox and ($create or (guarded($imgE) | not)) and ($all | any(
           . as $c
           | (if $poolImage and $c.name == $agent then (($params != null) and (($params.spec.image // null) != null) and ($c.image == $params.spec.image))
              else ($repos | any(. as $p | ($c.image | startswith($p + ":")) or ($c.image | startswith($p + "@")))) end) | not)) then "BR_SANDBOX_IMAGE" else empty end),
      (if $sandbox and (
           ($all | any(. as $c | ($pb | to_entries | any(. as $e | ($e.key | split("/")) as $k | under($c; $k[0]; $k[1]; $e.value) | not))))
           or (($create | not) and unguarded_params($resE))
           or (($create or (guarded($resE) | not)) and ($params != null) and ($all | any(. as $c | $c.name == $agent and ($ob | any(. as $o | ($params.spec.size[$o.size][$o.bound] // null) as $limit | ($limit != null) and (under($c; $o.kind; $o.res; ($limit | qty)) | not))))))
         ) then "BR_SANDBOX_RESOURCES" else empty end),
      (if $sandbox and ($account != null) and (($pod.spec.serviceAccountName // null) != $account) then "BR_SANDBOX_SERVICE_ACCOUNT" else empty end),
      (if $sandbox and ($ephE | contains("ephemeralContainers.size() == 0")) and ((($pod.spec.ephemeralContainers // []) | length) > 0) then "BR_SANDBOX_EPHEMERAL_CONTAINERS" else empty end),
      (if $sandbox and ($create | not) and ($frozen | any(. as $f | ($pod.spec[$f]) != ($r.old.spec[$f]))) then "BR_SANDBOX_IMMUTABLE" else empty end)
    ] | unique;

def decide($vap; $bindings; $r):
  [$bindings[] | select(.spec.policyName == $vap.metadata.name)
    | select(rule_matches($vap.spec.matchConstraints.resourceRules; $r.op; $r.res) and rule_matches(.spec.matchResources.resourceRules; $r.op; $r.res))] as $bound
  | [$bound[] | if (.spec | has("paramRef")) then
        (if ($r.pools | length) == 0 then (if .spec.paramRef.parameterNotFoundAction == "Allow" or $vap.spec.failurePolicy != "Fail" then [] else ["PARAMETER_NOT_FOUND"] end)
         else ([$r.pools[] | failing($vap; $r; .)] | add // []) end)
      else failing($vap; $r; null) end] | add // [] | unique
  | if length == 0 then "admit" else join(",") end;
'

verdict() {
    local render="$1" op="$2" res="$3" pod="$4" old="$5" pools="$6"
    jq -r --arg op "$op" --arg res "$res" --argjson pod "$pod" --argjson old "$old" --argjson pools "$pools" "${QTY}${MODEL}"'
      (.[] | select(.kind == "ValidatingAdmissionPolicy" and .metadata.name == "br-sandbox-pod")) as $vap
      | [.[] | select(.kind == "ValidatingAdmissionPolicyBinding")] as $bindings
      | decide($vap; $bindings; {op: $op, res: $res, pod: $pod, old: $old, pools: $pools})' "$WORK/${render}.json"
}

pool_of() {
    jq -nc --arg ns "$1" --arg image "$POOL_IMAGE" --argjson size "$POOL_SIZE" '{metadata: {namespace: $ns, name: "pool"}, spec: {image: $image, size: $size}}'
}

keeper_pod() {
    local render="$1" ns="$2" mutation="${3:-.}"
    jq -c --arg ns "$ns" --arg image "$POOL_IMAGE" --argjson resources "$POOL_RESOURCES" '
      .[] | select(.kind == "ConfigMap" and .metadata.name == "br-sandbox-pod-template" and .metadata.namespace == $ns) | (.data["pod-template.json"] | fromjson)
      | (.spec.containers[] | select(.name == "agent")) |= (.image = $image | .resources = $resources)
      | def agent: .spec.containers[] | select(.name == "agent");
      '"${mutation}" "$WORK/${render}.json"
}

created() {
    local render="$1" ns="$2" mutation="${3:-.}" pools="${4:-}"
    [[ -n "$pools" ]] || pools="[$(pool_of "$ns")]"
    verdict "$render" CREATE pods "$(keeper_pod "$render" "$ns" "$mutation")" null "$pools"
}

updated() {
    local render="$1" ns="$2" mutation="$3" pools="${4:-}" res="${5:-pods}"
    [[ -n "$pools" ]] || pools="[$(pool_of "$ns")]"
    local old
    old="$(keeper_pod "$render" "$ns")"
    verdict "$render" UPDATE "$res" "$(jq -c "def agent: .spec.containers[] | select(.name == \"agent\"); ${mutation}" <<<"$old")" "$old" "$pools"
}

template_pod() {
    jq -c --arg ns "$2" '.[] | select(.kind == "ConfigMap" and .metadata.name == "br-sandbox-pod-template" and .metadata.namespace == $ns) | (.data["pod-template.json"] | fromjson)' "$WORK/$1.json"
}

export AGENT='(.spec.containers[] | select(.name == "agent"))'

model_result() {
    local verdict
    verdict="$(created "${3:-ex}" "$1" "${2:-.}")"
    if [[ "$verdict" == admit ]]; then echo true; else echo false; fi
}
