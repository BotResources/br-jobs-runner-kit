#!/usr/bin/env bash
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
failures=0

pass() { echo "ok   $1"; }
fail() {
  echo "FAIL $1" >&2
  failures=$((failures + 1))
}

plan() {
  env -i PATH="$PATH" HEAD_SHA=2222222bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb "$@" bash "$here/keeper-image-plan.sh"
}

expect() {
  local label="$1" wanted="$2"
  shift 2
  local got
  got="$(plan "$@" | grep -E '^(push|tag)=' | tr '\n' ' ')"
  if [ "$got" = "$wanted " ]; then
    pass "${label}: ${got}"
  else
    fail "${label}: wanted '${wanted}' got '${got}'"
  fi
}

expect_refused() {
  local label="$1"
  shift
  if plan "$@" >/dev/null 2>&1; then
    fail "${label}: the plan accepted it"
  else
    pass "${label}"
  fi
}

expect "version changed on main" "push=true tag=0.2.0" EVENT_NAME=push REF=refs/heads/main VERSION=0.2.0 PREVIOUS_VERSION=0.1.0
expect "first version on main" "push=true tag=0.1.0" EVENT_NAME=push REF=refs/heads/main VERSION=0.1.0 PREVIOUS_VERSION=
expect "version unchanged on main" "push=false tag=0.1.0" EVENT_NAME=push REF=refs/heads/main VERSION=0.1.0 PREVIOUS_VERSION=0.1.0
expect "version changed on another branch" "push=false tag=0.2.0" EVENT_NAME=push REF=refs/heads/feature VERSION=0.2.0 PREVIOUS_VERSION=0.1.0
expect "tag already published" "push=false tag=0.2.0" EVENT_NAME=push REF=refs/heads/main VERSION=0.2.0 PREVIOUS_VERSION=0.1.0 PUBLISHED=true
expect "pull request" "push=false tag=0.2.0" EVENT_NAME=pull_request REF=refs/pull/7/merge VERSION=0.2.0 PREVIOUS_VERSION=0.1.0
expect "dispatch with push on main" "push=true tag=0.1.0" EVENT_NAME=workflow_dispatch REF=refs/heads/main VERSION=0.1.0 PUSH_INPUT=true
expect "dispatch with push on main, tag published" "push=false tag=0.1.0" EVENT_NAME=workflow_dispatch REF=refs/heads/main VERSION=0.1.0 PUSH_INPUT=true PUBLISHED=true
expect "dispatch without push" "push=false tag=0.1.0" EVENT_NAME=workflow_dispatch REF=refs/heads/main VERSION=0.1.0 PUSH_INPUT=false
expect "dispatch with push on another ref" "push=false tag=0.1.0" EVENT_NAME=workflow_dispatch REF=refs/heads/feature VERSION=0.1.0 PUSH_INPUT=true
expect_refused "an event that the workflow does not handle" EVENT_NAME=schedule REF=refs/heads/main VERSION=0.1.0
expect_refused "a version that is not plain" EVENT_NAME=push REF=refs/heads/main VERSION=0.1.0-rc1 PREVIOUS_VERSION=0.0.9
expect_refused "no version" EVENT_NAME=push REF=refs/heads/main
expect_refused "no head commit" EVENT_NAME=push REF=refs/heads/main VERSION=0.1.0 HEAD_SHA=

workflow="${here}/../workflows/keeper-image.yml"
if grep -q 'images-build.yml' "$workflow" && grep -q 'image: br-sandbox-keeper' "$workflow"; then
  pass "the workflow calls images-build.yml for br-sandbox-keeper"
else
  fail "the workflow calls images-build.yml for br-sandbox-keeper"
fi
top_permissions="$(awk '/^permissions:/{flag=1;next} /^[a-z]/{flag=0} flag' "$workflow")"
if grep -q 'packages: write' <<<"$top_permissions"; then
  fail "the workflow grants packages: write at its top level"
else
  pass "packages: write is granted only on the job that builds"
fi

if [ "$failures" -gt 0 ]; then
  echo "${failures} failure(s)" >&2
  exit 1
fi
echo "all keeper-image plan tests passed"
