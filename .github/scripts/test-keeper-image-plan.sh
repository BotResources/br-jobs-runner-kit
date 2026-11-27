#!/usr/bin/env bash
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
failures=0
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

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

reference="ghcr.io/botresources/br-sandbox-keeper:0.1.0"
mkdir "${tmp}/bin"
cat >"${tmp}/bin/docker" <<'STUB'
#!/usr/bin/env bash
if [ "$1 $2 $3" != "buildx imagetools inspect" ] || [ "$4" != "$STUB_REFERENCE" ]; then
  echo "unexpected call: $*" >&2
  exit 2
fi
case "$STUB_ANSWER" in
  found) echo "Name: ${STUB_REFERENCE}" ;;
  not-found) echo "ERROR: ${STUB_REFERENCE}: not found" >&2; exit 1 ;;
  manifest-unknown) echo "ERROR: ${STUB_REFERENCE}: unexpected status from HEAD request: 404 Not Found (MANIFEST_UNKNOWN: manifest unknown)" >&2; exit 1 ;;
  denied) echo "ERROR: ${STUB_REFERENCE}: unexpected status from HEAD request: 403 Forbidden" >&2; exit 1 ;;
  unauthorized) echo "ERROR: failed to authorize: failed to fetch anonymous token: 401 Unauthorized" >&2; exit 1 ;;
  unreachable) echo "ERROR: Head \"https://ghcr.io/v2/botresources/br-sandbox-keeper/manifests/0.1.0\": dial tcp: lookup ghcr.io: no such host" >&2; exit 1 ;;
  rate-limited) echo "ERROR: ${STUB_REFERENCE}: 429 Too Many Requests" >&2; exit 1 ;;
  manifest-unknown-code) echo "ERROR: ${STUB_REFERENCE}: 404 Not Found (MANIFEST_UNKNOWN)" >&2; exit 1 ;;
  manifest-unknown-text) echo "ERROR: ${STUB_REFERENCE}: 404 Not Found (manifest unknown)" >&2; exit 1 ;;
  name-unknown) echo "ERROR: ${STUB_REFERENCE}: unexpected status from HEAD request: 404 Not Found (NAME_UNKNOWN: repository name not known to registry)" >&2; exit 1 ;;
  silent) exit 1 ;;
  other-image-not-found) echo "ERROR: ghcr.io/botresources/other:1.0.0: not found" >&2; exit 1 ;;
  other-image-manifest-unknown) echo "ERROR: ghcr.io/botresources/other:1.0.0: unexpected status from HEAD request: 404 Not Found (MANIFEST_UNKNOWN: manifest unknown)" >&2; exit 1 ;;
  other-image-name-unknown) echo "ERROR: ghcr.io/botresources/other:1.0.0: unexpected status from HEAD request: 404 Not Found (NAME_UNKNOWN: repository name not known to registry)" >&2; exit 1 ;;
esac
STUB
chmod +x "${tmp}/bin/docker"

published() {
  env -i PATH="${tmp}/bin:$PATH" STUB_REFERENCE="$reference" STUB_ANSWER="$1" bash "$here/keeper-image-published.sh" "$reference"
}

expect_published() {
  local answer="$1" wanted="$2" got
  if got="$(published "$answer" 2>/dev/null)" && [ "$got" = "$wanted" ]; then
    pass "registry answer '${answer}': ${got}"
  else
    fail "registry answer '${answer}': wanted '${wanted}' got '${got:-}'"
  fi
}

expect_published_fails() {
  local answer="$1" got
  if got="$(published "$answer" 2>/dev/null)"; then
    fail "registry answer '${answer}': the check went on with '${got}'"
  elif [ -n "$got" ]; then
    fail "registry answer '${answer}': the check failed but printed '${got}'"
  else
    pass "registry answer '${answer}': the check fails"
  fi
}

expect_published found "published=true"
expect_published not-found "published=false"
for answer in manifest-unknown manifest-unknown-code manifest-unknown-text name-unknown; do
  expect_published "$answer" "published=false"
done
for answer in denied unauthorized unreachable rate-limited silent other-image-not-found other-image-manifest-unknown other-image-name-unknown; do
  expect_published_fails "$answer"
done
if env -i PATH="${tmp}/bin:$PATH" bash "$here/keeper-image-published.sh" >/dev/null 2>&1; then
  fail "the published check accepted a call with no reference"
else
  pass "the published check needs a reference"
fi

workflow="${here}/../workflows/keeper-image.yml"
if grep -q 'bash .github/scripts/keeper-image-published.sh' "$workflow" && ! grep -q 'imagetools inspect' "$workflow"; then
  pass "the workflow decides published with keeper-image-published.sh"
else
  fail "the workflow decides published with keeper-image-published.sh"
fi
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
