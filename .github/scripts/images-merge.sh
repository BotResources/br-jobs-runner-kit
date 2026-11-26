#!/usr/bin/env bash
set -euo pipefail

image="${1:?usage: images-merge.sh <image> <registry> <tag> <digest-dir> <out-dir>}"
registry="${2:?}"
tag="${3:?}"
digest_dir="${4:?}"
out_dir="${5:?}"
valid='^sha256:[0-9a-f]{64}$'

read_digest() {
  local file="${digest_dir}/${image}-$1.digest" value
  if [ ! -f "$file" ]; then
    echo "::error::no digest of ${image} for $1 (${file})" >&2
    exit 1
  fi
  value="$(tr -d '[:space:]' <"$file")"
  if ! [[ "$value" =~ $valid ]]; then
    echo "::error::'${value}' is not a sha256 manifest digest (${file})" >&2
    exit 1
  fi
  echo "$value"
}

amd64="$(read_digest amd64)"
arm64="$(read_digest arm64)"
if [ "$amd64" = "$arm64" ]; then
  echo "::error::the amd64 and the arm64 digests of ${image} are the same" >&2
  exit 1
fi

name="${registry}/${image}"
docker buildx imagetools create -t "${name}:${tag}" "${name}@${amd64}" "${name}@${arm64}" >&2

list="sha256:$(docker buildx imagetools inspect --raw "${name}:${tag}" | sha256sum | cut -d' ' -f1)"
platforms="$(docker buildx imagetools inspect --raw "${name}@${list}" | python3 -I -c '
import json, sys
manifests = json.load(sys.stdin)["manifests"]
print(" ".join(sorted("{}/{}".format(m["platform"]["os"], m["platform"]["architecture"]) for m in manifests)))
')"
if [ "$platforms" != "linux/amd64 linux/arm64" ]; then
  echo "::error::the manifest list ${list} holds '${platforms}', not linux/amd64 and linux/arm64" >&2
  exit 1
fi

mkdir -p "$out_dir"
printf '%s@%s\n' "$name" "$list" >"${out_dir}/${image}.ref"

{
  echo "### ${image}"
  echo
  echo "| Item | Value |"
  echo "|---|---|"
  echo "| Manifest list digest | \`${list}\` |"
  echo "| linux/amd64 | \`${amd64}\` |"
  echo "| linux/arm64 | \`${arm64}\` |"
  echo "| Reference | \`${name}@${list}\` |"
  echo "| Tag | \`${name}:${tag}\` |"
} >>"${GITHUB_STEP_SUMMARY:-/dev/null}"

echo "${name}@${list}"
