#!/usr/bin/env bash
set -euo pipefail
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib-cluster.sh"

case "${1:-}" in
  --closed) use_mode closed ;;
  --main) use_mode main ;;
  *) die "usage: teardown.sh --closed | --main" ;;
esac

if cluster_exists "$CLUSTER"; then
  k3d cluster delete "$CLUSTER" >&2
fi
if docker ps -a --format '{{.Names}}' | grep -qx "$STORE_CONTAINER"; then
  docker rm -f "$STORE_CONTAINER" >/dev/null
fi
if [ "$MODE" = closed ] && docker network inspect "$CLOSED_NETWORK" >/dev/null 2>&1; then
  docker network rm "$CLOSED_NETWORK" >/dev/null
fi
log "teardown done: $MODE"
