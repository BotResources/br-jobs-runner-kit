#!/usr/bin/env bash
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/store.sh"
source "$BATTERY_DIR/pki.sh"

PY_UID=65534

servers_file() {
  if [ "$MODE" = closed ]; then
    echo "$WORK/servers-closed.env"
  else
    echo "$WORK/servers.env"
  fi
}

servers_network() {
  if [ "$MODE" = closed ]; then
    echo "$SERVERS_NETWORK_CLOSED"
  else
    echo "$SERVERS_NETWORK_MAIN"
  fi
}

servers_subnet() {
  if [ "$MODE" = closed ]; then
    echo "$SERVERS_SUBNET_CLOSED"
  else
    echo "$SERVERS_SUBNET_MAIN"
  fi
}

ip_on() {
  docker inspect "$2" --format "{{with index .NetworkSettings.Networks \"$1\"}}{{.IPAddress}}{{end}}"
}

container_ip() {
  ip_on "$(store_network)" "$1"
}

server_ip() {
  ip_on "$(servers_network)" "$1"
}

network_subnet() {
  docker network inspect "$1" --format '{{(index .IPAM.Config 0).Subnet}}'
}

create_servers_network() {
  if docker network inspect "$(servers_network)" >/dev/null 2>&1; then
    log "network exists: $(servers_network)"
    return 0
  fi
  local internal=()
  if [ "$MODE" = closed ]; then
    internal=(--internal)
  fi
  docker network create ${internal[@]+"${internal[@]}"} --subnet "$(servers_subnet)" \
    --label "$SERVER_LABEL=$MODE" "$(servers_network)" >/dev/null
}

attach_to_servers_network() {
  if [ -n "$(server_ip "$1")" ]; then
    return 0
  fi
  docker network connect "$(servers_network)" "$1"
}

container_exists() {
  docker ps -a --format '{{.Names}}' | grep -qx "$1"
}

container_label() {
  docker inspect "$1" --format "{{index .Config.Labels \"$SERVER_LABEL\"}}"
}

container_reusable() {
  container_exists "$1" || return 1
  [ "$(container_label "$1")" = "$MODE" ] || return 1
  [ -n "$(ip_on "$2" "$1")" ] || return 1
  [ "$(docker inspect "$1" --format '{{.State.Running}}')" = true ]
}

remove_own_container() {
  container_exists "$1" || return 0
  [ "$(container_label "$1")" = "$MODE" ] || die "container $1 exists and is not made by this set-up"
  docker rm -f "$1" >/dev/null
}

put_files() {
  local container="$1" dest="$2" owner="$3" dir="$4"
  shift 4
  COPYFILE_DISABLE=1 tar -C "$dir" -c --no-xattrs --uid "$owner" --gid "$owner" -s "#^#$dest/#" "$@" 2>/dev/null | docker cp - "$container:/"
}

create_container() {
  local container="$1" network="$2" memory="$3"
  shift 3
  preflight
  docker create --name "$container" --network "$network" --label "$SERVER_LABEL=$MODE" \
    --cap-drop ALL --security-opt no-new-privileges --memory "$memory" \
    --sysctl net.ipv4.ip_unprivileged_port_start=0 "$@" >/dev/null
}

wait_log() {
  local container="$1" word="$2" _
  for _ in $(seq 1 60); do
    if [ -n "$(docker logs "$container" 2>&1 | grep "$word")" ]; then
      return 0
    fi
    sleep 1
  done
  die "$container did not log $word"
}

start_echo() {
  if container_reusable "$ECHO_CONTAINER" "$(servers_network)"; then
    log "reuse $ECHO_CONTAINER"
    return 0
  fi
  remove_own_container "$ECHO_CONTAINER"
  create_container "$ECHO_CONTAINER" "$(servers_network)" 128m --user "$PY_UID:$PY_UID" "$PYTHON_IMAGE" python3 -I -u /srv/echo.py 443
  put_files "$ECHO_CONTAINER" srv "$PY_UID" "$BATTERY_DIR/servers" echo.py
  put_files "$ECHO_CONTAINER" tls "$PY_UID" "$PKI_DIR" server.crt server.key
  docker start "$ECHO_CONTAINER" >/dev/null
  wait_log "$ECHO_CONTAINER" READY
}

start_probe() {
  if container_reusable "$PROBE_CONTAINER" "$(store_network)"; then
    log "reuse $PROBE_CONTAINER"
    return 0
  fi
  remove_own_container "$PROBE_CONTAINER"
  create_container "$PROBE_CONTAINER" "$(store_network)" 128m --user "$PY_UID:$PY_UID" "$PYTHON_IMAGE" python3 -I -u /srv/probe.py $PROBE_PORTS
  put_files "$PROBE_CONTAINER" srv "$PY_UID" "$BATTERY_DIR/servers" probe.py
  docker start "$PROBE_CONTAINER" >/dev/null
  wait_log "$PROBE_CONTAINER" READY
}

