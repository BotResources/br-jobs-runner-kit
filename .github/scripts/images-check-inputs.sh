#!/usr/bin/env bash
set -euo pipefail

push="${1:?usage: images-check-inputs.sh <push: true|false> <tag> [images_dir]}"
tag="${2-}"
images_dir="${3-images}"

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

if ! [[ "$images_dir" =~ ^[A-Za-z0-9._-]+(/[A-Za-z0-9._-]+)*$ ]] || [[ "/${images_dir}/" == *"/../"* ]] || [[ "/${images_dir}/" == *"/./"* ]]; then
  echo "::error::images_dir '${images_dir}' is not a relative path of plain folder names without '.' or '..'" >&2
  exit 1
fi

if [ "$push" = "true" ] && [ "$images_dir" != "images" ]; then
  echo "::error::only the folder images can be pushed, got images_dir '${images_dir}'" >&2
  exit 1
fi
