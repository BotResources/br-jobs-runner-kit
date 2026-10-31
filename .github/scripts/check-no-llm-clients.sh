#!/usr/bin/env bash
set -euo pipefail

crate="${1:?usage: check-no-llm-clients.sh <crate>}"
forbidden_prefix="br-llm-clients"

if [ -n "${CARGO_METADATA_FILE:-}" ]; then
  metadata_file="$CARGO_METADATA_FILE"
else
  metadata_file="$(mktemp)"
  trap 'rm -f "$metadata_file"' EXIT
  cargo metadata --format-version 1 --locked --all-features >"$metadata_file"
fi

python3 -I - "$metadata_file" "$crate" "$forbidden_prefix" <<'PY'
import json
import sys

path, crate, prefix = sys.argv[1:4]
with open(path) as handle:
    metadata = json.load(handle)

names = {package["id"]: package["name"] for package in metadata["packages"]}
edges = {node["id"]: [dep["pkg"] for dep in node["deps"]] for node in metadata["resolve"]["nodes"]}
roots = [package_id for package_id, name in names.items() if name == crate and package_id in metadata["workspace_members"]]
if len(roots) != 1:
    print(f"::error::{crate} is not exactly one workspace member of this graph", file=sys.stderr)
    sys.exit(1)

parent = {roots[0]: None}
queue = [roots[0]]
found = []
while queue:
    current = queue.pop(0)
    for dep in edges.get(current, []):
        if dep in parent:
            continue
        parent[dep] = current
        if names[dep].startswith(prefix):
            found.append(dep)
        queue.append(dep)

if found:
    for dep in found:
        chain = []
        cursor = dep
        while cursor is not None:
            chain.append(names[cursor])
            cursor = parent[cursor]
        print(f"::error::{crate} reaches {names[dep]} ({' <- '.join(chain)}); it may depend on no {prefix}* library, not even for its tests", file=sys.stderr)
    sys.exit(1)

print(f"ok {crate} reaches no {prefix}* library (dependencies and dev-dependencies, direct or transitive)")
PY
