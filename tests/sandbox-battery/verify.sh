#!/usr/bin/env bash
set -euo pipefail
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/servers.sh"

MODE_NAME=main
for arg in "$@"; do
  case "$arg" in
    --closed) MODE_NAME=closed ;;
    *) die "usage: verify.sh [--closed]" ;;
  esac
done
use_mode "$MODE_NAME"
if [ "$MODE" = closed ]; then
  CREDS="$WORK/creds-closed.env"
else
  CREDS="$WORK/creds.env"
fi
while IFS='=' read -r key value; do
  export "$key=$value"
done <"$CREDS"
load_servers_env

reason_of() {
  local line
  line="$(echo "$1" | grep -iE 'fatal|error|certificate|denied|unauthorized|refused|could not|timed out|http [0-9]{3}' | head -1)"
  echo "${line:-$(echo "$1" | tail -1)}" | cut -c1-140
}

expect_ok() {
  local name="$1"
  shift
  local out
  if out="$("$@" 2>&1)"; then
    check "$MODE/$name" pass "$(echo "$out" | tail -1 | cut -c1-160)"
  else
    check "$MODE/$name" fail "$(reason_of "$out")"
  fi
}

expect_fail() {
  local name="$1"
  shift
  local out
  if out="$("$@" 2>&1)"; then
    check "$MODE/$name" fail "unexpected success: $(echo "$out" | tail -1 | cut -c1-120)"
  else
    check "$MODE/$name" pass "refused: $(reason_of "$out")"
  fi
}

s3_status() {
  local method="$1" path="$2" secret="$3" payload="${4:-}" sha out_file="${5:-/dev/null}"
  sha="$(printf %s "$payload" | shasum -a 256 | cut -d' ' -f1)"
  local data_args=()
  if [ -n "$payload" ]; then
    data_args=(--data-binary "$payload")
  fi
  tbx curl -sS -o "$out_file" -w '%{http_code}' -X "$method" --aws-sigv4 "aws:amz:$S3_REGION:s3" \
    --user "$S3_ACCESS_KEY:$secret" -H "x-amz-content-sha256: $sha" ${data_args[@]+"${data_args[@]}"} \
    "${S3_ENDPOINT_OVERRIDE:-$S3_ENDPOINT_IN_CLUSTER}$path"
}

want_status() {
  local want="$1" got
  got="$("${@:2}")"
  echo "http $got"
  [ "$got" = "$want" ]
}

check_nodes() {
  kc get nodes --no-headers | grep -q ' Ready'
}

check_store() {
  local key="/$S3_BUCKET/saves/probe.txt" payload got
  payload="probe-$(date +%s)"
  expect_ok "store bucket exists" want_status 200 s3_status GET "/$S3_BUCKET/?list-type=2&max-keys=1" "$S3_SECRET_KEY"
  expect_ok "store put" want_status 200 s3_status PUT "$key" "$S3_SECRET_KEY" "$payload"
  got="$(tbx sh -c "rm -f /tmp/got.txt" >/dev/null; s3_status GET "$key" "$S3_SECRET_KEY" "" /tmp/got.txt >/dev/null; tbx cat /tmp/got.txt)"
  if [ "$got" = "$payload" ]; then check "$MODE/store get equals put" pass "$got"; else check "$MODE/store get equals put" fail "got '$got'"; fi
  expect_ok "store delete" want_status 204 s3_status DELETE "$key" "$S3_SECRET_KEY"
  expect_ok "store missing bucket is refused" want_status 404 s3_status GET "/no-such-bucket-sbx/" "$S3_SECRET_KEY"
  expect_ok "store wrong secret is refused" want_status 403 s3_status GET "/$S3_BUCKET/?list-type=2" "wrongsecretwrongsecretwrongsecret0000"
}