dns_records() {
  local git_ip echo_ip probe_ip store_ip
  git_ip="$(server_ip "$GITEA_CONTAINER")"
  echo_ip="$(server_ip "$ECHO_CONTAINER")"
  probe_ip="$(container_ip "$PROBE_CONTAINER")"
  store_ip="$(server_ip "$STORE_CONTAINER")"
  printf '%s %s\n' "$GITEA_HOST" "$git_ip" "$REGISTRY_HOST" "$git_ip" "$ECHO_HOST" "$echo_ip" \
    "$PROBE_HOST" "$probe_ip" "$STORE_HOST" "$store_ip"
}

start_dns() {
  local records dir
  records="$(dns_records)"
  if container_reusable "$DNS_CONTAINER" "$(servers_network)" && [ "$(docker exec "$DNS_CONTAINER" cat /srv/records.txt)" = "$records" ]; then
    log "reuse $DNS_CONTAINER"
    return 0
  fi
  remove_own_container "$DNS_CONTAINER"
  dir="$(mktemp -d)"
  printf '%s\n' "$records" >"$dir/records.txt"
  create_container "$DNS_CONTAINER" "$(servers_network)" 64m --user "$PY_UID:$PY_UID" "$PYTHON_IMAGE" python3 -I -u /srv/dns.py /srv/records.txt "$DNS_TTL"
  put_files "$DNS_CONTAINER" srv "$PY_UID" "$BATTERY_DIR/servers" dns.py
  put_files "$DNS_CONTAINER" srv "$PY_UID" "$dir" records.txt
  docker start "$DNS_CONTAINER" >/dev/null
  wait_log "$DNS_CONTAINER" READY
  find "$dir" -type f -delete
  rmdir "$dir"
}

node_containers() {
  kc get nodes -o jsonpath='{.items[*].metadata.name}'
}

node_cidrs() {
  local node list=""
  for node in $(node_containers); do
    list="$list $(docker inspect "$node" --format '{{range $name, $net := .NetworkSettings.Networks}}{{$net.IPAddress}}/32 {{end}}')"
  done
  list="$list $(network_subnet "$(store_network)")"
  echo "$list" | xargs | tr ' ' ','
}

node_addresses() {
  local node
  for node in $(node_containers); do
    docker inspect "$node" --format '{{.Name}}: {{range $name, $net := .NetworkSettings.Networks}}{{$name}}={{$net.IPAddress}} {{end}}'
  done
}

write_servers_env() {
  local file
  file="$(servers_file)"
  {
    echo "SERVERS_NETWORK=$(servers_network)"
    echo "SERVERS_CIDR=$(network_subnet "$(servers_network)")"
    echo "CLUSTER_NETWORK=$(store_network)"
    echo "CLUSTER_NETWORK_CIDR=$(network_subnet "$(store_network)")"
    echo "NODE_CIDRS=\"$(node_cidrs)\""
    echo "NODE_ADDRESSES=\"$(node_addresses | tr "\n" ";")\""
    echo "POD_CIDR=$POD_CIDR"
    echo "SERVICE_CIDR=$SERVICE_CIDR"
    echo "CA_FILE=$WORK/ca.crt"
    echo "GIT_NAME=$GITEA_HOST"
    echo "GIT_IP=$(server_ip "$GITEA_CONTAINER")"
    echo "GIT_CONTAINER=$GITEA_CONTAINER"
    echo "REGISTRY_NAME=$REGISTRY_HOST"
    echo "REGISTRY_IP=$(server_ip "$GITEA_CONTAINER")"
    echo "ECHO_NAME=$ECHO_HOST"
    echo "ECHO_IP=$(server_ip "$ECHO_CONTAINER")"
    echo "ECHO_CONTAINER=$ECHO_CONTAINER"
    echo "PROBE_NAME=$PROBE_HOST"
    echo "PROBE_IP=$(container_ip "$PROBE_CONTAINER")"
    echo "PROBE_PORTS=\"$PROBE_PORTS\""
    echo "PROBE_CONTAINER=$PROBE_CONTAINER"
    echo "STORE_NAME=$STORE_HOST"
    echo "STORE_IP=$(server_ip "$STORE_CONTAINER")"
    echo "STORE_PORT=$STORE_PORT"
    echo "DNS_IP=$(server_ip "$DNS_CONTAINER")"
    echo "DNS_CONTAINER=$DNS_CONTAINER"
  } >"$file"
}

load_servers_env() {
  local key value
  while IFS='=' read -r key value; do
    value="${value%\"}"
    export "$key=${value#\"}"
  done <"$(servers_file)"
}

conn_count() {
  docker logs "$1" 2>&1 | grep -c ' CONN ' || true
}

stop_servers() {
  local container
  for container in "$GITEA_CONTAINER" "$ECHO_CONTAINER" "$PROBE_CONTAINER" "$DNS_CONTAINER"; do
    remove_own_container "$container"
  done
}

remove_servers_network() {
  if docker network inspect "$(servers_network)" >/dev/null 2>&1; then
    docker network rm "$(servers_network)" >/dev/null
  fi
}
