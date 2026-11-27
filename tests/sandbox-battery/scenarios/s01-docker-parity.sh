#!/usr/bin/env bash
HERE=$(cd "$(dirname "$0")" && pwd)
. "$HERE/common.sh"
scenario_begin s01-docker-parity

ns=$(ns_of "$CLIENT_A" "$NET_NONE")
wait_warm "$ns" 1 120 || { check docker-parity-warm-pod fail "no warm pod in $ns"; scenario_end; }
claim_to pod "$ns" "docker-parity-$(new_id)" agent-a || { check docker-parity-claim fail "claim refused"; scenario_end; }
image=$(pod_image "$ns" "$pod")

expected_lines=("750" "background program alive" "pipe lines: 3" "after TERM: gone" "exit code: 3" "pip exit: 0" "import ok 1")
commands_succeeded() {
  local code=$1 output=$2 line
  [ "$code" = 0 ] || return 1
  for line in "${expected_lines[@]}"; do
    grep -qxF -- "$line" <<< "$output" || return 1
  done
}

sandbox_out=$(with_timeout 180 sxi "$ns" "$pod" sh -s < "$HERE/parity-commands.sh" 2>&1)
sandbox_rc=$?
plain_out=$(with_timeout 180 docker run --rm -i --entrypoint sh "$image" -s < "$HERE/parity-commands.sh" 2>&1)
plain_rc=$?

verdict docker-parity-sandbox-commands-succeeded "sandbox exit $sandbox_rc; every one of the five commands printed its success line" \
  commands_succeeded "$sandbox_rc" "$sandbox_out"

if [ "$plain_rc" -ge 125 ]; then
  check docker-parity-reference fail "plain docker run of $image failed with $plain_rc"
else
  verdict docker-parity-reference-commands-succeeded "plain docker exit $plain_rc; every one of the five commands printed its success line" \
    commands_succeeded "$plain_rc" "$plain_out"
  verdict docker-parity-stdout-identical "five commands plus ps/proc/pipe/signal/wheel, sandbox against plain docker" \
    test "$sandbox_out" = "$plain_out"
  verdict docker-parity-exit-code-identical "sandbox=$sandbox_rc plain=$plain_rc" test "$sandbox_rc" = "$plain_rc"
  if [ "$sandbox_out" != "$plain_out" ]; then
    diff <(printf '%s\n' "$sandbox_out") <(printf '%s\n' "$plain_out") | head -20
  fi
fi

verdict docker-parity-pid1-is-pause "comm of pid 1: $(sx "$ns" "$pod" cat /proc/1/comm)" \
  test "$(sx "$ns" "$pod" cat /proc/1/comm)" = pause

sx "$ns" "$pod" sh -c '(sleep 300 > /dev/null 2>&1 & echo $! > /tmp/docker-parity-orphan.pid)'
orphan_state=$(sx "$ns" "$pod" sh -c 'pid=$(cat /tmp/docker-parity-orphan.pid); kill -TERM "$pid"; sleep 1; if kill -0 "$pid" 2> /dev/null; then echo alive; else echo gone; fi')
verdict docker-parity-no-orphan-after-sigterm "orphan after SIGTERM: $orphan_state" test "$orphan_state" = gone

scenario_end
