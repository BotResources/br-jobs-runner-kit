#!/usr/bin/env bash
set -euo pipefail

image="${1:?usage: images-record-digest.sh <image> <registry> <tag> <digest> <out-dir>}"
registry="${2:?}"
tag="${3:?}"
digest="${4:?}"
out_dir="${5:?}"

if ! [[ "$digest" =~ ^sha256:[0-9a-f]{64}$ ]]; then
  echo "::error::'${digest}' is not a sha256 manifest digest" >&2
  exit 1
fi

reference="${registry}/${image}@${digest}"
mkdir -p "$out_dir"
printf '%s\n' "$reference" >"${out_dir}/${image}.ref"

{
  echo "### ${image}"
  echo
  echo "| Item | Value |"
  echo "|---|---|"
  echo "| Manifest list digest | \`${digest}\` |"
  echo "| Reference | \`${reference}\` |"
  echo "| Tag | \`${registry}/${image}:${tag}\` |"
  echo "| Platforms | linux/amd64, linux/arm64 |"
} >>"${GITHUB_STEP_SUMMARY:-/dev/null}"

echo "${reference}"
