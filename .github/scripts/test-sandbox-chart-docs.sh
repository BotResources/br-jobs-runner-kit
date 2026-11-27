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
    for phrase in '## Values' '## Modes' '## Label contract' '## Image contract' '## Why' '## Slots' '## Images and licences' '| `none` |' '| `allow` |' '| `full` |' '/bin/sh' '`cat` and `find`' '`br-sandbox-python`' 'Python 3.12, pandas, openpyxl, python-docx, python-pptx, LibreOffice' 'warm_max_age_seconds' '`imagePullPolicy: Always`' '30 minutes' 'Name `seed`' '`/skills`' '`/sbx/bin/bsdtar`' 'the keeper slot renders nothing yet' 'no scrub exists' 'role=proxy' 'cluster-test' 'BR_SANDBOX_EGRESS_DISABLED' 'One object takes any key: `keeper.settings`' '**Keeper document.**' '**Adapter document.**' 'the flat keys of `SandboxConfigInput`' '`store_credentials` is never in the document' 'The only reader of this ConfigMap is the runner identity' 'No pod mounts it, and it holds no secret' 'The old key `config.yaml` is not rendered' 'the adapter refuses mode `allow` with no list when it starts' 'The adapter document is proved by the real type' 'BR_SANDBOX_KEEPER_SETTING_COMPUTED' '`br-sandbox-keeper-config`' '`br.sandbox/keeper-config-sha256`' 'a change of the document rolls the keeper pods' 'the system namespace holds no such ConfigMap' 'the image `rancher/k3s` has no `nc`' 'the probe is tested with a busybox-style `nc`' '**Targets named by an IP literal.**' 'the render needs no name for it' 'TLS to an IP literal does not work' '**Targets named by a name.**' 'a DNS that answers its names' '**Limit.**' 'a response body that echoed the `Authorization` header carried the real value to the client' 'in every sandbox namespace and in the system namespace' 'subordinate id ranges' 'sets `pool.node_selector`' 'greps registries and host names only' 'checked by review' 'BR_SANDBOX_RUNNER_IS_POD_IDENTITY' 'RUN_AS_NON_ROOT' 'system:serviceaccount:{system namespace}:br-sandbox-netcheck' 'No rule protects the seed against an adapter that is compromised' 'A container with the seed name and another image is refused' 'does not share its process namespace' 'runAsNonRoot: true' 'Rule 10 is the only rule that reads `ephemeralContainers`' 'holds a verb on `pods/ephemeralcontainers`' 'The seed container of the regular container list (`containers`)' 'the exception does not apply to those two lists' 'Upgrade from a pool of chart 0.1.0 to 0.1.1' 'delete the old warm pods' 'warm or claimed' 'a claimed pod is not owned by the ReplicaSet' 'let the agent finish and delete the pod' 'A cluster test of the refusals of an unlabelled pod, of a `role=proxy` pod and of a relabel of the role' 'is to be made; no such test exists in this repository at this version and none was executed'; do
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
    t "the section says the suite uses a stub of nc, no real nc build is exercised, the probe image was not started, and what the first cluster test must confirm" grep -qF 'The suite uses a stub of `nc`; no real `nc` build is exercised by the suite. The real probe image and its `nc` were not started at this version. The first cluster test must confirm that the tools the probe needs exist there and that `nc` behaves as the probe expects (it reads until the proxy closes, `-w` is an idle timeout, it exits on close).' <<<"$paragraph"
}

