#!/usr/bin/env bash
HERE=$(cd "$(dirname "$0")" && pwd)
. "$HERE/common.sh"
scenario_begin m-window

. "$WORK/servers.env"

reps=${REPS:-10}
exec_seconds=${WINDOW_EXEC_SECONDS:-5}
start_seconds=${WINDOW_START_SECONDS:-15}
probe_port=${WINDOW_PROBE_PORT:-8080}
min_free_mb=${WINDOW_MIN_FREE_MB:-8000}
ns=$(ns_of "$CLIENT_A" "$NET_ALLOW")
pool=$(pool_of "$ns")
samples="$EVIDENCE/window-samples.tsv"
summary="$EVIDENCE/window.md"
watch_file="$BATTERY_OUT/window-watch.txt"
watch_pid=

cleanup_extra() {
  if [ -n "$watch_pid" ]; then kill "$watch_pid" 2> /dev/null; fi
}

disk_ok() {
  local free
  free=$(df -m /System/Volumes/Data | awk 'NR==2 {print $4}')
  [ "$free" -ge "$min_free_mb" ]
}

exec_observer() {
  local pod=$1 out=$2 end
  : > "$out"
  end=$(($(date +%s) + 90))
  while [ "$(date +%s)" -lt "$end" ]; do
    k -n "$ns" exec -i "$pod" -c "$SANDBOX_CONTAINER" -- "$SANDBOX_PYTHON" - "$PROBE_IP" "$probe_port" "$exec_seconds" \
      < "$HERE/window-probe.py" 2> /dev/null | python3 -u "$HERE/stamp.py" >> "$out"
    if [ -s "$out" ]; then return 0; fi
    sleep 0.02
  done
  return 1
}

start_observer() {
  local pod=$1 out=$2 end
  : > "$out"
  end=$(($(date +%s) + 90))
  while [ "$(date +%s)" -lt "$end" ]; do
    k -n "$ns" logs -f "$pod" -c "$SANDBOX_CONTAINER" 2> /dev/null | python3 -u "$HERE/stamp.py" >> "$out"
    if [ -s "$out" ]; then return 0; fi
    sleep 0.02
  done
  return 1
}

rules_observer() {
  local name=$1 out=$2 end
  : > "$out"
  end=$(($(date +%s) + 60))
  while [ "$(date +%s)" -lt "$end" ]; do
    if docker exec "$NODE" iptables -S KUBE-ROUTER-OUTPUT 2> /dev/null | grep -q -- "traffic from POD name:$name namespace: $ns to chain"; then
      now_ms > "$out"
      return 0
    fi
  done
  return 1
}

analyze() {
  awk '{s = $1 - $2; if (NR == 1 || s < m) m = s; r[NR] = $3; o[NR] = $2}
    END {
      for (i = 1; i <= NR; i++) if (r[i] != "R" && ne == "") {ne = o[i]; idx = i}
      for (i = 1; i <= NR; i++) if (r[i] == "R") {reached++; if (ne != "" && i > idx) flips++}
      print m, r[1], (ne == "" ? "none" : ne), reached + 0, flips + 0, r[NR], NR
    }' "$1"
}

cold_pod_json() {
  local template=$1 name=$2 code
  code=$(cat "$HERE/window-probe.py")
  jq -c --arg name "$name" --arg pool "$LBL_POOL" --arg agent "$SANDBOX_CONTAINER" --arg code "$code" \
    --argjson args "[\"$PROBE_IP\", \"$probe_port\", \"$start_seconds\"]" '
    .metadata.name = $name | .metadata.labels[$pool] = "battery" | .spec.restartPolicy = "Never"
    | .spec.containers |= map(if .name == $agent then .command = ["python3", "-u", "-c", $code] + $args | del(.args) else . end)' <<< "$template"
}

new_pod_since() {
  awk -v t="$1" -v pre="$2" 'BEGIN {n = split(pre, a, " "); for (i = 1; i <= n; i++) seen[a[i]] = 1} $1 >= t && !($2 in seen) {print $2; exit}' "$watch_file"
}

wait_new_pod() {
  local end name
  end=$(($(date +%s) + 60))
  while [ "$(date +%s)" -lt "$end" ]; do
    name=$(new_pod_since "$1" "$2")
    if [ -n "$name" ]; then echo "$name"; return 0; fi
    sleep 0.02
  done
  return 1
}

watch_stamp() { awk -v n="$1" -v want="$2" '$2 == n && $3 == want {print $1; exit}' "$watch_file"; }

