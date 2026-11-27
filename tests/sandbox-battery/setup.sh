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
remove_incluster_gitea
setup_store
start_servers
apply_toolbox
wait_pods_ready
setup_gitea
setup_seed

CREDS="$(creds_file)"
cat <<TXT
cluster:        $CLUSTER (kubectl context $CONTEXT, via node container: $KC_VIA_NODE)
store in pods:  $STORE_SERVICE.$NS_STORE.svc.cluster.local:$STORE_PORT, bucket $BUCKET
store on host:  $( [ "$MODE_NAME" = main ] && echo "http://127.0.0.1:$STORE_HOST_PORT" || echo "none (internal network)" )
git:            $GITEA_URL, container $GITEA_CONTAINER (CA file $WORK/ca.crt), repo $GITEA_ORG/$GITEA_REPO
registry:       $REGISTRY_URL (same container)
echo, probe:    $ECHO_HOST ($ECHO_CONTAINER), $PROBE_HOST ($PROBE_CONTAINER, ports $PROBE_PORTS)
networks:       servers $SERVERS_NETWORK $SERVERS_CIDR, cluster $CLUSTER_NETWORK $CLUSTER_NETWORK_CIDR, pods $POD_CIDR, services $SERVICE_CIDR
node addresses: $NODE_ADDRESSES
node cidrs:     $NODE_CIDRS
dns:            $DNS_IP ($DNS_CONTAINER); names and IPs in $(servers_file)
credentials:    $CREDS (mode 600, never in the repo)
toolbox pods:   $NS_TOOLS/$TOOLBOX_POD (git, curl), $NS_TOOLS/pytool (python, pip)
TXT
