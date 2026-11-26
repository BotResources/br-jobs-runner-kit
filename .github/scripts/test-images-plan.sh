#!/usr/bin/env bash
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
workflows="${here}/../workflows"
failures=0

pass() { echo "ok   $1"; }
fail() {
  echo "FAIL $1" >&2
  failures=$((failures + 1))
}

expect() {
  local label="$1" wanted="$2"
  shift 2
  local got
  got="$(env -i PATH="$PATH" REPOSITORY=BotResources/br-jobs-runner-kit SHA=1111111aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa "$@" bash "$here/images-plan.sh" | grep -E '^(push|tag)=' | tr '\n' ' ')"
  if [ "$got" = "$wanted " ]; then
    pass "${label}: ${got}"
  else
    fail "${label}: wanted '${wanted}' got '${got}'"
  fi
}

expect_refused() {
  local label="$1"
  shift
  if env -i PATH="$PATH" REPOSITORY=BotResources/br-jobs-runner-kit SHA=1111111aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa "$@" bash "$here/images-plan.sh" >/dev/null 2>&1; then
    fail "${label}: the plan accepted it"
  else
    pass "${label}"
  fi
}

head=HEAD_SHA=2222222bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb
expect "push to main" "push=true tag=sha-1111111" EVENT_NAME=push REF=refs/heads/main
expect "push to a release branch" "push=false tag=sha-1111111" EVENT_NAME=push REF=refs/heads/release/x
expect "pull request of this repository" "push=true tag=pr-7-2222222" EVENT_NAME=pull_request PR_NUMBER=7 "$head" HEAD_REPOSITORY=BotResources/br-jobs-runner-kit
expect "pull request of a fork" "push=false tag=pr-7-2222222" EVENT_NAME=pull_request PR_NUMBER=7 "$head" HEAD_REPOSITORY=someone/br-jobs-runner-kit
expect "pull request of Dependabot" "push=false tag=pr-7-2222222" EVENT_NAME=pull_request PR_NUMBER=7 "$head" ACTOR=dependabot[bot]
expect "dispatch without push" "push=false tag=sha-1111111" EVENT_NAME=workflow_dispatch PUSH_INPUT=false
expect "dispatch with push" "push=true tag=sha-1111111" EVENT_NAME=workflow_dispatch PUSH_INPUT=true
expect "dispatch with no input" "push=false tag=sha-1111111" EVENT_NAME=workflow_dispatch
expect_refused "an event that images.yml does not run on" EVENT_NAME=workflow_call PUSH_INPUT=true
expect_refused "a schedule" EVENT_NAME=schedule

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/images/a-one" "$tmp/images/b-two" "$tmp/images/c.d_e" "$tmp/images/no-dockerfile"
touch "$tmp/images/a-one/Dockerfile" "$tmp/images/b-two/Dockerfile"
discovered="$(IMAGES_ROOT="$tmp/images" bash "$here/images-discover.sh")"
if [ "$discovered" = '["a-one", "b-two"]' ]; then pass "discover all"; else fail "discover all: $discovered"; fi
discovered="$(IMAGES_ROOT="$tmp/images" bash "$here/images-discover.sh" b-two)"
if [ "$discovered" = '["b-two"]' ]; then pass "discover one"; else fail "discover one: $discovered"; fi
if IMAGES_ROOT="$tmp/images" bash "$here/images-discover.sh" no-dockerfile 2>/dev/null; then fail "discover accepted an image without Dockerfile"; else pass "discover refuses an image without Dockerfile"; fi
if IMAGES_ROOT="$tmp/images" bash "$here/images-discover.sh" 'x;rm' 2>/dev/null; then fail "discover accepted a bad name"; else pass "discover refuses a bad name"; fi
for bad in Upper Mixed-Case -leading "sp ace" "semi;colon"; do
  rm -rf "$tmp/bad"
  mkdir -p "$tmp/bad/$bad"
  touch "$tmp/bad/$bad/Dockerfile"
  if IMAGES_ROOT="$tmp/bad" bash "$here/images-discover.sh" 2>/dev/null; then fail "discover accepted the folder '${bad}'"; else pass "discover refuses the folder '${bad}'"; fi
done
rm -rf "$tmp/ok"
mkdir -p "$tmp/ok/a.b_c1"
touch "$tmp/ok/a.b_c1/Dockerfile"
discovered="$(IMAGES_ROOT="$tmp/ok" bash "$here/images-discover.sh")"
if [ "$discovered" = '["a.b_c1"]' ]; then pass "discover accepts a.b_c1"; else fail "discover accepts a.b_c1: $discovered"; fi

callable="${workflows}/images-build.yml"
if grep -q 'github\.event_name' "$callable"; then fail "images-build.yml uses github.event_name"; else pass "images-build.yml does not use github.event_name"; fi
if grep -q 'github\.event\.' "$callable"; then fail "images-build.yml reads the event of its caller"; else pass "images-build.yml does not read the event of its caller"; fi
triggers="$(sed -n '/^on:/,/^[a-z]/p' "$callable" | grep -E '^  [a-z_]+:' | tr -d ' \n')"
if [ "$triggers" = "workflow_call:" ]; then pass "images-build.yml has the trigger workflow_call only"; else fail "images-build.yml triggers: ${triggers}"; fi
if grep -q '^concurrency:' "$callable"; then fail "images-build.yml has a concurrency block"; else pass "images-build.yml has no concurrency block"; fi
if grep -Eq '^  group: images\.yml-' "${workflows}/images.yml"; then pass "images.yml names its own file in its concurrency group"; else fail "images.yml concurrency group does not name images.yml"; fi
if grep -q 'uses: \./\.github/workflows/images-build\.yml' "${workflows}/images.yml"; then pass "images.yml calls images-build.yml"; else fail "images.yml does not call images-build.yml"; fi

[ "$failures" -eq 0 ]
