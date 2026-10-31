#!/usr/bin/env bash
set -euo pipefail

crate="${1:?usage: check-bus-access.sh <crate> <module> [root]}"
module="${2:?usage: check-bus-access.sh <crate> <module> [root]}"
root="${3:-.}"
dir="$root/crates/$crate"
failed=0

fail() {
    echo "::error::$crate: $1" >&2
    failed=1
}

[ -d "$dir/src" ] || { echo "::error::$dir/src does not exist" >&2; exit 2; }

sources() {
    find "$dir/src" -name '*.rs' -not -path '*/tests/*' -not -path '*_tests/*' -not -name 'tests.rs' | sort
}

outside_module() {
    sources | grep -v "^$dir/src/$module/" || true
}

if [ ! -d "$dir/src/$module" ]; then
    fail "the module src/$module does not exist"
fi

for file in $(outside_module); do
    if grep -n -E 'async_nats|async-nats' "$file" >/dev/null; then
        fail "async_nats is named outside src/$module: $file"
    fi
    if grep -n -E 'Fabric::new|jetstream::new' "$file" >/dev/null; then
        fail "a Fabric is built outside src/$module: $file"
    fi
    if grep -n -E '\.bus\(\)|bus_request' "$file" >/dev/null; then
        fail "the bus accessor or the bus request of the Connection is used outside src/$module: $file"
    fi
done

for file in $(sources); do
    if grep -n -E '"(jobs\.|stream\.|STREAMING_|JOBS_|integration\.)' "$file" >/dev/null; then
        fail "a subject, stream or bucket literal is in $file"
    fi
done

if grep -E '^br-llm-clients[A-Za-z0-9_-]* *[=.]' "$dir/Cargo.toml" >/dev/null; then
    fail "Cargo.toml lists a br-llm-clients library"
fi

if [ "$failed" -ne 0 ]; then
    exit 1
fi

echo "ok $crate: async_nats, the Fabric build and the bus accessor stay in src/$module, and src holds no subject, stream or bucket literal"
