#!/usr/bin/env bash
HERE=$(cd "$(dirname "$0")" && pwd)
. "$HERE/common.sh"
scenario_begin s02-two-agents

ns=$(ns_of "$CLIENT_A" "$NET_NONE")
thread="two-agents-$(new_id)"
marker="two-agents-marker-$(new_id)"
wait_warm "$ns" 2 120 || { check two-agents-warm-pods fail "fewer than two warm pods in $ns"; scenario_end; }
claim_to pod_a "$ns" "$thread" agent-a || { check two-agents-claim fail "claim of agent a refused"; scenario_end; }
claim_to pod_b "$ns" "$thread" agent-b || { check two-agents-claim fail "claim of agent b refused"; scenario_end; }

verdict two-agents-two-pods "pods $pod_a and $pod_b" test "$pod_a" != "$pod_b"

sx "$ns" "$pod_a" sh -c "echo secret-of-a > /work/x && mkdir -p /home/agent && echo secret-of-a > /home/agent/$marker && (nohup sleep 987654 > /dev/null 2>&1 &)"
verdict two-agents-a-sees-own-file "a reads /work/x" test "$(sx "$ns" "$pod_a" cat /work/x)" = secret-of-a

verdict two-agents-b-no-work-x "b has no /work/x" sx "$ns" "$pod_b" sh -c 'test ! -e /work/x'
verdict two-agents-b-no-marker-anywhere "find for $marker over the whole file system of b" \
  test -z "$(sx "$ns" "$pod_b" find / -xdev -name "$marker" -print 2> /dev/null)"
verdict two-agents-b-no-process-of-a "b sees no sleep 987654 in /proc" \
  sx "$ns" "$pod_b" sh -c '! grep -l 987654 /proc/[0-9]*/cmdline 2> /dev/null | grep -q .'

shared_volume_kinds() {
  k -n "$ns" get pod "$1" -o json |
    jq -r '[.spec.volumes[]? | keys[] | select(. == "persistentVolumeClaim" or . == "hostPath" or . == "nfs" or . == "csi")] | join(",")'
}
verdict two-agents-no-shared-volume-kind "pod volumes use only emptyDir and configMap" \
  test -z "$(shared_volume_kinds "$pod_a")$(shared_volume_kinds "$pod_b")"
verdict two-agents-no-shared-pid-namespace "hostPID and shareProcessNamespace unset" \
  test "$(k -n "$ns" get pod "$pod_b" -o json | jq '[.spec.hostPID, .spec.shareProcessNamespace] | map(. // false) | any')" = false
verdict two-agents-different-pod-uids "uids differ" \
  test "$(k -n "$ns" get pod "$pod_a" -o jsonpath='{.metadata.uid}')" != "$(k -n "$ns" get pod "$pod_b" -o jsonpath='{.metadata.uid}')"

scenario_end
