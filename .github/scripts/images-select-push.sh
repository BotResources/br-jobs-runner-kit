#!/usr/bin/env bash
set -euo pipefail

images="${1:?usage: images-select-push.sh <JSON list of image names>}"
push="${PUSH:?}"
hold_marked="${HOLD_MARKED:?}"
root="${IMAGES_ROOT:-images}"
marker="PUSH_ON_MAIN_ONLY"

for value in "$push" "$hold_marked"; do
  case "$value" in
    true | false) ;;
    *)
      echo "::error::PUSH and HOLD_MARKED must be true or false, got '${value}'" >&2
      exit 1
      ;;
  esac
done

python3 -I - "$images" "$push" "$hold_marked" "$root" "$marker" <<'PY'
import json, os, re, sys

images, push, hold_marked, root, marker = sys.argv[1:6]
valid = re.compile(r"^[a-z0-9][a-z0-9._-]*$")
names = json.loads(images)
if not isinstance(names, list) or not all(isinstance(name, str) and valid.match(name) for name in names):
    print("::error::the images are not a JSON list of registry names", file=sys.stderr)
    sys.exit(1)

selected = []
if push == "true":
    for name in names:
        held = hold_marked == "true" and os.path.isfile(os.path.join(root, name, marker))
        if not held:
            selected.append(name)
print(json.dumps(sorted(selected)))
PY
