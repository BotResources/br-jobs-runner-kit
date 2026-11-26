#!/usr/bin/env bash
set -euo pipefail

image="${1:?usage: images-record-digest.sh <image> <platform> <digest> <out-dir>}"
platform="${2:?}"
digest="${3:?}"
out_dir="${4:?}"

if ! [[ "$digest" =~ ^sha256:[0-9a-f]{64}$ ]]; then
  echo "::error::'${digest}' is not a sha256 manifest digest" >&2
  exit 1
fi

mkdir -p "$out_dir"
printf '%s\n' "$digest" >"${out_dir}/${image}-${platform#linux/}.digest"
echo "$digest"
