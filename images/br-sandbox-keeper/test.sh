#!/bin/sh
set -eu

image="${1:?usage: test.sh <image reference> [platform]}"
platform="${2:-}"
results="${RESULTS_FILE:-/dev/null}"
scratch=$(mktemp -d)
mkdir "${scratch}/config"

cleanup() {
  rm -rf "$scratch"
}
trap cleanup EXIT

fail() {
  echo "FAIL: $1" | tee -a "$results" >&2
  exit 1
}

pass() {
  echo "ok   $1" | tee -a "$results"
}

dk() {
  docker run --rm ${platform:+--platform "$platform"} --network none --cap-drop ALL --security-opt no-new-privileges --read-only "$@"
}

expected_arch="${platform#linux/}"
if [ -n "$expected_arch" ] && [ "$(docker image inspect --format '{{.Architecture}}' "$image")" != "$expected_arch" ]; then
  fail "the image architecture is ${expected_arch}"
fi
pass "the image architecture is the platform"

if [ "$(docker image inspect --format '{{.Config.User}}' "$image")" != "65532:65532" ]; then
  fail "the image runs as 65532:65532"
fi
pass "the image runs as a numeric non-root user"

if [ "$(docker image inspect --format '{{json .Config.Entrypoint}}' "$image")" != '["/br-sandbox-keeper"]' ]; then
  fail "the entrypoint is the keeper binary"
fi
pass "the entrypoint is the keeper binary"

container=$(docker create ${platform:+--platform "$platform"} "$image")
if ! docker cp "${container}:/etc/ssl/certs/ca-certificates.crt" "${scratch}/ca.crt" >/dev/null 2>&1; then
  docker rm "$container" >/dev/null
  fail "the image holds the system root certificates"
fi
docker rm "$container" >/dev/null
if ! grep -q 'BEGIN CERTIFICATE' "${scratch}/ca.crt"; then
  fail "the root certificates are certificates"
fi
pass "the image holds the system root certificates"

if output=$(dk "$image" 2>&1); then
  fail "the keeper refuses to start with no configuration"
fi
case "$output" in
  *BR_SANDBOX_CONFIG*) pass "the keeper names the missing configuration variable" ;;
  *) fail "the keeper names the missing configuration variable" ;;
esac
case "$output" in
  '{'*'"level":"ERROR"'*'}') pass "the keeper writes its error as a JSON line" ;;
  *) fail "the keeper writes its error as a JSON line" ;;
esac

printf '%s' '{"save_ttl_seconds": 1, "unknown_key": true}' >"${scratch}/config/config.json"
chmod 0644 "${scratch}/config/config.json"
if output=$(dk -e BR_SANDBOX_CONFIG=/config/config.json -v "${scratch}/config:/config:ro" "$image" 2>&1); then
  fail "the keeper refuses a configuration with an unknown key"
fi
case "$output" in
  *unknown_key*) pass "the keeper refuses a configuration with an unknown key and names it" ;;
  *) fail "the keeper refuses a configuration with an unknown key and names it" ;;
esac

echo "smoke passed: $image"
