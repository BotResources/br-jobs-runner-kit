#!/usr/bin/env bash

QUOTA_NEEDS='
[.[] | select(.kind == "ResourceQuota")] as $quotas
| [.[] | select(.kind == "ReplicaSet")] as $pools
| [$quotas[] | . as $q
  | ($pools[] | select(.metadata.namespace == $q.metadata.namespace) | .spec.template.spec.containers) as $containers
  | (($q.spec.hard.pods | tonumber) - $extra) as $max
  | ["requests", "limits"][] as $kind
  | ["cpu", "memory", "ephemeral-storage"][] as $res
  | {namespace: $q.metadata.namespace, key: ($kind + "." + $res), max: $max,
     hard: ($q.spec.hard[$kind + "." + $res] | qty),
     need: ($max * ([$containers[].resources[$kind][$res] | qty] | add) + $extra * ($containers[0].resources[$kind][$res] | qty))}]
'

check_the_quota_holds_the_pool_and_the_seed_container() {
    section "the quota holds max_sandboxes pods with the seed container, plus the extra pods"
    local max
    for max in 100 1000; do
        render "quota_${max}" -f "$EXAMPLE" --set "pool.max_sandboxes=${max}"
        t "at ${max} sandboxes every quota quantity is at least max_sandboxes x (agent + seed) + extra_pods x agent" jq -e --argjson extra 4 "${QTY}${QUOTA_NEEDS} | length == 24 and all(.hard >= .need * 0.999999999)" "$WORK/quota_${max}.json"
        t "at ${max} sandboxes no quota quantity is larger than that need" jq -e --argjson extra 4 "${QTY}${QUOTA_NEEDS} | all(.hard <= .need * 1.000000001)" "$WORK/quota_${max}.json"
        t "at ${max} sandboxes the quota of a default pool holds the pods of the pool" jqt "quota_${max}" "[.[] | select(.kind == \"ResourceQuota\" and .metadata.namespace == \"sbx-alpha-none\") | .spec.hard.pods] == [\"$((max + 4))\"]"
    done
    render noseed -f "$EXAMPLE" --set tools.seed_container.enabled=false
    t "without the seed container the quota is the size times the pods" jqt noseed '.[] | select(.kind == "ResourceQuota" and .metadata.namespace == "sbx-alpha-none") | .spec.hard | .["requests.cpu"] == "12000m" and .["requests.memory"] == "24Gi" and .["limits.ephemeral-storage"] == "120Gi"'
    t "the seed container weighs in the quota at the default size" jqt ex '.[] | select(.kind == "ResourceQuota" and .metadata.namespace == "sbx-alpha-none") | .spec.hard | .["requests.cpu"] == "12200m" and .["limits.cpu"] == "50" and .["limits.memory"] == "49792Mi"'
    fails_with "BR_SANDBOX_QUANTITY_INVALID" -f "$EXAMPLE" --skip-schema-validation --set pool.size.cpu.request=1x
}
