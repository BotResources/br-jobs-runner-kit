#!/usr/bin/env bash
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
GATE="$HERE/check-no-llm-clients.sh"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/test-no-llm-clients.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT

failures=0
cases=0

expect() {
    local want_rc="$1" needle="$2" what="$3"
    shift 3
    local rc=0 out
    out="$("$@" 2>&1)" || rc=$?
    cases=$((cases + 1))
    if [[ "$rc" -eq "$want_rc" ]] && grep -qF -- "$needle" <<<"$out"; then
        printf '  ok  %s\n' "$what"
    else
        failures=$((failures + 1))
        printf 'FAIL  %s (exit %s)\n' "$what" "$rc"
        printf '%s\n' "$out" | sed 's/^/        | /' | head -12
    fi
}

write_metadata() {
    local target="$1" edges="$2" kind="${3:-normal}"
    python3 -I - "$target" "$edges" "$kind" <<'PY'
import json
import sys

target, edges, kind = sys.argv[1], json.loads(sys.argv[2]), sys.argv[3]
ids = sorted({name for name in edges} | {dep for deps in edges.values() for dep in deps})
packages = [{"id": f"id-{name}", "name": name} for name in ids]
nodes = [{"id": f"id-{name}", "deps": [{"pkg": f"id-{dep}", "dep_kinds": [{"kind": None if kind == "normal" else kind, "target": None}]} for dep in edges.get(name, [])]} for name in ids]
with open(target, "w") as handle:
    json.dump({"packages": packages, "workspace_members": ["id-br-runner-connection"], "resolve": {"nodes": nodes}}, handle)
PY
}

clean="$WORK/clean.json"
write_metadata "$clean" '{"br-runner-connection": ["tokio", "async-nats"], "tokio": ["bytes"]}'
expect 0 "reaches no br-llm-clients* library" "a crate with no family library passes" \
    env CARGO_METADATA_FILE="$clean" bash "$GATE" br-runner-connection

direct="$WORK/direct.json"
write_metadata "$direct" '{"br-runner-connection": ["tokio", "br-llm-clients-core"]}'
expect 1 "br-llm-clients-core (br-llm-clients-core <- br-runner-connection)" "a direct dependency on the family fails" \
    env CARGO_METADATA_FILE="$direct" bash "$GATE" br-runner-connection

transitive="$WORK/transitive.json"
write_metadata "$transitive" '{"br-runner-connection": ["helper"], "helper": ["br-llm-clients-common-http"]}'
expect 1 "br-llm-clients-common-http (br-llm-clients-common-http <- helper <- br-runner-connection)" "a transitive dependency on the family fails" \
    env CARGO_METADATA_FILE="$transitive" bash "$GATE" br-runner-connection

dev="$WORK/dev.json"
write_metadata "$dev" '{"br-runner-connection": ["tokio", "br-llm-clients-core"]}' dev
expect 1 "br-llm-clients-core (br-llm-clients-core <- br-runner-connection)" "a family library reachable only through a dev-dependency edge fails" \
    env CARGO_METADATA_FILE="$dev" bash "$GATE" br-runner-connection

empty_nodes="$WORK/empty-nodes.json"
printf '%s' '{"packages": [{"id": "id-br-runner-connection", "name": "br-runner-connection"}], "workspace_members": ["id-br-runner-connection"], "resolve": {"nodes": []}}' >"$empty_nodes"
expect 1 "has no node in the resolved graph" "a root package with no resolved node fails" \
    env CARGO_METADATA_FILE="$empty_nodes" bash "$GATE" br-runner-connection

malformed="$WORK/malformed.json"
printf '%s' '{"packages": [' >"$malformed"
expect 1 "cannot read the cargo metadata" "malformed JSON fails" \
    env CARGO_METADATA_FILE="$malformed" bash "$GATE" br-runner-connection

expect 1 "cannot read the cargo metadata" "a missing metadata file fails" \
    env CARGO_METADATA_FILE="$WORK/absent.json" bash "$GATE" br-runner-connection

other="$WORK/other.json"
write_metadata "$other" '{"br-runner-connection": ["tokio"], "br-runner-threads": ["br-llm-clients-core"]}'
expect 0 "reaches no br-llm-clients* library" "a sibling crate may depend on the family" \
    env CARGO_METADATA_FILE="$other" bash "$GATE" br-runner-connection

expect 1 "is not exactly one workspace member" "an unknown crate fails" \
    env CARGO_METADATA_FILE="$clean" bash "$GATE" br-no-such-crate

ROOT="$(cd "$HERE/../.." && pwd)"
expect 0 "reaches no br-llm-clients* library" "the real br-runner-connection reaches no family library" \
    bash -c 'cd "$1" && bash "$2" br-runner-connection' _ "$ROOT" "$GATE"

echo
if [[ "$failures" -gt 0 ]]; then
    echo "${failures}/${cases} case(s) failed" >&2
    exit 1
fi
echo "${cases} cases passed"
