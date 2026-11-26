#!/usr/bin/env bash
set -euo pipefail

wanted="${1:-}"
root="${IMAGES_ROOT:-images}"
valid='^[a-z0-9][a-z0-9._-]*$'

if [ -n "$wanted" ] && ! [[ "$wanted" =~ $valid ]]; then
  echo "::error::image name '${wanted}' is not a lowercase registry name" >&2
  exit 1
fi

names=()
for dockerfile in "$root"/*/Dockerfile; do
  [ -f "$dockerfile" ] || continue
  name="$(basename "$(dirname "$dockerfile")")"
  if ! [[ "$name" =~ $valid ]]; then
    echo "::error::the folder '${root}/${name}' is not a lowercase registry name (${valid})" >&2
    exit 1
  fi
  if [ -z "$wanted" ] || [ "$wanted" = "$name" ]; then
    names+=("$name")
  fi
done

if [ -n "$wanted" ] && [ "${#names[@]}" -eq 0 ]; then
  echo "::error::no ${root}/${wanted}/Dockerfile" >&2
  exit 1
fi

python3 -I -c 'import json, sys; print(json.dumps(sorted(sys.argv[1:])))' "${names[@]+"${names[@]}"}"
