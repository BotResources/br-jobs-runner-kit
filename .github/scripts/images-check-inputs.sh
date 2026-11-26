#!/usr/bin/env bash
set -euo pipefail

push="${1:?usage: images-check-inputs.sh <push: true|false> <tag>}"
tag="${2-}"

case "$push" in
  true | false) ;;
  *)
    echo "::error::push must be true or false, got '${push}'" >&2
    exit 1
    ;;
esac

if [ "$push" = "true" ]; then
  if [ -z "$tag" ]; then
    echo "::error::the input tag is required when push is true" >&2
    exit 1
  fi
  if ! [[ "$tag" =~ ^[A-Za-z0-9_][A-Za-z0-9_.-]{0,127}$ ]]; then
    echo "::error::'${tag}' is not a valid image tag" >&2
    exit 1
  fi
fi
