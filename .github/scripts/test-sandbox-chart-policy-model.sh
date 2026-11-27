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
def listed($e; $re): ([$e | capture($re).l] | .[0] // "") | if . == "" then [] else split(", ") | map(gsub("" + q + ""; "")) end;
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
  | txt($vap; "BR_SANDBOX_HOST_NAMESPACE") as $hostE
  | txt($vap; "BR_SANDBOX_PROBE_HOST") as $probeE
  | txt($vap; "BR_SANDBOX_PROC_MOUNT") as $procE
  | txt($vap; "BR_SANDBOX_APPARMOR") as $aaE
  | txt($vap; "BR_SANDBOX_SELINUX") as $seE
  | txt($vap; "BR_SANDBOX_SYSCTLS") as $sysE
  | txt($vap; "BR_SANDBOX_WINDOWS_HOST_PROCESS") as $winE
  | listed($aaE; "appArmorProfile\\.type in \\[(?<l>[^\\]]*)\\]") as $aaTypes
  | listed($seE; "seLinuxOptions\\.type in \\[(?<l>[^\\]]*)\\]") as $seTypes
  | listed($sysE; "s\\.name in \\[(?<l>[^\\]]*)\\]") as $sysSafe
  | ([$aaE | scan("k\\.startsWith\\(" + q + "([^" + q + "]+)" + q + "\\)")] | .[0][0]) as $aaPrefix
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
      (if $sandbox and (
           (($hostE | contains("hostNetwork")) and ($pod.spec.hostNetwork // false))
           or (($hostE | contains("hostPID")) and ($pod.spec.hostPID // false))
           or (($hostE | contains("hostIPC")) and ($pod.spec.hostIPC // false))
           or (($hostE | contains("p.hostPort == 0")) and ($all | any((.ports // []) | any((.hostPort // 0) != 0))))
         ) then "BR_SANDBOX_HOST_NAMESPACE" else empty end),
      (if $sandbox and ([["livenessProbe"], ["readinessProbe"], ["startupProbe"], ["lifecycle", "postStart"], ["lifecycle", "preStop"]] | any(
           . as $path | ["httpGet", "tcpSocket"] | any(
             . as $h | ($probeE | contains("c." + ($path | join(".")) + "." + $h + ".host == " + q + q)) and ($all | any(((getpath($path) // {})[$h].host // "") != ""))))) then "BR_SANDBOX_PROBE_HOST" else empty end),
      (if $sandbox and ($procE | contains("c.securityContext.procMount == " + q + "Default" + q)) and ($all | any((.securityContext.procMount // "Default") != "Default")) then "BR_SANDBOX_PROC_MOUNT" else empty end),
      (if $sandbox and ((($aaTypes | length) > 0 and (([$pod.spec.securityContext] + ($all | map(.securityContext // {}))) | any((.appArmorProfile.type // null) as $t | $t != null and (($aaTypes | index($t)) == null)))) or ($aaPrefix != null and (($pod.metadata.annotations // {}) | to_entries | any((.key | startswith($aaPrefix)) and (.value != "runtime/default") and ((.value | startswith("localhost/")) | not))))) then "BR_SANDBOX_APPARMOR" else empty end),
      (if $sandbox and (($seE | contains("seLinuxOptions.user == " + q + q)) or ($seTypes | length) > 0) and (([$pod.spec.securityContext] + ($all | map(.securityContext // {}))) | any((.seLinuxOptions // {}) as $o | ((($seTypes | length) > 0) and (($seTypes | index($o.type // "")) == null)) or (($seE | contains("seLinuxOptions.user == " + q + q)) and ($o.user // "") != "") or (($seE | contains("seLinuxOptions.role == " + q + q)) and ($o.role // "") != ""))) then "BR_SANDBOX_SELINUX" else empty end),
      (if $sandbox and ($sysSafe | length) > 0 and (($pod.spec.securityContext.sysctls // []) | any(. as $s | ($sysSafe | index($s.name)) == null)) then "BR_SANDBOX_SYSCTLS" else empty end),
      (if $sandbox and ($winE | contains("windowsOptions.hostProcess")) and (([$pod.spec.securityContext] + ($all | map(.securityContext // {}))) | any((.windowsOptions.hostProcess // false) == true)) then "BR_SANDBOX_WINDOWS_HOST_PROCESS" else empty end),
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
      (if $sandbox and $create and ($params != null) and (($params.spec // null) == null) and ($resE | contains("params.")) and (($resE | contains("!has(params.spec) ||")) | not) then "EVALUATION_ERROR" else empty end),
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

def vexpr($vap; $name): [$vap.spec.variables[]? | select(.name == $name) | .expression] | .[0] // "";
def skipped($e; $op): (($e | contains("request.operation != " + q + "CREATE" + q + " ||")) and ($op != "CREATE")) or (($e | contains("request.operation != " + q + "UPDATE" + q + " ||")) and ($op != "UPDATE"));
def ns_label($x; $k): (($x.metadata.labels // {})[$k] // "");

def ns_failing($vap; $r; $grants):
  $r.obj as $o
  | ($r.old // {metadata: {}}) as $old
  | txt($vap; "BR_SANDBOX_MODE_UNKNOWN") as $unknownE
  | txt($vap; "BR_SANDBOX_MODE_NOT_GRANTED") as $grantedE
  | txt($vap; "BR_SANDBOX_LABEL_IMMUTABLE") as $immutableE
  | txt($vap; "BR_SANDBOX_SCOPE") as $scopeE
  | quoted_list($unknownE; "in \\[(?<l>[^\\]]*)\\]") as $modes
  | ([$grantedE | scan("variables\\.mode == " + q + "([^" + q + "]+)" + q)] | .[0][0]) as $open
  | (vexpr($vap; "granted") | capture("\\(" + q + "(?<p>[^" + q + "]+)" + q + " \\+ object").p) as $prefix
  | (vexpr($vap; "writer") | capture("&& " + q + "(?<k>[a-z]+)" + q + " in params").k) as $wkey
  | (vexpr($vap; "scopes") | capture(q + "(?<k>[a-z]+)" + q + " in params\\.data\\)").k) as $skey
  | (vexpr($vap; "mode") | capture(q + "(?<k>[^" + q + "]+)" + q + " in object\\.metadata\\.labels\\)").k) as $modeKey
  | (vexpr($vap; "managed") | capture(q + "(?<k>[^" + q + "]+)" + q + " in object\\.metadata\\.labels\\)").k) as $managedKey
  | ($grants.data[$wkey] // "" | split(",")) as $writers
  | ($grants.data[$skey] // "" | split(",")) as $scopes
  | ($grants.data[$prefix + $o.metadata.name] // "" | split(",")) as $granted
  | ($r.user | IN($writers[])) as $writer
  | ns_label($o; $modeKey) as $mode
  | [
      (if (skipped($unknownE; $r.op) | not) and (($modes | index($mode)) == null) then "BR_SANDBOX_MODE_UNKNOWN" else empty end),
      (if (skipped($grantedE; $r.op) | not) and ($mode != $open or ($grantedE | contains("variables.mode == " + q + $open + q) | not)) and (($grantedE | contains("variables.writer")) and $writer | not) and (($granted | index($mode)) == null) then "BR_SANDBOX_MODE_NOT_GRANTED" else empty end),
      (if (skipped($immutableE; $r.op) | not) and ((($immutableE | contains("variables.writer")) and $writer) | not) and ((ns_label($o; $managedKey) != ns_label($old; $managedKey)) or ($mode != ns_label($old; $modeKey))) then "BR_SANDBOX_LABEL_IMMUTABLE" else empty end),
      (if (skipped($scopeE; $r.op) | not) and (($scopes | any(. as $s | $s != "" and ($o.metadata.name == $s or ($o.metadata.name | startswith($s + "-"))))) | not) then "BR_SANDBOX_SCOPE" else empty end)
    ] | unique;

def selected($b; $r): ($b.spec.matchResources.objectSelector.matchLabels // {}) as $want
  | all($want | to_entries[]; . as $e | ((($r.obj.metadata.labels // {})[$e.key]) == $e.value) or ((($r.old.metadata.labels // {})[$e.key]) == $e.value));

def ns_decide($vap; $bindings; $r):
  [$bindings[] | select(.spec.policyName == $vap.metadata.name)
    | select(rule_matches($vap.spec.matchConstraints.resourceRules; $r.op; $r.res) and rule_matches(.spec.matchResources.resourceRules; $r.op; $r.res) and selected(.; $r))] as $bound
  | [$bound[] | if ($r.grants == null) then (if .spec.paramRef.parameterNotFoundAction == "Allow" or $vap.spec.failurePolicy != "Fail" then [] else ["PARAMETER_NOT_FOUND"] end)
      else ns_failing($vap; $r; $r.grants) end] | add // [] | unique
  | if length == 0 then "admit" else join(",") end;

def fence_failing($vap; $r; $grants):
  vexpr($vap; "fenced") as $fe
  | txt($vap; "BR_SANDBOX_FENCE") as $re
  | ($r.res | split("/")[0]) as $kind
  | [$fe | scan("resource == " + q + "([a-z]+)" + q + " \\? variables\\.name == " + q + "([^" + q + "]+)" + q)] as $exact
  | [$fe | scan("resource == " + q + "([a-z]+)" + q + " \\? variables\\.name\\.startsWith\\(" + q + "([^" + q + "]+)" + q + "\\)")] as $prefix
  | ([$exact[] | select(.[0] == $kind)] | .[0]) as $e
  | ([$prefix[] | select(.[0] == $kind)] | .[0]) as $p
  | (if $e != null then $r.name == $e[1] elif $p != null then ($r.name | startswith($p[1])) else true end) as $fenced
  | ($grants.data[(vexpr($vap; "writer") | capture("&& " + q + "(?<k>[a-z]+)" + q + " in params").k)] // "" | split(",")) as $writers
  | ((vexpr($vap; "writer") | length) > 0 and ($re | contains("variables.writer")) and ($r.user | IN($writers[]))) as $writer
  | ([$re | scan("status\\.phase == " + q + "([^" + q + "]+)" + q)] | .[0][0]) as $phase
  | (($phase != null) and ($r.phase == $phase)) as $exempt
  | if $fenced and ($writer | not) and ($exempt | not) then ["BR_SANDBOX_FENCE"] else [] end;

def fence_decide($vap; $bindings; $r):
  [$bindings[] | select(.spec.policyName == $vap.metadata.name)
    | select(rule_matches($vap.spec.matchConstraints.resourceRules; $r.op; $r.res) and rule_matches(.spec.matchResources.resourceRules; $r.op; $r.res)
        and all((.spec.matchResources.namespaceSelector.matchLabels // {}) | to_entries[]; . as $e | ($r.nsLabels[$e.key] // null) == $e.value))] as $bound
  | [$bound[] | if ($r.grants == null) then (if .spec.paramRef.parameterNotFoundAction == "Allow" or $vap.spec.failurePolicy != "Fail" then [] else ["PARAMETER_NOT_FOUND"] end)
      else fence_failing($vap; $r; $r.grants) end] | add // [] | unique
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

ns_verdict() {
    local render="$1" op="$2" obj="$3" old="$4" user="$5" grants="$6"
    jq -r --arg op "$op" --arg user "$user" --argjson obj "$obj" --argjson old "$old" --argjson grants "$grants" "${QTY}${MODEL}"'
      (.[] | select(.kind == "ValidatingAdmissionPolicy" and .metadata.name == "br-sandbox-namespace")) as $vap
      | [.[] | select(.kind == "ValidatingAdmissionPolicyBinding")] as $bindings
      | ns_decide($vap; $bindings; {op: $op, res: "namespaces", obj: $obj, old: $old, user: $user, grants: $grants})' "$WORK/${render}.json"
}

grants_of() {
    jq -c '.[] | select(.kind == "ConfigMap" and .metadata.name == "br-sandbox-grants")' "$WORK/$1.json"
}

namespace_object() {
    jq -nc --arg name "$1" --arg managed "$2" --arg mode "$3" '{metadata: {name: $name, labels: ({"br.sandbox/managed": $managed, "br.sandbox/mode": $mode} | with_entries(select(.value != "")))}}'
}

fence_verdict() {
    local render="$1" op="$2" res="$3" name="$4" user="$5" grants="$6" phase="${7:-Active}" labels managed='{"br.sandbox/managed":"true"}'
    labels="${8:-$managed}"
    jq -r --arg op "$op" --arg res "$res" --arg name "$name" --arg user "$user" --arg phase "$phase" --argjson labels "$labels" --argjson grants "$grants" "${QTY}${MODEL}"'
      (.[] | select(.kind == "ValidatingAdmissionPolicy" and .metadata.name == "br-sandbox-fence")) as $vap
      | [.[] | select(.kind == "ValidatingAdmissionPolicyBinding")] as $bindings
      | fence_decide($vap; $bindings; {op: $op, res: $res, name: $name, user: $user, grants: $grants, phase: $phase, nsLabels: $labels})' "$WORK/${render}.json"
}
