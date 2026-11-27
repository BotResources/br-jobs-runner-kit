#!/bin/sh
set -eu

dir="${NETCHECK_DIR:-/netcheck}"
wait_seconds="${NETCHECK_WAIT_SECONDS:-180}"

status_code() {
  printf '%s\n' "$1" | sed -n 1p | awk '$1 ~ /^HTTP\/[0-9]/ && $2 ~ /^[0-9][0-9][0-9]$/ {printf "%s", $2}'
}

connect_state() {
  reply=$(printf 'CONNECT %s HTTP/1.1\r\nHost: %s\r\n\r\n' "$1" "$1" | nc -w 3 "${NETCHECK_PROXY%:*}" "${NETCHECK_PROXY##*:}" 2>/dev/null | tr -d '\r') || reply=""
  case "$(status_code "$reply")" in
    403 | 502) printf 'refused' ;;
    200) printf 'reached' ;;
    *) printf 'no_result' ;;
  esac
}

probe() {
  for target in $NETCHECK_TARGETS; do
    if nc -w 2 "${target%:*}" "${target##*:}" </dev/null >/dev/null 2>&1; then
      printf '%s reached\n' "$target"
    else
      printf '%s refused\n' "$target"
    fi
  done
  if [ -n "${NETCHECK_PROXY:-}" ]; then
    for target in $NETCHECK_TARGETS ${NETCHECK_OFFLIST:-}; do
      printf 'connect:%s %s\n' "$target" "$(connect_state "$target")"
    done
  fi
}

in_list() {
  while read -r line; do
    [ "$line" = "$1" ] && return 0
  done <"$2"
  return 1
}

observed() {
  while read -r name result; do
    if [ "$name" = "$1" ]; then
      printf '%s' "$result"
      return 0
    fi
  done <"$2"
  printf 'no_result'
}

add_target() {
  printf '%s\n' "$2" >>"$work/targets"
  printf '%s\n' "$1" >>"$work/classes"
}

first_service() {
  address=$(kubectl get svc "$1" -n "$2" -o jsonpath="{.spec.clusterIP}:{.spec.ports[0].port}") || address=""
  case "$address" in
    "" | :* | None:*) ;;
    *) add_target "$3" "$address" ;;
  esac
}

collect_targets() {
  : >"$work/targets"
  : >"$work/classes"
  : >"$work/stale"
  while read -r line; do
    if [ -n "$line" ]; then
      add_target targets "$line"
    fi
  done <"$dir/extra-targets"
  kubectl get nodes -o jsonpath='{range .items[*]}{range .status.addresses[*]}{.type}{" "}{.address}{"\n"}{end}{end}' >"$work/nodes" || : >"$work/nodes"
  while read -r type address; do
    case "$type" in
      InternalIP | ExternalIP) ;;
      *) continue ;;
    esac
    in_list "$address" "$dir/node-addresses" || printf '%s\n' "$address" >>"$work/stale"
    add_target node_kubelet "$address:10250"
    add_target node_api "$address:6443"
  done <"$work/nodes"
  first_service kubernetes default api_service
  first_service kube-dns kube-system dns_service
  add_target cluster_name kubernetes.default.svc.cluster.local:443
  kubectl get pods -A -l 'br.sandbox/role!=proxy' -o jsonpath='{range .items[*]}{.status.podIP}{":"}{.spec.containers[0].ports[0].containerPort}{"\n"}{end}' |
    while read -r pod; do
      case "$pod" in
        *[0-9]:[0-9]*)
          add_target pod "$pod"
          break
          ;;
      esac
    done || true
}

missing_classes() {
  for class in targets node_kubelet node_api api_service dns_service cluster_name pod; do
    if ! in_list "$class" "$work/classes"; then
      printf '{"net":"cluster","mode":"all","target":"%s","kind":"target_class","expected":"present","observed":"missing","control":"ok"}\n' "$class"
      printf '1\n' >>"$work/failures"
    fi
  done
}

control() {
  while read -r target; do
    [ -n "$target" ] || continue
    if nc -w 2 "${target%:*}" "${target##*:}" </dev/null >/dev/null 2>&1; then
      printf '%s ok\n' "$target"
    else
      printf '%s control_unavailable\n' "$target"
    fi
  done <"$work/targets" >"$work/control"
}

run_probe() {
  sed "s|__TARGETS__|$(tr '\n' ' ' <"$work/targets")|" "$dir/probe-$1.yaml" | kubectl create -f - >/dev/null
  kubectl wait --for=jsonpath='{.status.phase}'=Succeeded pod/br-sandbox-netcheck-probe -n "$1" --timeout="${wait_seconds}s" >/dev/null || true
  kubectl logs pod/br-sandbox-netcheck-probe -n "$1" >"$work/log-$1" || : >"$work/log-$1"
  kubectl delete pod/br-sandbox-netcheck-probe -n "$1" --wait=false >/dev/null || true
}

report() {
  while read -r target; do
    [ -n "$target" ] || continue
    seen=$(observed "$target" "$work/log-$1")
    state=$(observed "$target" "$work/control")
    printf '{"net":"%s","mode":"%s","target":"%s","kind":"tcp","expected":"refused","observed":"%s","control":"%s"}\n' "$1" "$2" "$target" "$seen" "$state"
    if [ "$seen" != refused ] || [ "$state" != ok ]; then
      printf '1\n' >>"$work/failures"
    fi
  done <"$work/targets"
}

report_connect_line() {
  printf '{"net":"%s","mode":"%s","target":"%s","kind":"connect","expected":"refused","observed":"%s","control":"not_applicable"}\n' "$1" "$2" "$3" "$4"
  if [ "$4" != refused ]; then
    printf '1\n' >>"$work/failures"
  fi
}

report_connect() {
  while read -r target; do
    [ -n "$target" ] || continue
    report_connect_line "$1" "$2" "$target" "$(observed "connect:$target" "$work/log-$1")"
  done <"$work/targets"
  if [ "$2" = allow ]; then
    offlist=$(cat "$dir/offlist-host")
    report_connect_line "$1" "$2" "$offlist" "$(observed "connect:$offlist" "$work/log-$1")"
  fi
}

discover() {
  work=$(mktemp -d)
  : >"$work/failures"
  collect_targets
  if [ -s "$work/stale" ]; then
    while read -r address; do
      printf '{"net":"cluster","mode":"all","target":"%s","kind":"node_list","expected":"listed","observed":"stale","control":"ok"}\n' "$address"
    done <"$work/stale"
    printf '1\n' >>"$work/failures"
  fi
  missing_classes
  control
  while read -r ns mode path; do
    [ -n "$ns" ] || continue
    run_probe "$ns"
    report "$ns" "$mode"
    if [ "$path" = proxy ]; then
      report_connect "$ns" "$mode"
    fi
  done <"$dir/namespaces"
  [ ! -s "$work/failures" ]
}

case "${1:-discover}" in
  probe) probe ;;
  discover) discover ;;
  *)
    printf 'usage: network-check.sh discover|probe\n' >&2
    exit 2
    ;;
esac
