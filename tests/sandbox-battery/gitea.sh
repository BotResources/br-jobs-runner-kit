#!/usr/bin/env bash
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/store.sh"
source "$BATTERY_DIR/servers.sh"

GITEA_UID=1000
GITEA_URL="https://$GITEA_HOST"
REGISTRY_URL="https://$REGISTRY_HOST"

creds_file() {
  if [ "$MODE" = closed ]; then
    echo "$WORK/creds-closed.env"
  else
    echo "$WORK/creds.env"
  fi
}

load_or_make_passwords() {
  local file
  file="$(creds_file)"
  if [ -s "$file" ]; then
    GITEA_ADMIN_PASSWORD="$(grep '^GITEA_ADMIN_PASSWORD=' "$file" | cut -d= -f2-)"
    GITEA_PASSWORD="$(grep '^GITEA_PASSWORD=' "$file" | cut -d= -f2-)"
    GITEA_TOKEN="$(grep '^GITEA_TOKEN=' "$file" | cut -d= -f2-)"
  fi
  GITEA_ADMIN_PASSWORD="${GITEA_ADMIN_PASSWORD:-$(random_alnum 24)}"
  GITEA_PASSWORD="${GITEA_PASSWORD:-$(random_alnum 24)}"
}

start_gitea() {
  if container_reusable "$GITEA_CONTAINER" "$(servers_network)"; then
    log "reuse $GITEA_CONTAINER"
    return 0
  fi
  remove_own_container "$GITEA_CONTAINER"
  local tmpfs_opts="rw,exec,uid=$GITEA_UID,gid=$GITEA_UID,mode=0755"
  create_container "$GITEA_CONTAINER" "$(servers_network)" 768m \
    --tmpfs "/var/lib/gitea:$tmpfs_opts,size=$GITEA_TMPFS_SIZE" --tmpfs "/etc/gitea:$tmpfs_opts,size=16m" \
    -e GITEA__server__PROTOCOL=https -e "GITEA__server__DOMAIN=$GITEA_HOST" -e "GITEA__server__ROOT_URL=$GITEA_URL/" \
    -e GITEA__server__HTTP_ADDR=0.0.0.0 -e GITEA__server__HTTP_PORT=443 \
    -e GITEA__server__CERT_FILE=/tls/server.crt -e GITEA__server__KEY_FILE=/tls/server.key \
    -e GITEA__server__DISABLE_SSH=true -e GITEA__server__START_SSH_SERVER=false -e GITEA__server__OFFLINE_MODE=true \
    -e GITEA__database__DB_TYPE=sqlite3 -e GITEA__database__PATH=/var/lib/gitea/data/gitea.db \
    -e GITEA__security__INSTALL_LOCK=true -e GITEA__service__DISABLE_REGISTRATION=true \
    -e GITEA__service__REQUIRE_SIGNIN_VIEW=true -e GITEA__service__DEFAULT_KEEP_EMAIL_PRIVATE=true \
    -e GITEA__picture__DISABLE_GRAVATAR=true -e GITEA__actions__ENABLED=false \
    -e GITEA__cron_0X2E_update_checker__ENABLED=false -e GITEA__repository__DEFAULT_PRIVATE=private \
    -e "GITEA__security__SECRET_KEY=$(random_alnum 48)" -e "GITEA__security__INTERNAL_TOKEN=$(random_alnum 105)" \
    "$GITEA_IMAGE"
  put_files "$GITEA_CONTAINER" tls "$GITEA_UID" "$PKI_DIR" server.crt server.key
  docker start "$GITEA_CONTAINER" >/dev/null
}

wait_gitea_up() {
  local _
  for _ in $(seq 1 90); do
    if [ "$(docker exec "$GITEA_CONTAINER" curl -sk -o /dev/null -w '%{http_code}' https://127.0.0.1/api/healthz 2>/dev/null)" = 200 ]; then
      return 0
    fi
    sleep 2
  done
  die "gitea not up"
}

start_servers() {
  create_servers_network
  local node
  for node in $(node_containers); do
    attach_to_servers_network "$node"
  done
  attach_to_servers_network "$STORE_CONTAINER"
  start_gitea
  wait_gitea_up
  start_echo
  start_probe
  start_dns
  write_servers_env
  load_servers_env
}

api() {
  local user="$1" password="$2" method="$3" path="$4" body="${5:-}"
  if [ -n "$body" ]; then
    tbx curl -sS -u "$user:$password" -X "$method" -H 'Content-Type: application/json' -d "$body" "$GITEA_URL/api/v1$path"
  else
    tbx curl -sS -u "$user:$password" -X "$method" "$GITEA_URL/api/v1$path"
  fi
}

token_valid() {
  [ "$(tbx curl -s -o /dev/null -w '%{http_code}' -H "Authorization: token $GITEA_TOKEN" "$GITEA_URL/api/v1/user")" = 200 ]
}

create_user() {
  local name="$1" password="$2" admin_flag="$3"
  docker exec "$GITEA_CONTAINER" gitea admin user create \
    --username "$name" --password "$password" --email "$name@sbx.test" \
    --must-change-password=false $admin_flag >/dev/null 2>&1 ||
    log "user $name exists or create refused"
}

create_users_org_token() {
  create_user "$GITEA_ADMIN_USER" "$GITEA_ADMIN_PASSWORD" --admin
  create_user "$GITEA_USER" "$GITEA_PASSWORD" ""
  api "$GITEA_ADMIN_USER" "$GITEA_ADMIN_PASSWORD" POST "/admin/users/$GITEA_USER/orgs" \
    "{\"username\":\"$GITEA_ORG\",\"visibility\":\"private\"}" >/dev/null
  if [ -n "${GITEA_TOKEN:-}" ] && token_valid; then
    log "token still valid"
    return 0
  fi
  api "$GITEA_USER" "$GITEA_PASSWORD" DELETE "/users/$GITEA_USER/tokens/sbx-battery" >/dev/null || true
  GITEA_TOKEN="$(api "$GITEA_USER" "$GITEA_PASSWORD" POST "/users/$GITEA_USER/tokens" \
    '{"name":"sbx-battery","scopes":["write:repository","write:package","read:organization","read:user"]}' | jq -r .sha1)"
  [ -n "$GITEA_TOKEN" ] && [ "$GITEA_TOKEN" != null ] || die "token not created"
}

write_creds() {
  local file
  file="$(creds_file)"
  umask 077
  {
    echo "GITEA_URL=$GITEA_URL"
    echo "GITEA_HOST=$GITEA_HOST"
    echo "REGISTRY_HOST=$REGISTRY_HOST"
    echo "REGISTRY_URL=$REGISTRY_URL"
    echo "GITEA_ADMIN_USER=$GITEA_ADMIN_USER"
    echo "GITEA_ADMIN_PASSWORD=$GITEA_ADMIN_PASSWORD"
    echo "GITEA_USER=$GITEA_USER"
    echo "GITEA_PASSWORD=$GITEA_PASSWORD"
    echo "GITEA_TOKEN=$GITEA_TOKEN"
    echo "GITEA_ORG=$GITEA_ORG"
    echo "GITEA_REPO_URL=$GITEA_URL/$GITEA_ORG/$GITEA_REPO.git"
    echo "CA_FILE=$WORK/ca.crt"
    cat "$(store_env_file)"
  } >"$file"
  chmod 600 "$file"
}

setup_gitea() {
  load_or_make_passwords
  create_users_org_token
  write_creds
}
