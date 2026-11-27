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

count_marker_files() {
  sx "$1" "$2" sh -c 'echo "count:$(find / \( -path /proc -o -path /sys \) -prune -o -name "$0" -print 2> /dev/null | wc -l | tr -d " ")"' "$3"
}

count_sleep_processes() {
  sx "$1" "$2" sh -c 'echo "count:$(grep -l "[9]87654" /proc/[0-9]*/cmdline 2> /dev/null | wc -l | tr -d " ")"'
}

verdict two-agents-b-no-work-x "b has no /work/x" sx "$ns" "$pod_b" sh -c 'test ! -e /work/x'
a_markers=$(count_marker_files "$ns" "$pod_a" "$marker")
b_markers=$(count_marker_files "$ns" "$pod_b" "$marker")
verdict two-agents-a-finds-own-marker "find for $marker in a: ${a_markers:-no output}" test "$a_markers" = count:1
verdict two-agents-b-no-marker-anywhere "find for $marker over the whole file system of b: ${b_markers:-no output}" test "$b_markers" = count:0
a_sleeps=$(count_sleep_processes "$ns" "$pod_a")
b_sleeps=$(count_sleep_processes "$ns" "$pod_b")
verdict two-agents-a-runs-the-sleep "a sees its own sleep 987654 in /proc: ${a_sleeps:-no output}" test "$a_sleeps" = count:1
verdict two-agents-b-no-process-of-a "b sees no sleep 987654 in /proc: ${b_sleeps:-no output}" test "$b_sleeps" = count:0

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
