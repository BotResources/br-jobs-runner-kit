#!/usr/bin/env bash
set -euo pipefail

reference="${1:?usage: keeper-image-published.sh <image reference with tag>}"

if message="$(docker buildx imagetools inspect "$reference" 2>&1 >/dev/null)"; then
  echo "published=true"
  exit 0
fi

not_found=false
while IFS= read -r line; do
  case "$line" in
    "ERROR: ${reference}: not found"* | "ERROR: ${reference}: "*"MANIFEST_UNKNOWN"* | "ERROR: ${reference}: "*"manifest unknown"* | "ERROR: ${reference}: "*"NAME_UNKNOWN"*)
      not_found=true
      ;;
  esac
done <<<"$message"

if [ "$not_found" = true ]; then
  echo "published=false"
  exit 0
fi

echo "::error::cannot tell whether ${reference} is published; the registry answered: ${message}" >&2
exit 1
