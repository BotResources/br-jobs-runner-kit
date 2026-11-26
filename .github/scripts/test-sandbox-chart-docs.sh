#!/usr/bin/env bash

README="$CHART/README.md"

check_licences_in_the_readme() {
    section "the licence table and the licence of the chart"
    t "the chart declares Apache-2.0" grep -q 'licenses: Apache-2.0' "$CHART/Chart.yaml"
    local image
    for image in br-sandbox-tools br-sandbox-python br-sandbox-keeper; do
        t "the images table has a row with an SPDX id for ${image}" grep -qE "^\| \`${image}\` .*(BSD-2-Clause|PSF-2.0|Apache-2.0)" "$README"
    done
    t "the tools row lists libarchive, musl and pause with their SPDX ids" grep -qE 'BSD-2-Clause; musl MIT; `pause` of ours Apache-2.0' "$README"
}

check_README_holds_the_contract() {
    section "README contract"
    local phrase
    for phrase in '## Values' '## Modes' '## Label contract' '## Image contract' '## Why' '## Slots' '## Images and licences' '| `none` |' '| `allow` |' '| `full` |' '/bin/sh' '`cat` and `find`' '`br-sandbox-python`' 'Python 3.12, pandas, openpyxl, python-docx, python-pptx, LibreOffice' 'warm_max_age_seconds' '`imagePullPolicy: Always`' '30 minutes' 'Name `seed`' '`/skills`' '`/sbx/bin/bsdtar`' 'They render nothing now' 'no scrub exists' 'role=proxy' 'cluster-test' 'BR_SANDBOX_EGRESS_DISABLED' 'Two objects take any key: `keeper.settings`' 'in every sandbox namespace and in the system namespace' 'subordinate id ranges' 'sets `pool.node_selector`' 'greps registries and host names only' 'checked by review' 'BR_SANDBOX_RUNNER_IS_POD_IDENTITY'; do
        t "the README holds: ${phrase}" grep -qF -- "$phrase" "$README"
    done
    local slot
    for slot in egress.env egress.volumes egress.volume_mounts egress.objects keeper.objects; do
        t "the README names the slot ${slot}" grep -qF "br-sandbox-chart.slot.${slot}" "$README"
    done
    local name
    for name in br.sandbox/managed br.sandbox/mode br.sandbox/client br.sandbox/role br.sandbox/pool br.sandbox/pool-id br.sandbox/thread br.sandbox/agent br.sandbox/creds br.sandbox/clean br.sandbox/claimed-at br.sandbox/idle-since br.sandbox/state-bytes br.sandbox/state-turn br.sandbox/credsets br.sandbox/system; do
        t "the label contract names ${name}" grep -qF "\`${name}\`" "$README"
    done
    local key
    for key in $(yq -r '[.. | select(tag == "!!map") | keys | .[]] | unique | .[]' "$CHART/values.yaml" | grep -v '^size$' | grep -vE '^(requests|limits|cpu|memory|ephemeral-storage|request|limit|disk)$'); do
        t "the README names the key ${key}" grep -qF "$key" "$README"
    done
}

check_no_scrub_claim() {
    section "no text of the chart claims a scrub"
    local hits
    hits="$(cd "$CHART" && grep -rniE 'scrub' . | grep -viE 'no scrub|no response scrub|claims a scrub|claim a scrub' || true)"
    t "every mention of a scrub denies it" test -z "$hits"
    t "the README says a bound host that sends a value back shows it to the sandbox" grep -qF 'A bound host that sends a credential value back shows it to the sandbox (no scrub exists; the proxy has no response scrub)' "$README"
}

check_file_hygiene() {
    section "files of 300 lines at most, no comment, no fixture secret"
    local file
    for file in $(cd "$ROOT" && git ls-files --others --cached --exclude-standard -- charts/br-sandbox-chart .github/scripts/test-sandbox-chart*.sh | sort -u); do
        [[ -f "$ROOT/$file" ]] || continue
        t "${file} has at most 300 lines" test "$(wc -l <"$ROOT/$file")" -le 300
    done
    t "no comment in the values, the templates and the fixtures" test -z "$(grep -rnE '^\s*#|\{\{-?\s*/\*' "$CHART/values.yaml" "$CHART/templates" "$CHART/ci" || true)"
    t "no comment in the check script but the shebang" test -z "$(grep -nE '^\s*#' "$CHART/files/network-check.sh" | grep -v '^1:#!' || true)"
    t "no secret-like string in the fixtures" test -z "$(grep -rniE 'password|passwd|token|secret_key|api_key|BEGIN [A-Z ]*KEY|AKIA' "$CHART/ci" || true)"
}

check_lint_strict_on_every_fixture() {
    section "helm lint --strict on the three fixtures"
    t "example" helm lint --strict "$CHART" -f "$EXAMPLE"
    t "example and mirror" helm lint --strict "$CHART" -f "$EXAMPLE" -f "$MIRROR_VALUES"
    t "example and keeper" helm lint --strict "$CHART" -f "$EXAMPLE" -f "$KEEPER_VALUES"
    t "the chart does not render without values" bash -c '! helm template t "$1" >/dev/null 2>&1' _ "$CHART"
}
