#!/usr/bin/env bash
set -uo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
failures=0
tmp="$(mktemp -d)"
prefix="imgtest-$$"
cleanup() {
  docker image ls -q --filter "reference=${prefix}-*" | sort -u | xargs -r docker image rm -f >/dev/null 2>&1
  rm -rf "$tmp"
}
trap cleanup EXIT

pass() { echo "ok   $1"; }
fail() {
  echo "FAIL $1" >&2
  failures=$((failures + 1))
}

succeeds() {
  local label="$1"
  shift
  if "$@" >"${tmp}/out" 2>"${tmp}/err"; then pass "$label"; else fail "${label}: $(cat "${tmp}/err")"; fi
}

refuses() {
  local label="$1" message="$2"
  shift 2
  if "$@" >"${tmp}/out" 2>"${tmp}/err"; then
    fail "${label}: accepted"
  elif grep -q -- "$message" "${tmp}/err"; then
    pass "$label"
  else
    fail "${label}: refused without the message '${message}': $(cat "${tmp}/err")"
  fi
}

import_image() {
  local name="${prefix}-$1" labels="$2" args=()
  local label
  for label in $labels; do args+=(-c "LABEL org.opencontainers.image.${label}=value-${label}"); done
  head -c 1024 /dev/zero | docker import "${args[@]}" - "$name" >/dev/null
  echo "$name"
}

all_labels="source version revision licenses"
succeeds "labels: all four present" bash "$here/images-check-labels.sh" "$(import_image all "$all_labels")"
for missing in $all_labels; do
  present="$(tr ' ' '\n' <<<"$all_labels" | grep -vx "$missing" | tr '\n' ' ')"
  refuses "labels: ${missing} missing" "has no label org.opencontainers.image.${missing}" \
    bash "$here/images-check-labels.sh" "$(import_image "no-${missing}" "$present")"
done
if bash "$here/images-check-labels.sh" "${prefix}-does-not-exist" >/dev/null 2>&1; then fail "labels: an image that does not exist"; else pass "labels: an image that does not exist is refused"; fi

succeeds "inputs: no push, no tag" bash "$here/images-check-inputs.sh" false ""
succeeds "inputs: push with a tag" bash "$here/images-check-inputs.sh" true pr-7-abc1234
refuses "inputs: push without a tag" "tag is required" bash "$here/images-check-inputs.sh" true ""
refuses "inputs: a tag with a space" "not a valid image tag" bash "$here/images-check-inputs.sh" true "a b"
refuses "inputs: a tag that starts with a dash" "not a valid image tag" bash "$here/images-check-inputs.sh" true "-x"
refuses "inputs: a tag of 129 characters" "not a valid image tag" bash "$here/images-check-inputs.sh" true "$(head -c 129 /dev/zero | tr '\0' a)"
refuses "inputs: push that is not a boolean" "true or false" bash "$here/images-check-inputs.sh" maybe x

good="sha256:$(head -c 64 /dev/zero | tr '\0' a)"
other="sha256:$(head -c 64 /dev/zero | tr '\0' b)"
for bad in "sha256:abc" "abc" "sha512:$(head -c 64 /dev/zero | tr '\0' a)" "sha256:$(head -c 64 /dev/zero | tr '\0' A)" "sha256:$(head -c 65 /dev/zero | tr '\0' a)"; do
  refuses "record: refuses '${bad:0:20}'" "not a sha256 manifest digest" bash "$here/images-record-digest.sh" img linux/arm64 "$bad" "${tmp}/rec"
done
if [ -e "${tmp}/rec" ]; then fail "record: wrote a file for a bad digest"; else pass "record: writes nothing for a bad digest"; fi
succeeds "record: a good digest" bash "$here/images-record-digest.sh" img linux/arm64 "$good" "${tmp}/rec"
if [ "$(cat "${tmp}/rec/img-arm64.digest")" = "$good" ]; then pass "record: file img-arm64.digest holds the digest"; else fail "record: file content"; fi

stub="${tmp}/stub"
mkdir -p "$stub"
cat >"${stub}/docker" <<'STUB'
#!/usr/bin/env bash
printf '%s\n' "$*" >>"$STUB_LOG"
case "$*" in
  "buildx imagetools create"*) exit 0 ;;
  "buildx imagetools inspect --raw"*) cat "$STUB_INDEX" ;;
  "pull"*) exit 0 ;;
  *) echo "unexpected: $*" >&2; exit 2 ;;
