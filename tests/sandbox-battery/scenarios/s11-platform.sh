#!/usr/bin/env bash
HERE=$(cd "$(dirname "$0")" && pwd)
. "$HERE/common.sh"
scenario_begin s11-platform

ns=$(ns_of "$CLIENT_A" "$NET_ALLOW")
branch="battery-$(new_id)"

if [ -z "${GIT_TOKEN:-}" ]; then check platform-creds fail "creds.env has no GIT_TOKEN"; scenario_end; fi
set_secret "$ns" "$PLATFORM_SECRET_NAME" "$PLATFORM_GIT_SECRET_KEY" "$GIT_TOKEN"
k -n "$ns" patch secret "$PLATFORM_SECRET_NAME" --type merge -p "{\"data\":{\"$PLATFORM_PKG_SECRET_KEY\":\"$(printf '%s' "$PKG_TOKEN" | base64 | tr -d '\n')\"}}" > /dev/null
proxy_restart "$ns" "$PLATFORM_SET" || { check platform-proxy-restart fail "proxy of set $PLATFORM_SET did not roll out"; scenario_end; }

wait_warm "$ns" 1 180 "$POOL_ID_PLATFORM" || { check platform-warm-pod fail "no warm pod of pool $POOL_ID_PLATFORM in $ns"; scenario_end; }
claim_to pod "$ns" "platform-$(new_id)" agent-a "$POOL_ID_PLATFORM" || { check platform-claim fail "claim refused"; scenario_end; }

flow_log=$(with_timeout 1500 sxi "$ns" "$pod" env \
  "GIT_HOST=$TEST_HOST_GIT" "PKG_HOST=$TEST_HOST_PKG" "REPO=$PLATFORM_REPO" "BRANCH=$branch" \
  "GIT_PH_NAME=$PLATFORM_GIT_PLACEHOLDER_ENV" "PKG_PH_NAME=$PLATFORM_PKG_PLACEHOLDER_ENV" \
  "REG_UPPER=$(printf '%s' "$PLATFORM_CARGO_REGISTRY" | tr '[:lower:]-' '[:upper:]_')" sh -s < "$HERE/platform-flow.sh" 2>&1)
flow_rc=$?
printf '%s\n' "$flow_log"

stage_ms() { printf '%s\n' "$flow_log" | awk -v s="$1" '$1 == "STAGE" && $2 == s {print $3}'; }
for stage in clone packages build push; do
  if [ -n "$(stage_ms "$stage")" ]; then record "platform-$stage" "$(stage_ms "$stage")" ms; fi
done

verdict platform-clone "private repo cloned over HTTPS through the proxy" test -n "$(stage_ms clone)"
verdict platform-packages "packages downloaded from the package registry" test -n "$(stage_ms packages)"
verdict platform-build "cargo build ended with exit code 0" test -n "$(stage_ms build)"
verdict platform-push "branch $branch pushed" test -n "$(stage_ms push)"
verdict platform-flow-exit-code "flow exit code $flow_rc" test "$flow_rc" = 0

remote_branches=$(k -n "$TESTSVC_NS" exec "$GITEA_DEPLOY" -- git -c safe.directory='*' --git-dir="$GITEA_REPO_ROOT/$PLATFORM_REPO.git" branch --list "$branch" 2> /dev/null)
verdict platform-branch-on-server "branch listed by the server: $remote_branches" test -n "$remote_branches"

scan_token() {
  sxi "$ns" "$pod" env "SCAN_CANARY=$1" "$SANDBOX_PYTHON" - scan < "$HERE/canary-scan.py" | awk '{for (i = 1; i <= NF; i++) if ($i ~ /^value_hits=/) {split($i, a, "="); sum += a[2]}} END {print sum + 0}'
}
git_hits=$(scan_token "$GIT_TOKEN")
pkg_hits=$(scan_token "$PKG_TOKEN")
record platform-git-token-hits "$git_hits" hits
record platform-pkg-token-hits "$pkg_hits" hits
verdict platform-no-real-token-in-sandbox "env, memory and files hold 0 value of either token: $git_hits and $pkg_hits" test "$git_hits" = 0 -a "$pkg_hits" = 0

scenario_end
