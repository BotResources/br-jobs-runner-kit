#!/usr/bin/env bash
set -euo pipefail
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/servers.sh"
source "$BATTERY_DIR/scenarios/names-chart.env"

MODE_NAME=main
ACTION=
for arg in "$@"; do
  case "$arg" in
    --closed) MODE_NAME=closed ;;
    fetch | render | install | objects | status) ACTION=$arg ;;
    *) die "usage: chart.sh [--closed] fetch | render | install | objects | status" ;;
  esac
done
[ -n "$ACTION" ] || die "usage: chart.sh [--closed] fetch | render | install | objects | status"
use_mode "$MODE_NAME"
load_servers_env
if [ "$MODE" = main ]; then
  RESOLVER_IP=$EXTRA_DNS_IP
else
  RESOLVER_IP=$DNS_IP
fi

indent() { sed "s/^/$1/"; }

make_proxy_ca() {
  mkdir -p "$PKI_DIR"
  if [ -s "$PKI_DIR/sbx-proxy-ca.crt" ] && openssl x509 -in "$PKI_DIR/sbx-proxy-ca.crt" -noout -checkend 86400 > /dev/null 2>&1; then
    return 0
  fi
  openssl ecparam -name prime256v1 -genkey -noout -out "$PKI_DIR/sbx-proxy-ca.key" 2> /dev/null
  openssl req -x509 -new -key "$PKI_DIR/sbx-proxy-ca.key" -sha256 -days 30 -subj "/CN=sbx-proxy-test-ca" \
    -addext "basicConstraints=critical,CA:TRUE" -addext "keyUsage=critical,keyCertSign,cRLSign" -out "$PKI_DIR/sbx-proxy-ca.crt"
  chmod 600 "$PKI_DIR/sbx-proxy-ca.key"
}

node_addresses_all() {
  local node
  for node in $(kc get nodes -o jsonpath='{.items[*].metadata.name}'); do
    docker inspect "$node" --format '{{range $name, $net := .NetworkSettings.Networks}}{{$net.IPAddress}} {{end}}'
  done | xargs -n1
}