esac
STUB
chmod +x "${stub}/docker"
export STUB_LOG="${tmp}/docker.log"
export STUB_INDEX="${tmp}/index.json"
: >"$STUB_LOG"

refuses "pull: a bad digest" "not a sha256 manifest digest" env PATH="${stub}:${PATH}" bash "$here/images-pull-digest.sh" ghcr.io/org img "sha256:zz" linux/arm64
if [ -s "$STUB_LOG" ]; then fail "pull: docker was called for a bad digest"; else pass "pull: docker is not called for a bad digest"; fi
reference="$(env PATH="${stub}:${PATH}" bash "$here/images-pull-digest.sh" ghcr.io/org img "$good" linux/arm64)"
if [ "$reference" = "ghcr.io/org/img@${good}" ] && grep -qx "pull --quiet --platform linux/arm64 ghcr.io/org/img@${good}" "$STUB_LOG"; then pass "pull: pulls the reference by digest and prints it"; else fail "pull: ${reference}"; fi

write_index() {
  python3 -I - "$STUB_INDEX" "$@" <<'PY'
import json, sys
manifests = [{"mediaType": "application/vnd.oci.image.manifest.v1+json", "digest": "sha256:" + "c" * 64, "size": 1, "platform": {"os": "linux", "architecture": arch}} for arch in sys.argv[2:]]
with open(sys.argv[1], "w") as handle:
    json.dump({"schemaVersion": 2, "mediaType": "application/vnd.oci.image.index.v1+json", "manifests": manifests}, handle)
PY
}

merge() { env PATH="${stub}:${PATH}" bash "$here/images-merge.sh" img ghcr.io/org sha-1234567 "${tmp}/digests" "${tmp}/refs"; }

mkdir -p "${tmp}/digests"
refuses "merge: no digest files" "no digest of img for amd64" merge
printf '%s\n' "$good" >"${tmp}/digests/img-amd64.digest"
refuses "merge: no arm64 digest" "no digest of img for arm64" merge
printf '%s\n' "not-a-digest" >"${tmp}/digests/img-arm64.digest"
refuses "merge: malformed digest" "not a sha256 manifest digest" merge
printf '%s\n' "$good" >"${tmp}/digests/img-arm64.digest"
refuses "merge: the same digest for both platforms" "are the same" merge
if grep -q "imagetools create" "$STUB_LOG"; then fail "merge: created a list from refused inputs"; else pass "merge: creates nothing from refused inputs"; fi

printf '%s\n' "$other" >"${tmp}/digests/img-arm64.digest"
write_index amd64 arm64
if merge >"${tmp}/merge.out" 2>"${tmp}/err"; then
  expected="sha256:$(sha256sum <"$STUB_INDEX" | cut -d' ' -f1)"
  if [ "$(cat "${tmp}/merge.out")" = "ghcr.io/org/img@${expected}" ] && [ "$(cat "${tmp}/refs/img.ref")" = "ghcr.io/org/img@${expected}" ]; then pass "merge: reports the digest of the list"; else fail "merge: $(cat "${tmp}/merge.out")"; fi
  if grep -qx "buildx imagetools create -t ghcr.io/org/img:sha-1234567 ghcr.io/org/img@${good} ghcr.io/org/img@${other}" "$STUB_LOG"; then pass "merge: creates the list from the two digests"; else fail "merge: create call: $(cat "$STUB_LOG")"; fi
else
  fail "merge: $(cat "${tmp}/err")"
fi

write_index amd64
refuses "merge: a list with one platform" "not linux/amd64 and linux/arm64" merge
write_index amd64 arm64 s390x
refuses "merge: a list with three platforms" "not linux/amd64 and linux/arm64" merge

mkdir -p "${tmp}/collect"
printf 'ghcr.io/org/a@%s\n' "$good" >"${tmp}/collect/a.ref"
printf 'ghcr.io/org/b@%s\n' "$other" >"${tmp}/collect/b.ref"
collected="$(bash "$here/images-collect-digests.sh" "${tmp}/collect")"
if [ "$collected" = "{\"a\":\"ghcr.io/org/a@${good}\",\"b\":\"ghcr.io/org/b@${other}\"}" ]; then pass "collect: one entry for each image"; else fail "collect: ${collected}"; fi

[ "$failures" -eq 0 ]
