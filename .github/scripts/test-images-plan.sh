#!/usr/bin/env bash
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
failures=0

expect() {
  local label="$1" wanted="$2"
  shift 2
  local got
  got="$(env -i PATH="$PATH" REPOSITORY=BotResources/br-jobs-runner-kit SHA=1111111aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa "$@" bash "$here/images-plan.sh" | grep -E '^(push|tag)=' | tr '\n' ' ')"
  if [ "$got" = "$wanted " ]; then
    echo "ok   ${label}: ${got}"
  else
    echo "FAIL ${label}: wanted '${wanted}' got '${got}'" >&2
    failures=$((failures + 1))
  fi
}

expect "push to main" "push=true tag=sha-1111111" EVENT_NAME=push REF=refs/heads/main
expect "push to a release branch" "push=false tag=sha-1111111" EVENT_NAME=push REF=refs/heads/release/x
expect "pull request of this repository" "push=true tag=pr-7-2222222" EVENT_NAME=pull_request PR_NUMBER=7 HEAD_SHA=2222222bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb HEAD_REPOSITORY=BotResources/br-jobs-runner-kit
expect "pull request of a fork" "push=false tag=pr-7-2222222" EVENT_NAME=pull_request PR_NUMBER=7 HEAD_SHA=2222222bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb HEAD_REPOSITORY=someone/br-jobs-runner-kit
expect "pull request of Dependabot" "push=false tag=pr-7-2222222" EVENT_NAME=pull_request PR_NUMBER=7 HEAD_SHA=2222222bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb ACTOR=dependabot[bot]
expect "dispatch without push" "push=false tag=sha-1111111" EVENT_NAME=workflow_dispatch PUSH_INPUT=false
expect "dispatch with push" "push=true tag=sha-1111111" EVENT_NAME=workflow_dispatch PUSH_INPUT=true
expect "dispatch with no input" "push=false tag=sha-1111111" EVENT_NAME=workflow_dispatch
expect "call with push" "push=true tag=sha-1111111" EVENT_NAME=workflow_call PUSH_INPUT=true
expect "call from a main push, push=false" "push=false tag=sha-1111111" EVENT_NAME=push REF=refs/heads/main PUSH_INPUT=false

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/images/a-one" "$tmp/images/b-two" "$tmp/images/no-dockerfile"
touch "$tmp/images/a-one/Dockerfile" "$tmp/images/b-two/Dockerfile"
discovered="$(IMAGES_ROOT="$tmp/images" bash "$here/images-discover.sh")"
if [ "$discovered" = '["a-one", "b-two"]' ]; then echo "ok   discover all"; else echo "FAIL discover all: $discovered" >&2; failures=$((failures + 1)); fi
discovered="$(IMAGES_ROOT="$tmp/images" bash "$here/images-discover.sh" b-two)"
if [ "$discovered" = '["b-two"]' ]; then echo "ok   discover one"; else echo "FAIL discover one: $discovered" >&2; failures=$((failures + 1)); fi
if IMAGES_ROOT="$tmp/images" bash "$here/images-discover.sh" no-dockerfile 2>/dev/null; then echo "FAIL discover unknown" >&2; failures=$((failures + 1)); else echo "ok   discover refuses an image without Dockerfile"; fi
if IMAGES_ROOT="$tmp/images" bash "$here/images-discover.sh" 'x;rm' 2>/dev/null; then echo "FAIL discover bad name" >&2; failures=$((failures + 1)); else echo "ok   discover refuses a bad name"; fi

[ "$failures" -eq 0 ]