check_the_docs_state_what_was_and_was_not_measured() {
    section "the README and the CHANGELOG say what was measured, what is documented behaviour of the product and what is not proved"
    local text file
    for file in "$README" "$CHART/CHANGELOG.md"; do
        t "$(basename "$file") says only the arm64 image was started and the amd64 image was not" grep -qF 'amd64 image' "$file"
        t "$(basename "$file") says the chart assumes a proxy with no token makes no call to its vendor and a new digest needs a new capture" grep -qF 'a new digest needs a new capture' "$file"
        t "$(basename "$file") says the binding of a placeholder to its hosts and require_tls are documented behaviour of the product" grep -qF 'documented behaviour of the proxy product' "$file"
        t "$(basename "$file") says no git client was used" grep -qF 'no git client was used' "$file"
    done
    text="$(grep '^\*\*Not proved offline\.\*\*' "$README")"
    t "the Not proved offline paragraph lists the binding of a placeholder to its hosts and require_tls" grep -qF 'that each placeholder is bound to its `hosts`, that `require_tls` keeps a bound value off plain HTTP' <<<"$text"
    t "the README marks the re-read of a file source at a reload call as documented behaviour of the proxy product, not measured here" grep -qF 'By the documented behaviour of the proxy product, not measured here, the proxy re-reads a `file` source at a reload call' "$README"
    t "the Not proved offline paragraph lists the re-read of a file source at a reload call" grep -qF 'that the proxy re-reads a `file` source at a reload call' <<<"$text"
    t "the README says the intended path for git and the measured form" grep -qF 'The intended path for git is HTTPS with the token as placeholder; git over ssh is not offered. The form measured on the pinned proxy image (0.61.0), in mode `full` with a credential set, is a plain HTTPS GET with a Bearer header; no git client was used.' "$README"
    t "the README does not say that git works over HTTPS" test "$(grep -c 'Git works over HTTPS' "$README")" -eq 0
    t "the README says the policy expressions were not executed where it says the policy refuses a Secret volume" grep -qF 'the admission policy refuses all three (the policy expressions were not executed, see Admission policy)' "$README"
    t "the README says a tool that ignores the CA variables refuses the proxy certificate" grep -qF 'a tool that ignores those variables refuses the proxy certificate' "$README"
    t "the CHANGELOG says the same about the CA variables" grep -qF 'a tool that ignores those variables refuses the proxy certificate' "$CHART/CHANGELOG.md"
    t "the CHANGELOG renders the proxy and has an upgrade note" test "$(grep -cF 'renders the proxy' "$CHART/CHANGELOG.md")" -eq 1 -a "$(grep -cF 'Upgrade from 0.1.0 or 0.1.1:' "$CHART/CHANGELOG.md")" -eq 1
    t "the CHANGELOG does not say a working proxy" test "$(grep -c 'working proxy' "$CHART/CHANGELOG.md")" -eq 0
    t "the CHANGELOG upgrade note says what happens to a warm pod made before the upgrade" grep -qF 'With `egress.enabled: true`, a warm pod of a mode with a proxy that was made before the upgrade keeps its old template (no proxy variables, no CA file, no placeholders), so it has no working egress until it is recreated: a ReplicaSet does not recreate running pods when its template changes. After the upgrade, delete the old warm pods of every sandbox namespace of a mode with a proxy; the ReplicaSet recreates them from the new template. A claimed pod made before the upgrade keeps its old template too, and it is not owned by the ReplicaSet, so deleting the old warm pods does not reach it: let the agent finish and delete the pod, or delete it at once. Where the keeper is deployed, it deletes a warm pod older than `warm_max_age_seconds` (default 1800).' "$CHART/CHANGELOG.md"
    t "the README says what happens to a warm pod made before the upgrade" grep -qF '**Upgrade to 0.2.0.** With `egress.enabled: true`, a warm pod of a mode with a proxy that was made before the upgrade keeps its old template (no proxy variables, no CA file, no placeholders), so it has no working egress until it is recreated: a ReplicaSet does not recreate running pods when its template changes. After the upgrade, delete the old warm pods of every sandbox namespace of a mode with a proxy; the ReplicaSet recreates them from the new template. A claimed pod made before the upgrade keeps its old template too, and it is not owned by the ReplicaSet, so deleting the old warm pods does not reach it: let the agent finish and delete the pod, or delete it at once. Where the keeper is deployed, it deletes a warm pod older than `warm_max_age_seconds` (default 1800).' "$README"
    t "the CHANGELOG upgrade note says a claimed pod made before the upgrade keeps its old template and is not reached by deleting warm pods" grep -qF 'A claimed pod made before the upgrade keeps its old template too, and it is not owned by the ReplicaSet, so deleting the old warm pods does not reach it: let the agent finish and delete the pod, or delete it at once.' "$CHART/CHANGELOG.md"
    t "the README says a claimed pod made before the upgrade keeps its old template and is not reached by deleting warm pods" grep -qF 'A claimed pod made before the upgrade keeps its old template too, and it is not owned by the ReplicaSet, so deleting the old warm pods does not reach it: let the agent finish and delete the pod, or delete it at once.' "$README"
    t "the README says a Service clusterIP cannot be changed in place, as documented Kubernetes behaviour not measured here, and the chart does not replace the Service" grep -qF 'The `clusterIP` of a Service cannot be changed in place (documented behaviour of Kubernetes, not measured here): changing `proxy.cluster_ip` needs the proxy Service of that mode deleted first, or a forced replace by the deploy tool; the chart does not do it.' "$README"
    t "the README does not say that a change of the proxy address replaces the Service" test "$(grep -cF 'replaces the Service' "$README")" -eq 0
    t "the Not proved offline paragraph lists the refusal of an in place change of a Service clusterIP as documented Kubernetes behaviour" grep -qF 'the refusal by the API server of a change of the `clusterIP` of a Service in place (documented behaviour of Kubernetes, not measured here)' <<<"$(grep '^\*\*Not proved offline\.\*\*' "$README")"
    t "the CHANGELOG pod bullet lists the service links, the tmp volume and the probes" grep -qF '`enableServiceLinks: false`, a writable `/tmp` as an `emptyDir` of 16Mi, a readiness and a liveness probe (TCP on the tunnel port, every 5 seconds)' "$CHART/CHANGELOG.md"
    t "the README pod paragraph lists the service links, the tmp volume and the probes" grep -qF '`enableServiceLinks: false`, a writable `/tmp` as an `emptyDir` of 16Mi, a readiness and a liveness probe (TCP on the tunnel port, every 5 seconds)' "$README"
    t "the CHANGELOG says the name target swap was measured in mode full and the swap in mode allow is not measured" grep -qF 'measured once by hand in mode `full` with a credential set (a placeholder swapped in a Bearer `Authorization` header); the swap in mode `allow` is the documented behaviour of the proxy product, not measured here;' "$CHART/CHANGELOG.md"
    t "the CHANGELOG lists the swap in mode allow as documented behaviour of the proxy product, not measured here" grep -qF 'and the swap of a placeholder in mode `allow` are the documented behaviour of the proxy product, not measured here' "$CHART/CHANGELOG.md"
    t "the Not proved offline paragraph lists the swap in mode full as measured by hand and the swap in mode allow" grep -qF 'the swap of a placeholder (measured by hand in mode `full` only), the swap of a placeholder in mode `allow`,' <<<"$text"
}

check_the_docs_state_the_credential_set_schema_rule() {
    section "the README and the CHANGELOG state that secret_ref and secrets go together and secrets has an item"
    t "the README states the rule and the effect on a 0.1.0 value" grep -qF '`secret_ref` and `secrets` must be given together and `secrets` needs at least one item: a 0.1.0 value with only one of them, or with an empty `secrets`, now fails the render.' "$README"
    t "the CHANGELOG states the rule and the effect on a 0.1.0 value" grep -qF '`secret_ref` and `secrets` must be given together and `secrets` needs at least one item: a 0.1.0 value with only one of them, or with an empty `secrets`, now fails.' "$CHART/CHANGELOG.md"
}
