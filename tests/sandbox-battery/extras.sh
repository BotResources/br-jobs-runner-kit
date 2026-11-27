#!/usr/bin/env bash
set -euo pipefail
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/servers.sh"

use_mode main
kc() { kubectl --context "$CONTEXT" "$@"; }
load_servers_env

rec_pod_ip() {
  kc -n "$NS_TESTSVC" get pod "$REC_POD" -o jsonpath='{.status.podIP}'
}

node_ip_on() {
  docker inspect "$NODE" --format "{{with index .NetworkSettings.Networks \"$1\"}}{{.IPAddress}}{{end}}"
}

resolver_records() {
  printf '%s %s\n' "$GITEA_HOST" "$GIT_IP" "$REGISTRY_HOST" "$GIT_IP" "$ECHO_HOST" "$ECHO_IP" \
    "$PROBE_HOST" "$PROBE_IP" "$STORE_HOST" "$STORE_IP" "$CANARY_NAME" "$EXTRA_CANARY_IP"
  if [ -n "$(rec_pod_ip 2>/dev/null)" ]; then
    printf '%s %s\n' "$POD_TARGET_NAME" "$(rec_pod_ip)" "$SVC_TARGET_NAME" "$REC_CLUSTER_IP" \
      "$NODE_TARGET_NAME" "$(node_ip_on "$SERVERS_NETWORK_MAIN")" "$NODEFIRST_TARGET_NAME" "$(node_ip_on "k3d-$CLUSTER")"
  fi
}

extra_container_ready() {
  container_exists "$1" && [ "$(docker inspect "$1" --format '{{.State.Running}}')" = true ]
}

remove_extra() {
  if container_exists "$1"; then
    [ "$(docker inspect "$1" --format "{{index .Config.Labels \"$EXTRA_LABEL\"}}")" = 1 ] || die "$1 exists and is not made by this script"
    docker rm -f "$1" > /dev/null
  fi
}

create_extra() {
  local name=$1 ip=$2 memory=$3
  shift 3
  preflight
  docker create --name "$name" --network "$SERVERS_NETWORK_MAIN" --ip "$ip" --label "$EXTRA_LABEL=1" \
    --cap-drop ALL --security-opt no-new-privileges --memory "$memory" \
    --sysctl net.ipv4.ip_unprivileged_port_start=0 --user "$PY_UID:$PY_UID" "$@" > /dev/null
}

resolver_up() {
  local dir
  dir=$(mktemp -d)
  resolver_records > "$dir/records.txt"
  remove_extra "$EXTRA_DNS_CONTAINER"
  create_extra "$EXTRA_DNS_CONTAINER" "$EXTRA_DNS_IP" 64m --pull=never "$PYTHON_IMAGE" python3 -I -u /srv/dns.py /srv/records.txt "$DNS_TTL"
  put_files "$EXTRA_DNS_CONTAINER" srv "$PY_UID" "$BATTERY_DIR/servers" dns.py
  put_files "$EXTRA_DNS_CONTAINER" srv "$PY_UID" "$dir" records.txt
  docker start "$EXTRA_DNS_CONTAINER" > /dev/null
  wait_log "$EXTRA_DNS_CONTAINER" READY
  find "$dir" -type f -delete
  rmdir "$dir"
}

canary_cert() {
  local dir=$1
  openssl genrsa -out "$dir/tls.key" 2048 2> /dev/null
  openssl req -new -key "$dir/tls.key" -subj "/CN=$CANARY_NAME" -out "$dir/tls.csr"
  printf 'subjectAltName=DNS:%s\nbasicConstraints=CA:FALSE\nkeyUsage=digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\n' "$CANARY_NAME" > "$dir/tls.ext"
  openssl x509 -req -in "$dir/tls.csr" -CA "$WORK/pki/ca.crt" -CAkey "$WORK/pki/ca.key" -CAcreateserial \
    -days 3 -sha256 -extfile "$dir/tls.ext" -out "$dir/tls.crt" 2> /dev/null
  chmod 644 "$dir/tls.key"
}

canary_up() {
  local dir value=${1:-unset}
  dir=$(mktemp -d)
  canary_cert "$dir"
  remove_extra "$EXTRA_CANARY_CONTAINER"
  create_extra "$EXTRA_CANARY_CONTAINER" "$EXTRA_CANARY_IP" 128m --pull=never --env "CANARY=$value" \
    "$PYTHON_IMAGE" python3 -I -u /srv/canary-echo-server.py
  put_files "$EXTRA_CANARY_CONTAINER" srv "$PY_UID" "$BATTERY_DIR/scenarios" canary-echo-server.py
  put_files "$EXTRA_CANARY_CONTAINER" tls "$PY_UID" "$dir" tls.crt tls.key
  docker start "$EXTRA_CANARY_CONTAINER" > /dev/null
  find "$dir" -type f -delete
  rmdir "$dir"
}

recorders_up() {
  kc -n "$NS_TESTSVC" create configmap rec-code --from-file=probe.py="$BATTERY_DIR/servers/probe.py" --dry-run=client -o yaml | kc apply -f - > /dev/null
  kc apply -f - > /dev/null << YAML
apiVersion: v1
kind: Pod
metadata: {name: $REC_POD, namespace: $NS_TESTSVC, labels: {app: $REC_POD}}
spec:
  containers:
  - name: rec
    image: $PYTHON_IMAGE
    imagePullPolicy: IfNotPresent
    command: [python3, -I, -u, /srv/probe.py, $(echo $REC_PORTS | sed 's/\([0-9]*\)/"\1"/g; s/" "/", "/g')]
    volumeMounts: [{name: code, mountPath: /srv}]
  volumes: [{name: code, configMap: {name: rec-code}}]
---
apiVersion: v1
kind: Pod
metadata: {name: $REC_NODE_POD, namespace: $NS_TESTSVC, labels: {app: $REC_NODE_POD}}
spec:
  hostNetwork: true
  dnsPolicy: ClusterFirstWithHostNet
  containers:
  - name: rec
    image: $PYTHON_IMAGE
    imagePullPolicy: IfNotPresent
    command: [python3, -I, -u, /srv/probe.py, $(echo $REC_PORTS | sed 's/\([0-9]*\)/"\1"/g; s/" "/", "/g')]
    volumeMounts: [{name: code, mountPath: /srv}]
  volumes: [{name: code, configMap: {name: rec-code}}]
---
apiVersion: v1
kind: Service
metadata: {name: $REC_SERVICE, namespace: $NS_TESTSVC}
spec:
  clusterIP: $REC_CLUSTER_IP
  selector: {app: $REC_POD}
  ports:
$(for p in $REC_PORTS; do echo "  - {name: p$p, port: $p, targetPort: $p}"; done)
YAML
  kc -n "$NS_TESTSVC" wait --for=condition=Ready "pod/$REC_POD" "pod/$REC_NODE_POD" --timeout=120s > /dev/null
}

extras_down() {
  remove_extra "$EXTRA_DNS_CONTAINER"
  remove_extra "$EXTRA_CANARY_CONTAINER"
  kc -n "$NS_TESTSVC" delete pod/"$REC_POD" pod/"$REC_NODE_POD" service/"$REC_SERVICE" configmap/rec-code --ignore-not-found > /dev/null
}

case "${1:-}" in
  up) recorders_up; resolver_up; canary_up ;;
  resolver) resolver_up ;;
  canary) canary_up "${2:-unset}" ;;
  down) extras_down ;;
  records) resolver_records ;;
  *) die "usage: extras.sh up | resolver | canary [value] | records | down" ;;
esac
