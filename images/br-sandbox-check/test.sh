#!/bin/sh
set -eu

image="${1:?usage: test.sh <image reference> [platform]}"
platform="${2:-}"
results="${RESULTS_FILE:-/dev/null}"
here=$(cd "$(dirname "$0")" && pwd)
netcheck=$(cd "${here}/../../charts/br-sandbox-chart/files" && pwd)
pinned=$(sed -n 's/^ARG KUBECTL_VERSION=//p' "${here}/Dockerfile")
scratch=$(mktemp -d)
ran="${scratch}/ran"
: >"$ran"

cleanup() {
  rm -rf "$scratch"
}
trap cleanup EXIT

[ -n "$pinned" ] || {
  echo "FAIL: no KUBECTL_VERSION in the Dockerfile" >&2
  exit 1
}

dk() {
  docker run --rm ${platform:+--platform "$platform"} --network none --cap-drop ALL --security-opt no-new-privileges "$@"
}

job() {
  dk --user 10001:10001 --read-only --tmpfs /tmp -e TMPDIR=/tmp -v "${netcheck}:/netcheck:ro" "$image" "$@"
}

job_without_tmp() {
  dk --user 10001:10001 --read-only -e TMPDIR=/tmp "$image" "$@"
}

probe_pod() {
  dk --user 0:0 --cap-add CHOWN --cap-add DAC_OVERRIDE --cap-add FOWNER --cap-add FSETID --cap-add SETUID --cap-add SETGID --cap-add KILL -v "${netcheck}:/netcheck:ro" "$image" "$@"
}

fail() {
  echo "FAIL: $1" | tee -a "$results" >&2
  exit 1
}

check() {
  name="$1"
  shift
  echo "$name" >>"$ran"
  if "$@" >/dev/null 2>&1; then
    echo "ok   $name" | tee -a "$results"
  else
    fail "$name"
  fi
}

no_entrypoint() {
  entrypoint=$(docker image inspect --format '{{json .Config.Entrypoint}}' "$image")
  [ "$entrypoint" = null ] || [ "$entrypoint" = '[]' ]
}

kubectl_is_pinned() {
  job kubectl version --client | grep -qx "Client Version: v${pinned}"
}

kubectl_licence_is_present() {
  job grep -q 'Apache License' /usr/share/kubectl-LICENSE
}

kubectl_fails_cleanly() {
  output=$(job kubectl --server=http://127.0.0.1:1 --request-timeout=3s get nodes 2>&1) && return 1
  case "$output" in
    *"connection refused"*) return 0 ;;
    *) return 1 ;;
  esac
}

mktemp_needs_the_volume() {
  job mktemp -d | grep -q '^/tmp/' && ! job_without_tmp mktemp -d
}

architecture_is_the_platform() {
  [ -z "$platform" ] || [ "$(docker image inspect --format '{{.Architecture}}' "$image")" = "${platform#linux/}" ]
}

peers='
nc -lk -p 9000 -e /bin/true &
nc -lk -p 9001 -e /bin/sh -c "printf \"HTTP/1.1 403 Forbidden\r\n\r\n\"" &
nc -lk -p 9002 -e /bin/sh -c "printf \"HTTP/1.1 200 OK\r\n\r\n\"" &
nc -lk -p 9003 -e /bin/sleep 20 &
sleep 1
'

nc_reaches_a_listener() {
  job sh -c "${peers}"'nc -w 2 127.0.0.1 9000 </dev/null >/dev/null 2>&1'
}

nc_fails_fast_on_a_closed_port() {
  job sh -c '
start=$(date +%s)
! nc -w 2 127.0.0.1 1 </dev/null >/dev/null 2>&1 || exit 1
[ "$(($(date +%s) - start))" -lt 2 ]
'
}

nc_reads_the_reply_until_the_peer_closes() {
  job sh -c "${peers}"'
reply=$(printf "CONNECT x HTTP/1.1\r\n\r\n" | nc -w 3 127.0.0.1 9001 | tr -d "\r")
[ "$reply" = "HTTP/1.1 403 Forbidden" ]
'
}

nc_ends_on_a_peer_that_never_answers() {
  job sh -c "${peers}"'
start=$(date +%s)
printf x | nc -w 1 127.0.0.1 9003 >/dev/null 2>&1 || true
[ "$(($(date +%s) - start))" -lt 5 ]
'
}

probe_lines() {
  "$1" sh -c "${peers}"'
export NETCHECK_TARGETS="127.0.0.1:9000 127.0.0.1:1" NETCHECK_ENFORCEMENT_WAIT=1 NETCHECK_OFFLIST=127.0.0.1:9002
for proxy in 9001 9002 9003; do
  NETCHECK_PROXY="127.0.0.1:${proxy}" sh /netcheck/network-check.sh probe
done
' | sed 's/^\(127.0.0.1:1 refused\) [0-9][0-9]*$/\1 N/'
}

