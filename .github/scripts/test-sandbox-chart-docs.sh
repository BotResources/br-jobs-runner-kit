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
    for phrase in '## Values' '## Modes' '## Label contract' '## Image contract' '## Why' '## Slots' '## Images and licences' '| `none` |' '| `allow` |' '| `full` |' '/bin/sh' '`cat` and `find`' '`br-sandbox-python`' 'Python 3.12, pandas, openpyxl, python-docx, python-pptx, LibreOffice' 'warm_max_age_seconds' '`imagePullPolicy: Always`' '30 minutes' 'Name `seed`' '`/skills`' '`/sbx/bin/bsdtar`' 'the keeper slot renders nothing yet' 'no scrub exists' 'role=proxy' 'cluster-test' 'BR_SANDBOX_EGRESS_DISABLED' 'One object takes any key: `keeper.settings`' '**Targets named by an IP literal.**' 'the render needs no name for it' 'TLS to an IP literal does not work' '**Targets named by a name.**' 'a DNS that answers its names' '**Limit.**' 'a response body that echoed the `Authorization` header carried the real value to the client' 'in every sandbox namespace and in the system namespace'; do
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
    t "the credential section holds the Limit sentence after the Credentials paragraph and before the Rotation paragraph" test "$(awk '/^\*\*Credentials\.\*\*/{c=NR} /^\*\*Limit\.\*\*/{l=NR} /^\*\*Rotation and config changes\.\*\*/{r=NR} END{print (c > 0 && l > c && r > l)}' "$README")" = 1
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
    section "helm lint --strict on the four fixtures"
    t "example" helm lint --strict "$CHART" -f "$EXAMPLE"
    t "example and mirror" helm lint --strict "$CHART" -f "$EXAMPLE" -f "$MIRROR_VALUES"
    t "example and keeper" helm lint --strict "$CHART" -f "$EXAMPLE" -f "$KEEPER_VALUES"
    t "example and egress" helm lint --strict "$CHART" -f "$EXAMPLE" -f "$EGRESS_VALUES"
    t "the CI lint loop names every overlay of the chart" grep -qE 'for overlay in mirror keeper egress; do' "$ROOT/.github/workflows/ci.yml"
    t "the chart does not render without values" bash -c '! helm template t "$1" >/dev/null 2>&1' _ "$CHART"
}

check_the_check_image_lists_every_tool_the_script_calls() {
    section "the check image section names each program of a fixed list of candidate names that the network check script calls"
    local paragraph tool in_script
    paragraph="$(grep '^\*\*The check image\.\*\*' "$README")"
    for tool in kubectl nc cat sed tr mktemp awk grep sleep head tail cut sort uniq wc date curl wget find xargs tee dirname basename tac rev expr env timeout jq yq base64 od ls rm mv cp touch tar; do
        in_script=no
        if grep -qE "(^|[^A-Za-z_./-])${tool}([^A-Za-z_./-]|\$)" "$CHART/files/network-check.sh"; then in_script=yes; fi
        if [[ "$in_script" == yes ]]; then
            t "the script calls ${tool} and the check image section names it" grep -qF "\`${tool}\`" <<<"$paragraph"
        else
            t "the script does not call ${tool} and the check image section does not name it" test "$(grep -cF "\`${tool}\`" <<<"$paragraph")" -eq 0
        fi
    done
    t "the section says the list is the programs called at this version, that a fixed list of candidate names is checked, and that no full parse is done" grep -qF 'the programs that the script calls at this version, besides the builtins of `sh`. A test checks the script against a fixed list of candidate program names; it does not parse every command of the script.' <<<"$paragraph"
    t "the section does not claim that the script calls no other program" test "$(grep -c 'no other program' <<<"$paragraph")" -eq 0
    t "the section says the probe reads the reply until the proxy closes" grep -qF 'keeps reading the reply until the proxy closes the connection' <<<"$paragraph"
    t "the section says -w is an idle timeout" grep -qF '`-w` is an idle timeout' <<<"$paragraph"
    t "the section says nc exits when the proxy closes" grep -qF 'it exits when the proxy closes the connection' <<<"$paragraph"
    t "the section says the suite uses a stub of nc, no real nc build is exercised, the probe image was not run, and what the first cluster run must confirm" grep -qF 'The suite uses a stub of `nc`; no real `nc` build is exercised by the suite. The real probe image and its `nc` were not run at this version. The first cluster run must confirm that the tools the probe needs exist there and that `nc` behaves as the probe expects (it reads until the proxy closes, `-w` is an idle timeout, it exits on close).' <<<"$paragraph"
}

check_the_docs_state_what_was_and_was_not_measured() {
    section "the README and the CHANGELOG say what was measured, what is documented behaviour of the product and what is not proved"
    local text file
    for file in "$README" "$CHART/CHANGELOG.md"; do
        t "$(basename "$file") says only the arm64 image was started and the amd64 image was not" grep -qF 'amd64 image' "$file"
        t "$(basename "$file") says the proxy is kept from its vendor by the absence of a token and a new digest needs a new capture" grep -qF 'a new digest needs a new capture' "$file"
        t "$(basename "$file") says the binding of a placeholder to its hosts and require_tls are documented behaviour of the product" grep -qF 'documented behaviour of the proxy product' "$file"
        t "$(basename "$file") says no git client was used" grep -qF 'no git client was used' "$file"
    done
    text="$(grep '^\*\*Not proved offline\.\*\*' "$README")"
    t "the Not proved offline paragraph lists the binding of a placeholder to its hosts and require_tls" grep -qF 'that each placeholder is bound to its `hosts`, that `require_tls` keeps a bound value off plain HTTP' <<<"$text"
    t "the README says the intended path for git and the measured form" grep -qF 'The intended path for git is HTTPS with the token as placeholder; git over ssh is not offered. The measured form is a plain HTTPS GET with a Bearer header; no git client was used.' "$README"
    t "the README does not say that git works over HTTPS" test "$(grep -c 'Git works over HTTPS' "$README")" -eq 0
    t "the README says the policy expressions were not executed where it says the policy refuses a Secret volume" grep -qF 'the admission policy refuses all three (the policy expressions were not executed, see Admission policy)' "$README"
    t "the README says a tool that ignores the CA variables refuses the proxy certificate" grep -qF 'a tool that ignores those variables refuses the proxy certificate' "$README"
    t "the CHANGELOG says the same about the CA variables" grep -qF 'a tool that ignores those variables refuses the proxy certificate' "$CHART/CHANGELOG.md"
    t "the CHANGELOG renders the proxy and has an upgrade note" test "$(grep -cF 'renders the proxy' "$CHART/CHANGELOG.md")" -eq 1 -a "$(grep -cF 'Upgrade from 0.1.0' "$CHART/CHANGELOG.md")" -eq 1
    t "the CHANGELOG does not say a working proxy" test "$(grep -c 'working proxy' "$CHART/CHANGELOG.md")" -eq 0
}

check_the_docs_state_the_credential_set_schema_rule() {
    section "the README and the CHANGELOG state that secret_ref and secrets go together and secrets has an item"
    t "the README states the rule and the effect on a 0.1.0 value" grep -qF '`secret_ref` and `secrets` must be given together and `secrets` needs at least one item: a 0.1.0 value with only one of them, or with an empty `secrets`, now fails the render.' "$README"
    t "the CHANGELOG states the rule and the effect on a 0.1.0 value" grep -qF '`secret_ref` and `secrets` must be given together and `secrets` needs at least one item: a 0.1.0 value with only one of them, or with an empty `secrets`, now fails.' "$CHART/CHANGELOG.md"
}
