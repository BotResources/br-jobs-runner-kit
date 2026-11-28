SCRATCH_NS=
SCRATCH_REF_NS=
SCRATCH_PREFIX=

fence_k() { k --as="$FENCE_WRITER" --as-group="$FENCE_WRITER_GROUP" "$@"; }

scratch_namespace_doc() {
  jq -n -c --arg name "$SCRATCH_NS" --argjson labels "$1" '{apiVersion: "v1", kind: "Namespace", metadata: {name: $name, labels: $labels}}'
}

scratch_copy_doc() {
  jq -c --arg ns "$SCRATCH_NS" '
    del(.metadata.uid, .metadata.resourceVersion, .metadata.creationTimestamp, .metadata.ownerReferences,
        .metadata.managedFields, .metadata.annotations, .metadata.labels, .status)
    | .metadata.namespace = $ns' <<< "$1"
}

scratch_wait_stamped() {
  local end
  end=$(($(date +%s) + 10))
  until k -n "$SCRATCH_NS" get configmap "$FENCE_TEMPLATE_MAP" > /dev/null 2>&1; do
    if [ "$(date +%s)" -ge "$end" ]; then return 1; fi
    sleep 1
  done
}

scratch_copy() {
  local ref=$1 doc out
  if k -n "$SCRATCH_NS" get "$ref" > /dev/null 2>&1; then return 0; fi
  if ! doc=$(k -n "$SCRATCH_REF_NS" get "$ref" -o json 2>&1); then
    check "$SCRATCH_PREFIX-fence-source" fail "$ref cannot be read in $SCRATCH_REF_NS: $(first_line "$doc")"
    return 1
  fi
  if ! out=$(fence_k apply -f - <<< "$(scratch_copy_doc "$doc")" 2>&1); then
    check "$SCRATCH_PREFIX-fence-copy" fail "$ref was not made in $SCRATCH_NS as $FENCE_WRITER (it must be in grants.writers of the release): $(first_line "$out")"
    return 1
  fi
}

scratch_pool_doc() {
  jq -c --arg ns "$SCRATCH_NS" --arg name "$SANDBOX_POOL_NAME" --arg image "$1" '
    {apiVersion: "sandbox.botresources.ai/v1alpha1", kind: "SandboxPool", metadata: {name: $name, namespace: $ns},
     spec: (.spec | del(.floors) | .image = $image | .minReplicas = 1 | .maxReplicas = 1 | .margin = 0)}' <<< "$2"
}

scratch_pool_make() {
  local image=$1 labels reference out ref
  SCRATCH_REF_NS=$(ns_of "$CLIENT_A" "$NET_NONE")
  SCRATCH_NS="$SCRATCH_REF_NS-$SCRATCH_PREFIX-$(new_id)"
  if ! labels=$(k get ns "$SCRATCH_REF_NS" -o json | jq -c '.metadata.labels | with_entries(select(.key | startswith("br.sandbox/") or startswith("pod-security.kubernetes.io/")))'); then
    check "$SCRATCH_PREFIX-reference-namespace" fail "the labels of namespace $SCRATCH_REF_NS cannot be read"
    return 1
  fi
  if ! reference=$(k -n "$SCRATCH_REF_NS" get "$SANDBOX_POOL_KIND" "$SANDBOX_POOL_NAME" -o json); then
    check "$SCRATCH_PREFIX-reference-pool" fail "the $SANDBOX_POOL_KIND $SANDBOX_POOL_NAME of namespace $SCRATCH_REF_NS cannot be read, so the size of the scratch pool is not known"
    return 1
  fi
  if ! out=$(fence_k apply -f - <<< "$(scratch_namespace_doc "$labels")" 2>&1); then
    check "$SCRATCH_PREFIX-namespace" fail "namespace $SCRATCH_NS was not made as $FENCE_WRITER: $(first_line "$out")"
    return 1
  fi
  log "set-up: namespace $SCRATCH_NS made; the fence objects of $SCRATCH_REF_NS are copied unless the stamper made them first"
  scratch_wait_stamped || true
  for ref in $FENCE_COPIES; do
    scratch_copy "$ref" || return 1
  done
  if ! out=$(k apply -f - <<< "$(scratch_pool_doc "$image" "$reference")" 2>&1); then
    check "$SCRATCH_PREFIX-pool" fail "the $SANDBOX_POOL_KIND $SANDBOX_POOL_NAME was not made in $SCRATCH_NS: $(first_line "$out")"
    return 1
  fi
}

scratch_pool_accepted() {
  local end status
  end=$(($(date +%s) + 120))
  while [ "$(date +%s)" -lt "$end" ]; do
    status=$(k -n "$SCRATCH_NS" get "$SANDBOX_POOL_KIND" "$SANDBOX_POOL_NAME" -o json | jq -r --arg type "$POOL_ACCEPTED_CONDITION" '[.status.conditions[]? | select(.type == $type) | .status] | first // ""')
    if [ "$status" = True ]; then return 0; fi
    sleep 2
  done
  status=$(k -n "$SCRATCH_NS" get "$SANDBOX_POOL_KIND" "$SANDBOX_POOL_NAME" -o json | jq -c '[.status.conditions[]? | {type, status, reason, message}]')
  check "$SCRATCH_PREFIX-pool-accepted" fail "the pool in $SCRATCH_NS is not $POOL_ACCEPTED_CONDITION=True in 120 s: $(first_line "$status")"
  return 1
}

scratch_pool_remove() {
  local name=$SCRATCH_NS end
  if [ -z "$name" ]; then return 0; fi
  k -n "$name" delete "$SANDBOX_POOL_KIND" "$SANDBOX_POOL_NAME" --ignore-not-found --wait=false > /dev/null 2>&1
  k delete namespace "$name" --ignore-not-found --wait=false > /dev/null 2>&1
  end=$(($(date +%s) + 180))
  while k get namespace "$name" > /dev/null 2>&1; do
    if [ "$(date +%s)" -ge "$end" ]; then
      check "$SCRATCH_PREFIX-scratch-removed" fail "namespace $name still exists 180 s after its delete; remove it by this exact name"
      return 0
    fi
    sleep 2
  done
  check "$SCRATCH_PREFIX-scratch-removed" pass "namespace $name (with its pool and pods) is gone"
  SCRATCH_NS=
}

warm_pods_json() { k -n "$1" get pods -l "$LBL_POOL=warm,$LBL_POOL_ID=$(pool_of "$1")" -o json; }

warm_image_ids() {
  warm_pods_json "$1" | jq -r --arg c "$SANDBOX_CONTAINER" '
    .items[] | select(.metadata.deletionTimestamp == null)
    | select(any(.status.conditions[]?; .type == "Ready" and .status == "True"))
    | "\(.metadata.name) \([.status.containerStatuses[]? | select(.name == $c) | .imageID] | first // "")"'
}
