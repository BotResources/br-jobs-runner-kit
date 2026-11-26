#!/usr/bin/env bash
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib.sh"

use_mode() {
  MODE="$1"
  case "$MODE" in
    main)
      CLUSTER="$CLUSTER_MAIN"
      STORE_CONTAINER="$STORE_CONTAINER_MAIN"
      ;;
    closed)
      CLUSTER="$CLUSTER_CLOSED"
      STORE_CONTAINER="$STORE_CONTAINER_CLOSED"
      ;;
    *) die "unknown mode $MODE" ;;
  esac
  CONTEXT="k3d-$CLUSTER"
  NODE="k3d-$CLUSTER-server-0"
  export MODE CLUSTER STORE_CONTAINER CONTEXT NODE
  pick_kubectl_route
}

pick_kubectl_route() {
  if kubectl --context "$CONTEXT" --request-timeout=5s get --raw /readyz >/dev/null 2>&1; then
    KC_VIA_NODE=0
  else
    KC_VIA_NODE=1
  fi
}

kc() {
  if [ "$KC_VIA_NODE" = 1 ]; then
    docker exec -i "$NODE" kubectl "$@"
  else
    kubectl --context "$CONTEXT" "$@"
  fi
}

apply_file() {
  kc apply -f - <"$1"
}

tbx() {
  kc exec -i -n "$NS_TOOLS" "$TOOLBOX_POD" -- "$@"
}

cluster_exists() {
  k3d cluster list --no-headers 2>/dev/null | awk '{print $1}' | grep -qx "$1"
}

image_local() {
  docker image inspect "$1" >/dev/null 2>&1
}

docker_arch() {
  case "$(docker info --format '{{.Architecture}}')" in
    aarch64 | arm64) echo arm64 ;;
    *) echo amd64 ;;
  esac
}

fetch_image() {
  if ! image_local "$PYTHON_IMAGE"; then
    docker pull "$PYTHON_IMAGE" >&2
  fi
  docker run --rm -i --pull=never "$PYTHON_IMAGE" python3 -I - "$1" "$(docker_arch)" <"$BATTERY_DIR/fetch_image.py" |
    docker load >&2
}

ensure_image() {
  if image_local "$1"; then
    log "image present: $1"
    return 0
  fi
  preflight
  log "fetch once: $1"
  fetch_image "$1"
}

image_digest() {
  docker image inspect "$1" --format '{{.ID}}'
}

import_images() {
  local img
  for img in "$@"; do
    preflight
    log "import $img into $CLUSTER"
    docker save --platform "linux/$(docker_arch)" "$img" | docker exec -i "$NODE" ctr -n k8s.io images import - >&2
  done
}

wait_node_ready() {
  for _ in $(seq 1 60); do
    if kc get nodes 2>/dev/null | grep -q ' Ready'; then
      return 0
    fi
    sleep 3
  done
  die "node of $CLUSTER not ready"
}

wait_rollout() {
  kc -n "$1" rollout status "$2" --timeout="${3:-180s}" >&2
}

render() {
  eval "cat <<__RENDER_END__
$(cat "$1")
__RENDER_END__"
}

apply_rendered() {
  render "$1" | kc apply -f -
}
