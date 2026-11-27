#!/usr/bin/env bash

CHECK_IMAGE_DEFAULT='ghcr.io/botresources/br-sandbox-check@sha256:51bd276e6c14b568a55633419d31a824338d423360c08d6584ecf28a7bcf556d'
CHECK_IMAGE_README="$ROOT/images/br-sandbox-check/README.md"

image_of_jobs() {
    {
        jq -r '.[] | select(.kind == "Job") | .spec.template.spec.containers[0].image' "$WORK/$1.json"
        jq -r '.[] | select(.kind == "CronJob") | .spec.jobTemplate.spec.template.spec.containers[0].image' "$WORK/$1.json"
    } | sort -u
}

check_the_default_check_image_is_the_digest() {
    section "network_check.image: the default is the check image by digest"
    local default_values
    default_values="$(vals defimg 'del(.network_check.image)')"
    t "the example fixture sets an image of its own, so the cases below remove it" test "$(yq '.network_check | has("image")' "$EXAMPLE")" = true
    t "the values file holds the image by digest" test "$(yq -r '.network_check.image' "$CHART/values.yaml")" = "$CHECK_IMAGE_DEFAULT"
    t "the default is a repository, then @sha256: and 64 hex digits" grep -qE '^[a-z0-9./-]+@sha256:[a-f0-9]{64}$' <<<"$CHECK_IMAGE_DEFAULT"
    render defimg -f "$default_values" --set network_check.cron='0 * * * *'
    render_yaml defimg_yaml -f "$default_values" --set network_check.cron='0 * * * *'
    t "a values file with no image renders" test -s "$WORK/defimg.json"
    t "the Job image is the digest" test "$(jq -r '.[] | select(.kind == "Job" and .metadata.name == "br-sandbox-netcheck-initial") | .spec.template.spec.containers[0].image' "$WORK/defimg.json")" = "$CHECK_IMAGE_DEFAULT"
    t "the CronJob image is the digest" test "$(jq -r '.[] | select(.kind == "CronJob") | .spec.jobTemplate.spec.template.spec.containers[0].image' "$WORK/defimg.json")" = "$CHECK_IMAGE_DEFAULT"
    t "the Job and the CronJob name no other image" test "$(image_of_jobs defimg)" = "$CHECK_IMAGE_DEFAULT"
    t "there are probe pods" test "$(namespace_count defimg_yaml)" -gt 0
    t "every probe pod runs the digest" test "$(probe_images defimg_yaml | sort -u)" = "$CHECK_IMAGE_DEFAULT"
    t "the Job and the probe pods are one image" test "$(distinct_images defimg_yaml defimg)" -eq 1
    t "the pull policy of the Job is IfNotPresent, as for any digest" test "$(jq -r '.[] | select(.kind == "Job" and .metadata.name == "br-sandbox-netcheck-initial") | .spec.template.spec.containers[0].imagePullPolicy' "$WORK/defimg.json")" = IfNotPresent
    render defimg_mirror -f "$default_values" -f "$MIRROR_VALUES"
    t "with the mirror the Job image is the mirror prefix and the digest" test "$(jq -r '.[] | select(.kind == "Job" and .metadata.name == "br-sandbox-netcheck-initial") | .spec.template.spec.containers[0].image' "$WORK/defimg_mirror.json")" = "mirror.example.test/${CHECK_IMAGE_DEFAULT}"
    t "the lint is strict on a values file with no image" helm lint --strict "$CHART" -f "$default_values"
    cp -R "$CHART" "$WORK/chartdefmut"
    sed -i.bak 's|^  image: ghcr.io/botresources/br-sandbox-check@sha256:.*$|  image: registry.example.test/tools/probe:1.0|' "$WORK/chartdefmut/values.yaml"
    t "mutation: the chart with another default differs from the chart" test "$(cmp -s "$WORK/chartdefmut/values.yaml" "$CHART/values.yaml" && echo same || echo differ)" = differ
    helm template t "$WORK/chartdefmut" -f "$default_values" 2>/dev/null | yq 'select(.kind == "Job" and .metadata.name == "br-sandbox-netcheck-initial") | .spec.template.spec.containers[0].image' - >"$WORK/chartdefmut.image"
    t "mutation: the Job of the chart with another default does not run the digest" test "$(cat "$WORK/chartdefmut.image")" != "$CHECK_IMAGE_DEFAULT"
}

