#!/usr/bin/env bash
set -euo pipefail
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib-cluster.sh"

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
    "$S3_ENDPOINT_IN_CLUSTER$path"
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
  local base="$GITEA_URL/api/packages/$GITEA_ORG"
  expect_ok "pypi download with pip" kc exec -n "$NS_TOOLS" pytool -- sh -c "rm -rf /tmp/pipdl && pip download -q --disable-pip-version-check --no-deps -d /tmp/pipdl --index-url 'https://$GITEA_USER:$GITEA_TOKEN@$GITEA_HOST/api/packages/$GITEA_ORG/pypi/simple' $PKG_PYPI && ls /tmp/pipdl"
  expect_ok "npm tarball download" tbx sh -c "curl -sSf -u '$GITEA_USER:$GITEA_TOKEN' -o /tmp/n.tgz '$base/npm/$PKG_NPM/-/$PKG_VERSION/$PKG_NPM-$PKG_VERSION.tgz' && tar tzf /tmp/n.tgz | tr '\n' ' '"
  expect_ok "cargo crate download" tbx sh -c "curl -sSf -u '$GITEA_USER:$GITEA_TOKEN' -o /tmp/c.crate '$base/cargo/api/v1/crates/$PKG_CARGO/$PKG_VERSION/download' && tar tzf /tmp/c.crate | tr '\n' ' '"
  expect_ok "cargo sparse index entry" tbx sh -c "curl -sSf -u '$GITEA_USER:$GITEA_TOKEN' '$base/cargo/sb/x-/$PKG_CARGO'"
  expect_fail "package download without credential" tbx sh -c "curl -sSf -o /dev/null '$base/npm/$PKG_NPM/-/$PKG_VERSION/$PKG_NPM-$PKG_VERSION.tgz'"
}

check_closed() {
  expect_fail "closed: no route to 1.1.1.1" tbx curl -sS --max-time 8 -o /dev/null https://1.1.1.1
  expect_fail "closed: no public name" tbx curl -sS --max-time 8 -o /dev/null https://example.com
  expect_ok "closed: test service reachable" tbx curl -sS --max-time 8 -o /dev/null --cacert /ca/ca.crt "$GITEA_URL/api/healthz"
  expect_ok "closed: store reachable" want_status 200 s3_status GET "/$S3_BUCKET/?list-type=2&max-keys=1" "$S3_SECRET_KEY"
}

check_netpol() {
  local ip started deadline got=refused
  ip="$(kc -n "$NS_TESTSVC" get svc "$GITEA_SERVICE" -o jsonpath='{.spec.clusterIP}')"
  kc delete namespace "$NS_NETPOL_PROBE" --ignore-not-found --wait=true >/dev/null
  kc create namespace "$NS_NETPOL_PROBE" >/dev/null
  render "$BATTERY_DIR/manifests/30-netpol-probe.yaml" | kc apply -f - >/dev/null
  kc -n "$NS_NETPOL_PROBE" wait --for=condition=Ready pod/probe --timeout=120s >/dev/null
  probe_gitea() {
    kc -n "$NS_NETPOL_PROBE" exec probe -- curl -sk --max-time 3 -o /dev/null --resolve "$GITEA_HOST:443:$ip" "$GITEA_URL/api/healthz"
  }
  expect_ok "netpol probe reaches gitea before policy" probe_gitea
  render "$BATTERY_DIR/manifests/31-netpol-deny.yaml" | kc apply -f - >/dev/null
  started="$(date +%s)"
  deadline=$((started + 60))
  while [ "$(date +%s)" -lt "$deadline" ]; do
    if ! probe_gitea >/dev/null 2>&1; then
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
check_netpol
if [ "$MODE" = closed ]; then
  check_closed
fi
log "verify done: $MODE"
