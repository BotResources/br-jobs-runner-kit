#!/usr/bin/env bash
set -euo pipefail

usage="usage: check-no-llm-clients.sh <crate> [--allow <library>]... [--allow-dev <library>]..."
crate="${1:?$usage}"
shift
forbidden_prefix="br-llm-clients"
allow=()
allow_dev=()

while [ "$#" -gt 0 ]; do
  case "$1" in
    --allow)
      [ "$#" -ge 2 ] || { echo "::error::--allow needs a library name" >&2; exit 2; }
      allow+=("$2")
      shift 2
      ;;
    --allow-dev)
      [ "$#" -ge 2 ] || { echo "::error::--allow-dev needs a library name" >&2; exit 2; }
      allow_dev+=("$2")
      shift 2
      ;;
    *)
      echo "::error::unknown argument $1 ($usage)" >&2
      exit 2
      ;;
  esac
done

if [ -n "${CARGO_METADATA_FILE:-}" ]; then
  metadata_file="$CARGO_METADATA_FILE"
else
  metadata_file="$(mktemp)"
  trap 'rm -f "$metadata_file"' EXIT
  cargo metadata --format-version 1 --locked --all-features >"$metadata_file"
fi

python3 -I - "$metadata_file" "$crate" "$forbidden_prefix" "$(IFS=,; echo "${allow[*]-}")" "$(IFS=,; echo "${allow_dev[*]-}")" <<'PY'
import json
import sys

path, crate, prefix, allow_text, allow_dev_text = sys.argv[1:6]
allow = {name for name in allow_text.split(",") if name}
allow_dev = {name for name in allow_dev_text.split(",") if name}

def fail(message):
    print(f"::error::{message}", file=sys.stderr)
    sys.exit(1)

for name in sorted(allow | allow_dev):
    if not name.startswith(prefix):
        fail(f"{name} is not a {prefix}* library, it cannot be allowed")

try:
    with open(path) as handle:
        metadata = json.load(handle)
    names = {package["id"]: package["name"] for package in metadata["packages"]}
    edges = {
        node["id"]: [
            (dep["pkg"], all(kind.get("kind") == "dev" for kind in dep.get("dep_kinds", [])) and bool(dep.get("dep_kinds")))
            for dep in node["deps"]
        ]
        for node in metadata["resolve"]["nodes"]
    }
except (OSError, ValueError, KeyError, TypeError, AttributeError) as error:
    fail(f"cannot read the cargo metadata ({type(error).__name__}: {error})")
roots = [package_id for package_id, name in names.items() if name == crate and package_id in metadata["workspace_members"]]
if len(roots) != 1:
    fail(f"{crate} is not exactly one workspace member of this graph")
if roots[0] not in edges:
    fail(f"{crate} has no node in the resolved graph")


def walk(skip_dev):
    parent = {roots[0]: None}
    queue = [roots[0]]
    found = []
    while queue:
        current = queue.pop(0)
        if current not in edges:
            fail(f"{names.get(current, current)} has no node in the resolved graph")
        for dep, dev_only in edges[current]:
            if dep in parent or (skip_dev and dev_only):
                continue
            if dep not in names:
                fail(f"{dep} is not a package of the metadata")
            parent[dep] = current
            if names[dep].startswith(prefix):
                found.append(dep)
            queue.append(dep)
    return parent, found


def chain_of(parent, dep):
    chain = []
    cursor = dep
    while cursor is not None:
        chain.append(names[cursor])
        cursor = parent[cursor]
    return " <- ".join(chain)


normal_parent, normal_found = walk(True)
all_parent, all_found = walk(False)
normal_ids = set(normal_found)
violations = []
for dep in normal_found:
    if names[dep] not in allow:
        violations.append((names[dep], chain_of(normal_parent, dep), "it is allowed through none of its dependencies"))
for dep in all_found:
    if dep in normal_ids:
        continue
    if names[dep] not in allow and names[dep] not in allow_dev:
        violations.append((names[dep], chain_of(all_parent, dep), "it is reachable through a dev-dependency and allowed for none"))

if violations:
    for name, chain, why in violations:
        print(f"::error::{crate} reaches {name} ({chain}); {why}: it may depend on no {prefix}* library outside the allow list", file=sys.stderr)
    sys.exit(1)

reached = sorted({names[dep] for dep in all_found})
if reached:
    print(f"ok {crate} reaches only the allowed {prefix}* libraries: {', '.join(reached)}")
else:
    print(f"ok {crate} reaches no {prefix}* library (dependencies and dev-dependencies, direct or transitive)")
PY
