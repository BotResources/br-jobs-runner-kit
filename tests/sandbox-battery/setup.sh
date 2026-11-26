#!/usr/bin/env bash
set -euo pipefail
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib-cluster.sh"
source "$BATTERY_DIR/cluster.sh"
source "$BATTERY_DIR/seed.sh"

MODE_NAME=main
for arg in "$@"; do
  case "$arg" in
    --closed) MODE_NAME=closed ;;
    *) die "usage: setup.sh [--closed]" ;;
  esac
done

preflight
ensure_all_images
if [ "$MODE_NAME" = closed ]; then
  create_closed_network
fi
use_mode "$MODE_NAME"
create_cluster
pick_kubectl_route
wait_node_ready
import_images $IMAGES_FOR_CLUSTER
preflight
apply_rendered "$BATTERY_DIR/manifests/00-namespaces.yaml" >&2
make_pki
install_pki
install_gitea_secrets
setup_store
apply_rendered "$BATTERY_DIR/manifests/20-toolbox.yaml" >&2
wait_pods_ready
setup_gitea
setup_seed

CREDS="$(creds_file)"
cat <<TXT
cluster:        $CLUSTER (kubectl context $CONTEXT, via node container: $KC_VIA_NODE)
store in pods:  $STORE_SERVICE.$NS_STORE.svc.cluster.local:$STORE_PORT, bucket $BUCKET
store on host:  $( [ "$MODE_NAME" = main ] && echo "http://127.0.0.1:$STORE_HOST_PORT" || echo "none (internal network)" )
git + registry: $GITEA_URL (CA file $WORK/ca.crt), repo $GITEA_ORG/$GITEA_REPO
credentials:    $CREDS (mode 600, never in the repo)
toolbox pods:   $NS_TOOLS/$TOOLBOX_POD (git, curl), $NS_TOOLS/pytool (python, pip)
TXT
