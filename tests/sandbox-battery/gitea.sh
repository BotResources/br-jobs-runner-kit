#!/usr/bin/env bash
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/store.sh"
source "$BATTERY_DIR/pki.sh"

GITEA_URL="https://$GITEA_HOST"

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

install_gitea_secrets() {
  if kc -n "$NS_TESTSVC" get secret gitea-secrets >/dev/null 2>&1; then
    return 0
  fi
  kubectl -n "$NS_TESTSVC" create secret generic gitea-secrets \
    --from-literal=secret-key="$(random_alnum 48)" \
    --from-literal=internal-token="$(random_alnum 105)" \
    --dry-run=client -o yaml | kc apply -f - >&2
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
  kc -n "$NS_TESTSVC" exec deploy/"$GITEA_SERVICE" -- gitea admin user create \
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
  apply_rendered "$BATTERY_DIR/manifests/10-gitea.yaml" >&2
  wait_rollout "$NS_TESTSVC" deploy/"$GITEA_SERVICE" 240s
  load_or_make_passwords
  create_users_org_token
  write_creds
}
