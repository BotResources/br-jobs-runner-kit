#!/usr/bin/env bash
HERE=$(cd "$(dirname "$0")" && pwd)
. "$HERE/common.sh"
scenario_begin m-coldstart

reps=${COLD_REPS:-10}
case $reps in
  '' | *[!0-9]* | 0 | 0[0-9]*) check m-coldstart-reps-valid fail "COLD_REPS must be a whole number of 1 or more, got: $reps"; scenario_end ;;
esac
ns=$(ns_of "$CLIENT_A" "$NET_NONE")
wait_warm "$ns" 1 120 || { check m-coldstart-warm fail "no warm pod in $ns to copy the template from"; scenario_end; }
warm_name=$(warm_list "$ns" | head -1 | cut -d' ' -f1)
template=$(k -n "$ns" get pod "$warm_name" -o json)
big_image=$(pod_image "$ns" "$warm_name")
tools_image=$(jq -r '.spec.initContainers[0].image' <<< "$template")

variant_json() {
  local filter
  case $1 in
    big-init) filter='.' ;;
    big-plain) filter='drop_init | .spec.containers |= map(if .name == $c then .command = ["sleep", "infinity"] else . end)' ;;
    small-init) filter='.spec.containers |= map(if .name == $c then .image = $tools else . end)' ;;
    small-plain) filter='drop_init | .spec.containers |= map(if .name == $c then .image = $tools | .command = [$pause] else . end)' ;;
  esac
  jq -c --arg c "$SANDBOX_CONTAINER" --arg tools "$tools_image" --arg pause "$TOOLS_PAUSE" --arg pool "$LBL_POOL" --arg poolid "$LBL_POOL_ID" --arg name "cold-$(new_id)" '
    def drop_init: (.spec.initContainers[0].volumeMounts[0].name) as $tv
      | del(.spec.initContainers)
      | .spec.volumes |= map(select(.name != $tv))
      | .spec.containers |= map(.volumeMounts |= map(select(.name != $tv)));
    del(.status, .metadata.uid, .metadata.resourceVersion, .metadata.creationTimestamp, .metadata.ownerReferences, .metadata.generateName, .metadata.managedFields, .spec.nodeName)
    | .metadata.name = $name | .metadata.labels[$pool] = "claimed" | .metadata.labels[$poolid] = "cold-test"
    | '"$filter" <<< "$template"
}

cold_start() {
  local json name started ready first_exec
  json=$(variant_json "$1")
  name=$(jq -r '.metadata.name' <<< "$json")
  started=$(now_ms)
  k -n "$ns" create -f - <<< "$json" > /dev/null || return 1
  if ! k -n "$ns" wait --for=condition=Ready "pod/$name" --timeout=180s > /dev/null; then
    k -n "$ns" delete pod "$name" --wait=true --grace-period=1 > /dev/null 2>&1
    return 1
  fi
  ready=$(now_ms)
  case $1 in
    big-*) sx "$ns" "$name" true > /dev/null 2>&1 ;;
    small-init) sx "$ns" "$name" "$AGENT_BSDTAR" --version > /dev/null 2>&1 ;;
    small-plain) sx "$ns" "$name" "$SEED_BSDTAR" --version > /dev/null 2>&1 ;;
  esac
  first_exec=$(now_ms)
  k -n "$ns" delete pod "$name" --wait=true --grace-period=1 > /dev/null 2>&1
  echo "$((ready - started)) $((first_exec - ready))"
}

pull_time() {
  local started
  docker exec "$NODE" crictl rmi "$1" > /dev/null 2>&1
  started=$(now_ms)
  docker exec "$NODE" crictl pull "$1" > /dev/null 2>&1
  echo $(($(now_ms) - started))
}

pullable() {
  case ${1%%/*} in
    *.* | *:* | localhost) [ "$1" != "${1%%/*}" ] ;;
    *) return 1 ;;
  esac
}

lock_timed || scenario_end
for image in "$big_image" "$tools_image"; do
  size=$(docker exec "$NODE" crictl inspecti "$image" 2> /dev/null | jq -r '.status.size // empty')
  label=$([ "$image" = "$big_image" ] && echo big || echo small)
  record "m-coldstart-image-$label-size" "${size:-0}" bytes
  if pullable "$image"; then
    record "m-coldstart-image-$label-pull" "$(pull_time "$image")" ms
  else
    record "m-coldstart-image-$label-pull" "not measured" "image loaded into the node, no registry to pull from"
  fi
done

variants="big-init big-plain small-init small-plain"
results="$BATTERY_OUT/coldstart-$(date +%Y%m%d-%H%M%S).txt"
: > "$results"
failed=0
for rep in $(seq 1 "$reps"); do
  for variant in $variants; do
    if line=$(cold_start "$variant"); then
      read -r ready_ms exec_ms <<< "$line"
      echo "$variant $ready_ms $exec_ms" >> "$results"
    else
      failed=$((failed + 1))
    fi
  done
done
for variant in $variants; do
  record "m-coldstart-$variant-ready-median" "$(awk -v v="$variant" '$1 == v {print $2}' "$results" | median_of)" ms
  record "m-coldstart-$variant-ready-max" "$(awk -v v="$variant" '$1 == v {print $2}' "$results" | max_of)" ms
  record "m-coldstart-$variant-first-exec-median" "$(awk -v v="$variant" '$1 == v {print $3}' "$results" | median_of)" ms
done
verdict m-coldstart-sample-count "$(count_lines < "$results") result lines for $((reps * 4)) planned" test "$(count_lines < "$results")" = "$((reps * 4))"
verdict m-coldstart-ran "$((reps * 4 - failed)) of $((reps * 4)) cold starts measured, results $results" test "$failed" = 0

scenario_end
