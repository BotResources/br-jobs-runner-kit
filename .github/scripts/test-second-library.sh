#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/test-lib.sh"

REPO_ROOT="$(cd "$SRC_SCRIPTS/../.." && pwd)"
FIRST=br-fixture-alpha
SECOND=br-fixture-beta
SECOND_ID="01a00000-0000-7000-8000-0000000000a2"

echo "== this repository: every tracked crate carries a library id, every other release crate is frozen"
tracked="$(cd "$REPO_ROOT" && source .github/scripts/tracked-libraries.sh && printf '%s\n' $TRACKED_LIBRARIES)"
for crate in $tracked; do
    run bash -c 'cd "$1" && source .github/scripts/tracked-libraries.sh && source .github/scripts/library-meta.sh && library_ids "$2"' _ "$REPO_ROOT" "$crate"
    if [[ "$RC" -eq 0 ]]; then verdict yes "$crate: tracked, registry.toml well-formed"; else verdict no "$crate: tracked, registry.toml well-formed"; fi
done
run bash -c 'cd "$1" && source .github/scripts/tracked-libraries.sh && source .github/scripts/library-meta.sh && refuse_untracked_releases' _ "$REPO_ROOT"
expect_equal "$RC" 0 "no workspace crate escapes both the tracked list and the frozen list"

install_stubs
init_repo
copy_scripts "$FIRST"
printf '[workspace]\nresolver = "3"\nmembers = ["crates/*"]\n' >"$DEV/Cargo.toml"
printf 'target\n' >"$DEV/.gitignore"
write_crate "$FIRST"
FIRST_ID="$ALPHA_ID"
printf 'library-id = "%s"\n' "$FIRST_ID" >"$DEV/crates/$FIRST/registry.toml"
(cd "$DEV" && cargo generate-lockfile --offline -q)
g add -A && g commit -q -m "one library" && g push -q origin main

