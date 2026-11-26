#!/bin/sh
set -eu

dir="${NETCHECK_DIR:-/netcheck}"
wait_seconds="${NETCHECK_WAIT_SECONDS:-180}"

probe() {
  for target in $NETCHECK_TARGETS; do
    if nc -w 2 "${target%:*}" "${target##*:}" </dev/null >/dev/null 2>&1; then
      printf '%s reached\n' "$target"
    else
      printf '%s refused\n' "$target"
    fi
  done
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

first_service() {
  printf '%s\n' "$(kubectl get svc "$1" -n "$2" -o jsonpath="{.spec.clusterIP}:{.spec.ports[0].port}")"
}

collect_targets() {
  cat "$dir/extra-targets" >"$work/targets"
  : >"$work/stale"
  kubectl get nodes -o jsonpath='{range .items[*]}{range .status.addresses[*]}{.type}{" "}{.address}{"\n"}{end}{end}' >"$work/nodes"
  while read -r type address; do
    case "$type" in
      InternalIP | ExternalIP) ;;
      *) continue ;;
    esac
    in_list "$address" "$dir/node-addresses" || printf '%s\n' "$address" >>"$work/stale"
    printf '%s:10250\n%s:6443\n' "$address" "$address" >>"$work/targets"
  done <"$work/nodes"
  first_service kubernetes default >>"$work/targets"
  first_service kube-dns kube-system >>"$work/targets"
  printf 'kubernetes.default.svc.cluster.local:443\n' >>"$work/targets"
  kubectl get pods -A -o jsonpath='{range .items[*]}{.status.podIP}{":"}{.spec.containers[0].ports[0].containerPort}{"\n"}{end}' |
    while read -r pod; do
      case "$pod" in
        *[0-9]:[0-9]*)
          printf '%s\n' "$pod" >>"$work/targets"
          break
          ;;
      esac
    done || true
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
  control
  while read -r ns mode; do
    [ -n "$ns" ] || continue
    run_probe "$ns"
    report "$ns" "$mode"
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
