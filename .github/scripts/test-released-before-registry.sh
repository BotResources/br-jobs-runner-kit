#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/test-lib.sh"

REPO_ROOT="$(cd "$SRC_SCRIPTS/../.." && pwd)"
CRATE=br-fixture-alpha
LEGACY_TAG="v0.2.0"
RELEASE_TAG="$CRATE/v0.3.0"
LIBRARY_ID="$ALPHA_ID"

install_stubs
init_repo

printf '[package]\nname = "%s"\nversion = "0.2.0"\nedition = "2024"\n' "$CRATE" >"$DEV/Cargo.toml"
mkdir -p "$DEV/src"
echo "pub fn x() {}" >"$DEV/src/lib.rs"
printf '# Changelog\n\n## Unreleased\n\n## 0.2.0 - 2026-09-22\n' >"$DEV/CHANGELOG.md"
printf 'target\n' >"$DEV/.gitignore"
g add -A && g commit -q -m "single-package layout, 0.2.0 released" && g push -q origin main
LEGACY_COMMIT="$(g rev-parse HEAD)"
g tag -a "$LEGACY_TAG" -m "released before the registry chain" && g push -q origin "$LEGACY_TAG"

g checkout -q -b feat/next
sed -e 's/^version = ".*"/version = "0.3.0"/' "$DEV/Cargo.toml" >"$DEV/Cargo.toml.new" && mv "$DEV/Cargo.toml.new" "$DEV/Cargo.toml"
printf '# Changelog\n\n## Unreleased\n\n## 0.3.0 - 2026-09-25\n\n- change\n\n## 0.2.0 - 2026-09-22\n' >"$DEV/CHANGELOG.md"
echo "pub fn y() {}" >>"$DEV/src/lib.rs"
BUMP_COMMIT="$(commit_all "0.3.0 in the single-package layout")"
mkdir -p "$DEV/crates/$CRATE"
g mv src "crates/$CRATE/src"
g mv Cargo.toml "crates/$CRATE/Cargo.toml"
g mv CHANGELOG.md "crates/$CRATE/CHANGELOG.md"
printf '[workspace]\nresolver = "3"\nmembers = ["crates/%s"]\n' "$CRATE" >"$DEV/Cargo.toml"
printf 'library-id = "%s"\n' "$LIBRARY_ID" >"$DEV/crates/$CRATE/registry.toml"
copy_scripts "$CRATE"
(cd "$DEV" && cargo generate-lockfile --offline -q)
MIGRATION_COMMIT="$(commit_all "workspace layout and release chain, version still 0.3.0")"