detect() {
    in_dev env GITHUB_REF=refs/heads/main EVENT_NAME=push GITHUB_SHA="$(g rev-parse HEAD)" bash .github/scripts/release-detect.sh
}
gate() { in_dev env BR_REGISTRY_KEY=k GITHUB_OUTPUT="$WORK/gate.out" bash .github/scripts/registry-gate.sh "$@"; }
record() { in_dev env BR_REGISTRY_KEY=k REGISTRY_WORK="$WORK/reg" bash .github/scripts/registry-record.sh "$@"; }
release() { in_dev env GITHUB_REPOSITORY=o/r bash .github/scripts/release-crate.sh "$@"; }
matrix_crates() {
    sed -n 's/^matrix=//p' <<<"$OUT" | head -1 | python3 -c '
import json, sys
print(",".join("%s@%s" % (e["crate"], e["version"]) for e in json.load(sys.stdin)))'
}
request_value() { python3 -c '
import json, sys
v = json.load(open(sys.argv[1]))["variables"]
for key in sys.argv[2].split("."):
    v = v[key]
print(v)' "$STUB_DIR/requests/$1.json" "$2"; }

echo "== one library"
set_version "$FIRST" 0.3.0
commit_all "$FIRST 0.3.0" >/dev/null
run detect
expect_equal "$(matrix_crates)" "$FIRST@0.3.0" "only $FIRST is released"

echo "== a second library needs a crate directory, a registry.toml and one tracked entry"
write_crate "$SECOND"
set_version "$SECOND" 0.1.0
commit_all "$SECOND crate directory only" >/dev/null
expect 1 "$SECOND declares 0.1.0 but is not in TRACKED_LIBRARIES" "crate directory alone: the release scan refuses" detect
sed -i.bak "s/\"\$/ $SECOND\"/" "$DEV/.github/scripts/tracked-libraries.sh" && rm "$DEV/.github/scripts/tracked-libraries.sh.bak"
expect_file_has "$DEV/.github/scripts/tracked-libraries.sh" "\"$FIRST $SECOND\"" "one entry added to tracked-libraries.sh"
commit_all "track $SECOND" >/dev/null
stub_reset
eligible_target null true 0.1.0
expect 1 'not found' "tracked but no registry.toml: the gate refuses" gate "$SECOND" 0.1.0
printf 'library-id = "%s"\n' "$SECOND_ID" >"$DEV/crates/$SECOND/registry.toml"
commit_all "$SECOND registry.toml" >/dev/null

echo "== both are released, independently, each under its own tag"
run detect
expect_equal "$(matrix_crates)" "$FIRST@0.3.0,$SECOND@0.1.0" "the matrix holds one entry per crate"
RELEASE_COMMIT="$(g rev-parse HEAD)"
stub_reset
eligible_target null true 0.3.0
expect 0 'eligible for release' "gate $FIRST" gate "$FIRST" 0.3.0
eligible_target null true 0.1.0
expect 0 'eligible for release' "gate $SECOND" gate "$SECOND" 0.1.0
expect_equal "$(request_value 1 id)" "$FIRST_ID" "the first gate asked for the library id of $FIRST"
expect_equal "$(request_value 2 id)" "$SECOND_ID" "the second gate asked for the library id of $SECOND"
expect 0 "creating tag $FIRST/v0.3.0" "tag $FIRST/v0.3.0" release "$FIRST" 0.3.0 refs/heads/main "$RELEASE_COMMIT"
expect 0 "creating tag $SECOND/v0.1.0" "tag $SECOND/v0.1.0" release "$SECOND" 0.1.0 refs/heads/main "$RELEASE_COMMIT"
expect 0 '' "$FIRST/v0.3.0 is on origin" git -C "$ORIGIN" rev-parse -q --verify "refs/tags/$FIRST/v0.3.0"
expect 0 '' "$SECOND/v0.1.0 is on origin" git -C "$ORIGIN" rev-parse -q --verify "refs/tags/$SECOND/v0.1.0"
stub_reset
expect 0 "$FIRST 0.3.0 recorded" "record $FIRST" record "$FIRST" 0.3.0 "$FIRST/v0.3.0" "$RELEASE_COMMIT"
expect 0 "$SECOND 0.1.0 recorded" "record $SECOND" record "$SECOND" 0.1.0 "$SECOND/v0.1.0" "$RELEASE_COMMIT"
expect_equal "$(request_value 1 input.libraryId)" "$FIRST_ID" "the records of $FIRST carry its library id"
expect_equal "$(request_value 3 input.releaseTag)" "$FIRST/v0.3.0" "the release record of $FIRST carries its tag"
expect_equal "$(request_value 4 input.libraryId)" "$SECOND_ID" "the records of $SECOND carry its library id"
expect_equal "$(request_value 6 input.releaseTag)" "$SECOND/v0.1.0" "the release record of $SECOND carries its tag"
expect 1 'must be br-fixture-beta/v0.1.0' "a tag of the other crate cannot be recorded" record "$SECOND" 0.1.0 "$FIRST/v0.3.0" "$RELEASE_COMMIT"

echo "== a version bump of one crate leaves the other alone"
commit_all "unrelated" >/dev/null
run detect
expect_equal "$(matrix_crates)" "" "both shipped, nothing to release"
set_version "$SECOND" 0.2.0
commit_all "$SECOND 0.2.0" >/dev/null
run detect
expect_equal "$(matrix_crates)" "$SECOND@0.2.0" "only $SECOND is released"
g branch "release/$FIRST-0.3" "$FIRST/v0.3.0"
g push -q origin "release/$FIRST-0.3"
run detect
expect_equal "$(matrix_crates)" "$SECOND@0.2.0" "a release line of $FIRST does not block $SECOND on main"
set_version "$FIRST" 0.3.1
commit_all "$FIRST 0.3.1 on main" >/dev/null
run detect
if [[ "$RC" -eq 0 && "$(matrix_crates)" == "$SECOND@0.2.0" ]] && grep -q "^refused=\[\"$FIRST 0.3.1" <<<"$OUT"; then verdict yes "the line of $FIRST refuses $FIRST 0.3.1 on main and $SECOND still goes out"; else verdict no "the line of $FIRST refuses $FIRST 0.3.1 on main and $SECOND still goes out"; fi

echo "== the pull request gate asks the registry once per bumped crate"
g push -q origin main
g checkout -q -b feat/two-bumps main
set_version "$FIRST" 0.4.0
set_version "$SECOND" 0.4.0
commit_all "both bumped" >/dev/null
stub_reset
eligible_target null true 0.4.0
expect 0 'br-fixture-beta 0.4.0: registry target sealed' "both bumped crates are gated" in_dev env BASE_REF=main BR_REGISTRY_KEY=k bash .github/scripts/pr-registry-gate.sh
expect_equal "$(ls "$STUB_DIR/requests" | wc -l | tr -d ' ')" 2 "one registry request per bumped crate"
expect_equal "$(request_value 1 id)" "$FIRST_ID" "the first request asked for $FIRST"
expect_equal "$(request_value 2 id)" "$SECOND_ID" "the second request asked for $SECOND"

echo "== a frozen crate stays out of the registry chain at its pinned version only"
FROZEN=br-fixture-frozen
write_crate "$FROZEN"
set_version "$FROZEN" 0.1.2
printf 'export FROZEN_CRATES="%s@0.1.2"\n' "$FROZEN" >>"$DEV/.github/scripts/tracked-libraries.sh"
commit_all "$FROZEN joins frozen at 0.1.2" >/dev/null
run detect
expect_equal "$RC" 0 "a frozen crate at its pin does not fail the release scan"
if [[ "$(matrix_crates)" == *"$FROZEN"* ]]; then verdict no "a frozen crate at its pin is not released"; else verdict yes "a frozen crate at its pin is not released"; fi
set_version "$FROZEN" 0.1.3
commit_all "$FROZEN 0.1.3" >/dev/null
expect 1 "$FROZEN declares 0.1.3 but is not in TRACKED_LIBRARIES" "a frozen crate that bumps is refused" detect
set_version "$FROZEN" 0.1.2
sed -i.bak 's/^export FROZEN_CRATES=.*$/export FROZEN_CRATES=""/' "$DEV/.github/scripts/tracked-libraries.sh" && rm "$DEV/.github/scripts/tracked-libraries.sh.bak"
commit_all "$FROZEN unlisted at 0.1.2" >/dev/null
expect 1 "$FROZEN declares 0.1.2 but is not in TRACKED_LIBRARIES" "an unlisted crate that is not 0.0.0 is refused" detect

summary