sample() {
  local kind=$1 i=$2 pre victim name t0 t_ready ex en conn_before conn_after load
  local ex_start ex_first ex_enf ex_reached ex_flips ex_last ex_n
  local st_start st_first st_enf st_reached st_flips st_last st_n
  local first_exec_ms enforced_ms before enf_from rules_ms agent_start_ms
  disk_ok || { check m-window-disk fail "free disk under $min_free_mb MB: stop"; return 2; }
  pre=$(k -n "$ns" get pods -o jsonpath='{.items[*].metadata.name}')
  conn_before=$(conn_docker "$PROBE_CONTAINER")
  load=$(load_now)
  if [ "$kind" = warm ]; then
    wait_warm "$ns" 1 120 || return 1
    victim=$(warm_list "$ns" | head -1 | cut -d' ' -f1)
    t0=$(now_ms)
    k -n "$ns" delete pod "$victim" --wait=false --grace-period=1 > /dev/null || return 1
    name=$(wait_new_pod "$t0" "$pre") || return 1
    t0=$(awk -v n="$name" '$2 == n {print $1; exit}' "$watch_file")
    rules_observer "$name" "$BATTERY_OUT/window-rules-$i.txt" &
    rules_pid=$!
  else
    name="window-cold-$(new_id)"
    template=$(pod_template "$ns")
    t0=$(now_ms)
    as_id "$RUNNER_ID" "$ns" create -f - <<< "$(cold_pod_json "$template" "$name")" > /dev/null || return 1
    track_pod "$ns" "$name"
    rules_observer "$name" "$BATTERY_OUT/window-rules-$i.txt" &
    rules_pid=$!
    start_observer "$name" "$BATTERY_OUT/window-start-$i.txt" &
    st_pid=$!
  fi
  exec_observer "$name" "$BATTERY_OUT/window-exec-$i.txt" || return 1
  if [ "$kind" = cold ]; then wait "$st_pid"; fi
  wait "$rules_pid"
  rules_ms=$(awk -v t0="$t0" '{print $1 - t0}' "$BATTERY_OUT/window-rules-$i.txt")
  rules_ms=${rules_ms:-none}
  sleep 1
  conn_after=$(conn_docker "$PROBE_CONTAINER")
  t_ready=$(watch_stamp "$name" True)
  read -r ex_start ex_first ex_enf ex_reached ex_flips ex_last ex_n <<< "$(analyze "$BATTERY_OUT/window-exec-$i.txt")"
  first_exec_ms=$((ex_start - t0))
  if [ "$kind" = cold ]; then
    read -r st_start st_first st_enf st_reached st_flips st_last st_n <<< "$(analyze "$BATTERY_OUT/window-start-$i.txt")"
    agent_start_ms=$((st_start - t0))
    if [ "$st_enf" = none ]; then enforced_ms=none; elif [ "$st_first" != R ]; then enforced_ms="<=$agent_start_ms"; else enforced_ms=$((st_start + st_enf - t0)); fi
    enf_from="pod-start-loop"
  else
    st_reached=0; st_flips=0; st_last="-"; st_n=0; agent_start_ms=-
    if [ "$ex_enf" = none ]; then enforced_ms=none; elif [ "$ex_first" != R ]; then enforced_ms="<=$first_exec_ms"; else enforced_ms=$((ex_start + ex_enf - t0)); fi
    enf_from="exec-loop"
  fi
  if [ "$ex_first" = R ]; then before=yes; else before=no; fi
  printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$kind" "$i" "$name" "$((t_ready - t0))" "$first_exec_ms" "$enforced_ms" "$enf_from" "$before" \
    "$((conn_after - conn_before))" "$((ex_reached + st_reached))" "$((ex_flips + st_flips))" "$ex_last" "$st_last" "$rules_ms" "$agent_start_ms" "$load" >> "$samples"
  record "m-window-$kind-ready-ms" "$((t_ready - t0))" ms
  record "m-window-$kind-first-exec-ms" "$first_exec_ms" ms
  record "m-window-$kind-enforced-ms" "$enforced_ms" ms
  record "m-window-$kind-rules-visible-ms" "$rules_ms" ms
  record "m-window-$kind-exec-before-enforced" "$before" yes-no
  record "m-window-$kind-probe-connections" "$((conn_after - conn_before))" connections
  if [ "$kind" = cold ]; then release_pod "$ns" "$name"; k -n "$ns" wait --for=delete "pod/$name" --timeout=60s > /dev/null 2>&1; fi
  sleep 3
}

stats_row() {
  awk -F'\t' -v kind="$1" -v label="$2" -v expr="$3" '
    function num(v) {sub(/^<=/, "", v); return v ~ /^-?[0-9]+$/ ? v : ""}
    $1 == kind {
      if (expr == "ready") v = num($4); else if (expr == "exec") v = num($5); else if (expr == "enforced") v = num($6)
      else if (expr == "rules") v = num($14); else if (expr == "agent") v = num($15)
      else if (expr == "margin") v = (num($5) != "" && num($14) != "") ? $5 - $14 : ""
      if (v != "") a[++n] = v + 0
    }
    END {
      for (i = 1; i <= n; i++) for (j = i + 1; j <= n; j++) if (a[j] < a[i]) {t = a[i]; a[i] = a[j]; a[j] = t}
      if (n) printf "| %s | %s | %s | %s | %d |\n", label, a[1], a[int((n + 1) / 2)], a[n], n
      else printf "| %s |  |  |  | 0 |\n", label
    }' "$samples"
}

