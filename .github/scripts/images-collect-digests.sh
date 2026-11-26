#!/usr/bin/env bash
set -euo pipefail

dir="${1:?usage: images-collect-digests.sh <dir with <image>.ref files>}"

python3 -I - "$dir" <<'PY'
import json, os, sys
digests = {}
for entry in sorted(os.listdir(sys.argv[1])):
    if entry.endswith(".ref"):
        with open(os.path.join(sys.argv[1], entry)) as handle:
            digests[entry[: -len(".ref")]] = handle.read().strip()
print(json.dumps(digests, separators=(",", ":")))
PY
