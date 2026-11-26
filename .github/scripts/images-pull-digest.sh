#!/usr/bin/env bash
set -euo pipefail

registry="${1:?usage: images-pull-digest.sh <registry> <image> <digest> <platform>}"
image="${2:?}"
digest="${3:?}"
platform="${4:?}"

if ! [[ "$digest" =~ ^sha256:[0-9a-f]{64}$ ]]; then
  echo "::error::'${digest}' is not a sha256 manifest digest" >&2
  exit 1
fi

reference="${registry}/${image}@${digest}"
docker pull --quiet --platform "$platform" "$reference" >/dev/null
echo "$reference"
