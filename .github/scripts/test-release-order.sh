#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/test-lib.sh"

REPO_ROOT="$(cd "$SRC_SCRIPTS/../.." && pwd)"
WORKFLOW="$REPO_ROOT/.github/workflows/release-tags.yml"

line_of() { grep -n -- "$1" "$WORKFLOW" | head -1 | cut -d: -f1; }

echo "== the workflow runs the gate, the computation, the tag and the record in that order"
check=$(line_of 'check-changelog.sh')
gate=$(line_of 'registry-gate.sh')
prepare=$(line_of 'registry-prepare.sh')
tag=$(line_of 'release-crate.sh')
record=$(line_of 'registry-record.sh')
if (( check < gate && gate < prepare && prepare < tag && tag < record )); then
    verdict yes "changelog ($check) < gate ($gate) < prepare ($prepare) < tag ($tag) < record ($record)"
else
    verdict no "changelog < gate < prepare < tag < record"
fi
expect_equal "$(grep -c "if: steps.gate.outputs.implemented != 'true'" "$WORKFLOW")" 3 "prepare, tag and record all wait for the gate verdict"

echo "== nothing but release-crate.sh creates or pushes a tag, and only the workflow reaches it"
tag_writers=$(cd "$REPO_ROOT" && grep -lE 'git tag -a|git push origin|gh release create' .github/scripts/*.sh .github/workflows/*.yml | grep -v '/test-' | tr '\n' ' ')
expect_equal "$tag_writers" ".github/scripts/release-crate.sh " "release-crate.sh is the only tag writer"
callers=$(cd "$REPO_ROOT" && grep -lF 'release-crate.sh' .github/scripts/*.sh .github/workflows/*.yml | grep -v '/test-' | tr '\n' ' ')
expect_equal "$callers" ".github/workflows/release-tags.yml " "release-tags.yml is the only caller"

echo "== the job sequence, step by step, as the workflow runs it"
make_fixture
install_stubs
set_tracked "br-alpha br-beta br-delta"
set_version br-alpha 0.1.0
COMMIT="$(commit_all "alpha 0.1.0")"
g push -q origin main

job() {
    in_dev env BR_REGISTRY_KEY=k GITHUB_OUTPUT="$WORK/gate.out" REGISTRY_WORK="$WORK/reg" GITHUB_REPOSITORY=o/r bash -c '
        set -e
        bash .github/scripts/registry-gate.sh br-alpha 0.1.0 br-alpha/v0.1.0
        grep -q "^implemented=true" "$GITHUB_OUTPUT" && exit 0
        bash .github/scripts/registry-prepare.sh br-alpha 0.1.0 "$1" ""
        bash .github/scripts/release-crate.sh br-alpha 0.1.0 refs/heads/main "$1"
        bash .github/scripts/registry-record.sh br-alpha 0.1.0 br-alpha/v0.1.0 "$1" ""
    ' _ "$COMMIT"
}
origin_tags() { git -C "$ORIGIN" tag --list | tr '\n' ' '; }
requests() { ls "$STUB_DIR/requests" | wc -l | tr -d ' '; }
request_query() { python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["query"])' "$STUB_DIR/requests/$1.json" | grep -o 'libraries[A-Za-z]*(' | head -1; }

for reason in PATCH_NOT_FOUND PATCH_NOT_SEALED; do
    stub_reset
    : >"$WORK/gate.out"
    stub_error librariesValidateReleaseTarget "$reason" "m"
    run job
    expect_equal "$RC" 1 "$reason: the job fails at the gate"
    grep -qF "($reason)" <<<"$OUT" && verdict yes "$reason: the failure names the reason" || verdict no "$reason: the failure names the reason"
    expect_equal "$(origin_tags)" "" "$reason: no tag on origin"
    expect_equal "$(g tag --list | tr '\n' ' ')" "" "$reason: no tag in the clone"
    expect_equal "$(requests)" 1 "$reason: the only registry request is the validation"
    [[ ! -e "$STUB_DIR/gh.log" ]] && verdict yes "$reason: no release created" || verdict no "$reason: no release created"
done

stub_reset
: >"$WORK/gate.out"
stub_error librariesValidateReleaseTarget PATCH_IMPLEMENTED "m"
run job
expect_equal "$(origin_tags)" "" "implemented in the registry, no tag here: the job fails, still no tag"
expect_equal "$RC" 1 "implemented in the registry, no tag here: exit 1"

stub_reset
rm -rf "$WORK/reg"
: >"$WORK/gate.out"
eligible_target null true 0.1.0
run job
expect_equal "$RC" 0 "sealed and not implemented: the job succeeds"
expect_equal "$(origin_tags)" "br-alpha/v0.1.0 " "the tag exists once the job is done"
expect_equal "$(requests)" 4 "one validation, then three records"
expect_equal "$(request_query 1)" "librariesValidateReleaseTarget(" "request 1 validates the target, before any tag"
expect_equal "$(request_query 2)" "librariesRecordApiListing(" "request 2 records the API listing"
expect_equal "$(request_query 3)" "librariesRecordDependencies(" "request 3 records the dependencies"
expect_equal "$(request_query 4)" "librariesRecordRelease(" "request 4, the last, records the release tag and commit"
expect_file_has "$STUB_DIR/gh.log" "gh release create br-alpha/v0.1.0" "the release is created after the gate"

summary