check_a_project_check_image_replaces_the_default() {
    section "network_check.image: a project value replaces the default, and enabled keeps its meaning"
    local own_digest='registry.example.test/tools/probe@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'
    render own_tag -f "$EXAMPLE" --set network_check.cron='0 * * * *'
    t "a project tag is the image of the Job and of the CronJob" test "$(image_of_jobs own_tag)" = "$(yq -r '.network_check.image' "$EXAMPLE")"
    t "the project tag is not the default" test "$(yq -r '.network_check.image' "$EXAMPLE")" != "$CHECK_IMAGE_DEFAULT"
    render own_digest -f "$EXAMPLE" --set network_check.image="$own_digest" --set network_check.cron='0 * * * *'
    render_yaml own_digest_yaml -f "$EXAMPLE" --set network_check.image="$own_digest"
    t "a project digest is the image of the Job and of the CronJob" test "$(image_of_jobs own_digest)" = "$own_digest"
    t "a project digest is the image of every probe pod" test "$(probe_images own_digest_yaml | sort -u)" = "$own_digest"
    t "no object of that render holds the default" test "$(jq -r '.. | strings | select(contains("br-sandbox-check"))' "$WORK/own_digest.json" | grep -c .)" -eq 0
    fails_with "/network_check/image" --set network_check.image='bad image' -f "$(vals badimage 'del(.network_check.image)')"
    fails_with "/network_check/image" --set network_check.image='registry.example.test/tools/probe@sha256:abc' -f "$(vals shortdigest 'del(.network_check.image)')"
    fails_with "/network_check/image" --set network_check.image='registry.example.test/tools/probe@sha256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA' -f "$(vals upperdigest 'del(.network_check.image)')"
    fails_with "BR_SANDBOX_MISSING_VALUE{key=network_check.image}" -f "$(vals nullimage '.network_check.image = null')"
    t "with the check off and no image the chart renders" render off_noimage -f "$(vals offnoimage '.network_check.enabled = false | .network_check.image = null')"
    t "with the check off and no image there is no netcheck Job and no CronJob" test "$(jq '[.[] | select(.kind == "Job" or .kind == "CronJob")] | length' "$WORK/off_noimage.json")" -eq 0
    render off_default -f "$(vals offdefault 'del(.network_check.image) | .network_check.enabled = false')"
    t "with the check off and the default image no object holds the default image" test "$(jq -r '.. | strings | select(contains("br-sandbox-check"))' "$WORK/off_default.json" | grep -c .)" -eq 0
    t "with the check off there is no netcheck Job and no CronJob" test "$(jq '[.[] | select(.kind == "Job" or .kind == "CronJob")] | length' "$WORK/off_default.json")" -eq 0
}

