#!/usr/bin/env bash
set -euo pipefail

reference="${1:?usage: keeper-image-published.sh <image reference with tag>}"

if message="$(docker buildx imagetools inspect "$reference" 2>&1 >/dev/null)"; then
  echo "published=true"
  exit 0
fi

case "$message" in
  *"${reference}: not found"* | *"MANIFEST_UNKNOWN"* | *"manifest unknown"* | *"NAME_UNKNOWN"*)
    echo "published=false"
    ;;
  *)
    echo "::error::cannot tell whether ${reference} is published; the registry answered: ${message}" >&2
    exit 1
    ;;
esac
