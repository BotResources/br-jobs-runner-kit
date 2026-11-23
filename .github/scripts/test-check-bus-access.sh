#!/usr/bin/env bash
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
GATE="$HERE/check-bus-access.sh"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/test-bus-access.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT

failures=0

expect() {
    local want_rc="$1" needle="$2" what="$3"
    shift 3
    local rc=0 out
    out="$("$@" 2>&1)" || rc=$?
    if [[ "$rc" -eq "$want_rc" ]] && grep -qF -- "$needle" <<<"$out"; then
        printf '  ok  %s\n' "$what"
    else
        failures=$((failures + 1))
        printf 'FAIL  %s (exit %s)\n' "$what" "$rc"
        printf '%s\n' "$out" | head -12 | while IFS= read -r line; do printf '        | %s\n' "$line"; done
    fi
}

fresh() {
    local root="$1"
    rm -rf "$root"
    mkdir -p "$root/crates/demo/src/transport" "$root/crates/demo/src/other/tests"
    printf '[dependencies]\nasync-nats = "0.48"\n' >"$root/crates/demo/Cargo.toml"
    printf 'use async_nats::Client;\nfn open(client: &Client) { let _ = client.clone(); }\n' >"$root/crates/demo/src/transport/mod.rs"
    printf 'pub fn nothing() {}\n' >"$root/crates/demo/src/lib.rs"
    printf 'use async_nats::Client;\n' >"$root/crates/demo/src/other/tests/mod.rs"
}

root="$WORK/ok"
fresh "$root"
expect 0 "ok demo" "a crate that keeps the bus in its module passes" bash "$GATE" demo transport "$root"

root="$WORK/nats"
fresh "$root"
printf 'use async_nats::Client;\n' >"$root/crates/demo/src/lib.rs"
expect 1 "async_nats is named outside src/transport" "async_nats outside the module fails" bash "$GATE" demo transport "$root"

root="$WORK/fabric"
fresh "$root"
printf 'fn make(js: J) { let _ = Fabric::new(js); }\n' >"$root/crates/demo/src/lib.rs"
expect 1 "a Fabric is built outside src/transport" "a Fabric built outside the module fails" bash "$GATE" demo transport "$root"

root="$WORK/bus"
fresh "$root"
printf 'fn go(c: C) { let _ = c.bus(); }\n' >"$root/crates/demo/src/lib.rs"
expect 1 "bus accessor or the bus request" "the bus accessor outside the module fails" bash "$GATE" demo transport "$root"

root="$WORK/request"
fresh "$root"
printf 'async fn go(c: C) { let _ = c.bus_request(a).await; }\n' >"$root/crates/demo/src/lib.rs"
expect 1 "bus accessor or the bus request" "the bus request outside the module fails" bash "$GATE" demo transport "$root"

for literal in 'jobs.trigger' 'stream.threads' 'STREAMING_x' 'JOBS_RUN_CANCEL' 'integration.evt'; do
    root="$WORK/literal-$literal"
    fresh "$root"
    printf 'const SUBJECT: &str = "%s";\n' "$literal" >"$root/crates/demo/src/transport/mod.rs"
    expect 1 "literal is in" "the literal $literal fails, also inside the module" bash "$GATE" demo transport "$root"
done

root="$WORK/clients"
fresh "$root"
printf 'br-llm-clients-core = { version = "1" }\n' >>"$root/crates/demo/Cargo.toml"
expect 1 "br-llm-clients library" "a direct br-llm-clients dependency fails" bash "$GATE" demo transport "$root"

root="$WORK/nomodule"
fresh "$root"
rm -rf "$root/crates/demo/src/transport"
expect 1 "does not exist" "a missing module fails" bash "$GATE" demo transport "$root"

root="$WORK/nocrate"
mkdir -p "$root"
expect 2 "does not exist" "a missing crate fails" bash "$GATE" demo transport "$root"

fresh_none() {
    local root="$1"
    rm -rf "$root"
    mkdir -p "$root/crates/demo/src/adapter/tests"
    printf '[dependencies]\nbr-runner-jobs-client = "0.1"\n' >"$root/crates/demo/Cargo.toml"
    printf 'pub fn nothing() {}\n' >"$root/crates/demo/src/lib.rs"
    printf 'use async_nats::Client;\n' >"$root/crates/demo/src/adapter/tests/mod.rs"
}

root="$WORK/none-ok"
fresh_none "$root"
expect 0 "ok demo: Cargo.toml lists no bus client" "a crate with no bus at all passes" bash "$GATE" demo --none "$root"

root="$WORK/none-nats"
fresh_none "$root"
printf 'use async_nats::Client;\n' >"$root/crates/demo/src/lib.rs"
expect 1 "the bus is named in" "none mode: async_nats in src fails" bash "$GATE" demo --none "$root"

root="$WORK/none-fabric"
fresh_none "$root"
printf 'fn make(js: J) { let _ = Fabric::new(js); }\n' >"$root/crates/demo/src/lib.rs"
expect 1 "the bus is named in" "none mode: a Fabric in src fails" bash "$GATE" demo --none "$root"

root="$WORK/none-accessor"
fresh_none "$root"
printf 'fn go(c: C) { let _ = c.bus(); }\n' >"$root/crates/demo/src/lib.rs"
expect 1 "the bus is named in" "none mode: the bus accessor fails" bash "$GATE" demo --none "$root"

for dependency in 'async-nats = "0.48"' 'br-util-nats-fabric = { version = "0.1" }'; do
    root="$WORK/none-dep-$dependency"
    fresh_none "$root"
    printf '%s\n' "$dependency" >>"$root/crates/demo/Cargo.toml"
    expect 1 "lists the bus client or a bus crate" "none mode: the dependency $dependency fails" bash "$GATE" demo --none "$root"
done

root="$WORK/none-dep-table"
fresh_none "$root"
printf '[dependencies.async-nats]\nversion = "0.48"\n' >>"$root/crates/demo/Cargo.toml"
expect 1 "lists the bus client or a bus crate" "none mode: the table form of the bus client fails" bash "$GATE" demo --none "$root"

root="$WORK/none-dep-renamed"
fresh_none "$root"
printf 'transport = { package = "async-nats", version = "0.48" }\n' >>"$root/crates/demo/Cargo.toml"
expect 1 "lists the bus client or a bus crate" "none mode: a renamed bus client fails" bash "$GATE" demo --none "$root"

root="$WORK/none-literal"
fresh_none "$root"
printf 'const SUBJECT: &str = "integration.evt";\n' >"$root/crates/demo/src/lib.rs"
expect 1 "literal is in" "none mode: a subject literal fails" bash "$GATE" demo --none "$root"

root="$WORK/none-nocrate"
mkdir -p "$root"
expect 2 "does not exist" "none mode: a missing crate fails" bash "$GATE" demo --none "$root"

if [[ "$failures" -ne 0 ]]; then
    echo "$failures case(s) failed" >&2
    exit 1
fi
echo "all cases passed"
