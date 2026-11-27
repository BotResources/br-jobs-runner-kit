#!/usr/bin/env bash
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib-cluster.sh"

ensure_all_images() {
  local img
  for img in "$K3S_IMAGE" $K3D_IMAGES "$SEAWEED_IMAGE" $IMAGES_FOR_CLUSTER; do
    ensure_image "$img"
  done
}

create_closed_network() {
  if docker network inspect "$CLOSED_NETWORK" >/dev/null 2>&1; then
    log "network exists: $CLOSED_NETWORK"
    return 0
  fi
  docker network create --internal --subnet "$CLOSED_SUBNET" --ip-range "$CLOSED_IP_RANGE" "$CLOSED_NETWORK" >/dev/null
}

create_cluster() {
  if cluster_exists "$CLUSTER"; then
    log "cluster exists: $CLUSTER"
    k3d cluster start "$CLUSTER" >/dev/null 2>&1 || true
    return 0
  fi
  preflight
  local network_args=()
  if [ "$MODE" = closed ]; then
    network_args=(--network "$CLOSED_NETWORK" --k3s-arg "--node-ip=$CLOSED_NODE_IP@server:0" --k3s-arg "--flannel-iface=eth0@server:0")
  fi
  local started
  started="$(date +%s)"
  k3d cluster create "$CLUSTER" --servers 1 --agents 0 --no-lb --image "$K3S_IMAGE" \
    --k3s-arg "--disable=traefik@server:0" \
    --k3s-arg "--disable=metrics-server@server:0" \
    --k3s-arg "--disable=servicelb@server:0" \
    --kubeconfig-switch-context=false ${network_args[@]+"${network_args[@]}"} >&2
  record "cluster_create_$MODE" "$(($(date +%s) - started))" seconds
}

wait_pods_ready() {
  kc -n "$NS_TOOLS" wait --for=condition=Ready "pod/$TOOLBOX_POD" pod/pytool --timeout=180s >&2
}

remove_incluster_gitea() {
  kc -n "$NS_TESTSVC" delete deployment/"$GITEA_SERVICE" service/"$GITEA_SERVICE" secret/gitea-secrets secret/gitea-tls \
    configmap/test-ca --ignore-not-found >&2
}

apply_toolbox() {
  local want have
  want="$GIT_IP $ECHO_IP $PROBE_IP $STORE_IP"
  have="$(kc -n "$NS_TOOLS" get pod "$TOOLBOX_POD" -o jsonpath='{.spec.hostAliases[*].ip}' 2>/dev/null || true)"
  if [ "$have" != "$want" ]; then
    kc -n "$NS_TOOLS" delete pod "$TOOLBOX_POD" pytool --ignore-not-found --wait=true >&2
  fi
  apply_rendered "$BATTERY_DIR/manifests/20-toolbox.yaml" >&2
}