expected_probe_lines() {
  cat <<'EOF'
127.0.0.1:9000 reached
127.0.0.1:1 refused N
connect:127.0.0.1:9000 refused
connect:127.0.0.1:1 refused
connect:127.0.0.1:9002 refused
127.0.0.1:9000 reached
127.0.0.1:1 refused N
connect:127.0.0.1:9000 reached
connect:127.0.0.1:1 reached
connect:127.0.0.1:9002 reached
127.0.0.1:9000 reached
127.0.0.1:1 refused N
connect:127.0.0.1:9000 no_result
connect:127.0.0.1:1 no_result
connect:127.0.0.1:9002 no_result
EOF
}

probe_gives_its_lines() {
  probe_lines "$1" >"${scratch}/probe.out" || return 1
  expected_probe_lines | diff - "${scratch}/probe.out" >&2
}

probe_as_job() {
  probe_gives_its_lines job
}

probe_as_probe_pod() {
  probe_gives_its_lines probe_pod
}

cat >"${scratch}/expected" <<EOF
the image architecture is the platform
the image has no entrypoint
the four labels are set
the Job runs as uid 10001 on a read-only root with a writable /tmp
sh runs
kubectl runs and prints the pinned version
nc runs and has the -w option
cat runs
sed runs
tr runs
mktemp makes a directory in /tmp and fails with no /tmp volume
awk runs the status line program of the script
sleep runs
date runs
kubectl reports an unreachable API server as an error on a read-only root
kubectl licence text is in the image
nc -w reaches a listener and exits 0
nc -w on a closed port exits non-zero in under 2 seconds
nc reads the reply until the peer closes
nc -w ends a connection to a peer that never answers
network-check.sh probe gives its lines as the Job pod user
network-check.sh probe gives its lines as the probe pod user
EOF

check "the image architecture is the platform" architecture_is_the_platform
check "the image has no entrypoint" no_entrypoint
check "the four labels are set" bash "${here}/../../.github/scripts/images-check-labels.sh" "$image"
check "the Job runs as uid 10001 on a read-only root with a writable /tmp" job sh -c '[ "$(id -u)" = 10001 ] && ! touch /x && touch /tmp/x'
check "sh runs" job sh -c 'command -v sh'
check "kubectl runs and prints the pinned version" kubectl_is_pinned
check "nc runs and has the -w option" job sh -c 'command -v nc && nc --help 2>&1 | grep -q -- "-w SEC"'
check "cat runs" job sh -c '[ "$(command -v cat >/dev/null && printf abc | cat)" = abc ]'
check "sed runs" job sh -c '[ "$(command -v sed >/dev/null && printf "a\nb\n" | sed -n 2p)" = b ]'
check "tr runs" job sh -c '[ "$(command -v tr >/dev/null && printf "a\r\n" | tr -d "\r")" = a ]'
check "mktemp makes a directory in /tmp and fails with no /tmp volume" mktemp_needs_the_volume
check "awk runs the status line program of the script" job sh -c '[ "$(command -v awk >/dev/null && printf "HTTP/1.1 403 x\n" | awk "\$1 ~ /^HTTP\/[0-9]/ && \$2 ~ /^[0-9][0-9][0-9]\$/ {printf \"%s\", \$2}")" = 403 ]'
check "sleep runs" job sh -c 'command -v sleep && sleep 1'
check "date runs" job sh -c 'command -v date && [ "$(date +%s)" -gt 1700000000 ]'
check "kubectl reports an unreachable API server as an error on a read-only root" kubectl_fails_cleanly
check "kubectl licence text is in the image" kubectl_licence_is_present
check "nc -w reaches a listener and exits 0" nc_reaches_a_listener
check "nc -w on a closed port exits non-zero in under 2 seconds" nc_fails_fast_on_a_closed_port
check "nc reads the reply until the peer closes" nc_reads_the_reply_until_the_peer_closes
check "nc -w ends a connection to a peer that never answers" nc_ends_on_a_peer_that_never_answers
check "network-check.sh probe gives its lines as the Job pod user" probe_as_job
check "network-check.sh probe gives its lines as the probe pod user" probe_as_probe_pod

sort "$ran" >"${scratch}/ran.sorted"
sort "${scratch}/expected" >"${scratch}/expected.sorted"
if ! diff "${scratch}/expected.sorted" "${scratch}/ran.sorted" >&2; then
  fail "the checks that ran differ from the expected list"
fi

echo "smoke passed: $image"