check_git() {
  local url_auth="https://$GITEA_USER:$GITEA_TOKEN@$GITEA_HOST/$GITEA_ORG/hello-rust.git"
  local url_plain="https://$GITEA_HOST/$GITEA_ORG/hello-rust.git"
  expect_ok "git clone private repo with token and test CA" tbx sh -c "rm -rf /tmp/clone-ok && git clone -q '$url_auth' /tmp/clone-ok && ls /tmp/clone-ok | tr '\n' ' '"
  expect_fail "git clone private repo without credential" tbx sh -c "rm -rf /tmp/clone-anon && GIT_TERMINAL_PROMPT=0 git clone -q '$url_plain' /tmp/clone-anon"
  expect_fail "git clone with token but without the test CA" tbx sh -c "rm -rf /tmp/clone-noca && env -u GIT_SSL_CAINFO git clone -q '$url_auth' /tmp/clone-noca"
  expect_ok "git push a branch with token" tbx sh -c "cd /tmp/clone-ok && git config user.name sbx && git config user.email sbx@sbx.test && git checkout -q -b verify-$(date +%s) && git commit -q --allow-empty -m verify && git push -q origin HEAD && echo pushed"
}

check_packages() {
  local base="$REGISTRY_URL/api/packages/$GITEA_ORG"
  expect_ok "pypi download with pip" kc exec -n "$NS_TOOLS" pytool -- sh -c "rm -rf /tmp/pipdl && pip download -q --disable-pip-version-check --no-deps -d /tmp/pipdl --index-url 'https://$GITEA_USER:$GITEA_TOKEN@$GITEA_HOST/api/packages/$GITEA_ORG/pypi/simple' $PKG_PYPI && ls /tmp/pipdl"
  expect_ok "npm tarball download" tbx sh -c "curl -sSf -u '$GITEA_USER:$GITEA_TOKEN' -o /tmp/n.tgz '$base/npm/$PKG_NPM/-/$PKG_VERSION/$PKG_NPM-$PKG_VERSION.tgz' && tar tzf /tmp/n.tgz | tr '\n' ' '"
  expect_ok "cargo crate download" tbx sh -c "curl -sSf -u '$GITEA_USER:$GITEA_TOKEN' -o /tmp/c.crate '$base/cargo/api/v1/crates/$PKG_CARGO/$PKG_VERSION/download' && tar tzf /tmp/c.crate | tr '\n' ' '"
  expect_ok "cargo sparse index entry" tbx sh -c "curl -sSf -u '$GITEA_USER:$GITEA_TOKEN' '$base/cargo/sb/x-/$PKG_CARGO'"
  expect_fail "package download without credential" tbx sh -c "curl -sSf -o /dev/null '$base/npm/$PKG_NPM/-/$PKG_VERSION/$PKG_NPM-$PKG_VERSION.tgz'"
}

dns_answer() {
  kc exec -i -n "$NS_TOOLS" pytool -- python3 -I - "$DNS_IP" "$1" <"$BATTERY_DIR/servers/dnsq.py"
}

want_dns() {
  local got
  got="$(dns_answer "$1")"
  echo "$got"
  [ "$got" = "$2" ]
}

echo_roundtrip() {
  local out
  out="$(tbx curl -sS --max-time 8 -X POST -H 'X-Sbx-Check: header-value' --data 'body-value' "https://$ECHO_HOST/a/b?x=1")" || return 1
  echo "$out" | jq -e '.method == "POST" and .path == "/a/b?x=1" and .body == "body-value" and ([.headers[] | select(.[0] == "X-Sbx-Check" and .[1] == "header-value")] | length == 1)' >/dev/null || return 1
  echo "echo returned method, path, header and body"
}

counts_connection() {
  local container="$1" before after
  shift
  before="$(conn_count "$container")"
  "$@" >/dev/null || return 1
  after="$(conn_count "$container")"
  echo "connections $before -> $after"
  [ "$after" -gt "$before" ]
}

probe_logged_port() {
  tbx curl -sS --max-time 8 -o /dev/null "http://$PROBE_HOST:$1/" || return 1
  [ -n "$(docker logs "$PROBE_CONTAINER" 2>&1 | grep "CONN .* port=$1")" ]
}

store_by_name() {
  S3_ENDPOINT_OVERRIDE="http://$STORE_HOST:$STORE_PORT" want_status 200 s3_status GET "/$S3_BUCKET/?list-type=2&max-keys=1" "$S3_SECRET_KEY"
}