internal_cidrs() {
  local entry
  echo "$POD_CIDR"
  echo "$SERVICE_CIDR"
  for entry in ${NODE_CIDRS//,/ }; do echo "$entry"; done
}

cidr_lines() { sed 's/^/    - /'; }

render_values() {
  local template=$1 out=$2
  make_proxy_ca
  export AGENT_IMAGE CHECK_IMAGE KEEPER_IMAGE POD_CIDR SERVICE_CIDR RESOLVER_IP CLOSED_MIRROR_HOST CLOSED_PULL_SECRET
  export CA_PEM_YAML NODE_ADDRESSES_YAML INTERNAL_CIDRS_YAML API_SERVER_CIDR STORE_CIDR NETCHECK_TARGET
  CA_PEM_YAML=$(indent '              ' < "$PKI_DIR/sbx-proxy-ca.crt")
  NODE_ADDRESSES_YAML=$(node_addresses_all | cidr_lines)
  INTERNAL_CIDRS_YAML=$(internal_cidrs | cidr_lines)
  API_SERVER_CIDR="$(node_addresses_all | sed -n 1p)/32"
  STORE_CIDR="$(ip_on "$(store_network)" "$STORE_CONTAINER")/32"
  NETCHECK_TARGET="$(ip_on "$(store_network)" "$STORE_CONTAINER"):$STORE_PORT"
  render "$template" > "$out"
  log "rendered $out"
}

render_all() {
  if [ "$MODE" = main ]; then
    render_values "$BATTERY_DIR/values-test.yaml" "$CHART_VALUES_FILE"
  else
    render_values "$BATTERY_DIR/values-test.yaml" "$CLOSED_BASE_VALUES_FILE"
    render_values "$BATTERY_DIR/values-closed.yaml" "$CLOSED_VALUES_FILE"
  fi
  echo "internal_cidrs: $(internal_cidrs | xargs | tr ' ' ',')"
}

set_values_args() {
  if [ "$MODE" = closed ]; then
    VALUES_ARGS=(-f "$CLOSED_BASE_VALUES_FILE" -f "$CLOSED_VALUES_FILE")
  else
    VALUES_ARGS=(-f "$CHART_VALUES_FILE")
  fi
}

fetch_chart() {
  mkdir -p "$WORK/chart-oci"
  if [ -e "$CHART_DIR" ]; then mv "$CHART_DIR" "$CHART_DIR.replaced-$(date +%Y%m%d-%H%M%S)"; fi
  helm pull "$CHART_OCI" --version "$CHART_VERSION" --untar --untardir "$WORK/chart-oci"
  log "chart $CHART_VERSION in $WORK/chart-oci"
}

put_secret() {
  local namespace=$1 name=$2
  shift 2
  kc -n "$namespace" create secret generic "$name" "$@" --dry-run=client -o yaml | kc apply -f - > /dev/null
}

egress_objects() {
  local namespace=$1 name token
  token=$(random_alnum 32)
  kc -n "$namespace" get secret "$PROXY_RELOAD_SECRET" > /dev/null 2>&1 || put_secret "$namespace" "$PROXY_RELOAD_SECRET" --from-literal=token="$token"
  put_secret "$namespace" "$PROXY_CA_KEY_SECRET" --from-file=ca.key="$PKI_DIR/sbx-proxy-ca.key"
  kc -n "$namespace" create configmap "$UPSTREAM_CA_MAP" --from-file=ca.crt="$PKI_DIR/ca.crt" --dry-run=client -o yaml | kc apply -f - > /dev/null
  case "$namespace" in
    sbx-canary-allow-egress) put_secret "$namespace" "$CANARY_SECRET_NAME" --from-literal="$CANARY_SECRET_KEY=brcanary-$(random_alnum 32)" ;;
    sbx-pivot-full-egress) put_secret "$namespace" pivot-credentials --from-literal=token="pivot-$(random_alnum 32)" ;;
    sbx-platform-allow-egress)
      . "$WORK/creds.env"
      put_secret "$namespace" "$PLATFORM_SECRET_NAME" --from-literal="$PLATFORM_GIT_SECRET_KEY=$GITEA_TOKEN" --from-literal="$PLATFORM_PKG_SECRET_KEY=$GITEA_TOKEN"
      ;;
  esac
}

store_credentials() {
  local file=$WORK/creds.env
  [ "$MODE" = closed ] && file=$CLOSED_CREDS_ENV
  (
    . "$file"
    put_secret "$SYSTEM_NS" "$KEEPER_STORE_CREDENTIALS_SECRET" --from-literal="AWS_ACCESS_KEY_ID=${S3_ACCESS_KEY:?}" --from-literal="AWS_SECRET_ACCESS_KEY=${S3_SECRET_KEY:?}"
  )
}

all_objects() {
  local namespace
  for namespace in $(kc get ns -l "$LBL_EGRESS=true" -o jsonpath='{.items[*].metadata.name}'); do
    egress_objects "$namespace"
  done
  store_credentials
}

install_chart() {
  preflight
  render_all
  set_values_args
  local release_ns=$HELM_NAMESPACE
  helm --kube-context "$CONTEXT" upgrade --install "$HELM_RELEASE" "$CHART_DIR" -n "$release_ns" "${VALUES_ARGS[@]}" --set-string "$VALUES_NETWORK_CHECK_RUNID=$(date +%s)"
  for _ in $(seq 1 30); do
    kc get ns "$SYSTEM_NS" > /dev/null 2>&1 && break
    sleep 1
  done
  all_objects
}

show_status() {
  kc get pods -A -l "$LBL_ROLE=$ROLE_PROXY" -o wide
  kc -n "$SYSTEM_NS" get pods
  kc get pods -A -l "$LBL_POOL=warm" --no-headers | awk '{print $1, $2, $3, $4}'
}

case "$ACTION" in
  fetch) fetch_chart ;;
  render) render_all ;;
  install) install_chart ;;
  objects) all_objects ;;
  status) show_status ;;
esac
