#!/usr/bin/env bash
set -uo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
workflow="${here}/../workflows/images-build.yml"
failures=0
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

pass() { echo "ok   $1"; }
fail() {
  echo "FAIL $1" >&2
  failures=$((failures + 1))
}

if bash "$here/images-check-gates.sh" "$workflow" >"${tmp}/out" 2>&1; then pass "gates: images-build.yml is accepted"; else fail "gates: images-build.yml is refused: $(cat "${tmp}/out")"; fi

mutated_refused() {
  local label="$1" message="$2" pattern="$3" replacement="$4"
  sed "s#${pattern}#${replacement}#" "$workflow" >"${tmp}/mutated.yml"
  if cmp -s "$workflow" "${tmp}/mutated.yml"; then
    fail "gates: ${label}: the mutation changed nothing"
  elif bash "$here/images-check-gates.sh" "${tmp}/mutated.yml" >"${tmp}/out" 2>&1; then
    fail "gates: ${label}: accepted"
  elif grep -q -- "$message" "${tmp}/out"; then
    pass "gates: ${label}"
  else
    fail "gates: ${label}: refused without '${message}': $(cat "${tmp}/out")"
  fi
}

mutated_refused "manifest does not need build" "'manifest' does not need the job 'build'" "needs: \[plan, build\]" "needs: [plan]"
mutated_refused "manifest runs after a failure" "'manifest' has a status function" "if: inputs.push && needs.plan.outputs.images != '\[\]'" "if: always() \&\& inputs.push"
mutated_refused "manifest runs unless cancelled" "'manifest' has a status function" "if: inputs.push && needs.plan.outputs.images != '\[\]'" "if: \${{ !cancelled() \&\& inputs.push }}"
mutated_refused "manifest runs on failure" "'manifest' has a status function" "if: inputs.push && needs.plan.outputs.images != '\[\]'" "if: failure()"
condition_refused() {
  local label="$1" replacement="$2"
  REPLACEMENT="$replacement" awk '$0 == "    if: inputs.push && needs.plan.outputs.images != '"'"'[]'"'"'" { print ENVIRON["REPLACEMENT"]; next } { print }' "$workflow" >"${tmp}/mutated.yml"
  if cmp -s "$workflow" "${tmp}/mutated.yml"; then
    fail "gates: ${label}: the mutation changed nothing"
  elif bash "$here/images-check-gates.sh" "${tmp}/mutated.yml" >"${tmp}/out" 2>&1; then
    fail "gates: ${label}: accepted"
  elif grep -q "'manifest' has a status function" "${tmp}/out"; then
    pass "gates: ${label}"
  else
    fail "gates: ${label}: refused without a status function message: $(cat "${tmp}/out")"
  fi
}

condition_refused "manifest condition folded with always()" $'    if: >-\n      inputs.push &&\n      always()'
condition_refused "manifest condition literal with failure()" $'    if: |\n      inputs.push\n      && failure()'
condition_refused "manifest condition folded, function on the last line" $'    if: >\n      inputs.push && needs.plan.outputs.images != \'[]\'\n      && !cancelled()'
condition_refused "manifest condition quoted with always()" '    if: "always()"'
condition_refused "manifest condition single-quoted with failure()" "    if: 'failure() && inputs.push'"
condition_refused "manifest condition in an expression with cancelled()" '    if: ${{ cancelled() }}'
condition_refused "manifest condition on several plain lines with always()" $'    if: inputs.push\n      && always()'

for function in Always FAILURE Cancelled SUCCESS; do
  condition_refused "manifest condition plain with ${function}()" "    if: inputs.push && ${function}()"
  condition_refused "manifest condition quoted with ${function}()" "    if: \"inputs.push && ${function}()\""
  condition_refused "manifest condition folded with ${function}()" $'    if: >-\n      inputs.push &&\n      '"${function}()"
  condition_refused "manifest condition literal with ${function}()" $'    if: |\n      inputs.push\n      && '"${function}()"
done

REPLACEMENT=$'    if: >-\n      inputs.push &&\n      needs.plan.outputs.images != \'[]\'' awk '$0 == "    if: inputs.push && needs.plan.outputs.images != '"'"'[]'"'"'" { print ENVIRON["REPLACEMENT"]; next } { print }' "$workflow" >"${tmp}/folded-ok.yml"
if cmp -s "$workflow" "${tmp}/folded-ok.yml"; then fail "gates: the folded condition without a status function: the mutation changed nothing"; elif bash "$here/images-check-gates.sh" "${tmp}/folded-ok.yml" >"${tmp}/out" 2>&1; then pass "gates: a folded manifest condition without a status function is accepted"; else fail "gates: a folded condition without a status function is refused: $(cat "${tmp}/out")"; fi