check_servers() {
  expect_ok "servers apart from node, pod and service ranges" ranges_ok
  expect_ok "each node has a second address on the servers network, listed in NODE_CIDRS" nodes_on_servers_network
  expect_ok "echo sees the node as client (pods reach servers through the node)" echo_sees_node
  expect_ok "dns stub answers the git name" want_dns "$GITEA_HOST" "rcode=0 ips=$GIT_IP"
  expect_ok "dns stub answers the registry name" want_dns "$REGISTRY_HOST" "rcode=0 ips=$GIT_IP"
  expect_ok "dns stub answers the echo name" want_dns "$ECHO_HOST" "rcode=0 ips=$ECHO_IP"
  expect_ok "dns stub answers the probe name" want_dns "$PROBE_HOST" "rcode=0 ips=$PROBE_IP"
  expect_ok "dns stub answers the store name" want_dns "$STORE_HOST" "rcode=0 ips=$STORE_IP"
  expect_ok "dns stub refuses a public name (NXDOMAIN)" want_dns example.com "rcode=3 ips="
  expect_ok "echo returns method, path, header, body over TLS by name" echo_roundtrip
  expect_ok "echo counts one more connection per new call" counts_connection "$ECHO_CONTAINER" tbx curl -sS --max-time 8 "https://$ECHO_HOST/count"
  expect_fail "echo by IP literal fails certificate check" tbx curl -sS --max-time 8 -o /dev/null "https://$ECHO_IP/"
  expect_fail "echo by name without the test CA fails" tbx sh -c "env -u CURL_CA_BUNDLE curl -sS --max-time 8 -o /dev/null https://$ECHO_HOST/"
  expect_ok "probe answers and counts a connection" counts_connection "$PROBE_CONTAINER" tbx curl -sS --max-time 8 "http://$PROBE_HOST/"
  expect_ok "probe records a connection on port 443" probe_logged_port 443
  expect_ok "store reachable by name" store_by_name
}

on_network() {
  local ip
  ip="$(ip_on "$2" "$1")"
  [ -n "$ip" ] && [ "$(docker inspect "$1" --format '{{len .NetworkSettings.Networks}}')" = "$3" ] && echo "$1 $ip on $2"
}

ranges_ok() {
  local node_podcidr service_ip
  node_podcidr="$(kc get node -o jsonpath='{.items[0].spec.podCIDR}')"
  service_ip="$(kc -n default get svc kubernetes -o jsonpath='{.spec.clusterIP}')"
  python3 -I - "$SERVERS_CIDR" "$CLUSTER_NETWORK_CIDR" "$POD_CIDR" "$SERVICE_CIDR" "$node_podcidr" "$service_ip" "$PROBE_IP" "$NODE_CIDRS" <<'PY'
import ipaddress
import sys

servers, cluster, pod, service, node_pod, service_ip, probe, node_cidrs = sys.argv[1:]
net = ipaddress.ip_network
servers, cluster, pod, service = net(servers), net(cluster), net(pod), net(service)
assert net(node_pod).subnet_of(pod), "node pod range outside POD_CIDR"
assert ipaddress.ip_address(service_ip) in service, "service address outside SERVICE_CIDR"
for other in (cluster, pod, service):
    assert not servers.overlaps(other), "SERVERS_CIDR overlaps %s" % other
for entry in node_cidrs.split(","):
    if entry != str(cluster):
        assert net(entry).subnet_of(cluster) or net(entry).subnet_of(servers), "node entry outside both networks: " + entry
assert ipaddress.ip_address(probe) in cluster, "probe outside the cluster network"
print("servers %s apart from cluster %s, pods %s, services %s; probe %s inside the cluster network" % (servers, cluster, pod, service, probe))
PY
}

nodes_on_servers_network() {
  local node cluster_ip servers_ip
  for node in $(node_containers); do
    cluster_ip="$(ip_on "$(store_network)" "$node")"
    servers_ip="$(ip_on "$SERVERS_NETWORK" "$node")"
    [ -n "$cluster_ip" ] && [ -n "$servers_ip" ] || return 1
    case ",$NODE_CIDRS," in *",$cluster_ip/32,"*) ;; *) return 1 ;; esac
    case ",$NODE_CIDRS," in *",$servers_ip/32,"*) ;; *) return 1 ;; esac
    echo "$node $(store_network)=$cluster_ip $SERVERS_NETWORK=$servers_ip (both in NODE_CIDRS as /32)"
  done
}

