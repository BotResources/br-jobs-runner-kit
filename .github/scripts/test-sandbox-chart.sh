#!/usr/bin/env bash
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$HERE/test-sandbox-chart-lib.sh"
source "$HERE/test-sandbox-chart-network.sh"
source "$HERE/test-sandbox-chart-pod.sh"
source "$HERE/test-sandbox-chart-quota.sh"
source "$HERE/test-sandbox-chart-policy.sh"
source "$HERE/test-sandbox-chart-user.sh"
source "$HERE/test-sandbox-chart-rbac.sh"
source "$HERE/test-sandbox-chart-keeper-config.sh"
source "$HERE/test-sandbox-chart-adapter-config.sh"
source "$HERE/test-sandbox-chart-adapter-names.sh"
source "$HERE/test-sandbox-chart-keeper-values.sh"
source "$HERE/test-sandbox-chart-ephemeral.sh"
source "$HERE/test-sandbox-chart-egress.sh"
source "$HERE/test-sandbox-chart-proxy.sh"
source "$HERE/test-sandbox-chart-proxy-config.sh"
source "$HERE/test-sandbox-chart-proxy-guards.sh"
source "$HERE/test-sandbox-chart-docs.sh"
source "$HERE/test-sandbox-chart-netcheck.sh"
source "$HERE/test-sandbox-chart-netcheck-proxy.sh"
source "$HERE/test-sandbox-chart-netcheck-window.sh"
source "$HERE/test-sandbox-chart-netcheck-leftover.sh"
source "$HERE/test-sandbox-chart-netcheck-guards.sh"
source "$HERE/test-sandbox-chart-netcheck-reading.sh"
source "$HERE/test-sandbox-chart-netcheck-contract.sh"

render ex -f "$EXAMPLE"
render mirror -f "$EXAMPLE" -f "$MIRROR_VALUES"
render keeper -f "$EXAMPLE" -f "$KEEPER_VALUES"
render nondefault -f "$EXAMPLE" -f "$NONDEFAULT_VALUES"
render egress_keeper -f "$EXAMPLE" -f "$EGRESS_VALUES" -f "$KEEPER_VALUES"
render_yaml ex_yaml -f "$EXAMPLE"
render_yaml keeper_yaml -f "$EXAMPLE" -f "$KEEPER_VALUES"
render_yaml mirror_yaml -f "$EXAMPLE" -f "$MIRROR_VALUES"
render_egress_fixtures
render_proxy_fixtures
render_proxy_config_fixtures
render_proxy_guard_fixtures

for check in $(declare -F | sed -n 's/^declare -f \(check_.*\)$/\1/p'); do
    "$check"
done

summary