mutated_refused "digests does not need manifest" "'digests' does not need the job 'manifest'" "needs: \[plan, manifest\]" "needs: [plan]"
mutated_refused "a job continues on error" "sets continue-on-error" "fail-fast: false" "continue-on-error: true"

sed 's#^  manifest:#  manifests:#' "$workflow" >"${tmp}/renamed.yml"
if bash "$here/images-check-gates.sh" "${tmp}/renamed.yml" >"${tmp}/out" 2>&1; then fail "gates: a workflow with no job manifest is accepted"; else pass "gates: a workflow with no job manifest is refused"; fi

source "${here}/test-tools-image-gate.sh"
expected="${tmp}/expected.txt"
for id in $EXPECTED_TEST_IDS; do echo "RESULT ${id} PASS a title | detail"; done >"$expected"
if check_test_ids "$expected" 2>"${tmp}/err"; then pass "ids: every expected test has a result"; else fail "ids: complete results refused: $(cat "${tmp}/err")"; fi

first="${EXPECTED_TEST_IDS%% *}"
last="${EXPECTED_TEST_IDS##* }"
grep -v "^RESULT ${last} " "$expected" >"${tmp}/one-missing.txt"
if check_test_ids "${tmp}/one-missing.txt" 2>"${tmp}/err"; then fail "ids: a missing result is accepted"; elif grep -q "did not run: ${last}\$" "${tmp}/err"; then pass "ids: a missing result is named"; else fail "ids: a missing result: $(cat "${tmp}/err")"; fi

grep -v "^RESULT ${first} " "$expected" >"${tmp}/first-missing.txt"
if check_test_ids "${tmp}/first-missing.txt" 2>"${tmp}/err"; then fail "ids: a missing first result is accepted"; elif grep -q "did not run: ${first}\$" "${tmp}/err"; then pass "ids: a missing first result is named"; else fail "ids: a missing first result: $(cat "${tmp}/err")"; fi

: >"${tmp}/empty.txt"
if check_test_ids "${tmp}/empty.txt" 2>/dev/null; then fail "ids: an empty result file is accepted"; else pass "ids: an empty result file is refused"; fi
if check_test_ids "${tmp}/absent.txt" 2>/dev/null; then fail "ids: an absent result file is accepted"; else pass "ids: an absent result file is refused"; fi

{ cat "$expected"; echo "RESULT 99 PASS an unlisted test | detail"; } >"${tmp}/unlisted.txt"
if check_test_ids "${tmp}/unlisted.txt" 2>"${tmp}/err"; then fail "ids: a test that is not in the list is accepted"; elif grep -q "not in EXPECTED_TEST_IDS: 99\$" "${tmp}/err"; then pass "ids: a test that is not in the list is named"; else fail "ids: unlisted test: $(cat "${tmp}/err")"; fi

sed "s/^RESULT ${last} PASS/RESULT ${last} SKIP/" "$expected" >"${tmp}/skipped.txt"
if check_test_ids "${tmp}/skipped.txt" 2>/dev/null; then pass "ids: a skipped test counts as run"; else fail "ids: a skipped test is refused"; fi

early_exit() {
  local label="$1" body="$2" code
  bash -c 'source "$1"; RESULTS_FILE="$2"; trap exit_gate EXIT; shift 2; eval "$1"' _ "${here}/test-tools-image-gate.sh" "${tmp}/early.txt" "$body" >/dev/null 2>"${tmp}/err"
  code=$?
  echo "$code"
}

cp "$expected" "${tmp}/early.txt"
if [ "$(early_exit "all results, exit 0" "exit 0")" -eq 0 ]; then pass "ids: the exit gate keeps exit 0 when every test ran"; else fail "ids: the exit gate changed a good exit"; fi
head -n 5 "$expected" >"${tmp}/early.txt"
if [ "$(early_exit "early exit 0" "exit 0")" -ne 0 ]; then pass "ids: an exit 0 after five tests turns into a failure"; else fail "ids: an early exit 0 passed the gate"; fi
if [ "$(early_exit "early return of a test file" "return 0 2>/dev/null; exit 0")" -ne 0 ]; then pass "ids: a script that stops early with exit 0 fails"; else fail "ids: a script that stops early passed"; fi

[ "$failures" -eq 0 ]
