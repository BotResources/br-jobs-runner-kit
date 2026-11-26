#!/usr/bin/env bash
set -euo pipefail

reference="${1:?usage: images-check-labels.sh <image reference>}"
missing=0

for label in source version revision licenses; do
  value="$(docker image inspect "$reference" --format "{{ index .Config.Labels \"org.opencontainers.image.${label}\" }}")"
  if [ -z "$value" ]; then
    echo "::error::${reference} has no label org.opencontainers.image.${label}" >&2
    missing=1
  else
    echo "org.opencontainers.image.${label}=${value}"
  fi
done

exit "$missing"
