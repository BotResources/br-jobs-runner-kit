#!/usr/bin/env bash
set -euo pipefail

crate="${1:?usage: crate-version.sh <crate> [<rev>]}"
rev="${2:-}"
manifest="crates/${crate}/Cargo.toml"

package_key() {
  awk -v key="$1" '
    /^\[package\]/ { in_pkg = 1; next }
    /^\[/ { in_pkg = 0 }
    in_pkg && $1 == key && $2 == "=" { gsub(/[" ]/, "", $3); print $3; exit }'
}

if [ -n "$rev" ]; then
  if content=$(git show "${rev}:${manifest}" 2>/dev/null); then
    :
  elif content=$(git show "${rev}:Cargo.toml" 2>/dev/null) \
    && [ "$(package_key name <<<"$content")" = "$crate" ]; then
    :
  else
    echo "::error::${manifest} is missing at ${rev}" >&2
    exit 1
  fi
else
  if [ ! -f "$manifest" ]; then
    echo "::error::${manifest} is missing" >&2
    exit 1
  fi
  content=$(cat "$manifest")
fi

version=$(package_key version <<<"$content")
if [ -z "$version" ]; then
  echo "::error file=${manifest}::could not extract the package version" >&2
  exit 1
fi

echo "$version"
