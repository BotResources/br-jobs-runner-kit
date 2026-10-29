#!/usr/bin/env bash
set -euo pipefail

crate="${1:?usage: check-readme-pins.sh <crate>}"
repo_url="https://github.com/BotResources/br-jobs-runner-kit"
readme="crates/${crate}/README.md"
version=$(bash "$(dirname "$0")/crate-version.sh" "$crate")

if [ "$version" = "0.0.0" ]; then
  echo "✓ ${crate} 0.0.0 scaffold — no tag has shipped, no self-pin to check"
  exit 0
fi

if [ ! -f "$readme" ]; then
  echo "::error::${readme} is missing" >&2
  exit 1
fi

expected_tag="${crate}/v${version}"
expected_pin="package = \"${crate}\", tag = \"${expected_tag}\", version = \"${version}\""

if ! grep -qF "${expected_pin}" "$readme"; then
  echo "::error file=${readme}::the ${crate} install pin must read ${expected_pin} (a tag-only git dep is a wildcard and cargo-deny denies it)" >&2
  exit 1
fi

fail=0
while IFS= read -r line; do
  [ -n "$line" ] || continue
  if ! grep -qF "${expected_pin}" <<<"$line"; then
    echo "::error file=${readme}::${crate} self-pin is stale: ${line}" >&2
    fail=1
  fi
done < <(grep -F "${repo_url}\"" "$readme" | grep -F "package = \"${crate}\"" || true)

if [ "$fail" -eq 0 ]; then
  echo "✓ ${readme}: ${crate} self-pin on ${expected_tag} / ${version}"
fi

exit $fail
