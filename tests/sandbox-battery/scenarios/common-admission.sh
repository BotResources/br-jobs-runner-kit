pod_template() {
  local name
  name=$(warm_list "$1" | head -1 | cut -d' ' -f1)
  k -n "$1" get pod "$name" -o json | jq -c '
    del(.status, .metadata.uid, .metadata.resourceVersion, .metadata.creationTimestamp, .metadata.ownerReferences,
        .metadata.generateName, .metadata.managedFields, .metadata.namespace, .spec.nodeName)'
}

variant() {
  jq -c --arg role "$LBL_ROLE" --arg seed "$SEED_CONTAINER" --arg agent "$SANDBOX_CONTAINER" --arg name "adm-$(new_id)" "$2 | .metadata.name = \$name" <<< "$1"
}

as_id() {
  local who=$1 ns=$2
  shift 2
  k --as="$who" -n "$ns" "$@"
}

accepted() { "$@" > /dev/null 2>&1; }

refused_with() {
  local code=$1 out
  shift
  out=$("$@" 2>&1) && return 1
  printf '%s\n' "$out" | grep -q -E "BR_SANDBOX_${code}([^A-Z_]|$)"
}

refused_forbidden() {
  local out
  out=$("$@" 2>&1) && return 1
  printf '%s\n' "$out" | grep -q -i 'forbidden'
}

root_pod_to() {
  local var=$1 ns=$2 template created
  template=$(pod_template "$ns") || return 1
  created=$(as_id "$NETCHECK_ID" "$ns" create -o jsonpath='{.metadata.name}' -f - <<< "$(variant "$template" '.metadata.labels["br.sandbox/pool"] = "battery" | .spec.containers |= map(if .name == $agent then .securityContext.runAsUser = 0 | .securityContext.runAsGroup = 0 | .securityContext.runAsNonRoot = false else . end)')") || return 1
  TRACKED_PODS+=("$ns $created")
  k -n "$ns" wait --for=condition=Ready "pod/$created" --timeout=120s > /dev/null || return 1
  printf -v "$var" '%s' "$created"
}
