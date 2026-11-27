#!/usr/bin/env bash
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/gitea.sh"

SEED_OUT="$WORK/seed-out"

render_seed() {
  mkdir -p "$SEED_OUT"
  local out="$SEED_OUT/hello-rust-$MODE"
  mkdir -p "$out/src" "$out/.cargo"
  local rel
  for rel in Cargo.toml .cargo/config.toml app.py package.json .npmrc pip.conf .gitignore src/main.rs; do
    render "$BATTERY_DIR/seed/hello-rust/$rel" >"$out/$rel"
  done
  echo "$out"
}

create_repo() {
  api "$GITEA_USER" "$GITEA_PASSWORD" POST "/orgs/$GITEA_ORG/repos" \
    "{\"name\":\"$GITEA_REPO\",\"private\":true,\"default_branch\":\"main\"}" | jq -r '.full_name // .message' >&2
}

push_repo() {
  local dir="$1"
  tbx sh -c 'rm -rf /tmp/seed && mkdir -p /tmp/seed'
  tar -C "$dir" -cf - . | tbx tar -C /tmp/seed -xf -
  tbx sh -c "cd /tmp/seed && git init -q -b main && git config user.name sbx && git config user.email sbx@sbx.test && git add -A && git commit -q -m seed && git push -q 'https://$GITEA_USER:$GITEA_TOKEN@$GITEA_HOST/$GITEA_ORG/$GITEA_REPO.git' main 2>&1 | sed 's#//[^@]*@#//***@#'"
}

publish_packages() {
  kc exec -i -n "$NS_TOOLS" pytool -- env \
    "REGISTRY_BASE=$REGISTRY_URL/api/packages/$GITEA_ORG" "REGISTRY_TOKEN=$GITEA_TOKEN" \
    "PKG_VERSION=$PKG_VERSION" "PKG_NPM=$PKG_NPM" "PKG_PYPI=$PKG_PYPI" "PKG_CARGO=$PKG_CARGO" \
    python3 -I - <"$BATTERY_DIR/seed/publish_packages.py"
}

repo_has_main() {
  [ "$(api "$GITEA_USER" "$GITEA_PASSWORD" GET "/repos/$GITEA_ORG/$GITEA_REPO/branches/main" | jq -r '.name // empty')" = main ]
}

setup_seed() {
  local dir
  dir="$(render_seed)"
  if repo_has_main; then
    log "seed repo exists"
  else
    create_repo
    push_repo "$dir"
  fi
  publish_packages
}