summarize() {
  local kind=$1
  {
    echo "### $kind pods"
    echo
    echo "| Measure (ms, from t0) | min | median | max | samples |"
    echo "|---|---|---|---|---|"
    stats_row "$kind" "t_ready" ready
    stats_row "$kind" "t_first_exec" exec
    stats_row "$kind" "t_enforced (in-pod loop; upper bound when the first connect was already refused)" enforced
    stats_row "$kind" "t_rules_visible (node firewall jump for the pod, polled from the host)" rules
    stats_row "$kind" "t_agent_start (first connect of the loop started as the container command)" agent
    stats_row "$kind" "t_first_exec - t_rules_visible (positive: rules were there first)" margin
    echo
    echo "Samples where the first command ran before enforcement: $(awk -F'\t' -v kind="$kind" '$1 == kind && $8 == "yes"' "$samples" | count_lines) of $(awk -F'\t' -v kind="$kind" '$1 == kind' "$samples" | count_lines)"
    echo "Samples where the first connect of the in-pod loop was already refused (enforced value is an upper bound): $(awk -F'\t' -v kind="$kind" '$1 == kind && $6 ~ /^<=/' "$samples" | count_lines)"
    echo "Connections the probe container recorded, per sample: $(awk -F'\t' -v kind="$kind" '$1 == kind {printf "%s ", $9}' "$samples")"
    echo
  } >> "$summary"
}

if ! kubectl --context "$CTX" --request-timeout=5s get --raw /readyz > /dev/null 2>&1; then
  check m-window-api fail "API of $CTX not reachable from the host: the watch needs it"
  scenario_end
fi
control=$(k -n "$NS_TOOLS" exec "$TOOLBOX_POD" -- curl -s -m 3 -o /dev/null -w '%{http_code}' "http://$PROBE_IP:$probe_port/" || true)
verdict m-window-control-reachable "an unrestricted pod reaches the recording probe at $PROBE_IP:$probe_port (status $control)" test "$control" = 200
loop_before=$(conn_docker "$PROBE_CONTAINER")
loop_lines=$(k -n "$NS_TOOLS" exec -i pytool -- "$SANDBOX_PYTHON" - "$PROBE_IP" "$probe_port" 2 < "$HERE/window-probe.py" 2> /dev/null)
sleep 1
loop_reached=$(grep -c ' R$' <<< "$loop_lines")
loop_recorded=$(($(conn_docker "$PROBE_CONTAINER") - loop_before))
verdict m-window-loop-control "the probe loop run for 2 s from an unrestricted pod: $loop_reached of $(count_lines <<< "$loop_lines") connects read R, the probe container recorded $loop_recorded connections" \
  test "$loop_reached" -gt 0 -a "$loop_recorded" -ge "$loop_reached"
lock_timed

printf 'kind\tsample\tpod\tready_ms\tfirst_exec_ms\tenforced_ms\tenforced_from\texec_before_enforced\tprobe_connections\tobserver_reached\tflips_back\tlast_exec_result\tlast_start_result\trules_ms\tagent_start_ms\tload_at_start\n' > "$samples"
: > "$watch_file"
kubectl --context "$CTX" -n "$ns" get pods --watch-only \
  -o jsonpath='{.metadata.name} {.status.conditions[?(@.type=="Ready")].status}{"\n"}' 2> /dev/null > >(python3 -u "$HERE/stamp.py" > "$watch_file") &
watch_pid=$!
sleep 2

failed=0
for i in $(seq 1 "$reps"); do
  for kind in warm cold; do
    sample "$kind" "$i"
    rc=$?
    if [ "$rc" = 2 ]; then scenario_end; fi
    if [ "$rc" != 0 ]; then failed=$((failed + 1)); check "m-window-$kind-$i" fail "sample not completed"; fi
  done
done

{
  echo "# Window between pod start and policy enforcement"
  echo
  echo "Namespace $ns (pool $pool), target $PROBE_IP:$probe_port, $reps samples of each kind, internal_cidrs=$(internal_cidrs_value)."
  echo
} > "$summary"
summarize warm
summarize cold
verdict m-window-all-samples "every sample completed ($failed incomplete)" test "$failed" = 0
verdict m-window-enforced-at-end "every sample ended with the probe refused or unreachable in both observers" \
  test "$(awk -F'\t' 'NR > 1 && ($12 == "R" || $13 == "R")' "$samples" | count_lines)" = 0
verdict m-window-no-flip-back "no sample saw the probe reachable again after the first refusal" test "$(awk -F'\t' 'NR > 1 && $11 > 0' "$samples" | count_lines)" = 0
scenario_end
