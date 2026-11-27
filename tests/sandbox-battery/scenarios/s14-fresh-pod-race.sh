#!/usr/bin/env bash
HERE=$(cd "$(dirname "$0")" && pwd)
. "$HERE/common.sh"
scenario_begin s14-fresh-pod-race

PROBE_NS=${PROBE_NS:-sbx-ca-none}
PROBE_TARGETS="${NETCHECK_PROBE_TARGETS:-$(k get svc kubernetes -o jsonpath='{.spec.clusterIP}'):443 $(k get nodes -o jsonpath='{.items[0].status.addresses[?(@.type=="InternalIP")].address}'):10250}"
ICIDRS=$(internal_cidrs_value)

race_pod() {
  local delay=$1 name=$2
  k -n "$SYSTEM_NS" get cm "$NETCHECK_CM" -o jsonpath="{.data.probe-$PROBE_NS\\.yaml}" \
    | sed "s|__TARGETS__|$PROBE_TARGETS|; s|name: br-sandbox-netcheck-probe|name: $name|; s|^    - /netcheck/network-check.sh|    - -c|" \
    | awk -v d="$delay" '/^    - -c$/ {print; print "    - \"sleep " d "; sh /netcheck/network-check.sh probe\""; next} /^    - probe$/ {next} {print}' \
    | as_id "$NETCHECK_ID" "$PROBE_NS" create -f - > /dev/null
}

race_log() {
  local delay=$1 name=race-probe-$1 end
  race_pod "$delay" "$name" || return 1
  end=$(($(date +%s) + 240))
  until [ "$(k -n "$PROBE_NS" get pod "$name" -o jsonpath='{.status.phase}')" = Succeeded ]; do
    if [ "$(date +%s)" -ge "$end" ]; then k -n "$PROBE_NS" delete pod "$name" --wait=false > /dev/null; return 1; fi
    sleep 2
  done
  k -n "$PROBE_NS" logs "$name" | grep -E '^[^ ]+ (refused [0-9]+|reached)$'
  k -n "$PROBE_NS" delete pod "$name" --wait=false > /dev/null
}

count_targets=$(wc -w <<< "$PROBE_TARGETS" | tr -d " ")
for delay in 0 30; do
  log=$(race_log "$delay") || log=error
  if [ "$log" = error ]; then
    reached=error
    refused=error
    seconds=error
  else
    reached=$(grep -c ' reached$' <<< "$log" || true)
    refused=$(grep -c ' refused [0-9]*$' <<< "$log" || true)
    seconds=$(awk '$2 == "refused" {printf "%s%s", sep, $3; sep=","}' <<< "$log")
  fi
  record "fresh-pod-race-start-delay-${delay}s-refused" "$refused" "of $count_targets targets"
  record "fresh-pod-race-start-delay-${delay}s-reached" "$reached" "of $count_targets targets"
  record "fresh-pod-race-start-delay-${delay}s-first-refusal-seconds" "${seconds:-none}" "seconds from the start of the probe script, per target"
  verdict "fresh-pod-race-delay-$delay-refused-after-retry" "probe pod in $PROBE_NS (mode none), probe script started $delay s after the pod start: $refused of $count_targets targets refused, $reached reached at the limit; seconds from the start of the probe script to the first refusal: ${seconds:-none}; internal_cidrs=$ICIDRS" \
    test "$refused" = "$count_targets" -a "$reached" = 0
done

scenario_end
