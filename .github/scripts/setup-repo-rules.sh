#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
DECL="$ROOT/.github/required-checks.json"
DRY_RUN=false

if [[ -z "${REPO:-}" ]]; then
    origin="$(git -C "$ROOT" remote get-url origin 2>/dev/null || true)"
    REPO="$(sed -E 's#^git@github\.com:##; s#^https://github\.com/##; s#\.git$##' <<<"$origin")"
fi

while [[ $# -gt 0 ]]; do
    case "$1" in
        --dry-run) DRY_RUN=true; shift ;;
        -h|--help)
            echo "usage: [REPO=owner/name] $0 [--dry-run]"
            exit 0
            ;;
        *)
            echo "ERROR: unknown argument: $1" >&2
            exit 2
            ;;
    esac
done

if [[ -z "${REPO}" || "${REPO}" != */* ]]; then
    echo "ERROR: could not read the repository from the 'origin' remote. Pass it: REPO=owner/name $0" >&2
    exit 1
fi
if [[ ! -f "$DECL" ]]; then
    echo "ERROR: $DECL not found" >&2
    exit 1
fi

CONTEXTS="$(jq -c '.required' "$DECL")"
if [[ "$(jq 'length' <<<"$CONTEXTS")" -eq 0 ]]; then
    echo "ERROR: $DECL declares no required check" >&2
    exit 1
fi

checks_param="$(jq -c '{strict_required_status_checks_policy: true, required_status_checks: map({context: .})}' <<<"$CONTEXTS")"

branch_ruleset="$(jq -n --argjson checks "$checks_param" '{
    name: "release-lines",
    target: "branch",
    enforcement: "active",
    bypass_actors: [],
    conditions: {ref_name: {include: ["refs/heads/release/**"], exclude: []}},
    rules: [
        {type: "deletion"},
        {type: "non_fast_forward"},
        {type: "required_linear_history"},
        {type: "pull_request", parameters: {
            required_approving_review_count: 0,
            dismiss_stale_reviews_on_push: true,
            require_code_owner_review: false,
            require_last_push_approval: false,
            required_review_thread_resolution: true
        }},
        {type: "required_status_checks", parameters: $checks}
    ]
}')"

tag_ruleset='{
    "name": "release-tags",
    "target": "tag",
    "enforcement": "active",
    "bypass_actors": [],
    "conditions": {"ref_name": {"include": ["refs/tags/**/v*"], "exclude": []}},
    "rules": [{"type": "deletion"}, {"type": "non_fast_forward"}, {"type": "update"}]
}'

main_checks="$(jq -c '{strict: true, contexts: .}' <<<"$CONTEXTS")"

echo "repo: ${REPO}"
echo "required checks:"
jq -r '.[] | "  - " + .' <<<"$CONTEXTS"
echo "rulesets: release-lines (refs/heads/release/**), release-tags (refs/tags/**/v*)"
echo "main: required status checks replaced by the declared list"
echo "security: secret scanning, push protection"
echo "actions: fork PR approval for all outside contributors, SHA pinning enforced"

if [[ "${DRY_RUN}" == "true" ]]; then
    echo "DRY-RUN: nothing applied"
    exit 0
fi

if ! gh auth status >/dev/null 2>&1; then
    echo "ERROR: gh CLI is not authenticated" >&2
    exit 1
fi

upsert_ruleset() {
    local name="$1" payload="$2" id
    id="$(gh api "repos/${REPO}/rulesets" --jq ".[] | select(.name == \"${name}\") | .id")"
    if [[ -n "$id" ]]; then
        gh api -X PUT --input - "repos/${REPO}/rulesets/${id}" <<<"$payload" >/dev/null
        echo "ruleset ${name}: updated"
    else
        gh api -X POST --input - "repos/${REPO}/rulesets" <<<"$payload" >/dev/null
        echo "ruleset ${name}: created"
    fi
}

upsert_ruleset release-lines "$branch_ruleset"
upsert_ruleset release-tags "$tag_ruleset"

gh api -X PATCH --input - "repos/${REPO}/branches/main/protection/required_status_checks" <<<"$main_checks" >/dev/null
echo "main: required status checks updated"

gh api -X PATCH --input - "repos/${REPO}" >/dev/null <<<'{"security_and_analysis":{"secret_scanning":{"status":"enabled"},"secret_scanning_push_protection":{"status":"enabled"}}}'
echo "security: secret scanning and push protection enabled"

gh api -X PUT --input - "repos/${REPO}/actions/permissions/fork-pr-contributor-approval" >/dev/null <<<'{"approval_policy":"all_external_contributors"}'
echo "actions: fork PR approval set to all outside contributors"

gh api -X PUT --input - "repos/${REPO}/actions/permissions" >/dev/null <<<'{"enabled":true,"allowed_actions":"all","sha_pinning_required":true}'
echo "actions: SHA pinning enforced"
