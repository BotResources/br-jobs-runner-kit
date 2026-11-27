#!/usr/bin/env bash
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib-cluster.sh"

PKI_DIR="$WORK/pki"

test_names() {
  echo "$GITEA_HOST $REGISTRY_HOST $ECHO_HOST $PROBE_HOST $STORE_HOST"
}

server_cert_current() {
  [ -s "$PKI_DIR/server.crt" ] || return 1
  openssl x509 -in "$PKI_DIR/server.crt" -noout -checkend 86400 >/dev/null 2>&1 || return 1
  local text name
  text="$(openssl x509 -in "$PKI_DIR/server.crt" -noout -ext subjectAltName 2>/dev/null)"
  for name in $(test_names); do
    echo "$text" | grep -q "DNS:$name" || return 1
  done
}

make_ca() {
  if [ -s "$PKI_DIR/ca.crt" ] && openssl x509 -in "$PKI_DIR/ca.crt" -noout -checkend 86400 >/dev/null 2>&1; then
    return 0
  fi
  log "make test CA"
  openssl genrsa -out "$PKI_DIR/ca.key" 2048 2>/dev/null
  openssl req -x509 -new -key "$PKI_DIR/ca.key" -sha256 -days 30 -subj "/CN=sbx-build-test-ca" -out "$PKI_DIR/ca.crt"
  cp "$PKI_DIR/ca.crt" "$WORK/ca.crt"
  chmod 600 "$PKI_DIR/ca.key"
}

make_server_cert() {
  local alt="" name
  for name in $(test_names); do
    alt="$alt,DNS:$name"
  done
  log "make server certificate for: $(test_names)"
  openssl genrsa -out "$PKI_DIR/server.key" 2048 2>/dev/null
  openssl req -new -key "$PKI_DIR/server.key" -subj "/CN=$GITEA_HOST" -out "$PKI_DIR/server.csr"
  printf 'subjectAltName=%s\nbasicConstraints=CA:FALSE\nkeyUsage=digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\n' \
    "${alt#,}" >"$PKI_DIR/server.ext"
  openssl x509 -req -in "$PKI_DIR/server.csr" -CA "$PKI_DIR/ca.crt" -CAkey "$PKI_DIR/ca.key" -CAcreateserial \
    -days 30 -sha256 -extfile "$PKI_DIR/server.ext" -out "$PKI_DIR/server.crt" 2>/dev/null
  chmod 600 "$PKI_DIR/server.key"
}

make_pki() {
  mkdir -p "$PKI_DIR"
  make_ca
  if server_cert_current; then
    log "pki present"
    return 0
  fi
  make_server_cert
}

install_pki() {
  kubectl -n "$NS_TOOLS" create configmap test-ca --from-file=ca.crt="$PKI_DIR/ca.crt" \
    --dry-run=client -o yaml | kc apply -f - >&2
}