check_image_default_sentences_and_the_digest_of_the_docs() {
    section "the README and the CHANGELOG hold the digest of the values file, and say what the image is only as the image README says it"
    local changelog_section readme_paragraph readme_row digest
    digest="$(yq -r '.network_check.image' "$CHART/values.yaml")"
    changelog_section="$(awk '/^## 0.3.0$/ {on=1; next} /^## / {on=0} on' "$CHART/CHANGELOG.md")"
    readme_paragraph="$(grep '^\*\*The check image\.\*\*' "$README")"
    readme_row="$(grep '^| `network_check.{' "$README")"
    t "the README holds the digest of the values file" grep -qF "$digest" "$README"
    t "the CHANGELOG 0.3.0 holds the digest of the values file" grep -qF "$digest" <<<"$changelog_section"
    t "the README row of network_check holds the digest of the values file" grep -qF "$digest" <<<"$readme_row"
    t "the check image paragraph holds the digest of the values file" grep -qF "$digest" <<<"$readme_paragraph"
    t "every reference to br-sandbox-check by digest in the README and the CHANGELOG is the digest of the values file" test "$(grep -ohE 'br-sandbox-check@sha256:[a-f0-9]+' "$README" "$CHART/CHANGELOG.md" | sort -u)" = "${digest#ghcr.io/botresources/}"
    t "the README row does not say the image is required" test "$(grep -c 'required if enabled' <<<"$readme_row")" -eq 0
    t "the README no longer says that the project chooses the check image or that no image of this repository has the tools" test "$(grep -cE 'chosen by the project|No image of this repository has them' "$README")" -eq 0
    t "the README says the default is br-sandbox-check, a digest of a manifest list for linux/amd64 and linux/arm64, read from the registry listing and not checked offline" grep -qF "The default is the image \`br-sandbox-check\`, \`${digest}\`: a digest of a manifest list for \`linux/amd64\` and \`linux/arm64\` (read from the registry listing of that digest; the offline suite does not check it)." <<<"$readme_paragraph"
    t "the README says what the image holds as read from the image README and not run by this suite" grep -qF 'By the README of that image, read there and not run by this suite: it is an Alpine base pinned by digest plus one pinned `kubectl`, it holds the ten programs that the check script calls (nine BusyBox applets of the base, and `kubectl`), and its own test, run by the image workflow, starts it on both platforms.' <<<"$readme_paragraph"
    t "the README says no cluster has run the Job with the image" grep -qF 'No cluster has run the check Job with this image at this version.' <<<"$readme_paragraph"
    t "the README says a project value replaces the default and that the schema takes a tag or a digest" grep -qF 'A project value of `network_check.image` replaces the default; the schema takes a tag or a digest, and a digest is advised.' <<<"$readme_paragraph"
    t "the README licence table names the check image and says its row is read from the image README" grep -qF '| `br-sandbox-check` (default of `network_check.image`) | check Job and probe pods | BusyBox GPL-2.0-only; musl MIT; `kubectl` Apache-2.0; the table of its README holds the rest (read from that README) |' "$README"
    t "the CHANGELOG 0.3.0 says the default is the image by the digest of a manifest list for linux/amd64 and linux/arm64, read from the registry listing" test "$(grep -cF "Added: the default of \`network_check.image\`, \`${digest}\`: the image \`br-sandbox-check\`, by the digest of a manifest list for \`linux/amd64\` and \`linux/arm64\` (read from the registry listing of that digest)." <<<"$changelog_section")" -eq 1
    t "the CHANGELOG 0.3.0 says what changes for a values file with no image and that null still gives the fault" test "$(grep -cF 'now a values file that sets no image renders the default, and a project value replaces it. A `null` value removes the key and still gives that fault.' <<<"$changelog_section")" -eq 1
    t "the CHANGELOG 0.3.0 says the content of the image is read from the image README and no cluster ran the Job" test "$(grep -cF 'is read from the README of the image and is not tested here; no cluster has run the check Job with this image.' <<<"$changelog_section")" -eq 1
    t "the CHANGELOG 0.3.0 names the test file" test "$(grep -cF '`test-sandbox-chart-netcheck-image.sh`' <<<"$changelog_section")" -eq 1
    t "the CHANGELOG 0.3.0 has one bullet on the default image" test "$(grep -c '^- Added: the default of `network_check.image`' <<<"$changelog_section")" -eq 1
}

check_the_image_readme_says_what_the_chart_docs_read_from_it() {
    section "the sentences read from the README of the image are in that README"
    local tool
    t "the image README exists" test -f "$CHECK_IMAGE_README"
    t "the image README says the chart value network_check.image takes it" grep -qF 'The chart value `network_check.image` takes it.' "$CHECK_IMAGE_README"
    t "the image README says the ten programs come from BusyBox, nine of them, and from kubectl" grep -qF 'The ten programs that the check script calls come from BusyBox (nine of them) and from `kubectl`.' "$CHECK_IMAGE_README"
    t "the image README says the base is Alpine pinned by index digest for amd64 and arm64" grep -qE '^\| Base \| `alpine:[0-9.]+`, pinned by index digest \(`linux/amd64` and `linux/arm64`\) \|$' "$CHECK_IMAGE_README"
    t "the image README says every pull request builds and tests it on both platforms" grep -qF 'Every pull request builds and tests it on both platforms' "$CHECK_IMAGE_README"
    for tool in sh nc cat sed tr mktemp awk sleep date kubectl; do
        t "the image README names ${tool} among the programs of the check script" grep -qF "\`${tool}\`" "$CHECK_IMAGE_README"
    done
    t "the image has a Dockerfile in the folder that the README describes" test -f "$ROOT/images/br-sandbox-check/Dockerfile"
}