echo_sees_node() {
  tbx curl -sS --max-time 8 -o /dev/null "https://$ECHO_HOST/who" || return 1
  local client node
  client="$(docker logs "$ECHO_CONTAINER" 2>&1 | grep ' REQ ' | tail -1 | awk '{print $3}')"
  node="$(ip_on "$SERVERS_NETWORK" "$NODE")"
  echo "echo saw client $client, node address on the servers network $node"
  [ "$client" = "$node" ]
}

container_has_no_route() {
  docker exec "$1" python3 -I -c "import socket; socket.create_connection(('1.1.1.1', 443), 5)"
}

check_closed() {
  expect_ok "closed: network is internal" test "$(docker network inspect "$CLOSED_NETWORK" --format '{{.Internal}}')" = true
  expect_ok "closed: servers network is internal" test "$(docker network inspect "$SERVERS_NETWORK" --format '{{.Internal}}')" = true
  local container
  for container in "$GITEA_CONTAINER" "$ECHO_CONTAINER" "$DNS_CONTAINER"; do
    expect_ok "closed: $container only on the internal servers network" on_network "$container" "$SERVERS_NETWORK" 1
  done
  expect_ok "closed: probe only on the internal cluster network" on_network "$PROBE_CONTAINER" "$CLOSED_NETWORK" 1
  expect_fail "closed: no route to 1.1.1.1 from a pod" tbx curl -sS --max-time 8 -o /dev/null https://1.1.1.1
  expect_fail "closed: no public name from a pod" tbx curl -sS --max-time 8 -o /dev/null https://example.com
  expect_fail "closed: no route to the internet from the echo container" container_has_no_route "$ECHO_CONTAINER"
  expect_ok "closed: git reachable by name" tbx curl -sS --max-time 8 -o /dev/null --cacert /ca/ca.crt "$GITEA_URL/api/healthz"
  expect_ok "closed: echo reachable by name" echo_roundtrip
  expect_ok "closed: probe reachable by name" counts_connection "$PROBE_CONTAINER" tbx curl -sS --max-time 8 "http://$PROBE_HOST/"
  expect_ok "closed: store reachable" want_status 200 s3_status GET "/$S3_BUCKET/?list-type=2&max-keys=1" "$S3_SECRET_KEY"
}

check_netpol() {
  local ip started deadline got=refused
  ip="$(kc -n "$NS_STORE" get svc "$STORE_SERVICE" -o jsonpath='{.spec.clusterIP}')"
  kc delete namespace "$NS_NETPOL_PROBE" --ignore-not-found --wait=true >/dev/null
  kc create namespace "$NS_NETPOL_PROBE" >/dev/null
  render "$BATTERY_DIR/manifests/30-netpol-probe.yaml" | kc apply -f - >/dev/null
  kc -n "$NS_NETPOL_PROBE" wait --for=condition=Ready pod/probe --timeout=120s >/dev/null
  probe_store() {
    kc -n "$NS_NETPOL_PROBE" exec probe -- curl -s --max-time 3 -o /dev/null "http://$ip:$STORE_PORT/"
  }
  expect_ok "netpol probe reaches the store service before policy" probe_store
  render "$BATTERY_DIR/manifests/31-netpol-deny.yaml" | kc apply -f - >/dev/null
  started="$(date +%s)"
  deadline=$((started + 60))
  while [ "$(date +%s)" -lt "$deadline" ]; do
    if ! probe_store >/dev/null 2>&1; then
      got=enforced
      break
    fi
    sleep 1
  done
  if [ "$got" = enforced ]; then
    check "$MODE/netpol enforced after default-deny" pass "blocked after $(($(date +%s) - started)) s"
    record "netpol_enforce_delay_$MODE" "$(($(date +%s) - started))" seconds
  else
    check "$MODE/netpol enforced after default-deny" fail "still reachable after 60 s"
  fi
  kc delete namespace "$NS_NETPOL_PROBE" --wait=true >/dev/null
}

preflight
expect_ok "cluster node ready" check_nodes
check_store
check_git
check_packages
check_servers
check_netpol
if [ "$MODE" = closed ]; then
  check_closed
fi
log "verify done: $MODE"
