#!/usr/bin/env bash
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib-cluster.sh"

PKI_DIR="$WORK/pki"

make_pki() {
  mkdir -p "$PKI_DIR"
  if [ -s "$PKI_DIR/server.crt" ] && openssl x509 -in "$PKI_DIR/server.crt" -noout -checkend 86400 >/dev/null 2>&1; then
    log "pki present"
    return 0
  fi
  log "make test CA and gitea certificate"
  openssl genrsa -out "$PKI_DIR/ca.key" 2048 2>/dev/null
  openssl req -x509 -new -key "$PKI_DIR/ca.key" -sha256 -days 30 -subj "/CN=sbx-build-test-ca" -out "$PKI_DIR/ca.crt"
  openssl genrsa -out "$PKI_DIR/server.key" 2048 2>/dev/null
  openssl req -new -key "$PKI_DIR/server.key" -subj "/CN=$GITEA_HOST" -out "$PKI_DIR/server.csr"
  printf 'subjectAltName=DNS:%s,DNS:%s.%s.svc,DNS:%s.%s,DNS:%s,DNS:localhost\nbasicConstraints=CA:FALSE\nkeyUsage=digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\n' \
    "$GITEA_HOST" "$GITEA_SERVICE" "$NS_TESTSVC" "$GITEA_SERVICE" "$NS_TESTSVC" "$GITEA_SERVICE" >"$PKI_DIR/server.ext"
  openssl x509 -req -in "$PKI_DIR/server.csr" -CA "$PKI_DIR/ca.crt" -CAkey "$PKI_DIR/ca.key" -CAcreateserial \
    -days 30 -sha256 -extfile "$PKI_DIR/server.ext" -out "$PKI_DIR/server.crt" 2>/dev/null
  cp "$PKI_DIR/ca.crt" "$WORK/ca.crt"
  chmod 600 "$PKI_DIR/ca.key" "$PKI_DIR/server.key"
}

install_pki() {
  kubectl -n "$NS_TESTSVC" create secret tls gitea-tls --cert="$PKI_DIR/server.crt" --key="$PKI_DIR/server.key" \
    --dry-run=client -o yaml | kc apply -f - >&2
  kubectl -n "$NS_TESTSVC" create configmap test-ca --from-file=ca.crt="$PKI_DIR/ca.crt" \
    --dry-run=client -o yaml | kc apply -f - >&2
  kubectl -n "$NS_TOOLS" create configmap test-ca --from-file=ca.crt="$PKI_DIR/ca.crt" \
    --dry-run=client -o yaml | kc apply -f - >&2
}
