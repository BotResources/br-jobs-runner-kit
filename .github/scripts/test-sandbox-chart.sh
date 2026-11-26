#!/usr/bin/env bash
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$HERE/test-sandbox-chart-lib.sh"
source "$HERE/test-sandbox-chart-network.sh"
source "$HERE/test-sandbox-chart-pod.sh"
source "$HERE/test-sandbox-chart-quota.sh"
source "$HERE/test-sandbox-chart-policy.sh"
source "$HERE/test-sandbox-chart-rbac.sh"
source "$HERE/test-sandbox-chart-egress.sh"
source "$HERE/test-sandbox-chart-docs.sh"
source "$HERE/test-sandbox-chart-netcheck.sh"

render ex -f "$EXAMPLE"
render mirror -f "$EXAMPLE" -f "$MIRROR_VALUES"
render keeper -f "$EXAMPLE" -f "$KEEPER_VALUES"
render_yaml ex_yaml -f "$EXAMPLE"
render_yaml keeper_yaml -f "$EXAMPLE" -f "$KEEPER_VALUES"
render_yaml mirror_yaml -f "$EXAMPLE" -f "$MIRROR_VALUES"
render_egress_fixtures

for check in $(declare -F | sed -n 's/^declare -f \(check_.*\)$/\1/p'); do
    "$check"
done

summary