requests() { ls "$STUB_DIR/requests" | wc -l | tr -d ' '; }
request_value() { python3 -c '
import json, sys
v = json.load(open(sys.argv[1]))["variables"]
for key in sys.argv[2].split("."):
    v = v[key]
print(v)' "$STUB_DIR/requests/$1.json" "$2"; }
detect() {
    in_dev env GITHUB_REF=refs/heads/main EVENT_NAME=push GITHUB_SHA="$(g rev-parse HEAD)" bash .github/scripts/release-detect.sh
}
gate() { in_dev env BR_REGISTRY_KEY=k GITHUB_OUTPUT="$WORK/gate.out" bash .github/scripts/registry-gate.sh "$@"; }
pr_gate() { in_dev env BASE_REF=main BR_REGISTRY_KEY=k bash .github/scripts/pr-registry-gate.sh; }

echo "== the version lookup reads revisions from before the workspace layout"
expect_equal "$(in_dev bash .github/scripts/crate-version.sh "$CRATE" "$LEGACY_COMMIT")" 0.2.0 "single-package revision on main: 0.2.0"
expect_equal "$(in_dev bash .github/scripts/crate-version.sh "$CRATE" "$BUMP_COMMIT")" 0.3.0 "single-package revision of the bump: 0.3.0"
expect_equal "$(in_dev bash .github/scripts/crate-version.sh "$CRATE" "$MIGRATION_COMMIT")" 0.3.0 "workspace revision: 0.3.0"
expect 1 'is missing at' "a single-package revision of another package: refuses" in_dev bash .github/scripts/crate-version.sh br-other "$LEGACY_COMMIT"
expect 0 "✓ $RELEASE_TAG" "the dated 0.3.0 heading satisfies the changelog check" in_dev bash .github/scripts/check-changelog.sh "$CRATE" "$MIGRATION_COMMIT"

echo "== the pull request moves the layout and carries the 0.2.0 -> 0.3.0 bump"
stub_reset
stub_error librariesValidateReleaseTarget PATCH_NOT_SEALED "not sealed"
expect 1 '(PATCH_NOT_SEALED)' "0.3.0 not yet sealed in the registry: the gate fails and names it" pr_gate
expect_equal "$(requests)" 1 "one registry request, for 0.3.0"
expect_equal "$(request_value 1 v)" 0.3.0 "the request asks for 0.3.0"
expect_equal "$(request_value 1 id)" "$LIBRARY_ID" "the request carries the committed library id"
stub_reset
eligible_target "\"$LEGACY_TAG\"" false 0.3.0
expect 0 'br-fixture-alpha: 0.2.0 -> 0.3.0' "the base version is read from the single-package main" pr_gate
expect 0 'eligible for release' "0.3.0 sealed and not implemented: the gate passes" pr_gate
expect 0 "no prior $CRATE/v* tag" "semver baseline: no tag of the new format, skipped" in_dev env SEMVER_STUB=1 bash .github/scripts/semver-check.sh "$CRATE"

echo "== the merge to main releases 0.3.0 under the new tag format"
g checkout -q main
g merge -q --squash feat/next >/dev/null
RELEASE_COMMIT="$(commit_all "0.3.0 (squash merge)")"
g push -q origin main
run detect
if [[ "$RC" -eq 0 ]] && python3 -c '
import json, sys
m = json.loads(sys.argv[1])
assert [(e["crate"], e["version"], e["line"], e["commit"]) for e in m] == [("br-fixture-alpha", "0.3.0", "main", sys.argv[2])], m
' "$(sed -n 's/^matrix=//p' <<<"$OUT" | head -1)" "$RELEASE_COMMIT"; then
    verdict yes "detect proposes exactly br-fixture-alpha 0.3.0 from main at the merge commit"
else
    verdict no "detect proposes exactly br-fixture-alpha 0.3.0 from main at the merge commit"
fi
stub_reset
eligible_target "\"$LEGACY_TAG\"" false 0.3.0
: >"$WORK/gate.out"
expect 0 'eligible for release' "the gate accepts 0.3.0" gate "$CRATE" 0.3.0 "$RELEASE_TAG"
expect_file_has "$WORK/gate.out" "baseline_tag=$LEGACY_TAG" "the baseline tag is the one the registry names, v0.2.0"
expect_file_has "$WORK/gate.out" 'implemented=false' "implemented=false written"
expect 0 'registry prepare' "prepare with the old-format baseline" in_dev env BR_REGISTRY_KEY=k REGISTRY_WORK="$WORK/reg" bash .github/scripts/registry-prepare.sh "$CRATE" 0.3.0 "$RELEASE_COMMIT" "$LEGACY_TAG"
expect_file_has "$STUB_DIR/cargo.log" "diff $LEGACY_TAG..$RELEASE_COMMIT" "the API diff runs from v0.2.0 to the release commit"
expect 0 "creating tag $RELEASE_TAG" "the release takes the new tag format" in_dev env GITHUB_REPOSITORY=o/r bash .github/scripts/release-crate.sh "$CRATE" 0.3.0 refs/heads/main "$RELEASE_COMMIT"
expect_equal "$(git -C "$ORIGIN" rev-list -n 1 "refs/tags/$RELEASE_TAG")" "$RELEASE_COMMIT" "$RELEASE_TAG is on origin, at the merge commit"
expect_file_has "$STUB_DIR/gh.log" "gh release create $RELEASE_TAG" "the GitHub release is created"
expect_file_has "$STUB_DIR/gh.log" '--latest=true' "main moves Latest"
stub_reset
expect 0 "$CRATE 0.3.0 recorded" "record with the old-format baseline" in_dev env BR_REGISTRY_KEY=k REGISTRY_WORK="$WORK/reg" bash .github/scripts/registry-record.sh "$CRATE" 0.3.0 "$RELEASE_TAG" "$RELEASE_COMMIT" "$LEGACY_TAG"
expect_equal "$(request_value 3 input.releaseTag)" "$RELEASE_TAG" "the release record carries the new tag"
expect_equal "$(request_value 3 input.releaseCommit)" "$RELEASE_COMMIT" "the release record carries the merge commit"
expect_equal "$(g rev-list -n 1 "refs/tags/$LEGACY_TAG")" "$LEGACY_COMMIT" "the legacy tag still points at the 0.2.0 commit"

echo "== a re-run on the merge commit, then the next push"
expect 0 '"version":"0.3.0"' "a re-run on the merge commit proposes 0.3.0 again" detect
stub_error librariesValidateReleaseTarget PATCH_IMPLEMENTED "implemented"
: >"$WORK/gate.out"
expect 0 'idempotent re-run' "the gate reports implemented and exits 0" gate "$CRATE" 0.3.0 "$RELEASE_TAG"
expect_file_has "$WORK/gate.out" 'implemented=true' "implemented=true written"
expect_file_has "$REPO_ROOT/.github/workflows/release-tags.yml" "if: steps.gate.outputs.implemented != 'true'" "the workflow skips prepare, tag and record when implemented=true"
commit_all "unrelated" >/dev/null
expect 0 'already shipped' "the next push: detect says 0.3.0 is already shipped" detect
expect 0 'matrix=[]' "the next push releases nothing" detect
: >"$STUB_DIR/cargo.log"
expect 0 '' "semver baseline: the tag of the new format" in_dev env SEMVER_STUB=1 bash .github/scripts/semver-check.sh "$CRATE"
expect_file_has "$STUB_DIR/cargo.log" "semver-checks -p $CRATE" "semver-checks runs against a baseline"
expect_file_has "$STUB_DIR/cargo.log" "at $RELEASE_TAG" "from the tree at $RELEASE_TAG"

summary
