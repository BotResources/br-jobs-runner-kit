#!/usr/bin/env bash
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$HERE/test-sandbox-chart-lib.sh"
source "$HERE/test-sandbox-chart-network.sh"
source "$HERE/test-sandbox-chart-pod.sh"
source "$HERE/test-sandbox-chart-quota.sh"
source "$HERE/test-sandbox-chart-policy-model.sh"
source "$HERE/test-sandbox-chart-policy.sh"
source "$HERE/test-sandbox-chart-pod-policy.sh"
source "$HERE/test-sandbox-chart-user.sh"
source "$HERE/test-sandbox-chart-rbac.sh"
source "$HERE/test-sandbox-chart-runner-role.sh"
source "$HERE/test-sandbox-chart-runner-role-docs.sh"
source "$HERE/test-sandbox-chart-operator-parts.sh"
source "$HERE/test-sandbox-chart-scopes.sh"
source "$HERE/test-sandbox-chart-operator-rights.sh"
source "$HERE/test-sandbox-chart-operator-template.sh"
source "$HERE/test-sandbox-chart-operator-crd.sh"
source "$HERE/test-sandbox-chart-keeper-config.sh"
source "$HERE/test-sandbox-chart-adapter-config.sh"
source "$HERE/test-sandbox-chart-adapter-names.sh"
source "$HERE/test-sandbox-chart-exec-timeout.sh"
source "$HERE/test-sandbox-chart-exec-timeout-docs.sh"
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
render fence -f "$EXAMPLE" -f "$CHART/ci/fence-values.yaml"
render platform -f "$EXAMPLE" -f "$CHART/ci/platform-values.yaml"
render mirror -f "$EXAMPLE" -f "$MIRROR_VALUES"
render keeper -f "$EXAMPLE" -f "$OPERATOR_VALUES"
render nondefault -f "$EXAMPLE" -f "$NONDEFAULT_VALUES"
render egress_keeper -f "$EXAMPLE" -f "$EGRESS_VALUES" -f "$OPERATOR_VALUES"
render_yaml ex_yaml -f "$EXAMPLE"
render_yaml keeper_yaml -f "$EXAMPLE" -f "$OPERATOR_VALUES"
render_yaml mirror_yaml -f "$EXAMPLE" -f "$MIRROR_VALUES"
render_egress_fixtures
render_rights_fixtures
render_crd_fixtures
render_proxy_fixtures
render_proxy_config_fixtures
render_proxy_guard_fixtures

for check in $(declare -F | sed -n 's/^declare -f \(check_.*\)$/\1/p'); do
    "$check"
done

summary
